use super::{AI_DOMAIN_SUFFIXES, FAKE_IP_FILTER_SUFFIXES, client_skip_cert_verify};
use crate::canonical::CanonicalNode;
use crate::config::DeploymentConfig;
use crate::subscription::artifacts::SubscriptionError;

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

/// The `sniffer:` block shared by both clash artifacts.
///
/// mihomo ships sniffing **off** (`Enable: false` in `DefaultRawConfig`, with no
/// sniffer names selected), so a subscriber who never touches the client's own
/// settings gets connections routed on the raw SNI/host only.
///
/// The list is `http`/`tls`/`quic` because that is exactly what the pinned core
/// accepts: `.scratch/mihomo-sniff-probe.sh` runs candidate names through
/// mihomo v1.19.30's own config parser, which rejects anything else with
/// `not find the sniffer[domain]` — the `domain` and `dns` names some guides
/// advertise do not exist in this build.
///
/// No `dns-hijack` is written on purpose. It lives under `tun:`, its default is
/// already `0.0.0.0:53`, and emitting a `tun:` block from a subscription would
/// overwrite whatever the client operator configured there.
///
/// `override-destination` stays unset: it makes a sniffed domain replace the
/// original destination for the whole connection, which changes what the remote
/// server sees, and that is not this project's call to make silently.
fn clash_sniffer() -> &'static str {
    concat!(
        "sniffer:\n",
        "  enable: true\n",
        "  sniffing:\n",
        "    - http\n",
        "    - tls\n",
        "    - quic\n"
    )
}

pub(crate) fn clash(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
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
    output.push_str(clash_sniffer());
    Ok(output)
}

/// The mihomo 1.18.x compatibility artifact: same node and group layout as the
/// current artifact, but with the pre-rule-set built-in GEOIP rules that the
/// previous major line shipped everywhere.
pub(crate) fn clash_legacy(
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
    output.push_str(clash_sniffer());
    Ok(output)
}
