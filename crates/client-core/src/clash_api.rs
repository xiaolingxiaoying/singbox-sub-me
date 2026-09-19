//! Minimal clash_api client for the sing-box core started by this TUI.
//!
//! The server-side full client profile exposes
//! `experimental.clash_api.external_controller = 127.0.0.1:9090`, which is the
//! control channel for group selection, latency tests, traffic, and
//! connections — the same primitives a Clash dashboard uses.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTROLLER: &str = "http://127.0.0.1:9090";

/// The selector group tag the sbctl server-side client profile generates.
pub const SELECTOR_TAG: &str = "🚀节点选择";
/// The automatic latency group tag the server generates.
pub const AUTO_TAG: &str = "♻️自动选择";
/// The direct outbound tag the server generates.
pub const DIRECT_TAG: &str = "🎯直连";

/// The default latency probe: a tiny 204 endpoint reachable from mainland
/// China without a proxy, matching the server-side selector default.
pub const DEFAULT_TEST_URL: &str = "http://aliyun.com/generate_204";

#[derive(Clone)]
pub struct ClashApi {
    base: String,
    client: reqwest::Client,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProxyGroup {
    /// The map key supplies the name, so the entry itself has no `name` field.
    #[serde(default)]
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
    #[serde(default)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Connection {
    pub id: String,
    #[serde(default)]
    pub upload: u64,
    #[serde(default)]
    pub download: u64,
    #[serde(default)]
    pub start: String,
    /// The routing rule that matched, as reported by clash_api.
    #[serde(default)]
    pub rule: String,
    /// The outbound chain the connection traverses, outermost last.
    #[serde(default)]
    pub chains: Vec<String>,
    #[serde(default)]
    pub metadata: ConnectionMetadata,
}

impl Connection {
    /// Case-insensitive match over the fields a user would search for.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        if query.is_empty() {
            return true;
        }
        let haystack = format!(
            "{} {} {} {} {} {}",
            self.metadata.destination_host,
            self.metadata.destination_ip,
            self.metadata.destination_port,
            self.metadata.network,
            self.rule,
            self.chains.join(" ")
        )
        .to_lowercase();
        haystack.contains(&query)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConnectionsSnapshot {
    #[serde(default, rename = "uploadTotal")]
    pub upload_total: u64,
    #[serde(default, rename = "downloadTotal")]
    pub download_total: u64,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboundMode {
    #[default]
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
        self.delay_with(node, DEFAULT_TEST_URL).await
    }

    /// Measures latency against a caller-supplied probe URL.
    pub async fn delay_with(&self, node: &str, test_url: &str) -> Result<u64> {
        let url = format!(
            "{}/proxies/{}/delay?timeout=5000&url={}",
            self.base,
            urlencoded(node),
            urlencoded(test_url)
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

    /// Reads one sample from the `/memory` endpoint: the heap bytes the core
    /// currently uses. sing-box streams this the same way as `/traffic`, so
    /// only the first complete object is read.
    pub async fn memory(&self) -> Result<u64> {
        let mut response = self
            .client
            .get(format!("{}/memory", self.base))
            .send()
            .await?
            .error_for_status()
            .context("clash_api memory request failed")?;
        let mut buffer: Vec<u8> = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            match tokio::time::timeout_at(deadline, response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    buffer.extend_from_slice(&chunk);
                    if let Some(value) = first_json_object(&buffer) {
                        return value
                            .get("inuse")
                            .and_then(|v| v.as_u64())
                            .context("the memory response lacks an inuse value");
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => return Err(error).context("reading the memory stream"),
                Err(_) => break,
            }
        }
        first_json_object(&buffer)
            .and_then(|value| value.get("inuse").and_then(|v| v.as_u64()))
            .context("the memory stream produced no complete sample")
    }

    /// Reads the first sample from the streaming `/traffic` endpoint. The
    /// endpoint never closes, so only the first complete `{up,down}` object is
    /// read (bounded by a short deadline) instead of awaiting the whole body.
    pub async fn traffic(&self) -> Result<TrafficSample> {
        let mut response = self
            .client
            .get(format!("{}/traffic", self.base))
            .send()
            .await?
            .error_for_status()
            .context("clash_api traffic request failed")?;
        let mut buffer: Vec<u8> = Vec::new();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            match tokio::time::timeout_at(deadline, response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    buffer.extend_from_slice(&chunk);
                    if let Some(sample) = first_traffic_sample(&buffer) {
                        return Ok(sample);
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(error)) => return Err(error).context("reading the traffic stream"),
                Err(_) => break,
            }
        }
        first_traffic_sample(&buffer).context("the traffic stream produced no complete sample")
    }
}

/// One `{"up":N,"down":M}` sample from the `/traffic` stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrafficSample {
    pub up: u64,
    pub down: u64,
}

/// Extracts the first complete JSON object from a partial stream.
/// Tolerates leading whitespace and concatenated samples.
fn first_json_object(buffer: &[u8]) -> Option<serde_json::Value> {
    let text = std::str::from_utf8(buffer).ok()?;
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut end = None;
    for (offset, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + offset + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    serde_json::from_str(&text[start..end?]).ok()
}

/// Extracts the first complete JSON object from a partial stream and reads its
/// `up`/`down` fields. Tolerates leading whitespace and concatenated samples.
fn first_traffic_sample(buffer: &[u8]) -> Option<TrafficSample> {
    let value = first_json_object(buffer)?;
    Some(TrafficSample {
        up: value.get("up").and_then(|v| v.as_u64()).unwrap_or(0),
        down: value.get("down").and_then(|v| v.as_u64()).unwrap_or(0),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A minimal one-request-per-connection HTTP server for the clash_api
    /// client tests. Routes are `(method, path, body)`; a path also matches its
    /// `?query` form so the delay endpoint works.
    async fn spawn_server(routes: Vec<(&'static str, &'static str, &'static str)>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock server binds");
        let port = listener.local_addr().expect("address").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 8192];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let request_line = request.lines().next().unwrap_or("");
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or("");
                    let path = parts.next().unwrap_or("");
                    let matched = routes.iter().find(|(route_method, route_path, _)| {
                        *route_method == method
                            && (path == *route_path || path.starts_with(&format!("{route_path}?")))
                    });
                    let (status, body) = match matched {
                        Some((_, _, body)) => ("200 OK", *body),
                        None => ("404 Not Found", ""),
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.flush().await;
                });
            }
        });
        port
    }

