//! Subscription artefacts: the client version matrix, the generated artefacts and
//! configuration transactions, the HTTP/TLS/ACME server, and the per-format
//! renderers. Every public item of the crate-facing surface is re-exported here
//! so callers keep using `sbctl::subscription::...` unchanged.
mod artifacts;
mod profile;
mod serve;
#[cfg(test)]
mod test_support;

use base64::Engine;
use rcgen::{CertificateParams, DnType, KeyPair};
use serde_json::{Value, json};
use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::path::Path;

use crate::canonical::CanonicalNode;
use crate::config::{
    CertificateMode, DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode,
};

pub use artifacts::{
    DeploymentSnapshot, SubscriptionError, apply_config_transaction, check_sing_box_config,
    generated_artifacts, read_authorized, regenerate, restore_config_transaction, route_url,
    subscription_url,
};

pub use profile::{
    CLASH_LEGACY_VERSION, ClientSubscriptionFormat, ClientSubscriptionRow, ClientVersion,
    SING_BOX_VERSION_PROFILES, SingBoxVersionProfile, SubscriptionFormat, SubscriptionLinkInfo,
    SubscriptionRoute, client_subscription_matrix, latest_version_profile, subscription_matrix,
};
pub use serve::{redact_secret, serve};

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
    Ok(
        serde_json::to_string_pretty(&json!({"outbounds": client_outbounds(config, nodes)}))
            .expect("JSON values serialize"),
    )
}

/// The five-protocol outbound list shared by the bare and full sing-box client
/// artifacts, so the two can never drift apart on protocol fields.
fn client_outbounds(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Vec<Value> {
    let skip_verify = client_skip_cert_verify(config);
    nodes
        .iter()
        .map(|node| match &node {
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
        })
        .collect()
}

/// Group tags shared by the full sing-box client profile and the clash
/// artifact so panel screenshots and docs read the same everywhere.
pub const SELECTOR_TAG: &str = "🚀节点选择";
pub const AUTO_TAG: &str = "♻️自动选择";
const DIRECT_TAG: &str = "direct";

/// Domains that must never be routed through the selector, kept in one place
/// for the sing-box full profile, the clash artifact, and their overrides.
pub const AI_DOMAIN_SUFFIXES: &[&str] = &[
    "chatgpt.com",
    "openai.com",
    "oaistatic.com",
    "oaiusercontent.com",
    "x.com",
    "twitter.com",
    "twimg.com",
];

/// Domains that must not receive a fake IP: LAN names, OS connectivity
/// checks, and NTP servers, matching the sing-box-yg client defaults.
const FAKE_IP_FILTER_SUFFIXES: &[&str] = &[
    "lan",
    "local",
    "msftconnecttest.com",
    "msftncsi.com",
    "captive.apple.com",
    "time.windows.com",
    "time.apple.com",
    "time.android.com",
    "ntp.org",
];

/// The full sing-box client configuration for one version profile: log, DNS
/// (fake-ip with a direct resolver and a proxied DoH fallback), the tun
/// inbound, grouped outbounds, rule-set routing, and the clash API used by
/// dashboards and sbtui. Field differences between sing-box versions are
/// concentrated here, guided by the changelog research in
/// `docs/research/sing-box-client-version-differences.md`.
fn sing_box_full(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
    profile: &SingBoxVersionProfile,
) -> Result<String, SubscriptionError> {
    // Pre-1.12 client cores have no AnyTLS outbound, so those profiles must
    // silently drop the node; refuse to generate an empty-node artifact and
    // say exactly which knob to turn instead.
    let compatible_nodes: Vec<CanonicalNode> = nodes
        .iter()
        .filter(|node| profile.supports_anytls || node.protocol() != ManagedProtocol::Anytls)
        .cloned()
        .collect();
    if compatible_nodes.is_empty() {
        return Err(SubscriptionError::ClientIncompatible(format!(
            "sing-box {} 客户端内核不支持 AnyTLS 协议（1.12.0 才加入）；\
             请在部署中启用至少一个其他协议，否则请移除 sing-box-{}.json 适配",
            profile.version, profile.version
        )));
    }
    let nodes = &compatible_nodes;
    let node_tags: Vec<&str> = nodes.iter().map(CanonicalNode::tag).collect();
    let mut outbounds = client_outbounds(config, nodes);
    let mut selector_members: Vec<&str> = vec![AUTO_TAG, DIRECT_TAG];
    selector_members.extend(node_tags.iter().copied());
    outbounds.push(json!({
        "type": "selector",
        "tag": SELECTOR_TAG,
        "outbounds": selector_members,
        "interrupt_exist_connections": false
    }));
    outbounds.push(json!({
        "type": "urltest",
        "tag": AUTO_TAG,
        "outbounds": node_tags,
        "url": config.client_latency_probe_url,
        "interval": "5m",
        "tolerance": 50,
        "idle_timeout": "30m"
    }));
    outbounds.push(json!({"type": "direct", "tag": DIRECT_TAG}));

    let fake_ip = config.client_dns_mode == crate::config::ClientDnsMode::FakeIp;
    // 1.12+ requires typed DNS server objects; 1.10/1.11 only accept the
    // legacy address-string format, with fake-ip as a special `fakeip`
    // address plus a top-level dns.fakeip object (removed in 1.14).
    let mut dns = if profile.typed_dns {
        let mut dns_servers = vec![
            json!({"type": "udp", "tag": "dns-direct", "server": "223.5.5.5"}),
            json!({"type": "https", "tag": "dns-proxy", "server": "1.1.1.1", "detour": SELECTOR_TAG}),
        ];
        if fake_ip {
            dns_servers.push(json!({
                "type": "fakeip",
                "tag": "dns-fakeip",
                "inet4_range": "198.18.0.0/15",
                "inet6_range": "fc00::/18"
            }));
        }
        json!({"servers": dns_servers})
    } else {
        let mut dns_servers = vec![
            json!({"tag": "dns-direct", "address": "223.5.5.5"}),
            json!({"tag": "dns-proxy", "address": "https://1.1.1.1/dns-query", "detour": SELECTOR_TAG}),
        ];
        if fake_ip {
            dns_servers.push(json!({"tag": "dns-fakeip", "address": "fakeip"}));
        }
        let mut dns = json!({"servers": dns_servers});
        if fake_ip {
            dns["fakeip"] = json!({
                "enabled": true,
                "inet4_range": "198.18.0.0/15",
                "inet6_range": "fc00::/18"
            });
        }
        dns
    };

    let mut dns_rules = Vec::new();
    if !profile.typed_dns {
        // Pre-1.12 cores have no route.default_domain_resolver; the legacy
        // `outbound: any` DNS rule (removed in 1.14) resolves proxy server
        // domains through direct DNS instead.
        dns_rules.push(json!({"outbound": "any", "server": "dns-direct"}));
    }
    dns_rules.push(json!({"clash_mode": "Direct", "server": "dns-direct"}));
    dns_rules.push(json!({"clash_mode": "Global", "server": "dns-proxy"}));
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        dns_rules.push(json!({"rule_set": ["geosite-cn"], "server": "dns-direct"}));
    }
    dns_rules.push(json!({
        "domain_suffix": FAKE_IP_FILTER_SUFFIXES,
        "server": "dns-direct"
    }));
    if fake_ip {
        dns_rules.push(json!({"query_type": ["A", "AAAA"], "server": "dns-fakeip"}));
    }
    // `independent_cache` is deprecated in 1.14 and removed in 1.16, and
    // brings no benefit here, so the DNS object stays lean across versions.
    dns["rules"] = json!(dns_rules);
    dns["final"] = json!("dns-proxy");

    let legacy_route = !profile.route_rule_actions;
    let mut tun = json!({
        "type": "tun",
        "tag": "tun-in",
        "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
        "mtu": 9000,
        "auto_route": true,
        "strict_route": true,
        "stack": "mixed"
    });
    if legacy_route {
        // 1.10 has no route rule actions; protocol sniffing is configured on
        // the inbound and DNS is hijacked through a special `dns` outbound.
        tun["sniff"] = json!(true);
    }

    let mut route_rules = Vec::new();
    if !legacy_route {
        route_rules.push(json!({"action": "sniff"}));
    }
    route_rules.push(if legacy_route {
        json!({"protocol": "dns", "outbound": "dns-out"})
    } else {
        json!({"protocol": "dns", "action": "hijack-dns"})
    });
    route_rules.push(json!({"ip_is_private": true, "outbound": DIRECT_TAG}));
    route_rules.push(json!({
        "domain_suffix": AI_DOMAIN_SUFFIXES,
        "outbound": SELECTOR_TAG
    }));
    let mut rule_sets: Vec<Value> = Vec::new();
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        route_rules.push(json!({"rule_set": ["geosite-cn", "geoip-cn"], "outbound": DIRECT_TAG}));
        rule_sets.push(remote_rule_set(
            "geosite-cn",
            &format!("{}/geosite/cn.srs", sing_box_rule_set_base(config)),
        ));
        rule_sets.push(remote_rule_set(
            "geoip-cn",
            &format!("{}/geoip/cn.srs", sing_box_rule_set_base(config)),
        ));
    }
    if legacy_route {
        outbounds.push(json!({"type": "dns", "tag": "dns-out"}));
    }
    let mut route = json!({
        "rules": route_rules,
        "rule_set": rule_sets,
        "final": SELECTOR_TAG,
        "auto_detect_interface": true
    });
    if profile.typed_dns {
        route["default_domain_resolver"] = json!({"server": "dns-direct"});
    }

    let mut cache_file = json!({"enabled": true, "store_fakeip": fake_ip});
    if profile.supports_store_dns {
        cache_file["store_dns"] = json!(true);
    }

    Ok(serde_json::to_string_pretty(&json!({
        "log": {"level": "info", "timestamp": true},
        "dns": dns,
        "inbounds": [tun],
        "outbounds": outbounds,
        "route": route,
        "experimental": {
            "clash_api": {
                "external_controller": "127.0.0.1:9090",
                "default_mode": "rule"
            },
            "cache_file": cache_file
        }
    }))
    .expect("JSON values serialize"))
}

