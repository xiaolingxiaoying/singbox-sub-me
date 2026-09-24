use super::{
    AI_DOMAIN_SUFFIXES, AI_TAG, AUTO_TAG, FAKE_IP_FILTER_SUFFIXES, FALLBACK_TAG, PROXY_TAG,
    STREAM_TAG, TELEGRAM_TAG, client_skip_cert_verify,
};
use crate::canonical::CanonicalNode;
use crate::config::{ClientRuleProfile, DeploymentConfig};
use crate::subscription::artifacts::SubscriptionError;
use crate::subscription::template::{GroupMember, GroupTag};
use crate::subscription::{GroupRole, GroupSpec, RuleMatcher, RuleSetKind, TemplateSpec};

/// The clash artifact's group tag vocabulary, kept apart from the sing-box
/// tags in `render/mod.rs` so neither format's names drift.
const CLASH_SELECTOR_TAG: &str = "🌍选择代理节点";
const CLASH_DIRECT_TAG: &str = "🎯全球直连";
/// The core's own direct policy, which is what a `GEOIP,LAN` / `GEOIP,CN`
/// verdict has always carried here: the pseudo-proxy, not the `🎯全球直连`
/// group that wraps it for the user.
const CLASH_BUILTIN_DIRECT: &str = "DIRECT";

/// Maps a template group identity to the clash artifact's established tag.
fn clash_tag(tag: GroupTag) -> &'static str {
    match tag {
        GroupTag::Selector => CLASH_SELECTOR_TAG,
        GroupTag::Auto => AUTO_TAG,
        GroupTag::Direct => CLASH_DIRECT_TAG,
        GroupTag::Fallback => FALLBACK_TAG,
        GroupTag::Proxy => PROXY_TAG,
        GroupTag::Ai => AI_TAG,
        GroupTag::Stream => STREAM_TAG,
        GroupTag::Telegram => TELEGRAM_TAG,
        // mihomo's built-in block policy: a rule target, never a proxy group, so
        // no template declares a `GroupSpec` for it.
        GroupTag::Reject => "REJECT",
    }
}

/// The `proxies:` block plus every group the template declares.
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
    for group in spec
        .groups
        .iter()
        .filter(|group| group.renderers.includes_clash())
    {
        let tag = clash_tag(group.tag);
        match group.role {
            GroupRole::Selector => {
                // The selector holds DIRECT, so latency tests must use a URL
                // that is reachable without a proxy; gstatic would time out
                // from China. aliyun.com answers with a redirect, which mihomo
                // counts as success.
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: select\n    url: http://aliyun.com/generate_204\n    interval: 300\n    proxies:\n"
                ));
            }
            GroupRole::UrlTest => {
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    tolerance: 50\n    proxies:\n"
                ));
            }
            GroupRole::Fallback => {
                // `url`/`interval` are the two probing fields this file already
                // writes for a url-test group; a fallback group's remaining knobs
                // (`timeout`, `max-failed`, `lazy`) have never been sent through
                // the pinned core here, so they stay at its defaults rather than
                // becoming an unverified claim.
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: fallback\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    proxies:\n"
                ));
            }
            GroupRole::Direct => {
                // No probe URL: this group exists so the client can see which
                // verdict "direct" is, and the nodes are in it only so a user
                // can promote one of them by hand.
                proxies.push_str(&format!(
                    "  - name: {tag}\n    type: select\n    proxies:\n"
                ));
            }
        }
        for member in clash_members(group, nodes) {
            proxies.push_str(&format!("      - {member}\n"));
        }
    }
    Ok(proxies)
}

/// A group's members as clash proxy names, in declaration order.
fn clash_members(group: &GroupSpec, nodes: &[CanonicalNode]) -> Vec<String> {
    if group.role == GroupRole::Direct {
        // The direct group opens with the core's own `DIRECT` pseudo-proxy —
        // the tag its built-in geo verdicts (`GEOIP,LAN`, `GEOIP,CN`) resolve
        // to as well — and then lists the nodes, which is the historical shape.
        let mut members = vec![CLASH_BUILTIN_DIRECT.to_owned()];
        members.extend(nodes.iter().map(|node| node.tag().to_owned()));
        return members;
    }
    let mut members = Vec::new();
    for member in &group.members {
        match member {
            GroupMember::Group(tag) => members.push(clash_tag(*tag).to_owned()),
            GroupMember::BuiltinDirect => members.push(CLASH_BUILTIN_DIRECT.to_owned()),
            GroupMember::AllNodes => members.extend(nodes.iter().map(|node| node.tag().to_owned())),
        }
    }
    members
}

