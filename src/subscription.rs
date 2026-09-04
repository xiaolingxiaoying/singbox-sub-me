use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rcgen::{CertificateParams, DnType, KeyPair};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;

use crate::canonical::CanonicalNode;
use crate::config::{
    CertificateMode, ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol,
    SubscriptionMode,
};

const SING_BOX_ARTIFACT: &str = "subscription-sing-box.json";
const CLASH_ARTIFACT: &str = "subscription-clash.yaml";
const URI_ARTIFACT: &str = "subscription-uri.txt";
const SING_BOX_SERVER_ARTIFACT: &str = "sing-box-server.json";
const ARTIFACTS_RELATIVE_DIR: &str = "var/lib/sbctl/artifacts";
const ACTIVE_CONFIG_RELATIVE_PATH: &str = "etc/sing-box/config.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionFormat {
    SingBox,
    Clash,
    Uri,
}

impl SubscriptionFormat {
    pub fn path_name(self) -> &'static str {
        match self {
            Self::SingBox => "sing-box.json",
            Self::Clash => "clash.yaml",
            Self::Uri => "uri",
        }
    }

    pub fn artifact_name(self) -> &'static str {
        match self {
            Self::SingBox => SING_BOX_ARTIFACT,
            Self::Clash => CLASH_ARTIFACT,
            Self::Uri => URI_ARTIFACT,
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::SingBox => "application/json; charset=utf-8",
            Self::Clash => "application/yaml; charset=utf-8",
            Self::Uri => "text/plain; charset=utf-8",
        }
    }
}

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error("external reverse-proxy subscription must bind a loopback address")]
    ExternalProxyBind,
    #[error("subscription listener port {0} is already in use")]
    ListenerUnavailable(u16),
    #[error("subscription listener failed: {0}")]
    ListenerIo(String),
    #[error("Direct HTTPS requires systemd socket activation: {0}")]
    SocketActivation(String),
    #[error("Direct HTTPS received an unexpected listener on port {0}")]
    UnexpectedDirectListener(u16),
    #[error("Direct HTTPS is missing the {0} listener")]
    MissingDirectListener(u16),
    #[error("HTTP handling failed: {0}")]
    Http(String),
    #[error("no subscription-capable Managed protocol is enabled")]
    MissingNodes,
    #[error("invalid subscription credential")]
    InvalidCredential,
    #[error("subscription artifact is unavailable: {0}")]
    Artifact(#[from] std::io::Error),
    #[error("self-signed certificate generation failed: {0}")]
    Certificate(String),
    #[error("TLS certificate could not be loaded: {0}")]
    Tls(String),
    #[error("sing-box configuration check failed: {0}")]
    Check(String),
    #[error(transparent)]
    Storage(#[from] ConfigError),
}

/// Regenerates the four cached artifacts from the canonical node model and
/// replaces them atomically under one operation lock. When `sing_box_bin` is
/// supplied the new server configuration is validated with `sing-box check`
/// before any file is replaced, so a failed check leaves every existing
/// artifact untouched. If any replacement fails mid-way, the already-replaced
/// files are restored to their previous complete versions. `update_active_config`
/// additionally re-syncs the active sing-box configuration consumed by the
/// managed service; reload/restart of the service is the caller's step.
pub fn regenerate(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    sing_box_bin: Option<&Path>,
    update_active_config: bool,
) -> Result<(), SubscriptionError> {
    let artifacts = generated_artifacts(config)?;
    if let Some(sing_box_bin) = sing_box_bin {
        let server = server_artifact(&artifacts)?;
        check_sing_box_config(sing_box_bin, server)?;
    }
    let _lock = store.acquire_operation_lock()?;
    let prior_artifacts = artifacts
        .iter()
        .map(|(name, _)| (*name, read_artifact(store, name)))
        .collect::<Vec<_>>();
    let prior_active = if update_active_config {
        fs::read(store.root().join(ACTIVE_CONFIG_RELATIVE_PATH)).ok()
    } else {
        None
    };
    for (name, contents) in &artifacts {
        if let Err(error) = store.write_artifact_locked(name, contents.as_bytes()) {
            restore_replaced(store, &prior_artifacts, prior_active.as_deref());
            return Err(SubscriptionError::Storage(error));
        }
    }
    if update_active_config {
        let server = server_artifact(&artifacts)?;
        if let Err(error) =
            store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, server.as_bytes())
        {
            restore_replaced(store, &prior_artifacts, prior_active.as_deref());
            return Err(SubscriptionError::Storage(error));
        }
    }
    Ok(())
}

fn server_artifact<'a>(
    artifacts: &'a [(&'static str, String)],
) -> Result<&'a str, SubscriptionError> {
    artifacts
        .iter()
        .find(|(name, _)| *name == SING_BOX_SERVER_ARTIFACT)
        .map(|(_, contents)| contents.as_str())
        .ok_or_else(|| {
            SubscriptionError::Check("no generated sing-box server configuration".to_owned())
        })
}

fn read_artifact(store: &DeploymentStore, name: &str) -> Option<Vec<u8>> {
    fs::read(store.root().join(ARTIFACTS_RELATIVE_DIR).join(name)).ok()
}

