use std::fs;
use std::path::Path;

use rcgen::{CertificateParams, DnType, KeyPair};
use serde_json::{Value, json};

#[cfg(unix)]
use std::io::Write;

use super::{
    AI_DOMAIN_SUFFIXES, AI_TAG, AUTO_TAG, DIRECT_TAG, FAKE_IP_FILTER_SUFFIXES, FALLBACK_TAG,
    PROXY_TAG, SELECTOR_TAG, STREAM_TAG, TELEGRAM_TAG, client_outbounds,
};
use crate::canonical::CanonicalNode;
use crate::config::{
    CertificateMode, ClientRuleProfile, DeploymentConfig, ManagedProtocol, SubscriptionMode,
};
use crate::subscription::artifacts::SubscriptionError;
use crate::subscription::profile::SingBoxVersionProfile;
use crate::subscription::template::{GroupMember, GroupTag};
use crate::subscription::{GroupRole, GroupSpec, RuleMatcher, TemplateSpec};

/// The outbound a pre-1.11 core blocks traffic with. Route rule actions arrived
/// in 1.11.0 and the `block` outbound was removed in 1.13.0, so this tag only
/// ever appears in the 1.10 profile, where `action: reject` does not exist.
const LEGACY_BLOCK_TAG: &str = "block-out";

/// A group's member list expanded into sing-box outbound tags, in declaration
/// order. [`GroupMember::AllNodes`] is the canonical node order, so a group can
/// never drift from the outbounds the same artifact carries.
fn group_members<'a>(group: &GroupSpec, node_tags: &'a [&'a str]) -> Vec<&'a str> {
    let mut members = Vec::new();
    for member in &group.members {
        match member {
            GroupMember::Group(tag) => members.push(sing_box_tag(*tag)),
            GroupMember::BuiltinDirect => members.push(DIRECT_TAG),
            GroupMember::AllNodes => members.extend(node_tags.iter().copied()),
        }
    }
    members
}

/// Maps a template group identity to the sing-box tag vocabulary. `🚀节点选择`,
/// `♻️自动选择` and `direct` are the tags every released artifact already
/// carries; the rest are new groups the richer templates declare, and they share
/// their spelling with clash because nothing has to translate them.
fn sing_box_tag(tag: GroupTag) -> &'static str {
    match tag {
        GroupTag::Selector => SELECTOR_TAG,
        GroupTag::Auto => AUTO_TAG,
        GroupTag::Direct => DIRECT_TAG,
        // Never reached: `purpose_groups` declares the fallback group clash-only,
        // and the referential test below would catch a rule naming it anyway.
        GroupTag::Fallback => FALLBACK_TAG,
        GroupTag::Proxy => PROXY_TAG,
        GroupTag::Ai => AI_TAG,
        GroupTag::Stream => STREAM_TAG,
        GroupTag::Telegram => TELEGRAM_TAG,
        GroupTag::Reject => LEGACY_BLOCK_TAG,
    }
}

/// The verdict half of a route rule, as the field pair sing-box expects.
fn sing_box_verdict(target: GroupTag, legacy_route: bool) -> serde_json::Map<String, Value> {
    let verdict = if target == GroupTag::Reject {
        // 1.10 has no rule actions, so the block verdict is an outbound there.
        if legacy_route {
            json!({"outbound": LEGACY_BLOCK_TAG})
        } else {
            json!({"action": "reject"})
        }
    } else {
        json!({"outbound": sing_box_tag(target)})
    };
    verdict
        .as_object()
        .expect("a verdict is a JSON object")
        .clone()
}