/// The `rules:` block, shared by the current and the legacy artifact. The two
/// differ only in whether external rule-sets exist at all: mihomo 1.18 has no
/// `.mrs` rule-providers to reference, which is precisely the `minimal` case, so
/// the legacy artifact renders the twins and nothing else.
fn clash_rules(spec: &TemplateSpec, remote_rule_sets: bool) -> String {
    let mut rules = String::from("rules:\n");
    for rule in spec
        .inline_rules
        .iter()
        .filter(|rule| rule.renderers.includes_clash())
    {
        // Which matcher survives to the artifact. An entry clash has no URL for
        // is not this renderer's content in either profile, and a rule-set rule
        // under `minimal` is the twin — because naming the CDN is exactly what
        // that profile forbids. Anything already inline needs no substitution.
        let matcher = match rule.matcher {
            RuleMatcher::RuleSet(tags) => {
                let referenced = tags.iter().copied().any(|tag| {
                    spec.rule_sets
                        .iter()
                        .any(|entry| entry.tag == tag && entry.clash_url.is_some())
                });
                if !referenced {
                    continue;
                }
                if remote_rule_sets {
                    rule.matcher
                } else {
                    match rule.minimal_twin {
                        Some(twin) => twin,
                        None => continue,
                    }
                }
            }
            other => other,
        };
        let target = match matcher {
            // The two built-in geo codes have always carried the core's own
            // `DIRECT` policy here, not the `🎯全球直连` group that wraps it.
            RuleMatcher::Private | RuleMatcher::Cn => CLASH_BUILTIN_DIRECT,
            _ => clash_tag(rule.outbound),
        };
        match matcher {
            // Both built-in geo codes, kept as the inline form they have always
            // taken here: the pinned core answers them from its own database, so
            // no CDN and no compiled-in list is involved.
            RuleMatcher::Private => rules.push_str(&format!("  - GEOIP,LAN,{target}\n")),
            RuleMatcher::Cn => rules.push_str(&format!("  - GEOIP,CN,{target}\n")),
            RuleMatcher::AiDomains => {
                for suffix in AI_DOMAIN_SUFFIXES {
                    rules.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},{target}\n"));
                }
            }
            RuleMatcher::DomainSuffix(suffixes) => {
                for suffix in suffixes.iter() {
                    rules.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},{target}\n"));
                }
            }
            RuleMatcher::IpCidr(cidrs) => {
                for cidr in cidrs.iter() {
                    // `no-resolve` keeps an IP rule from triggering a lookup of
                    // the destination it is about to match on.
                    rules.push_str(&format!("  - IP-CIDR,{cidr},{target},no-resolve\n"));
                }
            }
            RuleMatcher::RuleSet(tags) => {
                for tag in tags.iter().copied().filter(|tag| {
                    spec.rule_sets
                        .iter()
                        .any(|entry| entry.tag == *tag && entry.clash_url.is_some())
                }) {
                    rules.push_str(&format!("  - RULE-SET,{tag},{target}\n"));
                }
            }
        }
    }
    rules.push_str(&format!(
        "  - MATCH,{final_group}\n",
        final_group = clash_tag(spec.final_group)
    ));
    rules
}

/// The `rule-providers:` block for the rule-sets this template references.
fn clash_rule_providers(spec: &TemplateSpec) -> String {
    let mut providers = String::from("rule-providers:\n");
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
        providers.push_str(&format!(
            "  {tag}:\n    type: http\n    behavior: {behavior}\n    format: mrs\n    url: {url}\n    path: ./ruleset/{tag}.mrs\n    interval: 86400\n",
            tag = entry.tag
        ));
    }
    providers
}

pub(crate) fn clash(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let spec = TemplateSpec::for_template(config, config.client_template.clone());
    let remote_rule_sets = config.client_rule_profile == ClientRuleProfile::Standard;
    let mut output = clash_proxies(config, nodes, &spec)?;
    // The AI suffix rules must precede the CN rule-set so OpenAI/X domains
    // never fall into geosite-cn's direct verdict; the template's own rule order
    // already encodes that, and the renderer keeps it.
    output.push_str(&clash_rules(&spec, remote_rule_sets));
    if remote_rule_sets {
        output.push_str(&clash_rule_providers(&spec));
    }
    output.push_str(&clash_dns(config));
    if spec.sniff {
        output.push_str(clash_sniffer());
    }
    Ok(output)
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

/// The mihomo 1.18.x compatibility artifact: same node and group layout as the
/// current artifact, but rendered without external rule-sets, because that line
/// shipped with the built-in `GEOIP` rules everywhere and must not be asked to
/// fetch an `.mrs` it cannot parse. The `minimal` twin path is therefore the
/// only routing content it carries — which is also why the two renderers share
/// [`clash_rules`] instead of keeping a second copy of the rule vocabulary.
pub(crate) fn clash_legacy(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let spec = TemplateSpec::for_template(config, config.client_template.clone());
    let mut output = clash_proxies(config, nodes, &spec)?;
    output.push_str(&clash_rules(&spec, false));
    output.push_str(&clash_dns(config));
    if spec.sniff {
        output.push_str(clash_sniffer());
    }
    Ok(output)
}
