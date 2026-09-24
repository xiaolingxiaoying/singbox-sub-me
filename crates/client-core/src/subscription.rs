//! Subscription normalization, fetching, and parsing.
//!
//! Any sbctl subscription link (any format suffix, QR or index link) is
//! normalized to the same credential's `sing-box-full.json` URL so the
//! sing-box core always receives a full client configuration. Plain
//! sing-box JSON URLs and Base64 URI lists are also accepted.

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};

/// The canonical client profile the TUI manages.
pub const TARGET_FORMAT: &str = "sing-box-full.json";

/// Rewrites any sbctl subscription URL (any `/sub/<cred>/<suffix>` form,
/// including `qr/...` and `index`) into the same credential's
/// `sing-box-full.json` link. Non-`/sub/` URLs are returned unchanged.
///
/// The rewrite works on the path only: a provider that authenticates with a
/// query token (`/sub/abc?token=…`) must keep it, and folding the query into
/// the last path segment produced a URL no server would recognise.
pub fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    let Ok(mut url) = reqwest::Url::parse(trimmed) else {
        return trimmed.to_owned();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return trimmed.to_owned();
    }
    let Some((prefix, tail)) = url.path().rsplit_once("/sub/") else {
        return trimmed.to_owned();
    };
    let credential = tail.split('/').next().unwrap_or_default();
    if credential.is_empty() {
        return trimmed.to_owned();
    }
    url.set_path(&format!("{prefix}/sub/{credential}/{TARGET_FORMAT}"));
    url.into()
}

/// The URL in a form that is safe for a status line or the event log: an sbctl
/// subscription carries its credential in the path, so the credential segment
/// is replaced. ADR-0013 applies to client diagnostics too.
pub fn printable_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return "[unparseable subscription url]".to_owned();
    };
    let host = parsed.host_str().unwrap_or_default();
    if parsed.path().contains("/sub/") {
        format!("{}://{host}/sub/[redacted]", parsed.scheme())
    } else {
        format!("{}://{host}{}", parsed.scheme(), parsed.path())
    }
}

/// Builds the bare endpoint from a normalized sbctl full-profile route.
/// Unrelated URLs and arbitrary suffixes are never rewritten.
pub fn bare_sing_box_fallback_url(input: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let (prefix, tail) = url.path().rsplit_once("/sub/")?;
    let (credential, suffix) = tail.split_once('/')?;
    if credential.is_empty() || !matches!(suffix, "sing-box-full.json" | "sing-box.json") {
        return None;
    }
    let path = format!("{prefix}/sub/{credential}/sing-box.json");
    url.set_path(&path);
    Some(url.into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeSummary {
    pub tag: String,
    pub protocol: String,
    pub server: String,
    pub port: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SubscriptionSnapshot {
    pub nodes: Vec<NodeSummary>,
    /// Raw sing-box full client configuration JSON text.
    pub raw: String,
}

/// The `subscription-userinfo` metadata the server attaches to every format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionUserinfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: Option<u64>,
}

impl SubscriptionUserinfo {
    pub fn used(&self) -> u64 {
        self.upload.saturating_add(self.download)
    }

    pub fn remaining(&self) -> Option<u64> {
        (self.total > 0).then(|| self.total.saturating_sub(self.used()))
    }
}

/// A fetched subscription body plus its traffic metadata.
pub struct Fetched {
    pub body: String,
    pub userinfo: Option<SubscriptionUserinfo>,
}

/// Parses a `subscription-userinfo` header
/// (`upload=…; download=…; total=…; expire=…`). Returns `None` when no known
/// field is present so an unrelated header never shows bogus zeros.
pub fn parse_userinfo(header: &str) -> Option<SubscriptionUserinfo> {
    let mut info = SubscriptionUserinfo::default();
    let mut any = false;
    for pair in header.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "upload" => {
                info.upload = value.parse().unwrap_or(0);
                any = true;
            }
            "download" => {
                info.download = value.parse().unwrap_or(0);
                any = true;
            }
            "total" => {
                info.total = value.parse().unwrap_or(0);
                any = true;
            }
            "expire" => {
                if let Ok(seconds) = value.parse::<u64>() {
                    info.expire = (seconds > 0).then_some(seconds);
                }
            }
            _ => {}
        }
    }
    any.then_some(info)
}