/// The matcher half of a route rule: one object per rule the matcher expands
/// into. [`RuleMatcher::Cn`] yields two (sing-box rejects a rule that mixes
/// domain and IP matchers), and a rule-set whose tags this core has no URL for
/// yields none, which is how a clash-only entry stays out of this artifact.
fn sing_box_matchers(
    spec: &TemplateSpec,
    matcher: RuleMatcher,
    twin: Option<RuleMatcher>,
    remote_rule_sets: bool,
) -> Vec<Value> {
    let rule_set_urls = |tag: &str| {
        spec.rule_sets
            .iter()
            .any(|entry| entry.tag == tag && entry.sing_box_url.is_some())
    };
    match matcher {
        RuleMatcher::Private => vec![json!({"ip_is_private": true})],
        RuleMatcher::AiDomains => vec![json!({"domain_suffix": AI_DOMAIN_SUFFIXES})],
        RuleMatcher::Cn => vec![
            json!({"domain_suffix": RuleMatcher::CN_DOMAINS}),
            json!({"ip_cidr": RuleMatcher::CN_ADDRESSES}),
        ],
        RuleMatcher::DomainSuffix(suffixes) => vec![json!({"domain_suffix": suffixes})],
        RuleMatcher::IpCidr(cidrs) => vec![json!({"ip_cidr": cidrs})],
        RuleMatcher::RuleSet(tags) => {
            // An entry this core has no URL for is not this renderer's content
            // at all — in either profile. That is how the clash-only private
            // rule sets stay out of the sing-box artifact, where `ip_is_private`
            // above already answers the question.
            let visible: Vec<&str> = tags
                .iter()
                .copied()
                .filter(|tag| rule_set_urls(tag))
                .collect();
            if visible.is_empty() {
                Vec::new()
            } else if remote_rule_sets {
                vec![json!({"rule_set": visible})]
            } else {
                // `minimal` may never name a rule CDN, so the inline twin takes
                // over the verdict — that is the whole point of `minimal_twin`.
                twin.map(|twin| sing_box_matchers(spec, twin, None, false))
                    .unwrap_or_default()
            }
        }
    }
}

