use base64::Engine;
use serde_json::{Value, json};
use std::fs;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use crate::config::{
    ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode,
};

const SING_BOX_ARTIFACT: &str = "subscription-sing-box.json";
const CLASH_ARTIFACT: &str = "subscription-clash.yaml";
const URI_ARTIFACT: &str = "subscription-uri.txt";
const SING_BOX_SERVER_ARTIFACT: &str = "sing-box-server.json";

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
    #[error("subscription is unavailable in external reverse-proxy mode")]
    ExternalProxy,
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

pub fn regenerate(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<(), SubscriptionError> {
    for (name, contents) in generated_artifacts(config)? {
        store.write_artifact(name, contents.as_bytes())?;
    }
    Ok(())
}

pub fn generated_artifacts(
    config: &DeploymentConfig,
) -> Result<Vec<(&'static str, String)>, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    Ok(vec![
        (SING_BOX_SERVER_ARTIFACT, sing_box_server(config)?),
        (SING_BOX_ARTIFACT, sing_box(config)?),
        (CLASH_ARTIFACT, clash(config)?),
        (URI_ARTIFACT, uri(config)?),
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
    target
        .and_then(parse_route)
        .and_then(|(credential, format)| {
            read_authorized(store, config, credential, format)
                .ok()
                .and_then(|body| {
                    crate::traffic::reconcile(store, config)
                        .ok()
                        .map(|traffic| (body, format, traffic))
                })
        })
        .map(|(body, format, traffic)| {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nsubscription-userinfo: upload={}; download={}; total={}; expire={}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                format.content_type(),
                traffic.received,
                traffic.transmitted,
                traffic.monthly_traffic_limit,
                traffic.next_reset.timestamp(),
                body.len(),
                body
            )
        })
        .unwrap_or_else(not_found_response)
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
    if config.subscription_mode == SubscriptionMode::ExternalProxy {
        return Err(SubscriptionError::ExternalProxy);
    }
    if !config
        .enabled_protocols
        .iter()
        .any(ManagedProtocol::has_generated_subscription_artifacts)
    {
        return Err(SubscriptionError::MissingNodes);
    }
    Ok(())
}

fn sing_box(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let host = proxy_host(config);
    let mut outbounds = Vec::new();
    if let Some(node) = &config.vless_reality {
        let sni = config.reality_decoy_sni.as_deref().expect("validated SNI");
        outbounds.push(json!({"type": "vless", "tag": "sbctl-vless-reality", "server": host,
            "server_port": node.listen_port, "uuid": node.uuid, "flow": "xtls-rprx-vision",
            "tls": {"enabled": true, "server_name": sni, "utls": {"enabled": true, "fingerprint": "chrome"},
                "reality": {"enabled": true, "public_key": node.public_key, "short_id": node.short_id}}}));
    }
    if let Some(node) = &config.vmess_websocket {
        outbounds.push(
            json!({"type": "vmess", "tag": "sbctl-vmess-websocket", "server": host,
            "server_port": node.listen_port, "uuid": node.uuid, "security": "auto", "alter_id": 0,
            "transport": {"type": "ws", "path": node.path},
            "tls": {"enabled": true, "server_name": config.subscription_host}}),
        );
    }
    if let Some(node) = &config.hysteria2 {
        outbounds.push(
            json!({"type": "hysteria2", "tag": "sbctl-hysteria2", "server": host,
            "server_port": node.listen_port, "password": node.password,
            "tls": {"enabled": true, "server_name": config.subscription_host}}),
        );
    }
    if let Some(node) = &config.tuic {
        outbounds.push(json!({"type": "tuic", "tag": "sbctl-tuic", "server": host,
            "server_port": node.listen_port, "uuid": node.uuid, "password": node.password,
            "tls": {"enabled": true, "server_name": config.subscription_host}}));
    }
    if let Some(node) = &config.anytls {
        outbounds.push(
            json!({"type": "anytls", "tag": "sbctl-anytls", "server": host,
            "server_port": node.listen_port, "password": node.password,
            "tls": {"enabled": true, "server_name": config.subscription_host}}),
        );
    }
    Ok(
        serde_json::to_string_pretty(&json!({"outbounds": outbounds}))
            .expect("JSON values serialize"),
    )
}

fn sing_box_server(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let mut inbounds = Vec::new();
    if let Some(node) = &config.vless_reality {
        let sni = config.reality_decoy_sni.as_deref().expect("validated SNI");
        inbounds.push(json!({"type": "vless", "tag": "sbctl-vless-reality", "listen": "::",
            "listen_port": node.listen_port, "users": [{"uuid": node.uuid, "flow": "xtls-rprx-vision"}],
            "tls": {"enabled": true, "reality": {"enabled": true,
                "handshake": {"server": sni, "server_port": 443}, "private_key": node.private_key,
                "short_id": [node.short_id]}}}));
    }
    let certificate = certificate_tls_config(config);
    if let Some(node) = &config.vmess_websocket {
        inbounds.push(
            json!({"type": "vmess", "tag": "sbctl-vmess-websocket", "listen": "::",
            "listen_port": node.listen_port, "users": [{"uuid": node.uuid, "alter_id": 0}],
            "transport": {"type": "ws", "path": node.path}, "tls": certificate}),
        );
    }
    if let Some(node) = &config.hysteria2 {
        inbounds.push(json!({"type": "hysteria2", "tag": "sbctl-hysteria2", "listen": "::",
            "listen_port": node.listen_port, "users": [{"password": node.password}], "tls": certificate}));
    }
    if let Some(node) = &config.tuic {
        inbounds.push(json!({"type": "tuic", "tag": "sbctl-tuic", "listen": "::",
            "listen_port": node.listen_port, "users": [{"uuid": node.uuid, "password": node.password}],
            "tls": certificate}));
    }
    if let Some(node) = &config.anytls {
        inbounds.push(json!({"type": "anytls", "tag": "sbctl-anytls", "listen": "::",
            "listen_port": node.listen_port, "users": [{"password": node.password}], "tls": certificate}));
    }
    Ok(
        serde_json::to_string_pretty(&json!({"inbounds": inbounds}))
            .expect("JSON values serialize"),
    )
}

