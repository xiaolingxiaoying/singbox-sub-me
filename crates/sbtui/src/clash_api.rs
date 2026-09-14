//! Minimal clash_api client for the sing-box core started by this TUI.
//!
//! The server-side full client profile exposes
//! `experimental.clash_api.external_controller = 127.0.0.1:9090`, which is the
//! control channel for group selection, latency tests, traffic, and
//! connections — the same primitives a Clash dashboard uses.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTROLLER: &str = "http://127.0.0.1:9090";

pub struct ClashApi {
    base: String,
    client: reqwest::Client,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProxyGroup {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub now: String,
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProxyNode {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub now: String,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub delay: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Connection {
    pub id: String,
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub metadata: ConnectionMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ConnectionMetadata {
    #[serde(default)]
    pub network: String,
    #[serde(default, rename = "host")]
    pub destination_host: String,
    #[serde(default, rename = "destinationIP")]
    pub destination_ip: String,
    #[serde(default, rename = "destinationPort")]
    pub destination_port: String,
    #[serde(default)]
    pub inbound_ip: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ConnectionsSnapshot {
    #[serde(default)]
    pub upload_total: u64,
    #[serde(default)]
    pub download_total: u64,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboundMode {
    Rule,
    Global,
    Direct,
}

impl OutboundMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rule => "规则",
            Self::Global => "全局",
            Self::Direct => "直连",
        }
    }

    /// The clash_api wire name for PATCH /configs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Rule => Self::Global,
            Self::Global => Self::Direct,
            Self::Direct => Self::Rule,
        }
    }
}

impl ClashApi {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("clash_api client builds"),
        }
    }

    pub async fn alive(&self) -> bool {
        self.version().await.is_ok()
    }

    pub async fn version(&self) -> Result<String> {
        let value: serde_json::Value = self
            .client
            .get(format!("{}/version", self.base))
            .send()
            .await
            .context("clash_api version request failed")?
            .error_for_status()?
            .json()
            .await
            .context("clash_api version is not JSON")?;
        Ok(value
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned())
    }

    pub async fn proxies(&self) -> Result<(Vec<ProxyGroup>, Vec<ProxyNode>)> {
        let value: serde_json::Value = self
            .client
            .get(format!("{}/proxies", self.base))
            .send()
            .await
            .context("clash_api proxies request failed")?
            .error_for_status()?
            .json()
            .await
            .context("clash_api proxies is not JSON")?;
        let mut groups = Vec::new();
        let mut nodes = Vec::new();
        if let Some(map) = value.get("proxies").and_then(|v| v.as_object()) {
            for (name, entry) in map {
                let kind = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match kind {
                    "Selector" => {
                        let group: ProxyGroup =
                            serde_json::from_value(entry.clone()).unwrap_or_default();
                        groups.push(ProxyGroup {
                            name: name.clone(),
                            ..group
                        });
                    }
                    "URLTest" => {
                        let group: ProxyGroup =
                            serde_json::from_value(entry.clone()).unwrap_or_default();
                        groups.push(ProxyGroup {
                            name: name.clone(),
                            ..group
                        });
                    }
                    _ => {
                        let node: ProxyNode =
                            serde_json::from_value(entry.clone()).unwrap_or_default();
                        nodes.push(ProxyNode {
                            name: name.clone(),
                            ..node
                        });
                    }
                }
            }
        }
        nodes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok((groups, nodes))
    }

    /// Selects which member a selector group currently uses.
    pub async fn select(&self, group: &str, member: &str) -> Result<()> {
        self.client
            .put(format!("{}/proxies/{}", self.base, urlencoded(group)))
            .json(&serde_json::json!({ "name": member }))
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("selecting {member} in {group}"))?;
        Ok(())
    }

    /// Measures latency for one node through the core (returns milliseconds).
    pub async fn delay(&self, node: &str) -> Result<u64> {
        let url = format!(
            "{}/proxies/{}/delay?timeout=5000&url=http%3A%2F%2Faliyun.com%2Fgenerate_204",
            self.base,
            urlencoded(node)
        );
        let value: serde_json::Value = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("delay response is not JSON")?;
        value
            .get("delay")
            .and_then(|v| v.as_u64())
            .context("delay response lacks a delay value")
    }

    pub async fn mode(&self) -> Result<OutboundMode> {
        let value: serde_json::Value = self
            .client
            .get(format!("{}/configs", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        match value.get("mode").and_then(|v| v.as_str()) {
            Some("global") => Ok(OutboundMode::Global),
            Some("direct") => Ok(OutboundMode::Direct),
            _ => Ok(OutboundMode::Rule),
        }
    }

    /// Switches the core's outbound mode (rule / global / direct).
    pub async fn set_mode(&self, mode: OutboundMode) -> Result<()> {
        self.client
            .patch(format!("{}/configs", self.base))
            .json(&serde_json::json!({ "mode": mode.as_str() }))
            .send()
            .await?
            .error_for_status()
            .context("setting the outbound mode")?;
        Ok(())
    }

    pub async fn connections(&self) -> Result<ConnectionsSnapshot> {
        self.client
            .get(format!("{}/connections", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("clash_api connections is not JSON")
    }

    pub async fn close_connection(&self, id: &str) -> Result<()> {
        self.client
            .delete(format!("{}/connections/{}", self.base, urlencoded(id)))
            .send()
            .await?
            .error_for_status()
            .context("closing the connection")?;
        Ok(())
    }
}

fn urlencoded(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
