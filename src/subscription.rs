use serde_json::json;
use std::fs;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::config::{
    ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode,
};

const SING_BOX_ARTIFACT: &str = "subscription-sing-box.json";
const CLASH_ARTIFACT: &str = "subscription-clash.yaml";
const URI_ARTIFACT: &str = "subscription-uri.txt";
const SING_BOX_SERVER_ARTIFACT: &str = "sing-box-vless-reality.json";

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
    #[error("subscription is available only in IP fallback mode")]
    NotIpFallback,
    #[error("VLESS Reality is not enabled")]
    MissingVlessReality,
    #[error("invalid subscription credential")]
    InvalidCredential,
    #[error("subscription artifact is unavailable: {0}")]
    Artifact(#[from] std::io::Error),
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
    ensure_vless_ip_fallback(config)?;
    Ok(vec![
        (SING_BOX_SERVER_ARTIFACT, sing_box_server(config)?),
        (SING_BOX_ARTIFACT, sing_box(config)?),
        (CLASH_ARTIFACT, clash(config)?),
        (URI_ARTIFACT, uri(config)?),
    ])
}

pub fn read_authorized(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    credential: &str,
    format: SubscriptionFormat,
) -> Result<String, SubscriptionError> {
    ensure_vless_ip_fallback(config)?;
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
    ensure_vless_ip_fallback(config)?;
    Ok(format!(
        "http://{}:{}/sub/{}/{}",
        config.subscription_host,
        config.http_port.expect("validated IP fallback port"),
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
    ensure_vless_ip_fallback(config)?;
    let listener = TcpListener::bind(bind).await?;
    for _ in 0..max_requests.unwrap_or(usize::MAX) {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0_u8; 8192];
        let length = stream.read(&mut request).await?;
        let target = std::str::from_utf8(&request[..length])
            .ok()
            .and_then(|request| request.lines().next())
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|line| line.split_once(' '))
            .map(|(target, _)| target);
        let response = target
            .and_then(|target| parse_route(target))
            .and_then(|(credential, format)| {
                read_authorized(store, config, credential, format)
                    .ok()
                    .and_then(|body| crate::traffic::reconcile(store, config).ok().map(|traffic| (body, format, traffic)))
            })
            .map(|(body, format, traffic)| format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nsubscription-userinfo: upload={}; download={}; total={}; expire={}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                format.content_type(), traffic.received, traffic.transmitted, traffic.monthly_traffic_limit,
                traffic.next_reset.timestamp(), body.len(), body
            ))
            .unwrap_or_else(|| "HTTP/1.1 404 Not Found\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned());
        stream.write_all(response.as_bytes()).await?;
    }
    Ok(())
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

fn ensure_vless_ip_fallback(config: &DeploymentConfig) -> Result<(), SubscriptionError> {
    if config.subscription_mode != SubscriptionMode::IpFallback {
        return Err(SubscriptionError::NotIpFallback);
    }
    if !config
        .enabled_protocols
        .contains(&ManagedProtocol::VlessReality)
        || config.vless_reality.is_none()
    {
        return Err(SubscriptionError::MissingVlessReality);
    }
    Ok(())
}

fn reality_subscription_inputs(
    config: &DeploymentConfig,
) -> Result<(&str, &crate::config::VlessRealityCredentials, &str), SubscriptionError> {
    ensure_vless_ip_fallback(config)?;
    Ok((
        config
            .proxy_host
            .as_deref()
            .unwrap_or(&config.subscription_host),
        config.vless_reality.as_ref().expect("validated VLESS node"),
        config.reality_decoy_sni.as_deref().expect("validated SNI"),
    ))
}

fn sing_box(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let (host, node, sni) = reality_subscription_inputs(config)?;
    Ok(serde_json::to_string_pretty(&json!({"outbounds": [{
        "type": "vless", "tag": "sbctl-vless-reality", "server": host,
        "server_port": node.listen_port, "uuid": node.uuid, "flow": "xtls-rprx-vision",
        "tls": {"enabled": true, "server_name": sni, "utls": {"enabled": true, "fingerprint": "chrome"},
            "reality": {"enabled": true, "public_key": node.public_key, "short_id": node.short_id}}
    }]})).expect("JSON values serialize"))
}

fn sing_box_server(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let (_, node, sni) = reality_subscription_inputs(config)?;
    Ok(serde_json::to_string_pretty(&json!({"inbounds": [{
        "type": "vless", "tag": "sbctl-vless-reality", "listen": "::",
        "listen_port": node.listen_port,
        "users": [{"uuid": node.uuid, "flow": "xtls-rprx-vision"}],
        "tls": {"enabled": true, "reality": {"enabled": true,
            "handshake": {"server": sni, "server_port": 443},
            "private_key": node.private_key, "short_id": [node.short_id]}}
    }]}))
    .expect("JSON values serialize"))
}

fn clash(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let (host, node, sni) = reality_subscription_inputs(config)?;
    Ok(format!(
        "proxies:\n  - name: sbctl-vless-reality\n    type: vless\n    server: {host}\n    port: {}\n    uuid: {}\n    network: tcp\n    flow: xtls-rprx-vision\n    tls: true\n    servername: {sni}\n    client-fingerprint: chrome\n    reality-opts:\n      public-key: {}\n      short-id: {}\n",
        node.listen_port, node.uuid, node.public_key, node.short_id
    ))
}

fn uri(config: &DeploymentConfig) -> Result<String, SubscriptionError> {
    let (host, node, sni) = reality_subscription_inputs(config)?;
    Ok(format!(
        "vless://{}@{}:{}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={sni}&fp=chrome&pbk={}&sid={}&type=tcp#sbctl-vless-reality\n",
        node.uuid, host, node.listen_port, node.public_key, node.short_id
    ))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        different |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    different == 0
}