fn clash(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let host = proxy_host(config);
    let mut proxies = String::from("proxies:\n");
    if let Some(node) = &config.vless_reality {
        let sni = config.reality_decoy_sni.as_deref().expect("validated SNI");
        proxies.push_str(&format!("  - name: sbctl-vless-reality\n    type: vless\n    server: {host}\n    port: {}\n    uuid: {}\n    network: tcp\n    flow: xtls-rprx-vision\n    tls: true\n    servername: {sni}\n    client-fingerprint: chrome\n    reality-opts:\n      public-key: {}\n      short-id: {}\n", node.listen_port, node.uuid, node.public_key, node.short_id));
    }
    if let Some(node) = &config.vmess_websocket {
        proxies.push_str(&format!("  - name: sbctl-vmess-websocket\n    type: vmess\n    server: {host}\n    port: {}\n    uuid: {}\n    alterId: 0\n    cipher: auto\n    tls: true\n    servername: {}\n    network: ws\n    ws-opts:\n      path: {}\n      headers:\n        Host: {}\n", node.listen_port, node.uuid, config.subscription_host, node.path, config.subscription_host));
    }
    if let Some(node) = &config.hysteria2 {
        proxies.push_str(&format!("  - name: sbctl-hysteria2\n    type: hysteria2\n    server: {host}\n    port: {}\n    password: {}\n    sni: {}\n    skip-cert-verify: false\n", node.listen_port, node.password, config.subscription_host));
    }
    if let Some(node) = &config.tuic {
        proxies.push_str(&format!("  - name: sbctl-tuic\n    type: tuic\n    server: {host}\n    port: {}\n    uuid: {}\n    password: {}\n    sni: {}\n    alpn:\n      - h3\n    skip-cert-verify: false\n", node.listen_port, node.uuid, node.password, config.subscription_host));
    }
    if let Some(node) = &config.anytls {
        proxies.push_str(&format!("  - name: sbctl-anytls\n    type: anytls\n    server: {host}\n    port: {}\n    password: {}\n    tls: true\n    sni: {}\n    skip-cert-verify: false\n", node.listen_port, node.password, config.subscription_host));
    }
    Ok(proxies)
}

fn uri(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let host = proxy_host(config);
    let mut uris = String::new();
    if let Some(node) = &config.vless_reality {
        let sni = config.reality_decoy_sni.as_deref().expect("validated SNI");
        uris.push_str(&format!("vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={sni}&fp=chrome&pbk={}&sid={}&type=tcp#sbctl-vless-reality\n", node.uuid, host, node.listen_port, node.public_key, node.short_id));
    }
    if let Some(node) = &config.vmess_websocket {
        let payload = json!({"v": "2", "ps": "sbctl-vmess-websocket", "add": host, "port": node.listen_port.to_string(), "id": node.uuid, "aid": "0", "scy": "auto", "net": "ws", "type": "none", "host": config.subscription_host, "path": node.path, "tls": "tls", "sni": config.subscription_host});
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(&payload).expect("JSON values serialize"));
        uris.push_str(&format!("vmess://{encoded}\n"));
    }
    if let Some(node) = &config.hysteria2 {
        uris.push_str(&format!(
            "hysteria2://{}@{}:{}?insecure=0&sni={}#sbctl-hysteria2\n",
            node.password, host, node.listen_port, config.subscription_host
        ));
    }
    if let Some(node) = &config.tuic {
        uris.push_str(&format!(
            "tuic://{}:{}@{}:{}?congestion_control=bbr&alpn=h3&sni={}#sbctl-tuic\n",
            node.uuid, node.password, host, node.listen_port, config.subscription_host
        ));
    }
    if let Some(node) = &config.anytls {
        uris.push_str(&format!(
            "anytls://{}@{}:{}?security=tls&sni={}#sbctl-anytls\n",
            node.password, host, node.listen_port, config.subscription_host
        ));
    }
    Ok(uris)
}

fn proxy_host(config: &DeploymentConfig) -> &str {
    config
        .proxy_host
        .as_deref()
        .unwrap_or(&config.subscription_host)
}

fn certificate_tls_config(config: &DeploymentConfig) -> Value {
    json!({"enabled": true, "server_name": config.subscription_host,
        "certificate_path": format!("/etc/letsencrypt/live/{}/fullchain.pem", config.subscription_host),
        "key_path": format!("/etc/letsencrypt/live/{}/privkey.pem", config.subscription_host)})
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        different |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    different == 0
}