/// Best-effort rollback of already-replaced artifacts and the active
/// configuration after a mid-transaction write failure. Each write is atomic,
/// so a failed write leaves its own target on the previous complete version.
fn restore_replaced(
    store: &DeploymentStore,
    prior_artifacts: &[(&'static str, Option<Vec<u8>>)],
    prior_active: Option<&[u8]>,
) {
    for (name, prior) in prior_artifacts.iter().rev() {
        if let Some(prior) = prior {
            let _ = store.write_artifact_locked(name, prior);
        }
    }
    if let Some(prior_active) = prior_active {
        let _ = store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, prior_active);
    }
}

/// The prior complete versions of every file a configuration transaction can
/// replace, used to restore the previous known-good deployment after a failed
/// service health check.
pub struct DeploymentSnapshot {
    pub config: Vec<u8>,
    pub artifacts: Vec<(&'static str, Option<Vec<u8>>)>,
    pub active_config: Option<Vec<u8>>,
}

/// Validates, then atomically replaces the deployment configuration together
/// with any changed canonical artifacts and the active sing-box configuration
/// under one operation lock. The generated server configuration is checked with
/// `sing-box check` before any file is replaced, so a failed check leaves every
/// existing file untouched. A configuration-only change (one that does not alter
/// the canonical node model) skips the check and the artifact writes. The
/// returned snapshot lets the caller restore the previous deployment if the
/// subsequent service health check fails.
pub fn apply_config_transaction(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    sing_box_bin: Option<&Path>,
) -> Result<DeploymentSnapshot, SubscriptionError> {
    config.validate()?;
    let artifacts = generated_artifacts(config)?;
    let server = server_artifact(&artifacts)?;
    let _lock = store.acquire_operation_lock()?;
    let prior_artifacts = artifacts
        .iter()
        .map(|(name, _)| (*name, read_artifact(store, name)))
        .collect::<Vec<_>>();
    let prior_active = fs::read(store.root().join(ACTIVE_CONFIG_RELATIVE_PATH)).ok();
    let prior_config = fs::read(store.root().join(crate::config::CONFIG_RELATIVE_PATH)).ok();

    let artifacts_changed = prior_artifacts.iter().any(|(name, prior)| {
        artifacts
            .iter()
            .find(|(artifact_name, _)| artifact_name == name)
            .is_none_or(|(_, contents)| prior.as_deref() != Some(contents.as_bytes()))
    });
    // A deployment that has no active sing-box configuration yet (configuration
    // initialized without installation) is not synced: writing the active file
    // is the installation step. Once present, it is re-synced whenever the
    // canonical node model changes or it drifted from the generated server.
    let need_active_sync = prior_active.is_some()
        && (artifacts_changed || prior_active.as_deref() != Some(server.as_bytes()));

    if artifacts_changed || need_active_sync {
        let Some(sing_box_bin) = sing_box_bin else {
            return Err(SubscriptionError::Check(
                "configuration change requires a sing-box binary for validation".to_owned(),
            ));
        };
        check_sing_box_config(sing_box_bin, server)?;
        for (name, contents) in &artifacts {
            if let Err(error) = store.write_artifact_locked(name, contents.as_bytes()) {
                restore_replaced(store, &prior_artifacts, prior_active.as_deref());
                return Err(SubscriptionError::Storage(error));
            }
        }
        if need_active_sync
            && let Err(error) =
                store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, server.as_bytes())
        {
            restore_replaced(store, &prior_artifacts, prior_active.as_deref());
            return Err(SubscriptionError::Storage(error));
        }
    }
    store.replace_locked(config)?;
    Ok(DeploymentSnapshot {
        config: prior_config.unwrap_or_default(),
        artifacts: prior_artifacts,
        active_config: prior_active,
    })
}

/// Restores a previously captured deployment snapshot after a failed service
/// health check, then restarts the managed services to return the running
/// deployment to the previous known-good configuration.
pub fn restore_config_transaction(
    store: &DeploymentStore,
    snapshot: &DeploymentSnapshot,
) -> Result<(), SubscriptionError> {
    let _lock = store.acquire_operation_lock()?;
    for (name, prior) in snapshot.artifacts.iter().rev() {
        match prior {
            Some(prior) => store.write_artifact_locked(name, prior)?,
            None => {
                let _ = fs::remove_file(store.root().join(ARTIFACTS_RELATIVE_DIR).join(name));
            }
        }
    }
    if let Some(active) = &snapshot.active_config {
        store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, active)?;
    }
    store.write_relative_locked(crate::config::CONFIG_RELATIVE_PATH, &snapshot.config)?;
    Ok(())
}

pub fn generated_artifacts(
    config: &DeploymentConfig,
) -> Result<Vec<(&'static str, String)>, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let nodes = crate::canonical::nodes(config);
    Ok(vec![
        (SING_BOX_SERVER_ARTIFACT, sing_box_server(config, &nodes)?),
        (SING_BOX_ARTIFACT, sing_box(config, &nodes)?),
        (CLASH_ARTIFACT, clash(config, &nodes)?),
        (URI_ARTIFACT, uri(config, &nodes)?),
    ])
}

pub fn check_sing_box_config(
    sing_box_binary: &Path,
    config: &str,
) -> Result<(), SubscriptionError> {
    let mut temporary = tempfile::NamedTempFile::new().map_err(SubscriptionError::Artifact)?;
    temporary
        .write_all(config.as_bytes())
        .map_err(SubscriptionError::Artifact)?;
    let status = Command::new(sing_box_binary)
        .args(["check", "-c"])
        .arg(temporary.path())
        .status()
        .map_err(SubscriptionError::Artifact)?;
    if status.success() {
        Ok(())
    } else {
        Err(SubscriptionError::Check(format!(
            "sing-box check exited with {status}"
        )))
    }
}

pub fn read_authorized(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    credential: &str,
    format: SubscriptionFormat,
) -> Result<String, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    if !constant_time_eq(
        credential.as_bytes(),
        config.subscription_credential.as_bytes(),
    ) {
        return Err(SubscriptionError::InvalidCredential);
    }
    Ok(String::from_utf8_lossy(&fs::read(
        store
            .root()
            .join("var/lib/sbctl/artifacts")
            .join(format.artifact_name()),
    )?)
    .into_owned())
}

pub fn subscription_url(
    config: &DeploymentConfig,
    format: SubscriptionFormat,
) -> Result<String, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let prefix = match config.subscription_mode {
        SubscriptionMode::IpFallback => format!(
            "http://{}:{}",
            config.subscription_host,
            config.http_port.expect("validated IP fallback port")
        ),
        SubscriptionMode::Direct | SubscriptionMode::ExternalProxy => {
            format!("https://{}", config.subscription_host)
        }
    };
    Ok(format!(
        "{prefix}/sub/{}/{}",
        config.subscription_credential,
        format.path_name()
    ))
}

pub async fn serve(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    bind: &str,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let store = Arc::new(store.clone());
    let config = Arc::new(config.clone());
    if config.subscription_mode == SubscriptionMode::Direct {
        return serve_direct_socket_activated(&store, &config, max_requests).await;
    }
    if config.subscription_mode == SubscriptionMode::ExternalProxy
        && !bind
            .parse::<SocketAddr>()
            .ok()
            .is_some_and(|address| address.ip().is_loopback())
    {
        return Err(SubscriptionError::ExternalProxyBind);
    }
    let listener = TcpListener::bind(bind).await.map_err(listener_io)?;
    serve_http_listener(listener, &store, &config, max_requests).await
}