    fn api(port: u16) -> ClashApi {
        ClashApi::new(&format!("http://127.0.0.1:{port}"))
    }

    #[tokio::test]
    async fn proxies_separates_selector_groups_from_nodes() {
        let body = r#"{"proxies":{
            "🚀节点选择":{"type":"Selector","now":"node-a","all":["node-a","node-b"],"history":[]},
            "node-a":{"type":"vless","history":[{"time":"t","delay":120}]},
            "node-b":{"type":"hysteria2","history":[]}
        }}"#;
        let port = spawn_server(vec![("GET", "/proxies", body)]).await;
        let (groups, nodes) = api(port).proxies().await.expect("proxies");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "🚀节点选择");
        assert_eq!(groups[0].now, "node-a");
        assert_eq!(groups[0].all, vec!["node-a", "node-b"]);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "node-a");
        assert_eq!(nodes[1].kind, "hysteria2");
    }

    #[tokio::test]
    async fn mode_round_trips_and_delay_reads_the_milliseconds() {
        let port = spawn_server(vec![
            ("GET", "/configs", r#"{"mode":"global"}"#),
            ("PATCH", "/configs", "{}"),
            ("GET", "/proxies/node-a/delay", r#"{"delay":187}"#),
        ])
        .await;
        let api = api(port);
        assert_eq!(api.mode().await.expect("mode"), OutboundMode::Global);
        api.set_mode(OutboundMode::Direct).await.expect("set mode");
        assert_eq!(api.delay("node-a").await.expect("delay"), 187);
    }

    #[tokio::test]
    async fn traffic_reads_one_sample_from_the_streamed_endpoint() {
        let port = spawn_server(vec![(
            "GET",
            "/traffic",
            r#"{"up":10,"down":20}{"up":30,"down":40}"#,
        )])
        .await;
        let sample = api(port).traffic().await.expect("traffic");
        assert_eq!(sample, TrafficSample { up: 10, down: 20 });
    }

    #[tokio::test]
    async fn memory_reads_the_inuse_bytes_from_the_streamed_endpoint() {
        let port = spawn_server(vec![(
            "GET",
            "/memory",
            r#"{"inuse":1048576,"oslimit":0}{"inuse":2}"#,
        )])
        .await;
        assert_eq!(api(port).memory().await.expect("memory"), 1_048_576);
    }

    #[tokio::test]
    async fn connections_and_close_use_the_connections_endpoint() {
        let body = r#"{"uploadTotal":5,"downloadTotal":7,"connections":[
            {"id":"conn-1","upload":1,"download":2,"start":"t",
             "metadata":{"network":"tcp","host":"example.com","destinationIP":"1.2.3.4","destinationPort":"443"}}
        ]}"#;
        let port = spawn_server(vec![
            ("GET", "/connections", body),
            ("DELETE", "/connections/conn-1", "{}"),
        ])
        .await;
        let api = api(port);
        let snapshot = api.connections().await.expect("connections");
        assert_eq!(snapshot.upload_total, 5);
        assert_eq!(snapshot.connections.len(), 1);
        assert_eq!(
            snapshot.connections[0].metadata.destination_host,
            "example.com"
        );
        api.close_connection("conn-1").await.expect("close");
    }

    #[test]
    fn first_traffic_sample_reads_one_object_from_a_partial_stream() {
        assert_eq!(
            first_traffic_sample(br#"{"up":10,"down":20}"#),
            Some(TrafficSample { up: 10, down: 20 })
        );
        // A second, incomplete object must be ignored.
        assert_eq!(
            first_traffic_sample(br#"{"up":1,"down":2}{"up":3,"down":"#),
            Some(TrafficSample { up: 1, down: 2 })
        );
        assert_eq!(first_traffic_sample(b""), None);
        assert_eq!(first_traffic_sample(br#"{"up":"#,), None);
    }

    #[test]
    fn outbound_modes_cycle_and_map_to_wire_names() {
        assert_eq!(OutboundMode::Rule.next(), OutboundMode::Global);
        assert_eq!(OutboundMode::Direct.next(), OutboundMode::Rule);
        assert_eq!(OutboundMode::Global.as_str(), "global");
    }
}
