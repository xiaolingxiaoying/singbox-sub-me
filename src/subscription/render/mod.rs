//! The per-format subscription renderers: the bare sing-box outbound list, the
//! full client profiles, the clash artefacts and the share-link URIs. The group
//! tags and the shared domain lists live here so no two formats can drift apart.
mod clash;
mod singbox;
mod uri;

use serde_json::{Value, json};

use crate::canonical::CanonicalNode;
use crate::config::{CertificateMode, DeploymentConfig, ManagedProtocol};
use crate::subscription::artifacts::SubscriptionError;

pub(super) use clash::{clash, clash_legacy};
pub(super) use singbox::{sing_box_full, sing_box_server};
pub(super) use uri::{insecure_flag, node_uri, shadowrocket, uri};

pub(crate) fn ensure_subscription_nodes(
    config: &DeploymentConfig,
) -> Result<(), SubscriptionError> {
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

pub(super) fn sing_box(
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
pub(super) fn client_outbounds(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Vec<Value> {
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
pub(super) const DIRECT_TAG: &str = "direct";

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
pub(super) const FAKE_IP_FILTER_SUFFIXES: &[&str] = &[
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

/// Clients connecting to a self-signed certificate must be told to skip
/// verification; the domain certificate is verified normally.
pub(super) fn client_skip_cert_verify(config: &DeploymentConfig) -> bool {
    config.certificate_mode == CertificateMode::SelfSigned
}

#[cfg(test)]
mod tests {
    use super::{clash, clash_legacy, shadowrocket, sing_box, sing_box_full, uri};
    use crate::config::{DeploymentConfig, ManagedProtocol, ProtocolPorts, SubscriptionMode};
    use crate::subscription::latest_version_profile;

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
}