/// Downloads the subscription body with the configured mirror prefix.
///
/// Deliberately NOT `no_proxy()` the way [`crate::clash_api`] is: this reaches
/// the open internet, and a user whose only route to their own VPS is the proxy
/// in their environment must keep working. The loop the client could otherwise
/// create with itself is not reachable here - system-proxy mode writes the
/// registry/gsettings value and only *prints* the env-var suggestion
/// (`system_proxy.rs:433`), it never sets a proxy in our own process.
pub async fn fetch(url: &str, mirror: &str) -> Result<Fetched> {
    let url = apply_mirror(url, mirror);
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {}", printable_url(&url)))?
        .error_for_status()
        .with_context(|| {
            format!(
                "subscription endpoint returned an error for {}",
                printable_url(&url)
            )
        })?;
    let userinfo = response
        .headers()
        .get("subscription-userinfo")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_userinfo);
    let body = response.text().await?;
    Ok(Fetched { body, userinfo })
}

pub fn apply_mirror(url: &str, mirror: &str) -> String {
    let mirror = mirror.trim().trim_end_matches('/');
    if mirror.is_empty() || url.starts_with(mirror) {
        url.to_owned()
    } else {
        format!("{mirror}/{url}")
    }
}

/// Extracts a node summary from a sing-box outbound object.
pub fn summarize(outbounds: &[serde_json::Value]) -> Result<SubscriptionSnapshot> {
    let mut nodes = Vec::new();
    for outbound in outbounds {
        let kind = outbound
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if matches!(kind, "selector" | "urltest" | "direct" | "block") {
            continue;
        }
        let tag = outbound
            .get("tag")
            .and_then(|v| v.as_str())
            .context("outbound lacks a tag")?;
        nodes.push(NodeSummary {
            tag: tag.to_owned(),
            protocol: kind.to_owned(),
            server: outbound
                .get("server")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned(),
            port: outbound
                .get("server_port")
                .and_then(|v| v.as_u64())
                .unwrap_or_default(),
        });
    }
    nodes.sort_by(|a, b| a.tag.cmp(&b.tag));
    nodes.dedup_by(|a, b| a.tag == b.tag);
    Ok(SubscriptionSnapshot {
        nodes,
        raw: String::new(),
    })
}

/// Parses any supported subscription body into a snapshot with its raw
/// sing-box configuration. URI lists are converted to equivalent outbounds.
pub fn parse(body: &str) -> Result<SubscriptionSnapshot> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(trimmed).context("subscription is not valid JSON")?;
        let outbounds = value
            .get("outbounds")
            .and_then(|v| v.as_array())
            .cloned()
            .context("sing-box subscription lacks an outbounds array")?;
        let mut snapshot = summarize(&outbounds)?;
        snapshot.raw = wrap_bare_node_config(value, &snapshot.nodes)?;
        return Ok(snapshot);
    }
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(body.trim()) {
        let text = String::from_utf8(decoded).context("base64 subscription is not UTF-8")?;
        if text.contains("://") {
            return parse_uri_list(&text);
        }
    }
    if body.contains("://") {
        return parse_uri_list(body);
    }
    bail!("subscription body is not a sing-box JSON or URI list")
}

/// Turns an older sbctl bare-node response into a locally runnable sing-box
/// client configuration. Full profiles pass through untouched; only the
/// historical `{ "outbounds": [...] }` shape is augmented.
fn wrap_bare_node_config(value: serde_json::Value, nodes: &[NodeSummary]) -> Result<String> {
    let has_inbounds = value.get("inbounds").is_some();
    let has_clash_api = value.pointer("/experimental/clash_api").is_some();
    let has_selector = value
        .get("outbounds")
        .and_then(|outbounds| outbounds.as_array())
        .is_some_and(|outbounds| {
            outbounds.iter().any(|outbound| {
                outbound.get("type").and_then(|kind| kind.as_str()) == Some("selector")
            })
        });
    if has_inbounds || has_clash_api || has_selector {
        return Ok(serde_json::to_string_pretty(&value)?);
    }
    if nodes.is_empty() {
        bail!("bare sing-box subscription has no proxy nodes");
    }
    let mut outbounds = value
        .get("outbounds")
        .and_then(|outbounds| outbounds.as_array())
        .cloned()
        .context("sing-box subscription lacks an outbounds array")?;
    let selector_tag = "🚀节点选择";
    let direct_tag = "direct";
    outbounds.insert(
        0,
        serde_json::json!({
            "type": "selector",
            "tag": selector_tag,
            "outbounds": nodes.iter().map(|node| node.tag.clone()).collect::<Vec<_>>()
        }),
    );
    if !outbounds
        .iter()
        .any(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()) == Some(direct_tag))
    {
        outbounds.push(serde_json::json!({ "type": "direct", "tag": direct_tag }));
    }
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "log": { "level": "info" },
        "inbounds": [],
        "outbounds": outbounds,
        "route": { "auto_detect_interface": true, "final": selector_tag },
        "experimental": {
            "clash_api": {
                "external_controller": "127.0.0.1:9090",
                "secret": ""
            }
        }
    }))?)
}