/// The rule-set base for sing-box artifacts: the configured source root plus
/// the `@sing` branch that carries the `.srs` binary rule-sets.
fn sing_box_rule_set_base(config: &DeploymentConfig) -> String {
    format!(
        "{}@sing/geo",
        config.client_rule_set_base_url.trim_end_matches('/')
    )
}

fn remote_rule_set(tag: &str, url: &str) -> Value {
    json!({
        "type": "remote",
        "tag": tag,
        "format": "binary",
        "url": url,
        // Deprecated in 1.14 (moved to route.http_clients) but only removed
        // in 1.16, so every profile in the registry still accepts it.
        "download_detour": SELECTOR_TAG,
        "update_interval": "1d"
    })
}

/// Hosts without an IPv6 route cannot dial the AAAA addresses the default
/// resolution strategy prefers, so their server configuration pins IPv4 even
/// when the deployment never opted in; the explicit flag forces the same
/// restriction on dual-stack hosts.
fn ipv4_only_required(config: &DeploymentConfig) -> bool {
    config.ipv4_only || !host_has_ipv6_route()
}

/// UDP `connect` performs a route lookup without sending a packet, which makes
/// it a cheap probe for an IPv6 default route.
fn host_has_ipv6_route() -> bool {
    let Ok(socket) = std::net::UdpSocket::bind("[::]:0") else {
        return false;
    };
    socket.connect("[2001:4860:4860::8888]:443").is_ok()
}