/// The full sing-box client configuration for one version profile: log, DNS
/// (fake-ip with a direct resolver and a proxied DoH fallback), the tun
/// inbound, grouped outbounds, rule-set routing, and the clash API used by
/// dashboards and sbtui. Field differences between sing-box versions are
/// concentrated here, guided by the changelog research in
/// `docs/research/sing-box-client-version-differences.md`.
pub(crate) fn sing_box_full(
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
    let spec = TemplateSpec::for_template(config, config.client_template.clone());
    let remote_rule_sets = config.client_rule_profile == ClientRuleProfile::Standard;
    let node_tags: Vec<&str> = nodes.iter().map(CanonicalNode::tag).collect();
    let mut outbounds = client_outbounds(config, nodes);
    for group in spec
        .groups
        .iter()
        .filter(|group| group.renderers.includes_sing_box())
    {
        let tag = sing_box_tag(group.tag);
        let members = group_members(group, &node_tags);
        match group.role {
            GroupRole::Selector => outbounds.push(json!({
                "type": "selector",
                "tag": tag,
                "outbounds": members,
                "interrupt_exist_connections": false
            })),
            GroupRole::UrlTest => outbounds.push(json!({
                "type": "urltest",
                "tag": tag,
                "outbounds": members,
                "url": config.client_latency_probe_url,
                "interval": "5m",
                "tolerance": 50,
                "idle_timeout": "30m"
            })),
            GroupRole::Direct => {
                // A sing-box `direct` outbound takes no members: the template's
                // member list is the clash group's `DIRECT` + node list.
                outbounds.push(json!({"type": "direct", "tag": tag}));
            }
            GroupRole::Fallback => {
                // sing-box has no failover outbound type, and quietly rendering
                // the template's fallback group as a second urltest would give
                // the artifact a group that does not behave like its name. The
                // catalog declares this group clash-only; nothing to emit here.
            }
        }
    }
    // A reject verdict on a pre-1.11 core needs the block outbound to exist;
    // counted after the loop because only the emitted rules know.
    let needs_block_outbound = !profile.route_rule_actions
        && spec
            .inline_rules
            .iter()
            .filter(|rule| rule.renderers.includes_sing_box())
            .any(|rule| rule.outbound == GroupTag::Reject);

    let fake_ip = config.client_dns_mode == crate::config::ClientDnsMode::FakeIp;
    let dns_spec = &spec.dns;
    // 1.12+ requires typed DNS server objects; 1.10/1.11 only accept the
    // legacy address-string format, with fake-ip as a special `fakeip`
    // address plus a top-level dns.fakeip object (removed in 1.14).
    let mut dns = if profile.typed_dns {
        let mut dns_servers = vec![
            json!({"type": "udp", "tag": dns_spec.direct_tag, "server": dns_spec.direct_server}),
            json!({"type": "https", "tag": dns_spec.proxy_tag, "server": dns_spec.proxy_server, "detour": SELECTOR_TAG}),
        ];
        if fake_ip {
            dns_servers.push(json!({
                "type": "fakeip",
                "tag": dns_spec.fake_ip_tag,
                "inet4_range": dns_spec.fake_ip_inet4_range,
                "inet6_range": dns_spec.fake_ip_inet6_range
            }));
        }
        json!({"servers": dns_servers})
    } else {
        let mut dns_servers = vec![
            json!({"tag": dns_spec.direct_tag, "address": dns_spec.direct_server}),
            json!({"tag": dns_spec.proxy_tag, "address": dns_spec.proxy_url, "detour": SELECTOR_TAG}),
        ];
        if fake_ip {
            dns_servers.push(json!({"tag": dns_spec.fake_ip_tag, "address": "fakeip"}));
        }
        let mut dns = json!({"servers": dns_servers});
        if fake_ip {
            dns["fakeip"] = json!({
                "enabled": true,
                "inet4_range": dns_spec.fake_ip_inet4_range,
                "inet6_range": dns_spec.fake_ip_inet6_range
            });
        }
        dns
    };

    let mut dns_rules = Vec::new();
    if !profile.typed_dns {
        // Pre-1.12 cores have no route.default_domain_resolver; the legacy
        // `outbound: any` DNS rule (removed in 1.14) resolves proxy server
        // domains through direct DNS instead.
        dns_rules.push(json!({"outbound": "any", "server": dns_spec.direct_tag}));
    }
    dns_rules.push(json!({"clash_mode": "Direct", "server": dns_spec.direct_tag}));
    dns_rules.push(json!({"clash_mode": "Global", "server": dns_spec.proxy_tag}));
    if remote_rule_sets && let Some(rule_set) = dns_spec.direct_rule_set {
        dns_rules.push(json!({"rule_set": [rule_set], "server": dns_spec.direct_tag}));
    }
    dns_rules.push(json!({
        "domain_suffix": FAKE_IP_FILTER_SUFFIXES,
        "server": dns_spec.direct_tag
    }));
    if fake_ip {
        dns_rules.push(json!({"query_type": ["A", "AAAA"], "server": dns_spec.fake_ip_tag}));
    }
    // `independent_cache` is deprecated in 1.14 and removed in 1.16, and
    // brings no benefit here, so the DNS object stays lean across versions.
    dns["rules"] = json!(dns_rules);
    dns["final"] = json!(dns_spec.proxy_tag);

    let legacy_route = !profile.route_rule_actions;
    let mut tun = json!({
        "type": "tun",
        "tag": "tun-in",
        "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
        "mtu": 9000,
        "auto_route": true,
        "strict_route": true
    });
    // Inserted before the legacy `sniff` key below, so the object keeps the
    // same key order — and therefore the same bytes — as when stack was a
    // literal here. The goldens are the proof, not this comment.
    if let Some(stack) = profile.tun_stack {
        tun["stack"] = json!(stack);
    }
    if legacy_route && spec.sniff {
        // 1.10 has no route rule actions; protocol sniffing is configured on
        // the inbound and DNS is hijacked through a special `dns` outbound.
        tun["sniff"] = json!(true);
    }

    let mut route_rules = Vec::new();
    if !legacy_route && spec.sniff {
        route_rules.push(json!({"action": "sniff"}));
    }
    route_rules.push(if legacy_route {
        json!({"protocol": "dns", "outbound": "dns-out"})
    } else {
        json!({"protocol": "dns", "action": "hijack-dns"})
    });
    for rule in spec
        .inline_rules
        .iter()
        .filter(|rule| rule.renderers.includes_sing_box())
    {
        let verdict = sing_box_verdict(rule.outbound, legacy_route);
        for mut fields in
            sing_box_matchers(&spec, rule.matcher, rule.minimal_twin, remote_rule_sets)
        {
            // Matchers first, verdict last: the key order the goldens pin.
            let object = fields
                .as_object_mut()
                .expect("a matcher expands into a JSON object");
            for (key, value) in verdict.clone() {
                object.insert(key, value);
            }
            route_rules.push(fields);
        }
    }
    let mut rule_sets: Vec<Value> = Vec::new();
    if remote_rule_sets {
        for entry in spec
            .rule_sets
            .iter()
            .filter(|entry| entry.sing_box_url.is_some())
        {
            let url = entry
                .sing_box_url
                .as_deref()
                .expect("a sing-box rule-set carries a URL");
            rule_sets.push(remote_rule_set(entry.tag, url));
        }
    }
    if legacy_route {
        outbounds.push(json!({"type": "dns", "tag": "dns-out"}));
    }
    if needs_block_outbound {
        outbounds.push(json!({"type": "block", "tag": LEGACY_BLOCK_TAG}));
    }
    let final_group = sing_box_tag(spec.final_group);
    let mut route = json!({
        "rules": route_rules,
        "rule_set": rule_sets,
        "final": final_group,
        "auto_detect_interface": true
    });
    if profile.typed_dns {
        route["default_domain_resolver"] = json!({"server": dns_spec.direct_tag});
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
    config.ipv4_only || !crate::system::host_has_ipv6_route()
}

pub(crate) fn sing_box_server(
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

fn server_tls(tls_server_name: &str, certificate: &Value, alpn: &[&str]) -> Value {
    let mut tls = json!({"enabled": true, "server_name": tls_server_name,
        "certificate_path": certificate["certificate_path"],
        "key_path": certificate["key_path"]});
    if !alpn.is_empty() {
        tls["alpn"] = json!(alpn);
    }
    tls
}

#[cfg(test)]
mod tests {
    use super::sing_box_full;
    /// Two halves, because a field that is only ever read is not yet honoured:
    /// every generated profile must carry exactly the stack its own entry
    /// declares, and a profile that declares none must not inherit one.
    #[test]
    fn the_tun_stack_is_taken_from_the_profile_that_rendered_the_artifact() {
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
            let stack = value["inbounds"][0].get("stack").and_then(|v| v.as_str());
            assert_eq!(
                stack, profile.tun_stack,
                "the {} profile must carry the stack its registry entry declares",
                profile.version
            );
        }

        // The registry says "mixed" for every minor today, so the loop above
        // alone cannot tell a honoured field from a literal. This is the case
        // that can: a 1.15 entry that moved on, rendered through the same path.
        let mut moved_on = *crate::subscription::latest_version_profile();
        moved_on.tun_stack = Some("system");
        let nodes = crate::canonical::nodes(&config);
        let rendered = sing_box_full(&config, &nodes, &moved_on).expect("renders");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("rendered JSON");
        assert_eq!(value["inbounds"][0]["stack"], "system");

        moved_on.tun_stack = None;
        let rendered = sing_box_full(&config, &nodes, &moved_on).expect("renders");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("rendered JSON");
        assert!(
            value["inbounds"][0].get("stack").is_none(),
            "a profile that declares no stack must not inherit one"
        );
        assert_eq!(value["inbounds"][0]["type"], "tun");
    }

    #[cfg(unix)]
    use std::fs;

    use tempfile::TempDir;

    use crate::config::{DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode};
    use crate::subscription::test_support::{
        seed_all_protocols, seed_direct_subscription, vless_config,
    };
    use crate::subscription::{SING_BOX_VERSION_PROFILES, SubscriptionFormat, generated_artifacts};

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
}