/// Converts share URIs (vless/vmess/hysteria2/tuic/anytls) into sing-box
/// outbounds. Only the fields the TUI and the core need are preserved. A URI
/// list carries no inbounds, selector, or clash_api, so the result is wrapped
/// into the same locally runnable shape as a bare node list — without that,
/// the core would start with no control endpoint and be killed by the
/// startup probe.
pub fn parse_uri_list(text: &str) -> Result<SubscriptionSnapshot> {
    let mut outbounds = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // A subscription is third-party input: one line this client's sing-box
        // build has no outbound for (an unknown scheme, a malformed tail) must
        // not delete the nodes that do parse. When nothing parses, the count is
        // what makes the empty profile diagnosable instead of mysterious.
        match parse_uri(line) {
            Ok(Some(outbound)) => outbounds.push(outbound),
            Ok(None) => {}
            Err(_) => skipped += 1,
        }
    }
    if outbounds.is_empty() {
        bail!("订阅里没有可导入的节点（跳过 {skipped} 行无法解析）");
    }
    // Two nodes on one address with nothing to tell them apart collide on `tag`,
    // and `summarize` de-duplicates by tag: the user silently got fewer nodes
    // than the subscription listed, while the generated config still carried the
    // duplicates for the core to reject. Renaming keeps every node visible and
    // keeps the node list and the config describing the same set.
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for outbound in &mut outbounds {
        let Some(tag) = outbound
            .get("tag")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if !used.contains(&tag) {
            used.insert(tag);
            continue;
        }
        let mut nth = 2;
        while used.contains(&format!("{tag}-{nth}")) {
            nth += 1;
        }
        let renamed = format!("{tag}-{nth}");
        used.insert(renamed.clone());
        outbound["tag"] = serde_json::json!(renamed);
    }
    let mut snapshot = summarize(&outbounds)?;
    snapshot.raw = wrap_bare_node_config(
        serde_json::json!({ "outbounds": outbounds }),
        &snapshot.nodes,
    )?;
    Ok(snapshot)
}

