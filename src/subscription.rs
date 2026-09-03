use base64::Engine;
use serde_json::{Value, json};
use std::fs;
use std::io::BufReader;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::canonical::CanonicalNode;
use crate::config::{
    ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode,
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
    #[error("no subscription-capable Managed protocol is enabled")]
    MissingNodes,
    #[error("invalid subscription credential")]
    InvalidCredential,
    #[error("subscription artifact is unavailable: {0}")]
    Artifact(#[from] std::io::Error),
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
        if let Err(error) = store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, server.as_bytes()) {
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
        .ok_or_else(|| SubscriptionError::Check("no generated sing-box server configuration".to_owned()))
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

pub fn generated_artifacts(
    config: &DeploymentConfig,
) -> Result<Vec<(&'static str, String)>, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let nodes = crate::canonical::nodes(config);
    Ok(vec![
        (SING_BOX_SERVER_ARTIFACT, sing_box_server(config, &nodes)?),
        (SING_BOX_ARTIFACT, sing_box(&nodes)?),
        (CLASH_ARTIFACT, clash(&nodes)?),
        (URI_ARTIFACT, uri(&nodes)?),
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
    if config.subscription_mode == SubscriptionMode::Direct {
        return serve_direct(store, config).await;
    }
    if config.subscription_mode == SubscriptionMode::ExternalProxy
        && !bind
            .parse::<SocketAddr>()
            .ok()
            .is_some_and(|address| address.ip().is_loopback())
    {
        return Err(SubscriptionError::ExternalProxyBind);
    }
    let listener = TcpListener::bind(bind).await?;
    for _ in 0..max_requests.unwrap_or(usize::MAX) {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0_u8; 8192];
        let length = stream.read(&mut request).await?;
        let target = request_target(&request[..length]);
        let response = subscription_response(store, config, target);
        stream.write_all(response.as_bytes()).await?;
    }
    Ok(())
}

async fn serve_direct(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<(), SubscriptionError> {
    let http = match TcpListener::bind("[::]:80").await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind("0.0.0.0:80").await?,
    };
    let https = match TcpListener::bind("[::]:443").await {
        Ok(listener) => listener,
        Err(_) => TcpListener::bind("0.0.0.0:443").await?,
    };
    tokio::try_join!(
        serve_acme_webroot(store, http),
        serve_tls(store, config, https, None)
    )?;
    Ok(())
}

async fn serve_acme_webroot(
    store: &DeploymentStore,
    listener: TcpListener,
) -> Result<(), SubscriptionError> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0_u8; 8192];
        let length = stream.read(&mut request).await?;
        let target = request_target(&request[..length]);
        let body = target
            .and_then(|target| target.strip_prefix("/.well-known/acme-challenge/"))
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
        let response = body.map(|body| format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)).unwrap_or_else(not_found_response);
        stream.write_all(response.as_bytes()).await?;
    }
}

async fn serve_tls(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    listener: TcpListener,
    mut tls: Option<Arc<rustls::ServerConfig>>,
) -> Result<(), SubscriptionError> {
    loop {
        let (stream, _) = listener.accept().await?;
        if let Ok(reloaded) = load_tls_config(store, config) {
            tls = Some(reloaded);
        }
        let Some(tls) = &tls else {
            continue;
        };
        let acceptor = TlsAcceptor::from(Arc::clone(tls));
        let Ok(mut stream) = acceptor.accept(stream).await else {
            continue;
        };
        let mut request = [0_u8; 8192];
        if let Ok(length) = stream.read(&mut request).await {
            let target = request_target(&request[..length]);
            let _ = stream
                .write_all(subscription_response(store, config, target).as_bytes())
                .await;
        }
    }
}

fn request_target(request: &[u8]) -> Option<&str> {
    std::str::from_utf8(request)
        .ok()
        .and_then(|request| request.lines().next())
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.split_once(' '))
        .map(|(target, _)| target)
}

fn load_tls_config(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<Arc<rustls::ServerConfig>, SubscriptionError> {
    let directory = store
        .root()
        .join("etc/letsencrypt/live")
        .join(&config.subscription_host);
    let mut certificates = BufReader::new(fs::File::open(directory.join("fullchain.pem"))?);
    let certificates = rustls_pemfile::certs(&mut certificates)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| SubscriptionError::Tls(error.to_string()))?;
    let mut key = BufReader::new(fs::File::open(directory.join("privkey.pem"))?);
    let key = rustls_pemfile::private_key(&mut key)
        .map_err(|error| SubscriptionError::Tls(error.to_string()))?
        .ok_or_else(|| SubscriptionError::Tls("private key file contains no key".to_owned()))?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .map(Arc::new)
        .map_err(|error| SubscriptionError::Tls(error.to_string()))
}

fn subscription_response(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    target: Option<&str>,
) -> String {
    let Some((credential, format)) = target.and_then(parse_route) else {
        return not_found_response();
    };
    if !constant_time_eq(
        credential.as_bytes(),
        config.subscription_credential.as_bytes(),
    ) {
        return not_found_response();
    }
    let body = match read_authorized(store, config, credential, format) {
        Ok(body) => body,
        Err(error) => return unavailable_response(credential, &error.to_string()),
    };
    let traffic = match crate::traffic::report(store, config) {
        Ok(traffic) => traffic,
        Err(error) => return unavailable_response(credential, &error.to_string()),
    };
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nsubscription-userinfo: upload={}; download={}; total={}; expire={}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        format.content_type(),
        traffic.transmitted,
        traffic.received,
        traffic.total(),
        traffic.next_reset.timestamp(),
        body.len(),
        body
    )
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
fn unavailable_response(credential: &str, message: &str) -> String {
    eprintln!(
        "subscription request failed: {}",
        redact_secret(message, credential)
    );
    "HTTP/1.1 503 Service Unavailable\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        .to_owned()
}