fn sing_box_server(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
    root: &Path,
) -> Result<String, SubscriptionError> {
    let certificate = certificate_tls_config(config, root)?;
    let mut inbounds = Vec::new();
    let mut tags = Vec::new();
    for node in nodes {
        tags.push(node.tag());
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
    let mut server = json!({
        // Connection-level debug logging would expose proxied destinations.
        "log": {"level": "info"},
        "inbounds": inbounds
    });
    if ipv4_only_required(config) {
        // The inbound `domain_strategy` field was deprecated in 1.11 and
        // removed in 1.13, so the destination pin now lives on a route action
        // (the documented migration). Pinning the default DNS strategy keeps
        // every other lookup, including the Reality camouflage handshake that
        // dials its decoy independently of the inbound destination, on IPv4.
        server["dns"] = json!({"strategy": "ipv4_only"});
        server["route"] = json!({
            "rules": tags
                .iter()
                .map(|tag| json!({"inbound": tag, "action": "resolve", "strategy": "ipv4_only"}))
                .collect::<Vec<_>>()
        });
    }
    Ok(serde_json::to_string_pretty(&server).expect("JSON values serialize"))
}

/// The `proxies:` block plus the two historical groups shared by the current
/// and the legacy clash artifacts, so protocol fields cannot drift apart.
fn clash_proxies(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
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
    proxies.push_str(concat!(
        "mode: rule\n",
        "proxy-groups:\n",
        "  - name: 🌍选择代理节点\n",
        "    type: select\n",
        // The selector holds DIRECT, so latency tests must use a URL that is
        // reachable without a proxy; gstatic would time out from China.
        // aliyun.com answers with a redirect, which mihomo counts as success.
        "    url: http://aliyun.com/generate_204\n",
        "    interval: 300\n",
        "    proxies:\n",
        "      - ♻️自动选择\n",
        "      - DIRECT\n",
    ));
    for node in nodes {
        proxies.push_str(&format!("      - {}\n", node.tag()));
    }
    proxies.push_str(concat!(
        "  - name: ♻️自动选择\n",
        "    type: url-test\n",
        "    url: http://www.gstatic.com/generate_204\n",
        "    interval: 300\n",
        "    tolerance: 50\n",
        "    proxies:\n",
    ));
    for node in nodes {
        proxies.push_str(&format!("      - {}\n", node.tag()));
    }
    proxies.push_str("  - name: 🎯全球直连\n    type: select\n    proxies:\n      - DIRECT\n");
    for node in nodes {
        proxies.push_str(&format!("      - {}\n", node.tag()));
    }
    Ok(proxies)
}

/// The `dns:` block shared by the current and legacy clash artifacts; the
/// fake-ip filter keeps LAN names, OS connectivity checks, and NTP on real
/// DNS answers so captive-portal detection keeps working.
fn clash_dns(config: &DeploymentConfig) -> String {
    let mode = match config.client_dns_mode {
        crate::config::ClientDnsMode::FakeIp => "fake-ip",
        crate::config::ClientDnsMode::RedirHost => "redir-host",
    };
    let mut dns = format!(
        "dns:\n  enable: true\n  ipv6: false\n  enhanced-mode: {mode}\n  fake-ip-range: 198.18.0.1/16\n  fake-ip-filter:\n"
    );
    for suffix in FAKE_IP_FILTER_SUFFIXES {
        dns.push_str(&format!("    - '+.{suffix}'\n"));
    }
    dns.push_str(concat!(
        "  use-hosts: false\n  use-system-hosts: false\n",
        "  nameserver:\n    - 'https://1.1.1.1/dns-query#🌍选择代理节点'\n",
        "    - 'https://8.8.8.8/dns-query#🌍选择代理节点'\n",
        "  proxy-server-nameserver:\n    - https://223.5.5.5/dns-query\n",
    ));
    dns
}

fn clash(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
    let mut output = clash_proxies(config, nodes)?;
    // The AI suffix rules must precede the CN rule-set so OpenAI/X domains
    // never fall into geosite-cn's direct verdict.
    output.push_str("rules:\n");
    for suffix in AI_DOMAIN_SUFFIXES {
        output.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},🌍选择代理节点\n"));
    }
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        let base = format!(
            "{}@meta/geo",
            config.client_rule_set_base_url.trim_end_matches('/')
        );
        output.push_str(&format!(
            concat!(
                "  - RULE-SET,geosite-private,🎯全球直连\n",
                "  - RULE-SET,geoip-private,🎯全球直连\n",
                "  - RULE-SET,geosite-cn,🎯全球直连\n",
                "  - RULE-SET,geoip-cn,🎯全球直连\n",
                "  - MATCH,🌍选择代理节点\n",
                "rule-providers:\n",
                "  geosite-private:\n",
                "    type: http\n",
                "    behavior: domain\n",
                "    format: mrs\n",
                "    url: {base}/geosite/private.mrs\n",
                "    path: ./ruleset/geosite-private.mrs\n",
                "    interval: 86400\n",
                "  geoip-private:\n",
                "    type: http\n",
                "    behavior: ipcidr\n",
                "    format: mrs\n",
                "    url: {base}/geoip/private.mrs\n",
                "    path: ./ruleset/geoip-private.mrs\n",
                "    interval: 86400\n",
                "  geosite-cn:\n",
                "    type: http\n",
                "    behavior: domain\n",
                "    format: mrs\n",
                "    url: {base}/geosite/cn.mrs\n",
                "    path: ./ruleset/geosite-cn.mrs\n",
                "    interval: 86400\n",
                "  geoip-cn:\n",
                "    type: http\n",
                "    behavior: ipcidr\n",
                "    format: mrs\n",
                "    url: {base}/geoip/cn.mrs\n",
                "    path: ./ruleset/geoip-cn.mrs\n",
                "    interval: 86400\n",
            ),
            base = base
        ));
    } else {
        output.push_str(concat!(
            "  - GEOIP,LAN,DIRECT\n",
            "  - GEOIP,CN,DIRECT\n",
            "  - MATCH,🌍选择代理节点\n",
        ));
    }
    output.push_str(&clash_dns(config));
    Ok(output)
}

