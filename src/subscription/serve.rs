use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;

use super::artifacts::{SubscriptionError, read_authorized, subscription_url};
use super::constant_time_eq;
use super::profile::{
    CLASH_LEGACY_VERSION, ClientVersion, SING_BOX_VERSION_PROFILES, SubscriptionFormat,
    SubscriptionRoute,
};
use super::{DeploymentConfig, DeploymentStore, SubscriptionMode, ensure_subscription_nodes};

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

/// Serves the TLS subscription listener on TCP 443. The certificate is reloaded
/// whenever the pinned material changes, so a Certbot renewal takes effect on
/// the next handshake without signalling or restarting the service — without
/// re-parsing the pair for every accepted connection.
async fn serve_tls_listener(
    listener: TcpListener,
    store: Arc<DeploymentStore>,
    config: Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    let mut tls = None;
    let mut tls_stamp = None;
    let mut tls_failure: Option<(String, std::time::Instant)> = None;
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        let stamp = crate::certificate::pinned_material_stamp(&store, &config);
        if tls.is_none() || Some(stamp) != tls_stamp {
            match load_tls_config(&store, &config) {
                Ok(reloaded) => {
                    tls = Some(reloaded);
                    tls_stamp = Some(stamp);
                }
                Err(error) => {
                    // A TLS-terminating listener cannot emit an HTTP 5xx: the
                    // certificate is needed before the first HTTP byte. The
                    // failure is instead diagnosed with a redacted log line, and
                    // the last known-good configuration keeps serving until a
                    // valid certificate is pinned again.
                    tls_stamp = None;
                    let message =
                        redact_secret(&error.to_string(), &config.subscription_credential);
                    // One line per accepted connection turns a broken
                    // certificate into a flood that buries everything else, so
                    // repeat at most once a minute — or at once when the reason
                    // changes.
                    let due = match &tls_failure {
                        None => true,
                        Some((previous, at)) => {
                            *previous != message || at.elapsed() >= Duration::from_secs(60)
                        }
                    };
                    if due {
                        eprintln!(
                            "Direct HTTPS certificate unavailable; connection dropped: {message}"
                        );
                        tls_failure = Some((message, std::time::Instant::now()));
                    }
                }
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
    if request.method() != Method::GET {
        // A probe with HEAD or POST is a client asking about the route, not an
        // attacker: answering 404 tells it the subscription disappeared, while
        // 405 tells it to retry with GET.
        return method_not_allowed_http_response();
    }
    if request.uri().query().is_some() {
        return not_found_http_response();
    }
    let Some((credential, route)) = parse_route(request.uri().path()) else {
        return not_found_http_response();
    };
    if !constant_time_eq(
        credential.as_bytes(),
        config.subscription_credential.as_bytes(),
    ) {
        return not_found_http_response();
    }
    match route {
        SubscriptionRoute::Qr(format) => qr_http_response(config, credential, format),
        SubscriptionRoute::Index => index_http_response(store, config, credential),
        SubscriptionRoute::Format(format) => {
            let body = match read_authorized(store, config, credential, format) {
                Ok(body) => body,
                Err(error) => return unavailable_http_response(credential, &error.to_string()),
            };
            // The subscription-userinfo header is an addition to the artifact,
            // not a precondition: a broken or mid-repair accounting state must
            // not take the subscription itself offline. The failure is logged
            // redacted and the artifact is served without traffic metadata.
            let userinfo = match crate::traffic::report(store, config) {
                Ok(traffic) => {
                    // subscription-userinfo follows the common client convention:
                    // upload and download are the bytes used in the current period,
                    // while `total` is the configured monthly allowance. Keep the
                    // historical used-total value when no allowance is configured so
                    // unlimited deployments remain informative.
                    let quota = if traffic.monthly_traffic_limit > 0 {
                        traffic.monthly_traffic_limit
                    } else {
                        traffic.total()
                    };
                    Some(format!(
                        "upload={}; download={}; total={}; expire={}",
                        traffic.transmitted,
                        traffic.received,
                        quota,
                        traffic.next_reset.timestamp()
                    ))
                }
                Err(error) => {
                    eprintln!(
                        "subscription traffic metadata unavailable: {}",
                        redact_secret(&error.to_string(), credential)
                    );
                    None
                }
            };
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", format.content_type())
                .header("Cache-Control", "no-store")
                .header("X-Content-Type-Options", "nosniff")
                .header("Connection", "close");
            if let Some(value) = &userinfo {
                builder = builder.header("subscription-userinfo", value);
            }
            builder
                .body(Full::new(Bytes::from(body)))
                .expect("valid subscription response")
        }
    }
}

/// A scannable SVG QR code of the given format's subscription URL. The QR
/// content is derived from the configuration only, so no artifact file is
/// needed and the code always encodes the current URL.
fn qr_http_response(
    config: &DeploymentConfig,
    credential: &str,
    format: SubscriptionFormat,
) -> Response<Full<Bytes>> {
    let body = match subscription_url(config, format)
        .map_err(|error| error.to_string())
        .and_then(|url| crate::qr::render_svg(&url))
    {
        Ok(body) => body,
        Err(error) => return unavailable_http_response(credential, &error),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/svg+xml")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Connection", "close")
        .body(Full::new(Bytes::from(body)))
        .expect("valid QR response")
}

/// The Chinese overview page: every subscription link with its label, the
/// matching QR code, and per-client import instructions. Self-contained HTML
/// with no external resources, so it renders offline and leaks nothing extra.
fn index_http_response(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    credential: &str,
) -> Response<Full<Bytes>> {
    let body = match crate::index_page::render(store, config) {
        Ok(body) => body,
        Err(error) => return unavailable_http_response(credential, &error.to_string()),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Connection", "close")
        .body(Full::new(Bytes::from(body)))
        .expect("valid index response")
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

fn method_not_allowed_http_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header("Allow", "GET")
        .header("Cache-Control", "no-store")
        .header("Connection", "close")
        .body(Full::new(Bytes::new()))
        .expect("valid method-not-allowed response")
}

fn parse_route(target: &str) -> Option<(&str, SubscriptionRoute)> {
    if target.contains('?') {
        return None;
    }
    let mut parts = target.strip_prefix("/sub/")?.split('/');
    let credential = parts.next()?;
    let route = match parts.next()? {
        "index" => SubscriptionRoute::Index,
        // The trailing check below rejects anything after the format segment.
        "qr" => SubscriptionRoute::Qr(parse_format_path(parts.next()?)?),
        segment => SubscriptionRoute::Format(parse_format_path(segment)?),
    };
    parts.next().is_none().then_some((credential, route))
}

/// Parses one subscription format path segment. Versioned segments are only
/// accepted when the version exists in the profile registry, so unknown
/// versions 404 instead of surfacing as a missing artifact.
fn parse_format_path(segment: &str) -> Option<SubscriptionFormat> {
    match segment {
        "sing-box.json" => return Some(SubscriptionFormat::SingBox),
        "sing-box-full.json" => return Some(SubscriptionFormat::SingBoxFull),
        "clash.yaml" => return Some(SubscriptionFormat::Clash),
        "uri" => return Some(SubscriptionFormat::Uri),
        "uri.txt" => return Some(SubscriptionFormat::Base64Uri),
        "shadowrocket.txt" => return Some(SubscriptionFormat::Shadowrocket),
        _ => {}
    }
    if let Some(version) = segment
        .strip_prefix("sing-box-")
        .and_then(|rest| rest.strip_suffix(".json"))
    {
        let version = parse_client_version(version)?;
        return SING_BOX_VERSION_PROFILES
            .iter()
            .any(|profile| profile.version == version)
            .then_some(SubscriptionFormat::SingBoxVersion(version));
    }
    if let Some(version) = segment
        .strip_prefix("clash-")
        .and_then(|rest| rest.strip_suffix(".yaml"))
    {
        let version = parse_client_version(version)?;
        return (version == CLASH_LEGACY_VERSION)
            .then_some(SubscriptionFormat::ClashLegacy(version));
    }
    None
}

fn parse_client_version(text: &str) -> Option<ClientVersion> {
    let (major, minor) = text.split_once('.')?;
    Some(ClientVersion::new(major.parse().ok()?, minor.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use std::fs;
    use std::sync::Arc;

    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::config::{DeploymentConfig, DeploymentStore};
    use crate::subscription::test_support::seed_direct_subscription;

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

    #[test]
    fn parse_route_maps_every_matrix_route_and_rejects_malformed_paths() {
        use super::{SubscriptionFormat, SubscriptionRoute, parse_route};
        let credential = "cred";
        let cases = [
            (
                "sing-box.json",
                SubscriptionRoute::Format(SubscriptionFormat::SingBox),
            ),
            (
                "sing-box-full.json",
                SubscriptionRoute::Format(SubscriptionFormat::SingBoxFull),
            ),
            (
                "clash.yaml",
                SubscriptionRoute::Format(SubscriptionFormat::Clash),
            ),
            (
                "clash-1.18.yaml",
                SubscriptionRoute::Format(SubscriptionFormat::ClashLegacy(
                    super::CLASH_LEGACY_VERSION,
                )),
            ),
            ("uri", SubscriptionRoute::Format(SubscriptionFormat::Uri)),
            (
                "uri.txt",
                SubscriptionRoute::Format(SubscriptionFormat::Base64Uri),
            ),
            (
                "shadowrocket.txt",
                SubscriptionRoute::Format(SubscriptionFormat::Shadowrocket),
            ),
            ("qr/uri", SubscriptionRoute::Qr(SubscriptionFormat::Uri)),
            (
                "qr/sing-box-full.json",
                SubscriptionRoute::Qr(SubscriptionFormat::SingBoxFull),
            ),
            ("index", SubscriptionRoute::Index),
        ];
        for (path, route) in cases {
            let target = format!("/sub/{credential}/{path}");
            assert_eq!(
                parse_route(&target),
                Some((credential, route)),
                "route {path} must parse"
            );
        }
        for profile in super::SING_BOX_VERSION_PROFILES {
            let target = format!("/sub/{credential}/sing-box-{}.json", profile.version);
            assert_eq!(
                parse_route(&target),
                Some((
                    credential,
                    SubscriptionRoute::Format(SubscriptionFormat::SingBoxVersion(profile.version)),
                )),
                "version profile {} must parse",
                profile.version
            );
        }
    }

    #[test]
    fn parse_route_rejects_query_unknown_and_trailing_paths() {
        use super::parse_route;
        for target in [
            "/sub/cred/uri?credential=cred",
            "/sub/cred/bogus",
            "/sub/cred/sing-box-1.09.json",
            "/sub/cred/clash-1.17.yaml",
            "/sub/cred/uri/extra",
            "/sub/cred/qr",
            "/sub/cred/qr/index",
            "/sub/cred",
            "/sub/",
            "/other/cred/uri",
        ] {
            assert!(parse_route(target).is_none(), "must reject {target}");
        }
        // An empty credential parses but can never match the real one, so the
        // handler still returns a uniform 404 before reading any artifact.
        assert_eq!(
            parse_route("/sub//uri"),
            Some((
                "",
                super::SubscriptionRoute::Format(super::SubscriptionFormat::Uri)
            ))
        );
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
    async fn a_broken_accounting_state_degrades_the_userinfo_header_not_the_subscription() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, credential) = seed_direct_subscription(&fixture);
        store
            .write_state(b"not json")
            .expect("the accounting state is corrupted");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let store = Arc::new(store.clone());
        let config = Arc::new(config);
        let handler = tokio::spawn(async move {
            super::serve_http_listener(listener, &store, &config, Some(1)).await
        });

        let response = http_get(port, &format!("/sub/{credential}/uri")).await;
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "the subscription must survive a broken accounting state: {response}"
        );
        assert!(
            !response.contains("subscription-userinfo:"),
            "the degraded response must not carry traffic metadata"
        );
        assert!(
            response.contains("vless://"),
            "the artifact body still serves"
        );
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_serves_base64_uri_with_the_standard_traffic_headers() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, credential) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store.clone()),
            Arc::new(config),
            Some(1),
        ));

        let response = tls_get(port, &format!("/sub/{credential}/uri.txt")).await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("content-type: text/plain; charset=utf-8"));
        assert!(response.contains("subscription-userinfo:"));
        let (_, body) = response
            .split_once("\r\n\r\n")
            .expect("the response separates headers and body");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(body.trim())
                .expect("the response body is standard Base64"),
            fs::read(
                store
                    .root()
                    .join("var/lib/sbctl/artifacts/subscription-uri.txt")
            )
            .expect("the canonical URI artifact is readable")
        );
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
}