/// Direct subscription mode never binds 80/443 itself. systemd owns those
/// listeners through `sbctl-http.socket` and hands them to this process via
/// `LISTEN_FDS`; the two sockets are routed by their local port so TCP 80
/// serves the ACME challenge and TCP 443 serves the TLS subscription.
async fn serve_direct_socket_activated(
    store: &Arc<DeploymentStore>,
    config: &Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let listeners = crate::socket_activation::receive_listeners()
        .map_err(|error| SubscriptionError::SocketActivation(error.to_string()))?;
    let mut acme = None;
    let mut tls = None;
    for (port, listener) in listeners {
        match crate::socket_activation::direct_listener_role(port) {
            Some(crate::socket_activation::DirectListenerRole::Acme) => acme = Some(listener),
            Some(crate::socket_activation::DirectListenerRole::Tls) => tls = Some(listener),
            None => return Err(SubscriptionError::UnexpectedDirectListener(port)),
        }
    }
    let acme = tokio_listener(acme.ok_or(SubscriptionError::MissingDirectListener(80))?)?;
    let tls = tokio_listener(tls.ok_or(SubscriptionError::MissingDirectListener(443))?)?;
    tokio::try_join!(
        serve_acme_listener(acme, Arc::clone(store), max_requests),
        serve_tls_listener(tls, Arc::clone(store), Arc::clone(config), max_requests)
    )?;
    Ok(())
}

fn tokio_listener(listener: std::net::TcpListener) -> Result<TcpListener, SubscriptionError> {
    listener
        .set_nonblocking(true)
        .map_err(|error| SubscriptionError::ListenerIo(error.to_string()))?;
    TcpListener::from_std(listener)
        .map_err(|error| SubscriptionError::ListenerIo(error.to_string()))
}

/// The shared Hyper HTTP/1 builder: a bounded header size, a slow-read
/// timeout, and a Tokio timer so the timeout applies.
fn http1_builder() -> hyper::server::conn::http1::Builder {
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder.max_buf_size(MAX_REQUEST_HEADER_BYTES);
    builder.timer(hyper_util::rt::TokioTimer::new());
    builder.header_read_timeout(MAX_HEADER_READ_TIME);
    builder
}

/// Bounds applied to every HTTP connection so an oversized request header, a
/// slow reader, an idle client, or connection flooding cannot exhaust the
/// process. Responses set `Connection: close`, so each request is its own
/// connection and hyper never keeps an idle connection alive.
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_READ_TIME: Duration = Duration::from_secs(5);
const MAX_CONNECTION_TIME: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_CONNECTIONS: usize = 32;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Accepts the next connection, or returns `None` after a short poll when a
/// test-configured `max_requests` limit may have been reached by a task that
/// is already serving. Production operation (`max_requests == None`) blocks on
/// the accept until a connection arrives.
async fn accept_next(
    listener: &TcpListener,
    max_requests: Option<usize>,
) -> Result<Option<tokio::net::TcpStream>, SubscriptionError> {
    if max_requests.is_none() {
        let (stream, _) = listener.accept().await.map_err(listener_io)?;
        return Ok(Some(stream));
    }
    match tokio::time::timeout(ACCEPT_POLL_INTERVAL, listener.accept()).await {
        Ok(Ok((stream, _))) => Ok(Some(stream)),
        Ok(Err(error)) => Err(listener_io(error)),
        Err(_) => Ok(None),
    }
}

fn listener_io(error: std::io::Error) -> SubscriptionError {
    SubscriptionError::ListenerIo(error.to_string())
}

/// Accepts connections from one listener, bounding concurrency with a
/// semaphore and each connection's lifetime with a timeout. Serves at most
/// `max_requests` connections when a test supplies that limit.
async fn serve_http_listener(
    listener: TcpListener,
    store: &Arc<DeploymentStore>,
    config: &Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let store = Arc::clone(store);
        let config = Arc::clone(config);
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tokio::time::timeout(
                MAX_CONNECTION_TIME,
                serve_http_connection(TokioIo::new(stream), store, config),
            )
            .await;
            counter.fetch_add(1, Ordering::Release);
        });
    }
    Ok(())
}

/// Serves ACME HTTP-01 challenge responses from the listener on TCP 80 with
/// the same bounded connection handling as the subscription listener.
async fn serve_acme_listener(
    listener: TcpListener,
    store: Arc<DeploymentStore>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let store = Arc::clone(&store);
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tokio::time::timeout(
                MAX_CONNECTION_TIME,
                serve_acme_connection(TokioIo::new(stream), store),
            )
            .await;
            counter.fetch_add(1, Ordering::Release);
        });
    }
    Ok(())
}

/// Serves the TLS subscription listener on TCP 443. The certificate is
/// reloaded before every accepted connection, so a Certbot renewal takes
/// effect on the next handshake without signalling or restarting the service.
async fn serve_tls_listener(
    listener: TcpListener,
    store: Arc<DeploymentStore>,
    config: Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    let mut tls = None;
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        match load_tls_config(&store, &config) {
            Ok(reloaded) => tls = Some(reloaded),
            Err(error) => {
                // A TLS-terminating listener cannot emit an HTTP 5xx: the
                // certificate is needed before the first HTTP byte. The failure
                // is instead diagnosed with a redacted log line, and the last
                // known-good configuration keeps serving until a valid
                // certificate is pinned again.
                eprintln!(
                    "Direct HTTPS certificate unavailable; connection dropped: {}",
                    redact_secret(&error.to_string(), &config.subscription_credential)
                )
            }
        }
        let Some(tls) = tls.clone() else {
            drop(stream);
            continue;
        };
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let store = Arc::clone(&store);
        let config = Arc::clone(&config);
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let _permit = permit;
            let acceptor = TlsAcceptor::from(tls);
            let Ok(stream) = acceptor.accept(stream).await else {
                counter.fetch_add(1, Ordering::Release);
                return;
            };
            let _ = tokio::time::timeout(
                MAX_CONNECTION_TIME,
                serve_http_connection(TokioIo::new(Box::pin(stream)), store, config),
            )
            .await;
            counter.fetch_add(1, Ordering::Release);
        });
    }
    Ok(())
}

