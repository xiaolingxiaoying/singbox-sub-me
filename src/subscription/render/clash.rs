use super::{AI_DOMAIN_SUFFIXES, AUTO_TAG, FAKE_IP_FILTER_SUFFIXES, client_skip_cert_verify};
use crate::canonical::CanonicalNode;
use crate::config::DeploymentConfig;
use crate::subscription::artifacts::SubscriptionError;
use crate::subscription::{GroupRole, OutboundRole, RuleMatcher, RuleSetKind, TemplateSpec};

/// The clash artifact's group tag vocabulary, kept apart from the sing-box
/// tags in `render/mod.rs` so neither format's names drift.
const CLASH_SELECTOR_TAG: &str = "🌍选择代理节点";
const CLASH_DIRECT_TAG: &str = "🎯全球直连";

/// The `proxies:` block plus the two historical groups shared by the current
/// and the legacy clash artifacts, so protocol fields cannot drift apart.
fn clash_proxies(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
    spec: &TemplateSpec,
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
    proxies.push_str("mode: rule\nproxy-groups:\n");
    for group in &spec.groups {
        match group.role {
            GroupRole::Selector => {
                // The selector holds DIRECT, so latency tests must use a URL
                // that is reachable without a proxy; gstatic would time out
                // from China. aliyun.com answers with a redirect, which mihomo
                // counts as success.
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: select\n    url: http://aliyun.com/generate_204\n    interval: 300\n    proxies:\n      - {auto}\n      - DIRECT\n",
                    tag = CLASH_SELECTOR_TAG,
                    auto = AUTO_TAG
                ));
                for node in nodes {
                    proxies.push_str(&format!("      - {}\n", node.tag()));
                }
            }
            GroupRole::UrlTest => {
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    tolerance: 50\n    proxies:\n",
                    tag = AUTO_TAG
                ));
                for node in nodes {
                    proxies.push_str(&format!("      - {}\n", node.tag()));
                }
            }
            GroupRole::Direct => {
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: select\n    proxies:\n      - DIRECT\n",
                    tag = CLASH_DIRECT_TAG
                ));
                for node in nodes {
                    proxies.push_str(&format!("      - {}\n", node.tag()));
                }
            }
        }
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
    let spec = TemplateSpec::for_template(config, config.client_template.clone());
    let mut output = clash_proxies(config, nodes, &spec)?;
    // The AI suffix rules must precede the CN rule-set so OpenAI/X domains
    // never fall into geosite-cn's direct verdict.
    output.push_str("rules:\n");
    for rule in spec
        .inline_rules
        .iter()
        .filter(|rule| rule.renderers.includes_clash())
    {
        if let RuleMatcher::AiDomains = rule.matcher {
            let outbound = match rule.outbound {
                OutboundRole::Selector => CLASH_SELECTOR_TAG,
                OutboundRole::Direct => CLASH_DIRECT_TAG,
            };
            for suffix in AI_DOMAIN_SUFFIXES {
                output.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},{outbound}\n"));
            }
        }
    }
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        for entry in spec
            .rule_sets
            .iter()
            .filter(|entry| entry.clash_url.is_some())
        {
            output.push_str(&format!(
                "  - RULE-SET,{tag},{direct}\n",
                tag = entry.tag,
                direct = CLASH_DIRECT_TAG
            ));
        }
    } else {
        output.push_str(concat!("  - GEOIP,LAN,DIRECT\n", "  - GEOIP,CN,DIRECT\n"));
    }
    output.push_str(&format!(
        "  - MATCH,{final_group}\n",
        final_group = clash_tag(spec.final_group)
    ));
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        output.push_str("rule-providers:\n");
        for entry in spec
            .rule_sets
            .iter()
            .filter(|entry| entry.clash_url.is_some())
        {
            let behavior = match entry.kind {
                RuleSetKind::Domain => "domain",
                RuleSetKind::IpCidr => "ipcidr",
            };
            let url = entry
                .clash_url
                .as_deref()
                .expect("a clash rule-set carries a URL");
            output.push_str(&format!(
                "  {tag}:\n    type: http\n    behavior: {behavior}\n    format: mrs\n    url: {url}\n    path: ./ruleset/{tag}.mrs\n    interval: 86400\n",
                tag = entry.tag
            ));
        }
    }
    output.push_str(&clash_dns(config));
    if spec.sniff {
        output.push_str(clash_sniffer());
    }
    Ok(output)
}

/// Maps a template group role to the clash artifact's established tag.
fn clash_tag(role: GroupRole) -> &'static str {
    match role {
        GroupRole::Selector => CLASH_SELECTOR_TAG,
        GroupRole::UrlTest => AUTO_TAG,
        GroupRole::Direct => CLASH_DIRECT_TAG,
    }
}

/// The mihomo 1.18.x compatibility artifact: same node and group layout as the
/// current artifact, but with the pre-rule-set built-in GEOIP rules that the
/// previous major line shipped everywhere.
pub(crate) fn clash_legacy(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let spec = TemplateSpec::for_template(config, config.client_template.clone());
    let mut output = clash_proxies(config, nodes, &spec)?;
    output.push_str("rules:\n");
    for rule in spec
        .inline_rules
        .iter()
        .filter(|rule| rule.renderers.includes_clash())
    {
        if let RuleMatcher::AiDomains = rule.matcher {
            for suffix in AI_DOMAIN_SUFFIXES {
                output.push_str(&format!(
                    "  - DOMAIN-SUFFIX,{suffix},{CLASH_SELECTOR_TAG}\n"
                ));
            }
        }
    }
    output.push_str(concat!("  - GEOIP,LAN,DIRECT\n", "  - GEOIP,CN,DIRECT\n"));
    output.push_str(&format!(
        "  - MATCH,{final_group}\n",
        final_group = clash_tag(spec.final_group)
    ));
    output.push_str(&clash_dns(config));
    if spec.sniff {
        output.push_str(clash_sniffer());
    }
    Ok(output)
}