fn parse_uri(line: &str) -> Result<Option<serde_json::Value>> {
    let (scheme, rest) = line.split_once("://").context("URI lacks a scheme")?;
    // v2rayN, NekoBox and Shadowrocket export VMESS as `vmess://<base64 json>`:
    // the URI carries no `@host` at all, every field lives inside the payload.
    // Parsing that as `userinfo@host:port` put a base64 blob in the server field
    // and an empty uuid in the node, so no standard vmess link ever imported.
    if scheme == "vmess" && !rest.contains('@') {
        let (head, query, fragment) = split_uri_tail(rest);
        let params = parse_query(query);
        return vmess_from_payload(
            head.trim_end_matches('/'),
            fragment.as_deref(),
            params.get("insecure").map(|v| v == "1" || v == "true"),
        )
        .map(Some);
    }
    let (userinfo, host_part) = match rest.split_once('@') {
        Some((user, host)) => (user, host),
        None => ("", rest),
    };
    let (host_port, query, fragment) = split_uri_tail(host_part);
    let host_port = host_port.trim_end_matches('/');
    let (host, port_str) = host_port.rsplit_once(':').unwrap_or((host_port, ""));
    let port: u64 = if port_str.is_empty() {
        443
    } else {
        port_str.parse().context("URI port is not a number")?
    };
    let params = parse_query(query);
    let tag = fragment.clone().unwrap_or_else(|| host.to_owned());
    let insecure = params.get("insecure").map(|v| v == "1" || v == "true");
    let sni = params.get("sni").cloned();

    // `enabled` is per-protocol state, not a constant: a VLESS link without
    // `security=tls|reality` is plaintext by spec, and forcing TLS onto it
    // produced a node that could never connect.
    let outbound = match scheme {
        "vless" => {
            if userinfo.is_empty() {
                // Without this the tail of a malformed line became the server
                // field and the node imported as an empty-uuid outbound that
                // could only ever fail at connect time.
                bail!("vless link has no uuid");
            }
            // Xray's VLESS URI defaults `security` to none; only tls and reality
            // carry a TLS handshake.
            let security = params.get("security").map(String::as_str).unwrap_or("none");
            let mut value = serde_json::json!({
                "type": "vless", "tag": tag, "server": host, "server_port": port,
                "uuid": userinfo, "flow": params.get("flow").cloned().unwrap_or_default(),
                "tls": tls_block(sni, matches!(security, "tls" | "reality"), insecure),
            });
            if security == "reality" {
                value["tls"]["reality"] = serde_json::json!({
                    "enabled": true,
                    "public_key": params.get("pbk").cloned().unwrap_or_default(),
                    "short_id": params.get("sid").cloned().unwrap_or_default(),
                });
                value["tls"]["utls"] = serde_json::json!({
                    "enabled": true,
                    "fingerprint": params.get("fp").cloned().unwrap_or_else(|| "chrome".into()),
                });
            }
            value
        }
        "vmess" => {
            // The `vmess://<base64>@host:port` shape: alterId, cipher and
            // transport still live in the payload, the URI adds the address.
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(userinfo)
                .context("vmess payload is not base64")?;
            let payload: serde_json::Value =
                serde_json::from_slice(&decoded).context("vmess payload is not JSON")?;
            // The fragment, not the URI's derived tag: `tag` here already fell
            // back to the host, and passing it in would let the address win over
            // the name the panel chose in `ps` - the opposite of the intended
            // precedence, and on a host carrying several nodes it produced
            // identical tags that `summarize` then de-duplicated away.
            vmess_outbound(&payload, fragment.as_deref(), host, port, sni, insecure)?
        }
        "hysteria2" | "hy2" => serde_json::json!({
            "type": "hysteria2", "tag": tag, "server": host, "server_port": port,
            "password": percent_decode(userinfo),
            "tls": { "enabled": true, "server_name": sni, "alpn": ["h3"],
                "insecure": insecure.unwrap_or(false) }
        }),
        "tuic" => {
            let (uuid, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
            serde_json::json!({
                "type": "tuic", "tag": tag, "server": host, "server_port": port,
                "uuid": uuid, "password": password,
                "congestion_control": params.get("congestion_control").cloned().unwrap_or_else(|| "bbr".into()),
                "udp_relay_mode": params.get("udp_relay_mode").cloned().unwrap_or_else(|| "native".into()),
                "tls": { "enabled": true, "server_name": sni, "alpn": ["h3"],
                    "insecure": insecure.unwrap_or(false) }
            })
        }
        "anytls" => serde_json::json!({
            "type": "anytls", "tag": tag, "server": host, "server_port": port,
            "password": percent_decode(userinfo),
            "tls": { "enabled": true, "server_name": sni, "insecure": insecure.unwrap_or(false) }
        }),
        "ss" | "trojan" | "ssr" => {
            // Not a Managed protocol; skip quietly so a mixed list still loads.
            return Ok(None);
        }
        other => bail!("unsupported URI scheme: {other}"),
    };
    Ok(Some(outbound))
}

/// The `tls` block every protocol in this parser shares.
fn tls_block(
    server_name: Option<String>,
    enabled: bool,
    insecure: Option<bool>,
) -> serde_json::Value {
    let mut tls = serde_json::json!({ "enabled": enabled });
    if !enabled {
        return tls;
    }
    if let Some(name) = server_name {
        tls["server_name"] = serde_json::json!(name);
    }
    if let Some(true) = insecure {
        tls["insecure"] = serde_json::json!(true);
    }
    tls
}

fn vmess_from_payload(
    payload_b64: &str,
    fragment: Option<&str>,
    insecure: Option<bool>,
) -> Result<serde_json::Value> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .context("vmess payload is not base64")?;
    let payload: serde_json::Value =
        serde_json::from_slice(&decoded).context("vmess payload is not JSON")?;
    vmess_outbound(&payload, fragment, "", 0, None, insecure)
}