async fn serve_http_connection<S>(
    io: TokioIo<S>,
    store: Arc<DeploymentStore>,
    config: Arc<DeploymentConfig>,
) -> Result<(), SubscriptionError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let response = subscription_http_response(request, &store, &config);
        async { Ok::<_, std::convert::Infallible>(response) }
    });
    http1_builder()
        .serve_connection(io, service)
        .await
        .map_err(|error| SubscriptionError::Http(error.to_string()))
}

async fn serve_acme_connection<S>(
    io: TokioIo<S>,
    store: Arc<DeploymentStore>,
) -> Result<(), SubscriptionError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let response = acme_http_response(request, &store);
        async { Ok::<_, std::convert::Infallible>(response) }
    });
    http1_builder()
        .serve_connection(io, service)
        .await
        .map_err(|error| SubscriptionError::Http(error.to_string()))
}

fn acme_http_response(
    request: Request<Incoming>,
    store: &DeploymentStore,
) -> Response<Full<Bytes>> {
    let body = request
        .uri()
        .path()
        .strip_prefix("/.well-known/acme-challenge/")
        .filter(|token| !token.is_empty() && !token.contains('/') && !token.contains('?'))
        .and_then(|token| {
            fs::read_to_string(
                store
                    .acme_webroot()
                    .join(".well-known/acme-challenge")
                    .join(token),
            )
            .ok()
        });
    match body {
        Some(body) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Cache-Control", "no-store")
            .header("Connection", "close")
            .body(Full::new(Bytes::from(body)))
            .expect("valid ACME response"),
        None => not_found_http_response(),
    }
}

fn subscription_http_response(
    request: Request<Incoming>,
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Response<Full<Bytes>> {
    if request.method() != Method::GET || request.uri().query().is_some() {
        return not_found_http_response();
    }
    let Some((credential, format)) = parse_route(request.uri().path()) else {
        return not_found_http_response();
    };
    if !constant_time_eq(
        credential.as_bytes(),
        config.subscription_credential.as_bytes(),
    ) {
        return not_found_http_response();
    }
    let body = match read_authorized(store, config, credential, format) {
        Ok(body) => body,
        Err(error) => return unavailable_http_response(credential, &error.to_string()),
    };
    let traffic = match crate::traffic::report(store, config) {
        Ok(traffic) => traffic,
        Err(error) => return unavailable_http_response(credential, &error.to_string()),
    };
    // subscription-userinfo follows the common client convention: upload and
    // download are the bytes used in the current period, while `total` is the
    // configured monthly allowance. Keep the historical used-total value when
    // no allowance is configured so unlimited deployments remain informative.
    let quota = if traffic.monthly_traffic_limit > 0 {
        traffic.monthly_traffic_limit
    } else {
        traffic.total()
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", format.content_type())
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header(
            "subscription-userinfo",
            format!(
                "upload={}; download={}; total={}; expire={}",
                traffic.transmitted,
                traffic.received,
                quota,
                traffic.next_reset.timestamp()
            ),
        )
        .header("Connection", "close")
        .body(Full::new(Bytes::from(body)))
        .expect("valid subscription response")
}

/// Loads and validates the pinned certificate for Direct HTTPS. Every loading
/// check — validity period, SAN coverage, private-key match — runs before the
/// TLS acceptor is built, and the acceptor refuses connections whose SNI does
/// not equal the subscription host. The daemon reloads before every handshake,
/// so a Certbot renewal pinned by the deploy hook takes effect on the next
/// connection without a service restart.
fn load_tls_config(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<Arc<rustls::ServerConfig>, SubscriptionError> {
    crate::certificate::load_pinned(store, config)
        .map_err(|error| SubscriptionError::Tls(error.to_string()))
        .and_then(|validated| {
            validated
                .server_config()
                .map_err(|error| SubscriptionError::Tls(error.to_string()))
        })
}

/// Replaces every occurrence of a Subscription credential in a diagnostic so
/// logs and errors never expose the full secret. ADR-0013.
pub fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    text.replace(secret, "[redacted]")
}

/// A redacted 503 for state or artifact failures after a valid Subscription
/// credential authenticated. The body carries no authorization or deployment
/// details; the diagnostic log omits the credential.
fn unavailable_http_response(credential: &str, message: &str) -> Response<Full<Bytes>> {
    eprintln!(
        "subscription request failed: {}",
        redact_secret(message, credential)
    );
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header("Cache-Control", "no-store")
        .header("Connection", "close")
        .body(Full::new(Bytes::new()))
        .expect("valid unavailable response")
}

fn not_found_http_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Cache-Control", "no-store")
        .header("Connection", "close")
        .body(Full::new(Bytes::new()))
        .expect("valid not-found response")
}

fn parse_route(target: &str) -> Option<(&str, SubscriptionFormat)> {
    if target.contains('?') {
        return None;
    }
    let mut parts = target.strip_prefix("/sub/")?.split('/');
    let credential = parts.next()?;
    let format = match parts.next()? {
        "sing-box.json" => SubscriptionFormat::SingBox,
        "clash.yaml" => SubscriptionFormat::Clash,
        "uri" => SubscriptionFormat::Uri,
        _ => return None,
    };
    parts.next().is_none().then_some((credential, format))
}

fn ensure_subscription_nodes(config: &DeploymentConfig) -> Result<(), SubscriptionError> {
    if !config
        .enabled_protocols
        .iter()
        .any(ManagedProtocol::has_generated_subscription_artifacts)
    {
        return Err(SubscriptionError::MissingNodes);
    }
    Ok(())
}

pub fn ensure_external_proxy_listener_available(port: u16) -> Result<(), SubscriptionError> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map(drop)
        .map_err(|_| SubscriptionError::ListenerUnavailable(port))
}

