//! The compile-time client content templates (ADR-0022).
//!
//! A [`ClientTemplate`] selects a [`TemplateSpec`]: the structural content a
//! full client artifact carries (proxy groups, rule sets, inline rules, DNS,
//! sniffing and the final group). The catalog is compiled in, never a file on
//! disk, so a malformed administrator paste cannot take down the default
//! subscription. `Standard` reproduces the pre-template output byte-for-byte —
//! the artifact goldens in `src/subscription/snapshots/` are the proof.
//!
//! `Global` and `Split` are declared on the axis but not implemented yet: for
//! now every template resolves to the `Standard` spec, so all three render
//! byte-for-byte identical artifacts. Their richer groups, rule sets, inline
//! rules and DNS content land in PR(c).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::DeploymentConfig;

/// The client content template axis (ADR-0022). `standard` is the historical
/// structure; `global` and `split` are declared and will diverge in PR(c).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientTemplate {
    #[default]
    Standard,
    Global,
    Split,
}

impl fmt::Display for ClientTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Standard => "standard",
            Self::Global => "global",
            Self::Split => "split",
        })
    }
}

pub fn default_client_template() -> ClientTemplate {
    ClientTemplate::Standard
}

/// A proxy group a template declares. Each renderer maps a role to its own tag
/// vocabulary, so the sing-box and clash artifacts keep their established
/// names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRole {
    Selector,
    UrlTest,
    Direct,
}

/// The outbound a template's inline rule targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboundRole {
    Selector,
    Direct,
}

/// What a template's inline rule matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleMatcher {
    /// Private address ranges.
    Private,
    /// The AI domain suffixes.
    AiDomains,
}

/// Which renderers emit a template entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleRenderers {
    SingBox,
    Clash,
    Both,
}

impl RuleRenderers {
    pub fn includes_sing_box(self) -> bool {
        matches!(self, Self::SingBox | Self::Both)
    }

    pub fn includes_clash(self) -> bool {
        matches!(self, Self::Clash | Self::Both)
    }
}

/// One routing rule the template contributes, independent of a renderer's
/// syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineRule {
    pub matcher: RuleMatcher,
    pub outbound: OutboundRole,
    pub renderers: RuleRenderers,
}

/// One proxy group the template declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupSpec {
    pub role: GroupRole,
}

/// The rule-set behaviour, which clash spells as `behavior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSetKind {
    Domain,
    IpCidr,
}

/// One external rule-set the template references, with the URL each renderer
/// resolves for it. A renderer with no URL for an entry does not emit it, which
/// is how the clash-only private rule sets stay out of the sing-box artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleSetSpec {
    pub tag: &'static str,
    pub kind: RuleSetKind,
    pub sing_box_url: Option<String>,
    pub clash_url: Option<String>,
}

/// The DNS resolver identities and the direct rule-set a template contributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DnsSpec {
    pub direct_tag: &'static str,
    pub direct_server: &'static str,
    pub proxy_tag: &'static str,
    pub proxy_server: &'static str,
    pub proxy_url: &'static str,
    pub fake_ip_tag: &'static str,
    pub fake_ip_inet4_range: &'static str,
    pub fake_ip_inet6_range: &'static str,
    /// The rule-set whose domains resolve through the direct server; `None`
    /// leaves the rule out.
    pub direct_rule_set: Option<&'static str>,
}

/// The structural content a client template carries.
///
/// Both full-profile renderers read this rather than spelling the structure out
/// inline, so a template can add or reshape content without touching the
/// renderers' version- and config-dependent mechanics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateSpec {
    pub groups: Vec<GroupSpec>,
    pub rule_sets: Vec<RuleSetSpec>,
    pub inline_rules: Vec<InlineRule>,
    pub dns: DnsSpec,
    pub sniff: bool,
    pub final_group: GroupRole,
}

impl TemplateSpec {
    /// The spec for `template`.
    ///
    /// `Global` and `Split` are declared on the `ClientTemplate` axis but are
    /// not implemented yet: every template resolves to the `Standard` catalog
    /// below, so all three render byte-for-byte identical artifacts. Their
    /// richer content lands in PR(c).
    pub fn for_template(config: &DeploymentConfig, template: ClientTemplate) -> Self {
        let _ = template;
        let sing_box_base = format!(
            "{}@sing/geo",
            config.client_rule_set_base_url.trim_end_matches('/')
        );
        let clash_base = format!(
            "{}@meta/geo",
            config.client_rule_set_base_url.trim_end_matches('/')
        );
        Self {
            groups: vec![
                GroupSpec {
                    role: GroupRole::Selector,
                },
                GroupSpec {
                    role: GroupRole::UrlTest,
                },
                GroupSpec {
                    role: GroupRole::Direct,
                },
            ],
            rule_sets: vec![
                RuleSetSpec {
                    tag: "geosite-private",
                    kind: RuleSetKind::Domain,
                    sing_box_url: None,
                    clash_url: Some(format!("{clash_base}/geosite/private.mrs")),
                },
                RuleSetSpec {
                    tag: "geoip-private",
                    kind: RuleSetKind::IpCidr,
                    sing_box_url: None,
                    clash_url: Some(format!("{clash_base}/geoip/private.mrs")),
                },
                RuleSetSpec {
                    tag: "geosite-cn",
                    kind: RuleSetKind::Domain,
                    sing_box_url: Some(format!("{sing_box_base}/geosite/cn.srs")),
                    clash_url: Some(format!("{clash_base}/geosite/cn.mrs")),
                },
                RuleSetSpec {
                    tag: "geoip-cn",
                    kind: RuleSetKind::IpCidr,
                    sing_box_url: Some(format!("{sing_box_base}/geoip/cn.srs")),
                    clash_url: Some(format!("{clash_base}/geoip/cn.mrs")),
                },
            ],
            inline_rules: vec![
                InlineRule {
                    matcher: RuleMatcher::Private,
                    outbound: OutboundRole::Direct,
                    renderers: RuleRenderers::SingBox,
                },
                InlineRule {
                    matcher: RuleMatcher::AiDomains,
                    outbound: OutboundRole::Selector,
                    renderers: RuleRenderers::Both,
                },
            ],
            dns: DnsSpec {
                direct_tag: "dns-direct",
                direct_server: "223.5.5.5",
                proxy_tag: "dns-proxy",
                proxy_server: "1.1.1.1",
                proxy_url: "https://1.1.1.1/dns-query",
                fake_ip_tag: "dns-fakeip",
                fake_ip_inet4_range: "198.18.0.0/15",
                fake_ip_inet6_range: "fc00::/18",
                direct_rule_set: Some("geosite-cn"),
            },
            sniff: true,
            final_group: GroupRole::Selector,
        }
    }
}
