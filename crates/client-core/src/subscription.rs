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
pub fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    let Some(path_start) = trimmed.find("/sub/") else {
        return trimmed.to_owned();
    };
    let (base, rest) = trimmed.split_at(path_start + "/sub/".len());
    // rest = "<credential>/<format...>"; drop QR prefixes and any extra
    // segments, then swap the format suffix.
    let mut segments = rest.split('/').filter(|s| !s.is_empty());
    let Some(credential) = segments.next() else {
        return trimmed.to_owned();
    };
    let credential = credential.trim_end_matches('/');
    format!("{base}{credential}/{TARGET_FORMAT}")
}

/// Returns the original bare sing-box JSON endpoint when it can act as a
/// compatibility fallback for an older sbctl server. Such servers predate the
/// full client profile route but still serve a valid node list at this URL.
/// All other inputs deliberately return `None`: changing a Clash, URI, QR, or
/// local-file source into a fallback would silently alter its meaning.
pub fn bare_sing_box_fallback_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let (_, tail) = trimmed.split_once("/sub/")?;
    let mut parts = tail.split('/').filter(|part| !part.is_empty());
    let credential = parts.next()?;
    let suffix = parts.collect::<Vec<_>>().join("/");
    (!credential.is_empty() && suffix == "sing-box.json").then(|| trimmed.to_owned())
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
pub async fn fetch(url: &str, mirror: &str) -> Result<Fetched> {
    let url = apply_mirror(url, mirror);
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("subscription endpoint returned an error for {url}"))?;
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
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(outbound) = parse_uri(line)? {
            outbounds.push(outbound);
        }
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

    let tls = |server_name: Option<String>| {
        let mut tls = serde_json::json!({ "enabled": true });
        if let Some(name) = server_name {
            tls["server_name"] = serde_json::json!(name);
        }
        if let Some(true) = insecure {
            tls["insecure"] = serde_json::json!(true);
        }
        tls
    };

    let outbound = match scheme {
        "vless" => {
            let mut value = serde_json::json!({
                "type": "vless", "tag": tag, "server": host, "server_port": port,
                "uuid": userinfo, "flow": params.get("flow").cloned().unwrap_or_default(),
                "tls": tls(sni),
            });
            if params
                .get("security")
                .map(|s| s == "reality")
                .unwrap_or(false)
            {
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
            // v2rayN base64 JSON link.
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(userinfo)
                .context("vmess payload is not base64")?;
            let payload: serde_json::Value =
                serde_json::from_slice(&decoded).context("vmess payload is not JSON")?;
            let mut value = serde_json::json!({
                "type": "vmess", "tag": tag, "server": host, "server_port": port,
                "uuid": payload.get("id").cloned().unwrap_or_default(),
                "security": payload.get("scy").cloned().unwrap_or_else(|| "auto".into()),
                "alter_id": payload.get("aid").and_then(|v| v.as_str().and_then(|s| s.parse::<u64>().ok())).unwrap_or(0),
                "tls": tls(payload.get("sni").and_then(|v| v.as_str()).map(str::to_owned)),
            });
            if payload.get("net").and_then(|v| v.as_str()) == Some("ws") {
                let mut transport = serde_json::json!({ "type": "ws" });
                if let Some(path) = payload.get("path").and_then(|v| v.as_str()) {
                    transport["path"] = serde_json::json!(path);
                }
                value["transport"] = transport;
            }
            value
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
    fn recognizes_only_a_bare_sing_box_source_as_a_compatibility_fallback() {
        let bare = "https://sub.example.test/sub/cred-abc/sing-box.json";
        assert_eq!(bare_sing_box_fallback_url(bare).as_deref(), Some(bare));
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