/// One builder for both vmess link shapes, because every field except the
/// address is the same JSON in both.
fn vmess_outbound(
    payload: &serde_json::Value,
    uri_tag: Option<&str>,
    uri_host: &str,
    uri_port: u64,
    uri_sni: Option<String>,
    insecure: Option<bool>,
) -> Result<serde_json::Value> {
    let server = as_text(payload.get("add"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| uri_host.to_owned());
    if server.is_empty() {
        bail!("vmess link has no address");
    }
    let port = as_port(payload.get("port"))
        .or((uri_port != 0).then_some(uri_port))
        .context("vmess link has no port")?;
    let uuid = as_text(payload.get("id")).filter(|s| !s.is_empty());
    let Some(uuid) = uuid else {
        bail!("vmess payload has no uuid");
    };
    // `ps` is the display name the panel chose; a URI fragment beats it, and
    // the address beats both, so a node is never anonymous.
    let tag = uri_tag
        .map(str::to_owned)
        .filter(|t| !t.is_empty())
        .or_else(|| as_text(payload.get("ps")).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| server.clone());
    // `aid` arrives as a string from some generators and a number from others;
    // reading only one shape silently turned alterId into 0, which is an auth
    // failure the user cannot see from here.
    let alter_id = as_port(payload.get("aid")).unwrap_or(0);
    let uses_tls = as_text(payload.get("tls")).is_some_and(|t| !t.is_empty() && t != "none");
    let sni = as_text(payload.get("sni"))
        .filter(|s| !s.is_empty())
        .or(uri_sni);
    let mut value = serde_json::json!({
        "type": "vmess", "tag": tag, "server": server, "server_port": port,
        "uuid": uuid,
        "security": as_text(payload.get("scy")).filter(|s| !s.is_empty()).unwrap_or_else(|| "auto".into()),
        "alter_id": alter_id,
        "tls": tls_block(sni, uses_tls, insecure),
    });
    // Only the transports sing-box names the same way the vmess `net` field
    // does; anything else is left out so the core uses its own default rather
    // than the profile failing `sing-box check`.
    let net = as_text(payload.get("net")).filter(|n| matches!(n.as_str(), "ws" | "grpc" | "http"));
    if let Some(net) = net {
        let mut transport = serde_json::json!({ "type": net });
        if let Some(path) = as_text(payload.get("path")).filter(|p| !p.is_empty()) {
            transport["path"] = serde_json::json!(path);
        }
        // vmess `host` is the WebSocket Host header, a different thing from the
        // TLS SNI that borrowed it above.
        if let Some(host_header) = as_text(payload.get("host")).filter(|h| !h.is_empty()) {
            transport["headers"] = serde_json::json!({ "Host": host_header });
        }
        value["transport"] = transport;
    }
    Ok(value)
}

/// A payload field that any generator may write as a string or as a number.
fn as_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn as_port(value: Option<&serde_json::Value>) -> Option<u64> {
    as_text(value)?.parse().ok()
}

fn split_uri_tail(rest: &str) -> (&str, &str, Option<String>) {
    let (host_query, fragment) = match rest.split_once('#') {
        Some((left, right)) => (left, Some(percent_decode(right))),
        None => (rest, None),
    };
    let (host_port, query) = match host_query.split_once('?') {
        Some((left, right)) => (left, right),
        None => (host_query, ""),
    };
    (host_port, query, fragment)
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.entry(percent_decode(key))
            .or_insert_with(|| percent_decode(value));
    }
    map
}