/// The mihomo 1.18.x compatibility artifact: same node and group layout as the
/// current artifact, but with the pre-rule-set built-in GEOIP rules that the
/// previous major line shipped everywhere.
fn clash_legacy(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let mut output = clash_proxies(config, nodes)?;
    output.push_str("rules:\n");
    for suffix in AI_DOMAIN_SUFFIXES {
        output.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},🌍选择代理节点\n"));
    }
    output.push_str(concat!(
        "  - GEOIP,LAN,DIRECT\n",
        "  - GEOIP,CN,DIRECT\n",
        "  - MATCH,🌍选择代理节点\n",
    ));
    output.push_str(&clash_dns(config));
    Ok(output)
}

/// The Shadowrocket-adapted Base64 URI list (research:
/// `docs/research/sing-box-client-version-differences.md` §6). Differences
/// from the plain `uri` rendering: passwords and SNI values are always
/// percent-encoded (Shadowrocket 2.2.44 fixed URI password decoding, which
/// implies special characters must arrive encoded), TUIC carries
/// `udp_relay_mode`, and AnyTLS follows the official anytls-go scheme with
/// the path slash and without the non-standard `security` parameter.
fn shadowrocket(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    // A URI authority needs IPv6 hosts bracketed; the server and Clash
    // renderers deliberately keep the bare address.
    let nodes = nodes
        .iter()
        .map(CanonicalNode::with_bracketed_host)
        .collect::<Vec<_>>();
    for node in &nodes {
        match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => uris.push_str(&format!("vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={public_key}&sid={short_id}&type=tcp#{}\n", percent_encode(decoy_sni), node.tag())),
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
                "hysteria2://{}@{}:{}?insecure={}&sni={}#{}\n",
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{}:{}@{}:{}?congestion_control=bbr&udp_relay_mode=native&alpn=h3&insecure={}&sni={}#{}\n",
                percent_encode(uuid),
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{}@{}:{}/?insecure={}&sni={}#{}\n",
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
        }
    }
    Ok(base64_uri(&uris))
}

/// Percent-encodes everything outside the RFC 3986 unreserved set so secrets
/// with special characters survive URI parsing in every client.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
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