fn sing_box(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let skip_verify = client_skip_cert_verify(config);
    let mut outbounds = Vec::new();
    for node in nodes {
        outbounds.push(match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => json!({"type": "vless", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "flow": "xtls-rprx-vision",
                "tls": {"enabled": true, "server_name": decoy_sni, "utls": {"enabled": true, "fingerprint": "chrome"},
                    "reality": {"enabled": true, "public_key": public_key, "short_id": short_id}}}),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => json!({"type": "vmess", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "security": "auto", "alter_id": 0,
                "transport": {"type": "ws", "path": path},
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify}}),
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => json!({"type": "hysteria2", "tag": node.tag(), "server": host,
                "server_port": port, "password": password,
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify,
                    "alpn": ["h3"]}}),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => json!({"type": "tuic", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "password": password,
                "congestion_control": "bbr", "udp_relay_mode": "native",
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify,
                    "alpn": ["h3"]}}),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => json!({"type": "anytls", "tag": node.tag(), "server": host,
                "server_port": port, "password": password,
                "idle_session_check_interval": "30s", "idle_session_timeout": "30s",
                "min_idle_session": 5,
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify}}),
        });
    }
    Ok(
        serde_json::to_string_pretty(&json!({"outbounds": outbounds}))
            .expect("JSON values serialize"),
    )
}

fn sing_box_server(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let certificate = certificate_tls_config(config)?;
    let mut inbounds = Vec::new();
    for node in nodes {
        inbounds.push(match &node {
            CanonicalNode::VlessReality {
                port,
                uuid,
                private_key,
                short_id,
                decoy_sni,
                ..
            } => json!({"type": "vless", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "flow": "xtls-rprx-vision"}],
                "tls": {"enabled": true, "server_name": decoy_sni, "reality": {"enabled": true,
                    "handshake": {"server": decoy_sni, "server_port": 443}, "private_key": private_key,
                    "short_id": [short_id]}}}),
            CanonicalNode::VmessWebsocket {
                port,
                tls_server_name,
                uuid,
                path,
                ..
            } => json!({"type": "vmess", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "alterId": 0}],
                "transport": {"type": "ws", "path": path},
                "tls": server_tls(tls_server_name, &certificate, &[])}),
            CanonicalNode::Hysteria2 {
                port,
                tls_server_name,
                password,
                ..
            } => json!({"type": "hysteria2", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"password": password}],
                "tls": server_tls(tls_server_name, &certificate, &["h3"])}),
            CanonicalNode::Tuic {
                port,
                tls_server_name,
                uuid,
                password,
                ..
            } => json!({"type": "tuic", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "password": password}],
                "tls": server_tls(tls_server_name, &certificate, &["h3"])}),
            CanonicalNode::Anytls {
                port,
                tls_server_name,
                password,
                ..
            } => json!({"type": "anytls", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"password": password}],
                "tls": server_tls(tls_server_name, &certificate, &[])}),
        });
    }
    Ok(
        serde_json::to_string_pretty(&json!({"inbounds": inbounds}))
            .expect("JSON values serialize"),
    )
}

fn clash(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
    let skip = client_skip_cert_verify(config);
    let mut proxies = String::from("proxies:\n");
    for node in nodes {
        let entry = match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => format!(
                "  - name: {}\n    type: vless\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    network: tcp\n    udp: true\n    flow: xtls-rprx-vision\n    tls: true\n    servername: {decoy_sni}\n    client-fingerprint: chrome\n    reality-opts:\n      public-key: {public_key}\n      short-id: {short_id}\n",
                node.tag()
            ),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => format!(
                "  - name: {}\n    type: vmess\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    alterId: 0\n    cipher: auto\n    tls: true\n    servername: {tls_server_name}\n    skip-cert-verify: {skip}\n    network: ws\n    ws-opts:\n      path: {path}\n      headers:\n        Host: {tls_server_name}\n",
                node.tag()
            ),
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => format!(
                "  - name: {}\n    type: hysteria2\n    server: {host}\n    port: {port}\n    password: {password}\n    sni: {tls_server_name}\n    skip-cert-verify: {skip}\n",
                node.tag()
            ),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => format!(
                "  - name: {}\n    type: tuic\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    password: {password}\n    sni: {tls_server_name}\n    alpn:\n      - h3\n    skip-cert-verify: {skip}\n",
                node.tag()
            ),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => format!(
                "  - name: {}\n    type: anytls\n    server: {host}\n    port: {port}\n    password: {password}\n    client-fingerprint: chrome\n    udp: true\n    idle-session-check-interval: 30\n    idle-session-timeout: 30\n    tls: true\n    sni: {tls_server_name}\n    skip-cert-verify: {skip}\n",
                node.tag()
            ),
        };
        proxies.push_str(&entry);
    }
    Ok(proxies)
}

fn uri(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    for node in nodes {
        match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => uris.push_str(&format!("vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={decoy_sni}&fp=chrome&pbk={public_key}&sid={short_id}&type=tcp#{}\n", node.tag())),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => {
                let payload = json!({"v": "2", "ps": node.tag(), "add": host, "port": port.to_string(), "id": uuid, "aid": "0", "scy": "auto", "net": "ws", "type": "none", "host": tls_server_name, "path": path, "tls": "tls", "sni": tls_server_name});
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&payload).expect("JSON values serialize"));
                uris.push_str(&format!("vmess://{encoded}\n"));
            }
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "hysteria2://{password}@{host}:{port}?insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{uuid}:{password}@{host}:{port}?congestion_control=bbr&alpn=h3&insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{password}@{host}:{port}?security=tls&insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
        }
    }
    Ok(uris)
}