/// Percent-decodes a URI component; invalid escapes pass through unchanged.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_every_sbctl_suffix_to_the_full_profile() {
        let base = "https://sub.example.test/sub";
        for suffix in [
            "sing-box.json",
            "clash.yaml",
            "clash-1.18.yaml",
            "uri",
            "uri.txt",
            "shadowrocket.txt",
            "sing-box-1.12.json",
            "qr/uri",
            "index",
        ] {
            assert_eq!(
                normalize_url(&format!("{base}/cred-abc/{suffix}")),
                format!("{base}/cred-abc/{TARGET_FORMAT}"),
                "suffix {suffix} should normalize"
            );
        }
        assert_eq!(
            normalize_url("https://other/sub/xyz"),
            format!("https://other/sub/xyz/{TARGET_FORMAT}"),
        );
    }

    #[test]
    fn normalizing_rewrites_the_route_without_swallowing_a_query_or_fragment() {
        let base = "https://sub.example.test";
        assert_eq!(
            normalize_url(&format!("{base}/sub/cred-abc?token=xyz")),
            format!("{base}/sub/cred-abc/{TARGET_FORMAT}?token=xyz"),
            "a provider token must survive the rewrite"
        );
        assert_eq!(
            normalize_url(&format!("{base}/sub/cred-abc/uri#note")),
            format!("{base}/sub/cred-abc/{TARGET_FORMAT}#note"),
        );
        assert_eq!(
            normalize_url("not a url"),
            "not a url",
            "unparseable input is passed through untouched"
        );
        assert_eq!(
            normalize_url("file:///tmp/keep/sub/toy"),
            "file:///tmp/keep/sub/toy",
            "only http(s) routes are rewritten"
        );
    }

    #[test]
    fn printable_urls_hide_the_subscription_credential() {
        assert_eq!(
            printable_url("https://sub.example.test/sub/super-secret-credential/uri"),
            "https://sub.example.test/sub/[redacted]"
        );
        assert_eq!(
            printable_url("https://example.test/api/feed?key=super-secret"),
            "https://example.test/api/feed",
            "a non-sbctl path is shown, minus its query"
        );
        assert!(
            !printable_url("https://sub.example.test/sub/abc/sing-box.json?token=t")
                .contains("abc"),
            "neither the credential nor the token may reach a status line"
        );
    }

    #[test]
    fn normalized_full_profiles_fall_back_only_to_the_same_credentials_bare_route() {
        let bare = "https://sub.example.test/sub/cred-abc/sing-box.json";
        assert_eq!(bare_sing_box_fallback_url(bare).as_deref(), Some(bare));
        assert_eq!(
            bare_sing_box_fallback_url(&normalize_url(bare)).as_deref(),
            Some(bare)
        );
        assert_eq!(
            bare_sing_box_fallback_url("https://sub.example.test/sub/cred-abc/clash.yaml"),
            None
        );
        assert_eq!(bare_sing_box_fallback_url("file:C:/profile.json"), None);
    }

    #[test]
    fn wraps_a_bare_node_list_into_a_local_runtime_profile() {
        let body = serde_json::json!({
            "outbounds": [
                {"type": "vless", "tag": "node-a", "server": "1.2.3.4", "server_port": 443, "uuid": "u"}
            ]
        })
        .to_string();
        let snapshot = parse(&body).expect("bare node list parses");
        let value: serde_json::Value = serde_json::from_str(&snapshot.raw).expect("runtime JSON");
        assert_eq!(value["outbounds"][0]["type"], "selector");
        assert_eq!(value["outbounds"][0]["tag"], "🚀节点选择");
        assert_eq!(value["route"]["final"], "🚀节点选择");
        assert_eq!(
            value["experimental"]["clash_api"]["external_controller"],
            "127.0.0.1:9090"
        );
    }

    #[test]
    fn parses_a_base64_uri_list_with_all_five_protocols() {
        let uris = [
            "vless://uuid-x@example.com:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.cloudflare.com&fp=chrome&pbk=pub-key&sid=abcd&type=tcp#vless-node",
            "hysteria2://pass%40word@example.com:8443?insecure=1&sni=www.bing.com#hy2-node",
            "tuic://uuid-y:pass@example.com:8443?congestion_control=bbr&udp_relay_mode=native&alpn=h3&insecure=0&sni=www.bing.com#tuic-node",
            "anytls://pass@example.com:443/?insecure=0&sni=www.bing.com#anytls-node",
        ]
        .join("\n");
        let encoded = base64::engine::general_purpose::STANDARD.encode(&uris);
        let snapshot = parse(&encoded).expect("base64 URI list parses");
        assert_eq!(snapshot.nodes.len(), 4);
        let mut protocols: Vec<&str> = snapshot.nodes.iter().map(|n| n.protocol.as_str()).collect();
        protocols.sort();
        assert_eq!(protocols, vec!["anytls", "hysteria2", "tuic", "vless"]);
        // A URI list only carries node outbounds, so the parsed raw config
        // must be augmented into a locally runnable skeleton: a selector to
        // switch nodes, a direct fallback, and the clash_api endpoint the
        // UIs control the core through.
        let value: serde_json::Value = serde_json::from_str(&snapshot.raw).expect("raw is JSON");
        assert_eq!(value["outbounds"][0]["type"], "selector");
        assert_eq!(value["route"]["final"], "🚀节点选择");
        assert!(value["inbounds"].as_array().is_some());
        assert_eq!(
            value["experimental"]["clash_api"]["external_controller"],
            "127.0.0.1:9090"
        );
    }

    #[test]
    fn wraps_a_plain_uri_list_into_a_local_runtime_profile() {
        let body = "hysteria2://pass@example.com:8443?insecure=1#hy2-node\n\n";
        let snapshot = parse(body).expect("plain URI list parses");
        let value: serde_json::Value = serde_json::from_str(&snapshot.raw).expect("raw is JSON");
        assert_eq!(value["outbounds"][0]["tag"], "🚀节点选择");
        assert!(value["experimental"]["clash_api"].is_object());
    }

    #[test]
    fn vless_tls_follows_the_security_parameter() {
        let cases = [
            ("vless://u@h.example:443?encryption=none#no-param", false),
            (
                "vless://u@h.example:443?encryption=none&security=none#none",
                false,
            ),
            (
                "vless://u@h.example:443?encryption=none&security=tls&sni=h.example#tls",
                true,
            ),
            (
                "vless://u@h.example:443?encryption=none&security=reality&sni=h.example&pbk=k&sid=1#reality",
                true,
            ),
        ];
        for (uri, enabled) in cases {
            let snapshot = parse(uri).unwrap_or_else(|e| panic!("{uri} must import: {e}"));
            let value: serde_json::Value =
                serde_json::from_str(&snapshot.raw).expect("raw is JSON");
            let node = node_of(&value, "vless");
            assert_eq!(node["tls"]["enabled"], enabled, "{uri}");
            if enabled {
                assert_eq!(node["tls"]["server_name"], "h.example", "{uri}");
            } else {
                // Carrying a name/alpn for a handshake that will not happen is
                // how a plaintext node looks like a TLS one in the node list.
                assert!(node["tls"].get("server_name").is_none(), "{uri}");
            }
        }
    }

    /// v2rayN and Shadowrocket export `vmess://<base64 json>` with no `@host`
    /// part at all - the address lives inside the payload.
    #[test]
    fn imports_a_standard_vmess_link_the_way_v2rayn_exports_it() {
        let payload = serde_json::json!({
            "v": "2", "ps": "东京-A", "add": "1.2.3.4", "port": "443",
            "id": "uuid-z", "aid": 0, "net": "ws", "host": "h.example",
            "path": "/ray", "tls": "tls", "sni": "h.example"
        });
        let uri = format!(
            "vmess://{}",
            base64::engine::general_purpose::STANDARD.encode(payload.to_string())
        );
        let snapshot = parse(&uri).unwrap_or_else(|e| panic!("vmess link must import: {e}"));
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].tag, "东京-A");
        let value: serde_json::Value = serde_json::from_str(&snapshot.raw).expect("raw is JSON");
        let node = node_of(&value, "vmess");
        assert_eq!(node["server"], "1.2.3.4");
        assert_eq!(node["server_port"], 443);
        assert_eq!(node["uuid"], "uuid-z");
        assert_eq!(node["tls"]["enabled"], true);
        assert_eq!(node["transport"]["type"], "ws");
    }

    /// Two generators, two shapes for the same field; reading only the string
    /// form made a numeric alterId silently become 0, which is an auth failure
    /// the user cannot see.
    #[test]
    fn vmess_alter_id_accepts_a_number_and_a_string() {
        for aid in [serde_json::json!(7), serde_json::json!("7")] {
            let payload =
                serde_json::json!({"ps":"n","add":"1.2.3.4","port":2053,"id":"u","aid":aid});
            let uri = format!(
                "vmess://{}",
                base64::engine::general_purpose::STANDARD.encode(payload.to_string())
            );
            let snapshot = parse(&uri).expect("vmess payload parses");
            let value: serde_json::Value =
                serde_json::from_str(&snapshot.raw).expect("raw is JSON");
            assert_eq!(node_of(&value, "vmess")["alter_id"], 7, "{aid}");
        }
    }

    /// One line this build cannot represent used to abort the whole import, so
    /// a single exotic entry cost the user every node in the subscription.
    #[test]
    fn one_unreadable_line_does_not_lose_the_readable_ones() {
        let body = [
            "hysteria2://pass@example.com:8443?insecure=1#good",
            "wireguard://invalid_public_key@host:51820?private_key=x#unsupported-scheme",
            "vless://not-even-a-uri",
            "trojan://pass@example.com:443#not-managed-protocol",
        ]
        .join("\n");
        let snapshot = parse_uri_list(&body).expect("the list still imports");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].tag, "good");
    }

    #[test]
    fn a_list_of_only_junk_says_so_instead_of_looking_empty() {
        let error = parse_uri_list("wireguard://a@b:1\nnonsense\n\n")
            .expect_err("nothing importable must not read as a valid subscription");
        let message = error.to_string();
        assert!(message.contains('2'), "count missing: {message}");
    }

    /// Three nodes behind one address, named only inside the payload. While the
    /// URI-derived tag won, all three came out as `1.2.3.4`, and the
    /// de-duplication in `summarize` left the user with one node out of three -
    /// silently, with the config still carrying the duplicates.
    #[test]
    fn same_host_nodes_keep_their_names_and_all_survive() {
        let encode = |name: &str| {
            let payload = serde_json::json!({
                "ps": name, "add": "1.2.3.4", "port": 443, "id": format!("uuid-{name}")
            });
            format!(
                "vmess://{}",
                base64::engine::general_purpose::STANDARD.encode(payload.to_string())
            )
        };
        let body = [encode("东京-A"), encode("东京-B"), encode("东京-B")].join("\n");
        let snapshot = parse_uri_list(&body).expect("three vmess links import");
        let mut tags: Vec<String> = snapshot.nodes.iter().map(|node| node.tag.clone()).collect();
        tags.sort();
        assert_eq!(
            tags,
            vec![
                "东京-A".to_owned(),
                "东京-B".to_owned(),
                "东京-B-2".to_owned()
            ],
            "ps names the node and a collision is renamed rather than dropped"
        );
        let raw: serde_json::Value = serde_json::from_str(&snapshot.raw).expect("raw is JSON");
        let raw_tags: Vec<&str> = raw["outbounds"]
            .as_array()
            .expect("outbounds is a list")
            .iter()
            .filter_map(|outbound| outbound["tag"].as_str())
            .collect();
        for tag in &tags {
            assert!(
                raw_tags.contains(&tag.as_str()),
                "{tag} is in the node list but not in the config: {raw_tags:?}"
            );
        }
    }

    fn node_of(value: &serde_json::Value, kind: &str) -> serde_json::Value {
        value["outbounds"]
            .as_array()
            .expect("outbounds is a list")
            .iter()
            .find(|outbound| outbound["type"] == kind)
            .unwrap_or_else(|| panic!("no {kind} outbound in {value}"))
            .clone()
    }

    #[test]
    fn parses_a_sing_box_full_profile_and_skips_groups() {
        let body = serde_json::json!({
            "outbounds": [
                {"type": "selector", "tag": "🚀节点选择", "outbounds": ["a"]},
                {"type": "vless", "tag": "a", "server": "1.2.3.4", "server_port": 443, "uuid": "u"},
                {"type": "direct", "tag": "direct"}
            ]
        })
        .to_string();
        let snapshot = parse(&body).expect("full profile parses");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].tag, "a");
    }

    #[test]
    fn percent_decode_handles_escapes_and_passes_through_garbage() {
        assert_eq!(percent_decode("pass%40word"), "pass@word");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("bad%2"), "bad%2");
    }

    /// Our own server appends `profile-update-interval` to this header; a parser
    /// that treated an unknown key as fatal would break its own downloads.
    #[test]
    fn an_unknown_trailing_userinfo_key_is_ignored() {
        let info = parse_userinfo(
            "upload=71; download=36; total=10737418240; expire=1790000000; profile-update-interval=24",
        )
        .expect("the known fields still parse");
        assert_eq!(
            (info.upload, info.download, info.total),
            (71, 36, 10_737_418_240)
        );
        assert_eq!(info.expire, Some(1_790_000_000));
    }

    #[test]
    fn parses_subscription_userinfo_headers() {
        let info = parse_userinfo("upload=71; download=36; total=10737418240; expire=1790000000")
            .expect("known fields parse");
        assert_eq!(info.upload, 71);
        assert_eq!(info.download, 36);
        assert_eq!(info.total, 10 * 1024 * 1024 * 1024);
        assert_eq!(info.used(), 107);
        assert_eq!(info.remaining(), Some(info.total - 107));
        assert_eq!(info.expire, Some(1_790_000_000));
        // An empty expire or an unrelated header yields no bogus values.
        assert_eq!(
            parse_userinfo("upload=0; download=0; total=0; expire=").map(|i| i.expire),
            Some(None)
        );
        assert_eq!(parse_userinfo("noop"), None);
    }

    #[test]
    fn userinfo_without_a_quota_reports_no_remaining() {
        let info = parse_userinfo("upload=5; download=5; total=0; expire=0").expect("parses");
        assert_eq!(info.remaining(), None);
        assert_eq!(info.expire, None);
    }
}