fn not_found_response() -> String {
    "HTTP/1.1 404 Not Found\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
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

fn sing_box(nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
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
                "tls": {"enabled": true, "server_name": tls_server_name}}),
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => json!({"type": "hysteria2", "tag": node.tag(), "server": host,
                "server_port": port, "password": password,
                "tls": {"enabled": true, "server_name": tls_server_name}}),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => json!({"type": "tuic", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "password": password,
                "tls": {"enabled": true, "server_name": tls_server_name}}),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => json!({"type": "anytls", "tag": node.tag(), "server": host,
                "server_port": port, "password": password,
                "tls": {"enabled": true, "server_name": tls_server_name}}),
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
    let certificate = certificate_tls_config(config);
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
                "tls": {"enabled": true, "reality": {"enabled": true,
                    "handshake": {"server": decoy_sni, "server_port": 443}, "private_key": private_key,
                    "short_id": [short_id]}}}),
            CanonicalNode::VmessWebsocket {
                port,
                tls_server_name,
                uuid,
                path,
                ..
            } => json!({"type": "vmess", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "alter_id": 0}],
                "transport": {"type": "ws", "path": path},
                "tls": server_tls(tls_server_name, &certificate)}),
            CanonicalNode::Hysteria2 {
                port,
                tls_server_name,
                password,
                ..
            } => json!({"type": "hysteria2", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"password": password}],
                "tls": server_tls(tls_server_name, &certificate)}),
            CanonicalNode::Tuic {
                port,
                tls_server_name,
                uuid,
                password,
                ..
            } => json!({"type": "tuic", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "password": password}],
                "tls": server_tls(tls_server_name, &certificate)}),
            CanonicalNode::Anytls {
                port,
                tls_server_name,
                password,
                ..
            } => json!({"type": "anytls", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"password": password}],
                "tls": server_tls(tls_server_name, &certificate)}),
        });
    }
    Ok(
        serde_json::to_string_pretty(&json!({"inbounds": inbounds}))
            .expect("JSON values serialize"),
    )
}

fn clash(nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
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
            } => format!("  - name: {}\n    type: vless\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    network: tcp\n    flow: xtls-rprx-vision\n    tls: true\n    servername: {decoy_sni}\n    client-fingerprint: chrome\n    reality-opts:\n      public-key: {public_key}\n      short-id: {short_id}\n", node.tag()),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => format!("  - name: {}\n    type: vmess\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    alterId: 0\n    cipher: auto\n    tls: true\n    servername: {tls_server_name}\n    network: ws\n    ws-opts:\n      path: {path}\n      headers:\n        Host: {tls_server_name}\n", node.tag()),
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => format!("  - name: {}\n    type: hysteria2\n    server: {host}\n    port: {port}\n    password: {password}\n    sni: {tls_server_name}\n    skip-cert-verify: false\n", node.tag()),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => format!("  - name: {}\n    type: tuic\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    password: {password}\n    sni: {tls_server_name}\n    alpn:\n      - h3\n    skip-cert-verify: false\n", node.tag()),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => format!("  - name: {}\n    type: anytls\n    server: {host}\n    port: {port}\n    password: {password}\n    tls: true\n    sni: {tls_server_name}\n    skip-cert-verify: false\n", node.tag()),
        };
        proxies.push_str(&entry);
    }
    Ok(proxies)
}

fn uri(nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
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
                "hysteria2://{password}@{host}:{port}?insecure=0&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{uuid}:{password}@{host}:{port}?congestion_control=bbr&alpn=h3&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{password}@{host}:{port}?security=tls&sni={tls_server_name}#{}\n",
                node.tag()
            )),
        }
    }
    Ok(uris)
}

fn certificate_tls_config(config: &DeploymentConfig) -> Value {
    json!({"enabled": true, "server_name": config.subscription_host,
        "certificate_path": format!("/etc/letsencrypt/live/{}/fullchain.pem", config.subscription_host),
        "key_path": format!("/etc/letsencrypt/live/{}/privkey.pem", config.subscription_host)})
}

fn server_tls(tls_server_name: &str, certificate: &Value) -> Value {
    json!({"enabled": true, "server_name": tls_server_name,
        "certificate_path": certificate["certificate_path"],
        "key_path": certificate["key_path"]})
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

    use tempfile::TempDir;

    use super::{generated_artifacts, regenerate};
    use crate::config::{
        DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode,
    };

    fn checker(fixture: &TempDir, accepts: bool) -> PathBuf {
        let path = fixture.path().join("sing-box-check");
        fs::write(
            &path,
            if accepts { "#!/bin/sh\nexit 0\n" } else { "#!/bin/sh\nexit 1\n" },
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
        assert!(result.is_err(), "a rejected check must fail the regeneration");
        for (name, old) in [
            ("sing-box-server.json", "old server".as_bytes()),
            ("subscription-sing-box.json", "old sing-box".as_bytes()),
            ("subscription-clash.yaml", "old clash".as_bytes()),
            ("subscription-uri.txt", "old uri".as_bytes()),
        ] {
            assert_eq!(artifact(&store, name), old, "{name} stays on the old complete version");
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
}