fn uri(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    // A URI authority needs IPv6 hosts bracketed; the server and Clash
    // renderers deliberately keep the bare address.
    let nodes = nodes
        .iter()
        .map(CanonicalNode::with_bracketed_host)
        .collect::<Vec<_>>();
    for node in &nodes {
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

fn base64_uri(uri: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(uri.as_bytes())
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
fn certificate_tls_config(
    config: &DeploymentConfig,
    root: &Path,
) -> Result<Value, SubscriptionError> {
    let (certificate_path, key_path) = match config.certificate_mode {
        CertificateMode::SelfSigned => ensure_self_signed_certificate(config, root)?,
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
        json!({"enabled": true, "server_name": config.protocol_server_name(),
        "certificate_path": certificate_path,
        "key_path": key_path}),
    )
}

/// Generates and pins a long-lived self-signed certificate for the subscription
/// host, or reuses the pinned copy. rcgen's default validity window (1975 to
/// 4096) is left in place, so a no-domain deployment never breaks on an expired
/// administrator-managed certificate. Files are created private (directory
/// 0750, key and certificate 0640) so the TLS private key is never
/// world-readable, even before the daemon-storage preparation runs.
fn ensure_self_signed_certificate(
    config: &DeploymentConfig,
    root: &Path,
) -> Result<(String, String), SubscriptionError> {
    let server_name = config.protocol_server_name();
    let directory = self_signed_certificate_directory(root, server_name);
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
    let mut params = CertificateParams::new(vec![server_name.to_owned()])
        .map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    params
        .distinguished_name
        .push(DnType::CommonName, server_name.to_owned());
    params
        .distinguished_name
        .push(DnType::OrganizationName, "sbctl");
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    fs::create_dir_all(&directory).map_err(SubscriptionError::Artifact)?;
    restrict_directory_permissions(&directory)?;
    write_private_file(&certificate_path, certificate.pem().as_bytes())?;
    write_private_file(&key_path, key_pair.serialize_pem().as_bytes())?;
    Ok((
        certificate_path.to_string_lossy().into_owned(),
        key_path.to_string_lossy().into_owned(),
    ))
}

/// The directory holding the long-lived self-signed certificate for a protocol
/// SNI. The live host uses the absolute path consumed by the generated sing-box
/// configuration and the service accounts; a fixture root keeps every write
/// inside that root so tests and `--root` operations never touch host storage.
fn self_signed_certificate_directory(root: &Path, server_name: &str) -> std::path::PathBuf {
    if root == Path::new("/") {
        Path::new(crate::config::CERTIFICATES_ABSOLUTE_PATH).join(server_name)
    } else {
        root.join(crate::config::CERTIFICATES_RELATIVE_PATH)
            .join(server_name)
    }
}

fn restrict_directory_permissions(directory: &Path) -> Result<(), SubscriptionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o750))
            .map_err(SubscriptionError::Artifact)?;
    }
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), SubscriptionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o640)
            .open(path)
            .and_then(|mut file| file.write_all(contents))
            .map_err(SubscriptionError::Artifact)
    }
    #[cfg(not(unix))]
    fs::write(path, contents).map_err(SubscriptionError::Artifact)
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
    use base64::Engine;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::{
        SING_BOX_VERSION_PROFILES, SubscriptionFormat, SubscriptionRoute, clash, clash_legacy,
        client_subscription_matrix, generated_artifacts, latest_version_profile, regenerate,
        route_url, shadowrocket, sing_box, sing_box_full, uri,
    };
    use crate::config::{
        DeploymentConfig, DeploymentStore, ManagedProtocol, ProtocolPorts, SubscriptionMode,
    };
    use crate::subscription::test_support::{
        seed_all_protocols, seed_direct_subscription, seed_single_protocol, vless_config,
    };

    #[test]
    fn route_url_builds_matrix_links_for_formats_qr_and_index() {
        use super::{SubscriptionFormat, SubscriptionRoute, route_url, subscription_url};
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, credential) = seed_direct_subscription(&fixture);
        let base = format!("https://sub.example.test/sub/{credential}");
        assert_eq!(
            subscription_url(&config, SubscriptionFormat::SingBox).expect("url builds"),
            format!("{base}/sing-box.json")
        );
        assert_eq!(
            route_url(&config, SubscriptionRoute::Qr(SubscriptionFormat::Uri))
                .expect("qr url builds"),
            format!("{base}/qr/uri")
        );
        assert_eq!(
            route_url(&config, SubscriptionRoute::Index).expect("index url builds"),
            format!("{base}/index")
        );
    }

    #[test]
    fn the_bare_sing_box_artifact_stays_outbounds_only_and_legacy_uri_forms_are_stable() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_direct_subscription(&fixture);
        let snapshot =
            |artifacts: Vec<(String, String)>| -> std::collections::BTreeMap<String, String> {
                artifacts.into_iter().collect()
            };
        let first =
            snapshot(generated_artifacts(&config, fixture.path()).expect("artifacts generate"));
        let second =
            snapshot(generated_artifacts(&config, fixture.path()).expect("artifacts regenerate"));
        for name in [
            "subscription-sing-box.json",
            "subscription-uri.txt",
            "subscription-base64-uri.txt",
        ] {
            assert_eq!(first[name], second[name], "{name} must be deterministic");
        }
        let bare: serde_json::Value = serde_json::from_str(&first["subscription-sing-box.json"])
            .expect("bare artifact is JSON");
        let object = bare.as_object().expect("bare artifact is a JSON object");
        assert_eq!(
            object.len(),
            1,
            "the bare sing-box artifact must stay outbounds-only"
        );
        assert!(object.contains_key("outbounds"));
        let base64 = first["subscription-base64-uri.txt"].clone();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(base64.trim())
                .expect("base64 artifact decodes"),
            first["subscription-uri.txt"].as_bytes(),
            "the base64 artifact must stay the exact URI artifact"
        );
    }

    #[test]
    fn client_overrides_merge_into_full_profiles_but_never_the_bare_artifact() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_direct_subscription(&fixture);
        let overrides = fixture.path().join("etc/sbctl/overrides");
        fs::create_dir_all(&overrides).expect("override directory is created");
        fs::write(
            overrides.join("sing-box-override.json"),
            r#"{"route":{"rules":[{"domain_suffix":["novixlink"],"outbound":"🚀节点选择"}]}}"#,
        )
        .expect("sing-box override is written");
        fs::write(
            overrides.join("clash-override.yaml"),
            "rules:\n  - DOMAIN-SUFFIX,novixlink,🚀选择代理节点\n",
        )
        .expect("clash override is written");
        let artifacts = generated_artifacts(&config, fixture.path())
            .expect("artifacts generate with overrides");
        let get = |name: &str| {
            artifacts
                .iter()
                .find(|(artifact, _)| artifact == name)
                .map(|(_, contents)| contents.clone())
                .unwrap_or_else(|| panic!("missing artifact {name}"))
        };

        let full: serde_json::Value =
            serde_json::from_str(&get("subscription-sing-box-full.json")).expect("full is JSON");
        assert_eq!(
            full["route"]["rules"][0]["domain_suffix"][0], "novixlink",
            "the override rule must be prepended to the generated route rules"
        );
        let versioned: serde_json::Value =
            serde_json::from_str(&get("subscription-sing-box-1.12.json"))
                .expect("versioned profile is JSON");
        assert_eq!(
            versioned["route"]["rules"][0]["domain_suffix"][0],
            "novixlink"
        );

        let bare: serde_json::Value =
            serde_json::from_str(&get("subscription-sing-box.json")).expect("bare is JSON");
        assert!(
            bare.get("route").is_none(),
            "the historical bare artifact must never gain override fields"
        );

        for name in ["subscription-clash.yaml", "subscription-clash-1.18.yaml"] {
            let clash: serde_yaml::Value =
                serde_yaml::from_str(&get(name)).expect("clash artifact is YAML");
            let rules = clash["rules"].as_sequence().expect("clash has rules");
            assert!(
                rules[0]
                    .as_str()
                    .expect("the first rule is a string")
                    .contains("novixlink"),
                "{name} must prepend the override rule"
            );
        }
    }

    #[test]
    fn an_invalid_override_aborts_regeneration_and_preserves_the_previous_artifacts() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, _) = seed_direct_subscription(&fixture);
        regenerate(&store, &config, None, false).expect("baseline artifacts regenerate");
        let baseline = artifact(&store, "subscription-sing-box-full.json");
        let overrides = fixture.path().join("etc/sbctl/overrides");
        fs::create_dir_all(&overrides).expect("override directory is created");
        fs::write(overrides.join("sing-box-override.json"), "{ not valid json")
            .expect("invalid override is written");

        let error = regenerate(&store, &config, None, false).expect_err("invalid override aborts");
        assert!(
            matches!(error, super::SubscriptionError::Override(_)),
            "unexpected error: {error}"
        );
        assert_eq!(
            artifact(&store, "subscription-sing-box-full.json"),
            baseline,
            "a rejected override must not touch the served artifacts"
        );
    }

    #[test]
    fn minimal_rule_profile_drops_remote_rule_sets_while_standard_keeps_them() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, mut config, _) = seed_direct_subscription(&fixture);
        let full = |config: &DeploymentConfig| {
            let artifacts =
                generated_artifacts(config, fixture.path()).expect("artifacts generate");
            let contents = artifacts
                .iter()
                .find(|(name, _)| name == "subscription-sing-box-full.json")
                .map(|(_, contents)| contents.clone())
                .expect("full artifact exists");
            serde_json::from_str::<serde_json::Value>(&contents).expect("full artifact is JSON")
        };

        config.client_rule_profile = crate::config::ClientRuleProfile::Standard;
        let standard = full(&config);
        assert!(
            standard["route"]["rule_set"]
                .as_array()
                .expect("rule_set is an array")
                .iter()
                .any(|rule| rule["tag"] == "geosite-cn"),
            "standard must reference remote rule-sets"
        );
        assert!(
            standard["route"]["rules"]
                .as_array()
                .expect("rules is an array")
                .iter()
                .any(|rule| rule.get("rule_set").is_some()),
            "standard must route through rule_set"
        );

        config.client_rule_profile = crate::config::ClientRuleProfile::Minimal;
        let minimal = full(&config);
        assert!(
            minimal["route"]["rule_set"]
                .as_array()
                .expect("rule_set is an array")
                .is_empty(),
            "minimal must not reference remote rule-sets"
        );
        for rule in minimal["route"]["rules"]
            .as_array()
            .expect("rules is an array")
        {
            assert!(
                rule.get("rule_set").is_none(),
                "minimal rules stay built-in"
            );
        }
        for rule in minimal["dns"]["rules"]
            .as_array()
            .expect("dns rules is an array")
        {
            assert!(
                rule.get("rule_set").is_none(),
                "minimal DNS rules stay built-in"
            );
        }
    }

    #[test]
    fn version_profiles_only_carry_store_dns_where_the_changelog_allows_it() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_direct_subscription(&fixture);
        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");
        for profile in SING_BOX_VERSION_PROFILES {
            let name = SubscriptionFormat::SingBoxVersion(profile.version)
                .artifact_name()
                .into_owned();
            let contents = artifacts
                .iter()
                .find(|(artifact, _)| *artifact == name)
                .map(|(_, contents)| contents)
                .unwrap_or_else(|| panic!("missing profile artifact {name}"));
            let value: serde_json::Value = serde_json::from_str(contents).expect("profile is JSON");
            let has_store_dns = value["experimental"]["cache_file"]
                .get("store_dns")
                .is_some();
            assert_eq!(
                has_store_dns, profile.supports_store_dns,
                "store_dns mismatch for {}",
                profile.version
            );
        }
    }

    #[test]
    fn pre_anytls_client_profiles_drop_the_anytls_node_and_say_so() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_all_protocols(&fixture);
        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");
        let parsed = |name: &str| -> serde_json::Value {
            let contents = artifacts
                .iter()
                .find(|(artifact, _)| artifact == name)
                .map(|(_, contents)| contents.clone())
                .unwrap_or_else(|| panic!("missing profile artifact {name}"));
            serde_json::from_str(&contents).expect("profile is JSON")
        };
        let has_node = |value: &serde_json::Value, tag: &str| {
            value["outbounds"]
                .as_array()
                .expect("outbounds is an array")
                .iter()
                .any(|outbound| outbound["tag"] == tag)
        };

        for profile in SING_BOX_VERSION_PROFILES {
            let name = SubscriptionFormat::SingBoxVersion(profile.version)
                .artifact_name()
                .into_owned();
            let value = parsed(&name);
            assert_eq!(
                has_node(&value, "sbctl-anytls"),
                profile.supports_anytls,
                "AnyTLS node presence mismatch for {name}"
            );
        }

        // 1.10: legacy DNS servers, top-level fakeip, inbound sniff, a special
        // dns outbound, and no route/domain-resolver fields.
        let legacy = parsed("subscription-sing-box-1.10.json");
        for server in legacy["dns"]["servers"].as_array().expect("dns servers") {
            assert!(server.get("address").is_some(), "1.10 DNS must be legacy");
            assert!(server.get("type").is_none(), "1.10 DNS must not be typed");
        }
        assert!(
            legacy["dns"]["fakeip"]["enabled"]
                .as_bool()
                .unwrap_or(false),
            "1.10 fake-ip must use the top-level dns.fakeip object"
        );
        assert!(
            !legacy["route"].get("default_domain_resolver").is_some(),
            "1.10 has no route.default_domain_resolver"
        );
        assert!(
            has_node(&legacy, "dns-out"),
            "1.10 hijacks DNS through a special dns outbound"
        );
        assert_eq!(
            legacy["inbounds"][0]["sniff"], true,
            "1.10 sniffs at the inbound"
        );

        // 1.11: legacy DNS but rule actions are available; no dns outbound.
        let one_eleven = parsed("subscription-sing-box-1.11.json");
        assert!(
            one_eleven["dns"]["servers"]
                .as_array()
                .expect("dns servers")
                .iter()
                .all(|server| server.get("type").is_none()),
            "1.11 DNS must stay legacy"
        );
        assert!(
            !has_node(&one_eleven, "dns-out"),
            "1.11 hijacks DNS through the hijack-dns rule action"
        );
        let rules = one_eleven["route"]["rules"]
            .as_array()
            .expect("route rules");
        assert!(
            rules.iter().any(|rule| rule["action"] == "hijack-dns"),
            "1.11 route rules use actions"
        );

        // 1.12+: typed DNS and the domain resolver default.
        let typed = parsed("subscription-sing-box-1.14.json");
        assert!(
            typed["dns"]["servers"]
                .as_array()
                .expect("dns servers")
                .iter()
                .all(|server| server.get("type").is_some()),
            "1.14 DNS must be typed"
        );
        assert_eq!(
            typed["route"]["default_domain_resolver"]["server"], "dns-direct",
            "1.14 resolves outbound server domains through default_domain_resolver"
        );
    }

    #[test]
    fn an_anytls_only_deployment_skips_pre_anytls_profiles_with_a_warning() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_single_protocol(&fixture, ManagedProtocol::Anytls);
        let artifacts =
            generated_artifacts(&config, fixture.path()).expect("other formats still generate");
        let names: Vec<&str> = artifacts.iter().map(|(name, _)| name.as_str()).collect();
        assert!(
            !names.contains(&"subscription-sing-box-1.10.json"),
            "the 1.10 profile must be skipped for an AnyTLS-only deployment"
        );
        assert!(
            !names.contains(&"subscription-sing-box-1.11.json"),
            "the 1.11 profile must be skipped for an AnyTLS-only deployment"
        );
        assert!(
            names.contains(&"subscription-clash.yaml")
                && names.contains(&"subscription-sing-box-full.json"),
            "the formats AnyTLS supports must still generate"
        );
    }

    #[test]
    fn the_client_matrix_covers_the_mainstream_clients() {
        let rows = client_subscription_matrix();
        let clients: Vec<&str> = rows.iter().map(|row| row.client).collect();
        for client in [
            "Clash Party",
            "Clash Verge",
            "sing-box",
            "V2rayN",
            "Shadowrocket",
        ] {
            assert!(
                clients.contains(&client),
                "the client matrix must cover {client}"
            );
        }
        let sing_box_row = rows
            .iter()
            .find(|row| row.client == "sing-box")
            .expect("the sing-box row exists");
        // One recommendation per version profile plus sing-box-full.
        assert_eq!(
            sing_box_row.formats.len(),
            SING_BOX_VERSION_PROFILES.len() + 1,
            "the sing-box row must recommend one format per supported version"
        );
    }

    #[test]
    fn an_ipv6_host_is_bracketed_only_where_a_uri_authority_requires_it() {
        let config = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "2001:db8::1".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::Hysteria2],
            Some("www.cloudflare.com".into()),
            ProtocolPorts::default(),
        )
        .expect("an IPv6 no-domain deployment is valid");
        let nodes = crate::canonical::nodes(&config);

        let uri = uri(&config, &nodes).expect("uri artifacts generate");
        assert!(
            uri.contains("@[2001:db8::1]:"),
            "an unbracketed IPv6 authority is not a parseable URI: {uri}"
        );
        let url = route_url(&config, SubscriptionRoute::Format(SubscriptionFormat::Uri))
            .expect("the subscription URL builds");
        assert!(
            url.starts_with("http://[2001:db8::1]:2080/sub/"),
            "the share link must bracket the IPv6 host: {url}"
        );

        let clash = clash(&config, &nodes).expect("clash artifacts generate");
        assert!(
            !clash.contains("[2001:db8::1]"),
            "configuration fields take a bare address; brackets leak into server fields"
        );
        let sing_box = sing_box(&config, &nodes).expect("sing-box artifacts generate");
        assert!(
            !sing_box.contains("[2001:db8::1]"),
            "sing-box `server` must carry the bare address"
        );
    }

    #[test]
    fn no_domain_ip_fallback_artifacts_use_the_fake_protocol_sni_and_insecure_tls() {
        let config = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![
                ManagedProtocol::VlessReality,
                ManagedProtocol::VmessWebsocket,
                ManagedProtocol::Hysteria2,
                ManagedProtocol::Tuic,
                ManagedProtocol::Anytls,
            ],
            Some("www.cloudflare.com".into()),
            ProtocolPorts::default(),
        )
        .expect("a no-domain deployment with all five protocols is valid");
        let nodes = crate::canonical::nodes(&config);

        let sing_box = sing_box(&config, &nodes).expect("sing-box artifacts generate");
        let clash = clash(&config, &nodes).expect("clash artifacts generate");
        let clash_legacy = clash_legacy(&config, &nodes).expect("legacy clash generates");
        let shadowrocket = shadowrocket(&config, &nodes).expect("shadowrocket generates");
        let sing_box_full = sing_box_full(&config, &nodes, latest_version_profile())
            .expect("full config generates");
        let uri = uri(&config, &nodes).expect("uri artifacts generate");

        assert!(
            clash.contains("  - name: 🌍选择代理节点\n    type: select\n    url: http://aliyun.com/generate_204\n    interval: 300\n    proxies:\n      - ♻️自动选择\n      - DIRECT\n"),
            "clash subscription exposes the manual selection group"
        );
        assert!(
            clash.contains("  - name: ♻️自动选择\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    tolerance: 50\n"),
            "clash subscription exposes the automatic selection group"
        );
        assert!(
            clash
                .contains("  - name: 🎯全球直连\n    type: select\n    proxies:\n      - DIRECT\n"),
            "clash subscription exposes the direct selection group"
        );
        for rule in [
            "  - DOMAIN-SUFFIX,chatgpt.com,🌍选择代理节点\n",
            "  - DOMAIN-SUFFIX,x.com,🌍选择代理节点\n",
            "  - RULE-SET,geosite-cn,🎯全球直连\n",
            "  - RULE-SET,geoip-cn,🎯全球直连\n",
            "  - MATCH,🌍选择代理节点\n",
        ] {
            assert!(
                clash.contains(rule),
                "clash subscription carries rule: {rule}"
            );
        }
        assert!(
            clash.contains(
                "url: https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/cn.mrs"
            ),
            "clash rule-providers reference the meta-branch rule-set base URL"
        );
        assert!(
            clash.contains("enhanced-mode: fake-ip\n"),
            "clash subscription defaults to fake-ip DNS"
        );
        // The legacy mihomo variant keeps the built-in GEOIP rules the 1.18
        // line shipped, so old cores never need the remote rule-providers.
        for rule in [
            "  - RULE-SET,geosite-cn,🎯全球直连\n",
            "  - GEOIP,LAN,DIRECT\n",
            "  - GEOIP,CN,DIRECT\n",
            "  - MATCH,🌍选择代理节点\n",
        ] {
            assert_eq!(
                clash_legacy.contains(rule),
                matches!(
                    rule,
                    "  - GEOIP,LAN,DIRECT\n"
                        | "  - GEOIP,CN,DIRECT\n"
                        | "  - MATCH,🌍选择代理节点\n"
                ),
                "legacy clash keeps GEOIP rules and skips rule-sets: {rule}"
            );
        }

        for artifact in [&sing_box, &clash, &uri] {
            assert!(
                artifact.contains("www.bing.com"),
                "artifact carries the default fake protocol SNI"
            );
            assert!(
                artifact.contains("www.cloudflare.com"),
                "artifact carries the Reality decoy SNI"
            );
            assert!(
                artifact.contains("203.0.113.7"),
                "artifact addresses the VPS IP rather than a domain"
            );
        }
        assert!(
            sing_box.contains("\"insecure\": true"),
            "sing-box clients skip certificate verification"
        );
        assert!(
            clash.contains("skip-cert-verify: true"),
            "clash clients skip certificate verification"
        );
        assert!(
            uri.contains("insecure=1"),
            "URI clients skip certificate verification"
        );
        // The Shadowrocket artifact decodes to URIs with the SR-specific
        // adaptations: encoded passwords and the official anytls scheme.
        {
            use base64::Engine as _;
            let decoded = String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(&shadowrocket)
                    .expect("shadowrocket artifact is valid base64"),
            )
            .expect("shadowrocket URIs are UTF-8");
            assert!(
                decoded.contains("anytls://") && decoded.contains("/?insecure="),
                "shadowrocket anytls URI follows the official scheme: {decoded}"
            );
            assert!(
                decoded.contains("tuic://") && decoded.contains("udp_relay_mode=native"),
                "shadowrocket tuic URI carries udp_relay_mode"
            );
            assert!(
                !decoded.contains("security=tls"),
                "shadowrocket URIs drop the non-standard security parameter"
            );
        }
        // The full sing-box client profile carries DNS, tun, groups, routing,
        // rule-sets, and the clash API for dashboards.
        for fragment in [
            "\"tun\"",
            "🚀节点选择",
            "♻️自动选择",
            "\"selector\"",
            "\"urltest\"",
            "geosite-cn",
            "geoip-cn",
            "clash_api",
            "cache_file",
            "\"fakeip\"",
            "223.5.5.5",
            "chatgpt.com",
        ] {
            assert!(
                sing_box_full.contains(fragment),
                "full sing-box client profile carries {fragment}"
            );
        }
    }

    #[test]
    fn self_signed_certificates_are_generated_inside_the_deployment_root_with_private_permissions()
    {
        let fixture = TempDir::new().expect("temporary root is created");
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::Hysteria2],
            None,
        )
        .expect("an IP fallback Hysteria2 deployment is valid");

        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");

        let server: serde_json::Value = serde_json::from_str(
            &artifacts
                .iter()
                .find(|(name, _)| *name == "sing-box-server.json")
                .map(|(_, contents)| contents.clone())
                .expect("server artifact is present"),
        )
        .expect("server configuration is JSON");
        let certificate_path = server["inbounds"][0]["tls"]["certificate_path"]
            .as_str()
            .expect("the TLS inbound references a certificate path");
        assert!(
            certificate_path.starts_with(fixture.path().to_str().expect("fixture path is UTF-8")),
            "the self-signed certificate is written inside the deployment root: {certificate_path}"
        );
        let directory = fixture
            .path()
            .join("var/lib/sbctl/certificates/www.bing.com");
        assert!(directory.join("key.pem").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key_mode = fs::metadata(directory.join("key.pem"))
                .expect("private key exists")
                .permissions()
                .mode();
            assert_eq!(
                key_mode & 0o777,
                0o640,
                "the TLS private key is never world-readable"
            );
            let directory_mode = fs::metadata(&directory)
                .expect("certificate directory exists")
                .permissions()
                .mode();
            assert_eq!(directory_mode & 0o777, 0o750);
        }
    }

    fn checker(fixture: &TempDir, accepts: bool) -> PathBuf {
        #[cfg(windows)]
        let path = fixture.path().join("sing-box-check.cmd");
        #[cfg(not(windows))]
        let path = fixture.path().join("sing-box-check");
        fs::write(
            &path,
            #[cfg(windows)]
            if accepts {
                "@exit /b 0\r\n"
            } else {
                "@exit /b 1\r\n"
            },
            #[cfg(not(windows))]
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

    #[test]
    fn ipv4_only_survives_persistence_and_pins_resolution() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let mut config = vless_config();
        config.ipv4_only = true;
        store.initialize(&config).unwrap();
        let config = store.load().unwrap();
        let artifacts = generated_artifacts(&config, fixture.path()).unwrap();
        let server: serde_json::Value = serde_json::from_str(
            &artifacts
                .iter()
                .find(|(name, _)| *name == "sing-box-server.json")
                .unwrap()
                .1,
        )
        .unwrap();
        // The legacy inbound field was removed in sing-box 1.13: resolution is
        // now pinned through a route action plus the default DNS strategy.
        assert!(server["inbounds"][0].get("domain_strategy").is_none());
        assert_eq!(server["dns"]["strategy"], "ipv4_only");
        let rules = server["route"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|rule| {
            rule["inbound"] == "sbctl-vless-reality"
                && rule["action"] == "resolve"
                && rule["strategy"] == "ipv4_only"
        }));
    }

    #[test]
    fn server_artifact_pins_info_logging() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = vless_config();
        store.initialize(&config).unwrap();
        let config = store.load().unwrap();
        let artifacts = generated_artifacts(&config, fixture.path()).unwrap();
        let server: serde_json::Value = serde_json::from_str(
            &artifacts
                .iter()
                .find(|(name, _)| *name == "sing-box-server.json")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(server["log"]["level"], "info");
    }

    fn write_old_artifacts(store: &DeploymentStore) {
        for (name, contents) in [
            ("sing-box-server.json", "old server".as_bytes()),
            ("subscription-sing-box.json", "old sing-box".as_bytes()),
            ("subscription-clash.yaml", "old clash".as_bytes()),
            ("subscription-uri.txt", "old uri".as_bytes()),
            ("subscription-base64-uri.txt", "old Base64 URI".as_bytes()),
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
        let expected =
            generated_artifacts(&config, fixture.path()).expect("new artifacts are generated");
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

    fn write_initial_deployment(store: &DeploymentStore, config: &DeploymentConfig) {
        let artifacts = generated_artifacts(config, store.root()).expect("artifacts generate");
        let references = artifacts
            .iter()
            .map(|(name, contents)| (name.clone(), contents.as_bytes()))
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
        let expected =
            generated_artifacts(&old, fixture.path()).expect("old artifacts are generated");
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

        let expected =
            generated_artifacts(&new, fixture.path()).expect("new artifacts are generated");
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
        let artifacts_before =
            generated_artifacts(&old, fixture.path()).expect("old artifacts are generated");
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

        let old_artifacts =
            generated_artifacts(&old, fixture.path()).expect("old artifacts are generated");
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