/// The certificate path written into the sing-box server configuration for the
/// TLS-terminating Managed protocols. Direct subscription mode uses the pinned
/// copy that the deploy hook grants to the `sbctl` and `sing-box` accounts.
/// External proxy mode leaves certificate management entirely to the existing
/// reverse proxy and its own Certbot setup.
/// The Managed protocol listeners present this certificate to their clients.
/// `SelfSigned` mode generates a long-lived self-signed certificate (sing-box-yg
/// style, never expires, no ACME dependency) that clients are told to skip
/// verifying; `Domain` mode uses the administrator-managed certificate.
fn certificate_tls_config(config: &DeploymentConfig) -> Result<Value, SubscriptionError> {
    let (certificate_path, key_path) = match config.certificate_mode {
        CertificateMode::SelfSigned => ensure_self_signed_certificate(config)?,
        CertificateMode::Domain => {
            if config.subscription_mode == SubscriptionMode::Direct {
                let directory = crate::config::DeploymentStore::certificate_directory_absolute(
                    &config.subscription_host,
                );
                (
                    directory
                        .join("fullchain.pem")
                        .to_string_lossy()
                        .into_owned(),
                    directory.join("privkey.pem").to_string_lossy().into_owned(),
                )
            } else {
                (
                    format!(
                        "/etc/letsencrypt/live/{}/fullchain.pem",
                        config.subscription_host
                    ),
                    format!(
                        "/etc/letsencrypt/live/{}/privkey.pem",
                        config.subscription_host
                    ),
                )
            }
        }
    };
    Ok(
        json!({"enabled": true, "server_name": config.subscription_host,
        "certificate_path": certificate_path,
        "key_path": key_path}),
    )
}

/// Generates and pins a long-lived self-signed certificate for the subscription
/// host, or reuses the pinned copy. The certificate stays valid for 36500 days,
/// matching the sing-box-yg default, so the proxy listeners never break on an
/// expired administrator-managed certificate.
fn ensure_self_signed_certificate(
    config: &DeploymentConfig,
) -> Result<(String, String), SubscriptionError> {
    let directory =
        Path::new(crate::config::CERTIFICATES_ABSOLUTE_PATH).join(&config.subscription_host);
    let certificate_path = directory.join("cert.pem");
    let key_path = directory.join("key.pem");
    if certificate_path.is_file() && key_path.is_file() {
        return Ok((
            certificate_path.to_string_lossy().into_owned(),
            key_path.to_string_lossy().into_owned(),
        ));
    }
    let key_pair =
        KeyPair::generate().map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    let mut params = CertificateParams::new(vec![config.subscription_host.clone()])
        .map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    params
        .distinguished_name
        .push(DnType::CommonName, config.subscription_host.clone());
    params
        .distinguished_name
        .push(DnType::OrganizationName, "sbctl");
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    fs::create_dir_all(&directory).map_err(SubscriptionError::Artifact)?;
    fs::write(&certificate_path, certificate.pem()).map_err(SubscriptionError::Artifact)?;
    fs::write(&key_path, key_pair.serialize_pem()).map_err(SubscriptionError::Artifact)?;
    Ok((
        certificate_path.to_string_lossy().into_owned(),
        key_path.to_string_lossy().into_owned(),
    ))
}

/// Clients connecting to a self-signed certificate must be told to skip
/// verification; the domain certificate is verified normally.
fn client_skip_cert_verify(config: &DeploymentConfig) -> bool {
    config.certificate_mode == CertificateMode::SelfSigned
}

fn server_tls(tls_server_name: &str, certificate: &Value, alpn: &[&str]) -> Value {
    let mut tls = json!({"enabled": true, "server_name": tls_server_name,
        "certificate_path": certificate["certificate_path"],
        "key_path": certificate["key_path"]});
    if !alpn.is_empty() {
        tls["alpn"] = json!(alpn);
    }
    tls
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        different |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    different == 0
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{generated_artifacts, regenerate};
    use crate::config::{DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode};

    async fn http_get(port: u16, path: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("subscription service accepts connections");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .expect("request is sent");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("response is readable");
        response
    }

    /// Establishes the minimal traffic fixture so a subscription response can
    /// read a legal accounting state and the generated URI artifact.
    fn seed_direct_subscription(fixture: &TempDir) -> (DeploymentStore, DeploymentConfig, String) {
        let statistics = fixture.path().join("sys/class/net/ens3/statistics");
        fs::create_dir_all(&statistics).expect("statistics directory is created");
        fs::write(statistics.join("rx_bytes"), "100\n").expect("RX counter is written");
        fs::write(statistics.join("tx_bytes"), "200\n").expect("TX counter is written");
        let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
        fs::create_dir_all(boot_path.parent().expect("boot ID has a parent"))
            .expect("boot ID directory is created");
        fs::write(boot_path, "boot-a").expect("boot ID is written");
        let store = DeploymentStore::new(fixture.path());
        let config = DeploymentConfig::new(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("a Direct VLESS deployment is valid");
        let artifacts = generated_artifacts(&config).expect("artifacts generate");
        let references = artifacts
            .iter()
            .map(|(name, contents)| (*name, contents.as_bytes()))
            .collect::<Vec<_>>();
        store
            .initialize_with_artifacts(&config, &references)
            .expect("subscription deployment is initialized");
        crate::traffic::reset(&store, &config).expect("accounting state is established");
        let credential = config.subscription_credential.clone();
        (store, config, credential)
    }

    #[tokio::test]
    async fn acme_listener_serves_the_challenge_and_rejects_every_other_path() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let challenge = store.acme_webroot().join(".well-known/acme-challenge");
        fs::create_dir_all(&challenge).expect("challenge directory is created");
        fs::write(challenge.join("token-1"), "challenge-body").expect("challenge is written");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_acme_listener(
            listener,
            Arc::new(store),
            Some(1),
        ));

        let served = http_get(port, "/.well-known/acme-challenge/token-1").await;
        assert!(served.starts_with("HTTP/1.1 200 OK"), "challenge is served");
        assert!(served.contains("challenge-body"));
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn acme_listener_returns_404_for_a_foreign_or_malformed_challenge_path() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_acme_listener(
            listener,
            Arc::new(store),
            Some(3),
        ));

        let missing = http_get(port, "/.well-known/acme-challenge/unknown").await;
        assert!(missing.starts_with("HTTP/1.1 404 Not Found"));
        let traversal = http_get(port, "/.well-known/acme-challenge/../config.toml").await;
        assert!(traversal.starts_with("HTTP/1.1 404 Not Found"));
        let wrong_root = http_get(port, "/sub/anything/uri").await;
        assert!(wrong_root.starts_with("HTTP/1.1 404 Not Found"));
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_serves_the_subscription_after_a_real_handshake() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, credential) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store),
            Arc::new(config),
            Some(1),
        ));

        let response = tls_get(port, &format!("/sub/{credential}/uri")).await;
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "TLS subscription is served"
        );
        assert!(response.contains("vless://"));
        assert!(response.contains("subscription-userinfo:"));
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_rejects_a_handshake_whose_sni_is_not_the_subscription_host() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, _) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store),
            Arc::new(config),
            Some(1),
        ));

        let handshake = tls_handshake_sni(port, "attacker.example.test").await;
        assert!(
            handshake.is_err(),
            "an SNI mismatch is rejected before any HTTP request"
        );
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_rejects_a_handshake_without_an_sni() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, _) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store),
            Arc::new(config),
            Some(1),
        ));

        let handshake = tls_handshake_sni(port, "").await;
        assert!(
            handshake.is_err(),
            "a missing SNI is rejected before any HTTP request"
        );
        handler.await.expect("handler completes").expect("no error");
    }

    /// Writes a valid certificate into the Certbot live directory and pins it
    /// into the sbctl-owned copy that the daemon actually serves.
    fn seed_direct_certificate(
        fixture: &TempDir,
        store: &DeploymentStore,
        config: &DeploymentConfig,
    ) {
        let certificate_directory = fixture.path().join("etc/letsencrypt/live/sub.example.test");
        fs::create_dir_all(&certificate_directory).expect("certificate directory is created");
        let certificate = rcgen::generate_simple_self_signed(vec!["sub.example.test".into()])
            .expect("a self-signed certificate is generated");
        fs::write(
            certificate_directory.join("fullchain.pem"),
            certificate.cert.pem(),
        )
        .expect("fullchain is written");
        fs::write(
            certificate_directory.join("privkey.pem"),
            certificate.signing_key.serialize_pem(),
        )
        .expect("private key is written");
        let validated =
            crate::certificate::load(store, config).expect("the fixture certificate is valid");
        crate::certificate::pin(store, config, &validated)
            .expect("the certificate is pinned for the daemon");
    }

    /// Opens a TLS connection that accepts any certificate and returns the
    /// response to a single GET request. Certificates are verified separately
    /// by the deploy hook and the certificate ticket; this test exercises the
    /// listener's TLS termination path, not certificate trust.
    async fn tls_get(port: u16, path: &str) -> String {
        let mut stream = tls_connect(port, "sub.example.test")
            .await
            .expect("TLS handshake completes");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: sub.example.test\r\n\r\n").as_bytes())
            .await
            .expect("request is sent");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("response is readable");
        response
    }

    /// Opens a TLS connection with a caller-supplied SNI and returns whether
    /// the handshake completed. An empty `sni` connects without a DNS SNI (an
    /// IP server name is used, which rustls omits from the ClientHello).
    async fn tls_handshake_sni(port: u16, sni: &str) -> Result<(), std::io::Error> {
        tls_connect(port, sni).await.map(|_| ())
    }

    async fn tls_connect(
        port: u16,
        sni: &str,
    ) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, std::io::Error> {
        use rustls::client::danger::{
            HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
        };
        use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
        use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
        use tokio_rustls::TlsConnector;

        #[derive(Debug)]
        struct AcceptsEverything;
        impl ServerCertVerifier for AcceptsEverything {
            fn verify_server_cert(
                &self,
                _end_entity: &CertificateDer<'_>,
                _intermediates: &[CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: UnixTime,
            ) -> Result<ServerCertVerified, rustls::Error> {
                Ok(ServerCertVerified::assertion())
            }
            fn verify_tls12_signature(
                &self,
                _message: &[u8],
                _cert: &CertificateDer<'_>,
                _dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn verify_tls13_signature(
                &self,
                _message: &[u8],
                _cert: &CertificateDer<'_>,
                _dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                vec![
                    SignatureScheme::ECDSA_NISTP256_SHA256,
                    SignatureScheme::ECDSA_NISTP384_SHA384,
                    SignatureScheme::ED25519,
                    SignatureScheme::RSA_PSS_SHA256,
                    SignatureScheme::RSA_PSS_SHA384,
                    SignatureScheme::RSA_PSS_SHA512,
                    SignatureScheme::RSA_PKCS1_SHA256,
                    SignatureScheme::RSA_PKCS1_SHA384,
                    SignatureScheme::RSA_PKCS1_SHA512,
                ]
            }
        }

        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptsEverything))
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("TLS listener accepts connections");
        let server_name = if sni.is_empty() {
            ServerName::try_from("203.0.113.7".to_owned()).expect("a valid IP server name")
        } else {
            ServerName::try_from(sni.to_owned()).expect("valid server name")
        };
        connector.connect(server_name, stream).await
    }

    fn checker(fixture: &TempDir, accepts: bool) -> PathBuf {
        let path = fixture.path().join("sing-box-check");
        fs::write(
            &path,
            if accepts {
                "#!/bin/sh\nexit 0\n"
            } else {
                "#!/bin/sh\nexit 1\n"
            },
        )
        .expect("checker is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("checker is executable");
        }
        path
    }

    fn vless_config() -> DeploymentConfig {
        DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an IP fallback VLESS deployment is valid")
    }

    fn write_old_artifacts(store: &DeploymentStore) {
        for (name, contents) in [
            ("sing-box-server.json", "old server".as_bytes()),
            ("subscription-sing-box.json", "old sing-box".as_bytes()),
            ("subscription-clash.yaml", "old clash".as_bytes()),
            ("subscription-uri.txt", "old uri".as_bytes()),
        ] {
            store
                .write_artifact(name, contents)
                .expect("an old artifact is committed");
        }
    }

    fn artifact(store: &DeploymentStore, name: &str) -> Vec<u8> {
        fs::read(store.root().join("var/lib/sbctl/artifacts").join(name))
            .expect("artifact is readable")
    }

    #[test]
    fn regenerate_with_a_failed_check_leaves_artifacts_and_active_config_unchanged() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        store
            .write_relative_locked("etc/sing-box/config.json", b"old active config")
            .expect("old active config is committed");
        let rejecting = checker(&fixture, false);

        let result = regenerate(&store, &vless_config(), Some(&rejecting), true);
        assert!(
            result.is_err(),
            "a rejected check must fail the regeneration"
        );
        for (name, old) in [
            ("sing-box-server.json", "old server".as_bytes()),
            ("subscription-sing-box.json", "old sing-box".as_bytes()),
            ("subscription-clash.yaml", "old clash".as_bytes()),
            ("subscription-uri.txt", "old uri".as_bytes()),
        ] {
            assert_eq!(
                artifact(&store, name),
                old,
                "{name} stays on the old complete version"
            );
        }
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            b"old active config"
        );
    }

    #[test]
    fn regenerate_with_a_passing_check_replaces_all_artifacts_and_active_config() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        store
            .write_relative_locked("etc/sing-box/config.json", b"old active config")
            .expect("old active config is committed");
        let config = vless_config();
        let accepting = checker(&fixture, true);

        regenerate(&store, &config, Some(&accepting), true)
            .expect("a passing check allows the regeneration");
        let expected = generated_artifacts(&config).expect("new artifacts are generated");
        for (name, contents) in &expected {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is replaced by the complete new version"
            );
        }
        let server = expected
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents)
            .expect("server artifact is present");
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            server.as_bytes(),
            "the active sing-box configuration is re-synced"
        );
    }

    #[test]
    fn regenerate_without_active_config_sync_leaves_it_untouched() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        store
            .write_relative_locked("etc/sing-box/config.json", b"old active config")
            .expect("old active config is committed");
        let accepting = checker(&fixture, true);

        regenerate(&store, &vless_config(), Some(&accepting), false)
            .expect("artifacts are regenerated without the active config");
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            b"old active config"
        );
    }

    #[test]
    fn regenerate_restores_earlier_artifacts_when_a_later_replacement_fails() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        let accepting = checker(&fixture, true);

        let blocked = store
            .root()
            .join("var/lib/sbctl/artifacts/subscription-uri.txt");
        fs::remove_file(&blocked).expect("blocked artifact is removed");
        fs::create_dir(&blocked).expect("blocked artifact is replaced by a directory");

        let result = regenerate(&store, &vless_config(), Some(&accepting), true);
        assert!(result.is_err(), "a blocked artifact fails the regeneration");
        assert_eq!(
            artifact(&store, "sing-box-server.json"),
            "old server".as_bytes(),
            "an earlier replaced artifact is restored after a later write failure"
        );
        assert_eq!(
            artifact(&store, "subscription-sing-box.json"),
            "old sing-box".as_bytes(),
            "an earlier replaced artifact is restored after a later write failure"
        );
    }

    #[test]
    fn redact_secret_replaces_every_occurrence_of_the_credential() {
        let secret = "deadbeef-credential";
        let message = format!("subscription artifact failed: {secret}; retry with {secret}");
        assert_eq!(
            super::redact_secret(&message, secret),
            "subscription artifact failed: [redacted]; retry with [redacted]"
        );
    }

    #[test]
    fn redact_secret_leaves_unrelated_text_untouched() {
        assert_eq!(
            super::redact_secret("subscription artifact failed: no such file", "secret"),
            "subscription artifact failed: no such file"
        );
    }

    fn write_initial_deployment(store: &DeploymentStore, config: &DeploymentConfig) {
        let artifacts = generated_artifacts(config).expect("artifacts generate");
        let references = artifacts
            .iter()
            .map(|(name, contents)| (*name, contents.as_bytes()))
            .collect::<Vec<_>>();
        store
            .initialize_with_artifacts(config, &references)
            .expect("initial deployment is written");
        let server = artifacts
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents.as_bytes())
            .expect("server artifact exists");
        store
            .write_relative_locked("etc/sing-box/config.json", server)
            .expect("active config is written");
    }

    fn persisted_config(store: &DeploymentStore) -> Vec<u8> {
        fs::read(store.root().join("etc/sbctl/config.toml")).expect("config is readable")
    }

    #[test]
    fn apply_config_transaction_with_a_failed_check_leaves_everything_unchanged() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let mut new = old.clone();
        new.subscription_host = "198.51.100.9".into();
        let rejecting = checker(&fixture, false);

        let result = super::apply_config_transaction(&store, &new, Some(&rejecting));

        assert!(
            result.is_err(),
            "a rejected check must fail the transaction"
        );
        let expected = generated_artifacts(&old).expect("old artifacts are generated");
        for (name, contents) in &expected {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} stays on the old complete version"
            );
        }
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&old)
                .expect("old config serializes")
                .as_bytes()
        );
    }

    #[test]
    fn apply_config_transaction_replaces_config_artifacts_and_active_config_together() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let mut new = old.clone();
        new.subscription_host = "198.51.100.9".into();
        let accepting = checker(&fixture, true);

        let snapshot =
            super::apply_config_transaction(&store, &new, Some(&accepting)).expect("transaction");

        let expected = generated_artifacts(&new).expect("new artifacts are generated");
        for (name, contents) in &expected {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is replaced by the new complete version"
            );
        }
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&new)
                .expect("new config serializes")
                .as_bytes()
        );
        let server = expected
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents.as_bytes())
            .expect("server artifact exists");
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            server
        );
        assert_eq!(
            snapshot.config,
            toml::to_string_pretty(&old)
                .expect("old serializes")
                .as_bytes()
                .to_vec()
        );
    }

    #[test]
    fn apply_config_transaction_skips_the_check_for_a_config_only_change() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let artifacts_before = generated_artifacts(&old).expect("old artifacts are generated");
        let active_before = fs::read(store.root().join("etc/sing-box/config.json"))
            .expect("active config is readable");
        let mut new = old.clone();
        new.monthly_traffic_limit = 1_000_000;

        super::apply_config_transaction(&store, &new, None)
            .expect("config-only change needs no check");

        for (name, contents) in &artifacts_before {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is untouched by a config-only change"
            );
        }
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            active_before
        );
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&new)
                .expect("new config serializes")
                .as_bytes()
        );
    }

    #[test]
    fn restore_config_transaction_returns_the_previous_deployment() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let mut new = old.clone();
        new.subscription_host = "198.51.100.9".into();
        let accepting = checker(&fixture, true);

        let snapshot =
            super::apply_config_transaction(&store, &new, Some(&accepting)).expect("transaction");
        super::restore_config_transaction(&store, &snapshot).expect("restore succeeds");

        let old_artifacts = generated_artifacts(&old).expect("old artifacts are generated");
        for (name, contents) in &old_artifacts {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is restored"
            );
        }
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&old)
                .expect("old config serializes")
                .as_bytes()
        );
    }
}
