//! The compile-time client content templates (ADR-0022).
//!
//! A [`ClientTemplate`] selects a [`TemplateSpec`]: the structural content a
//! full client artifact carries (proxy groups, rule sets, inline rules, DNS,
//! sniffing and the final group). The catalog is compiled in, never a file on
//! disk, so a malformed administrator paste cannot take down the default
//! subscription. `Standard` reproduces the pre-template output byte-for-byte —
//! the artifact goldens in `src/subscription/snapshots/` are the proof.
//!
//! `Global` and `Split` are the richer catalogs the axis exists for: eight
//! declared policy groups instead of three (seven for sing-box, which has no
//! failover outbound type), eight or nine external rule-sets instead of four,
//! and one inline routing rule per verdict. Every rule-set entry carries an
//! inline twin (see [`InlineRule::minimal_twin`]) rendered from the compiled-in
//! lists below, so `client_rule_profile = "minimal"` keeps making the same
//! routing *decisions* while never naming a rule CDN — that is the regression
//! ADR-0022 closes.
//!
//! The two templates differ where the owner's spec says they must:
//!
//! - `Global` is proxy-oriented. Only private/LAN destinations get a direct
//!   verdict; the CN rule-set is routed to a dedicated proxy group instead of
//!   direct, and DNS asks no rule-set which domains to resolve directly.
//! - `Split` is split-routing oriented. CN domains and CN addresses go direct,
//!   ads are rejected, and the remaining traffic is proxied.
//!
//! Neither changes the node list: `subscription-sing-box.json`, `uri`,
//! `subscription-base64-uri.txt` and `subscription-shadowrocket.txt` stay
//! byte-identical across all three templates (ADR-0022, and the test
//! `the_node_list_artifacts_are_byte_identical_across_templates`).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::DeploymentConfig;

/// The client content template axis (ADR-0022). `standard` is the historical
/// structure; `global` and `split` are the richer catalogs defined below.
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

/// The mechanism a proxy group uses — the `type` each renderer writes. It is
/// deliberately *not* the group's identity: two groups can share a mechanism
/// (`🚀节点选择` and `🤖AI服务` are both selectors) and one identity can only
/// exist once, so a renderer cannot derive a name from the mechanism any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRole {
    Selector,
    UrlTest,
    /// clash `fallback`. sing-box has no failover outbound, so a fallback group
    /// is declared clash-only rather than silently rendered as a second urltest.
    Fallback,
    Direct,
}

/// A routing target's identity, in the template's own vocabulary. Each renderer
/// maps it to its own tag string (see `sing_box_tag` / `clash_tag` in
/// `render/`), so the sing-box and clash artifacts keep their established names
/// and a new group never renames an existing tag.
///
/// [`GroupTag::Reject`] is the odd one out: it is the renderer's built-in block
/// policy (`action: reject` / `REJECT`), not a group, so a template never
/// declares a [`GroupSpec`] for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupTag {
    Selector,
    Auto,
    Direct,
    Fallback,
    Proxy,
    Ai,
    Stream,
    Telegram,
    Reject,
}

/// The rule target vocabulary under its established name: a rule points at a
/// group identity (or the built-in reject policy). Re-exported so
/// `crate::subscription::OutboundRole` keeps resolving for the renderers while
/// both spellings address one enum.
pub use self::GroupTag as OutboundRole;

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

/// One entry in a group's member list, kept in declaration order because that
/// order is part of the frozen `Standard` bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupMember {
    /// Another group, by identity.
    Group(GroupTag),
    /// The renderer's *built-in* direct policy: sing-box's `direct` outbound tag
    /// (which is also its Direct group's tag), and clash's `DIRECT` pseudo-proxy
    /// — which is not the `🎯全球直连` group, so it needs its own variant.
    BuiltinDirect,
    /// Every node the artifact carries, in canonical order.
    AllNodes,
}

/// What a routing rule matches. Compile-time data only: every variant carries
/// what a renderer needs to write the rule without consulting a file, a
/// database or the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleMatcher {
    /// Private/LAN addresses, from the core's own built-in knowledge: sing-box
    /// `ip_is_private`, clash `GEOIP,LAN`. Only meaningful for a direct target.
    Private,
    /// The AI domain suffixes shared with the bare artifacts
    /// (`render::AI_DOMAIN_SUFFIXES`).
    AiDomains,
    /// China, from built-in or compiled-in knowledge instead of a rule-set:
    /// clash `GEOIP,CN`; sing-box has no built-in geo database on 1.12+, so it
    /// gets [`CN_DOMAIN_SUFFIXES`] *and* [`CN_IP_CIDRS`] as two separate rules
    /// (sing-box rejects a rule that mixes domain and IP matchers). Only
    /// meaningful for a direct target.
    Cn,
    /// One or more external rule-sets, by tag. A renderer with no URL for *any*
    /// of them emits nothing at all — that is how the clash-only private rule
    /// sets stay out of the sing-box artifact, where `ip_is_private` already
    /// answers the question.
    RuleSet(&'static [&'static str]),
    /// A compiled-in domain suffix list.
    DomainSuffix(&'static [&'static str]),
    /// A compiled-in CIDR list.
    IpCidr(&'static [&'static str]),
}

impl RuleMatcher {
    /// The compiled-in CN domain list, for a core that cannot answer `Cn` from
    /// its own database: sing-box removed the `geoip`/`geosite` rule fields in
    /// 1.12.0, so its CN verdict has to be spelled out as data.
    pub const CN_DOMAINS: &'static [&'static str] = CN_DOMAIN_SUFFIXES;

    /// The coarsest compiled-in CN address blocks, for the same reason. Read
    /// together with [`RuleMatcher::CN_DOMAINS`] as *two* rules, because
    /// sing-box refuses a rule that mixes domain and IP matchers.
    pub const CN_ADDRESSES: &'static [&'static str] = CN_IP_CIDRS;
}

/// One routing decision the template contributes, in first-match order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineRule {
    pub matcher: RuleMatcher,
    /// What the *same* verdict matches when no rule CDN may be contacted
    /// (`client_rule_profile = "minimal"`). Every rule-set entry in this
    /// catalog ships one, because a rule-set that simply disappears under
    /// `minimal` is the ADR-0022 regression: the destinations it used to name
    /// would fall through to the final group.
    pub minimal_twin: Option<RuleMatcher>,
    pub outbound: GroupTag,
    pub renderers: RuleRenderers,
}

/// One proxy group the template declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSpec {
    /// Identity: which tag vocabulary entry this group owns.
    pub tag: GroupTag,
    /// Mechanism: which `type` each renderer writes.
    pub role: GroupRole,
    /// Which renderers declare it. clash-only because sing-box has no
    /// equivalent (the fallback group), never because a format "forgot" one.
    pub renderers: RuleRenderers,
    pub members: Vec<GroupMember>,
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
    pub final_group: GroupTag,
}

impl TemplateSpec {
    /// The spec for `template`. `Standard` must keep reproducing the pre-axis
    /// artifacts byte-for-byte (the goldens in `snapshots/` are the gate);
    /// `Global` and `Split` are the catalogs that grew the content.
    pub fn for_template(config: &DeploymentConfig, template: ClientTemplate) -> Self {
        let bases = RuleSetBases::for_config(config);
        match template {
            ClientTemplate::Standard => Self::standard(&bases),
            ClientTemplate::Global => Self::global(&bases),
            ClientTemplate::Split => Self::split(&bases),
        }
    }

    /// The historical structure: three groups, the CN rule-sets routed direct,
    /// the AI suffixes pinned to the selector, and `ip_is_private` for LAN.
    /// Every byte here is pinned by `the_generated_artifact_set_matches_the_pinned_goldens`.
    fn standard(bases: &RuleSetBases) -> Self {
        Self {
            groups: vec![
                group(GroupTag::Selector, GroupRole::Selector, MANUAL_MEMBERS),
                group(GroupTag::Auto, GroupRole::UrlTest, ALL_NODES),
                group(GroupTag::Direct, GroupRole::Direct, &[]),
            ],
            rule_sets: vec![
                bases.clash_only("geosite-private", RuleSetKind::Domain, "geosite/private"),
                bases.clash_only("geoip-private", RuleSetKind::IpCidr, "geoip/private"),
                bases.both("geosite-cn", RuleSetKind::Domain, "geosite/cn"),
                bases.both("geoip-cn", RuleSetKind::IpCidr, "geoip/cn"),
            ],
            // The order is the artifact's order: LAN first (it must never reach
            // a proxy), then the AI suffixes (they must beat the CN direct
            // verdict below), then the rule-set verdicts.
            inline_rules: vec![
                direct_rule(RuleMatcher::Private, None, RuleRenderers::SingBox),
                rule(
                    RuleMatcher::AiDomains,
                    None,
                    GroupTag::Selector,
                    RuleRenderers::Both,
                ),
                direct_rule(
                    RuleMatcher::RuleSet(&["geosite-private", "geoip-private"]),
                    Some(RuleMatcher::Private),
                    RuleRenderers::Both,
                ),
                direct_rule(
                    RuleMatcher::RuleSet(&["geosite-cn", "geoip-cn"]),
                    Some(RuleMatcher::Cn),
                    RuleRenderers::Both,
                ),
            ],
            dns: shared_dns(Some("geosite-cn")),
            sniff: true,
            final_group: GroupTag::Selector,
        }
    }

    /// 全局代理导向: everything except private/LAN is proxied, ads are
    /// rejected, and the CN rule-set goes to a pin-able proxy group instead of
    /// direct. `final` stays the manual selector so a client that discovers its
    /// group through `route.final` keeps finding the same group as before.
    fn global(bases: &RuleSetBases) -> Self {
        Self {
            groups: purpose_groups(),
            rule_sets: vec![
                bases.clash_only("geosite-private", RuleSetKind::Domain, "geosite/private"),
                bases.clash_only("geoip-private", RuleSetKind::IpCidr, "geoip/private"),
                bases.both(
                    "geosite-ads",
                    RuleSetKind::Domain,
                    "geosite/category-ads-all",
                ),
                bases.both(
                    "geosite-proxy",
                    RuleSetKind::Domain,
                    "geosite/geolocation-!cn",
                ),
                bases.both("geosite-openai", RuleSetKind::Domain, "geosite/openai"),
                bases.both("geosite-netflix", RuleSetKind::Domain, "geosite/netflix"),
                bases.both("geosite-telegram", RuleSetKind::Domain, "geosite/telegram"),
                // `geosite-cn` is kept (its domains get the proxy group, so an
                // operator can pin a domestic node for them); `geoip-cn` is not,
                // because a template with no CN-direct verdict has nothing for a
                // CN address database to decide.
                bases.both("geosite-cn", RuleSetKind::Domain, "geosite/cn"),
            ],
            inline_rules: vec![
                // Private/LAN: the only direct verdict this template has.
                direct_rule(RuleMatcher::Private, None, RuleRenderers::SingBox),
                direct_rule(
                    RuleMatcher::RuleSet(&["geosite-private", "geoip-private"]),
                    // clash has no `private` rule-set of its own to consult
                    // under `minimal`, so its twin is the built-in LAN verdict.
                    Some(RuleMatcher::DomainSuffix(PRIVATE_DOMAIN_SUFFIXES)),
                    RuleRenderers::Both,
                ),
                // Blocking precedes every proxy verdict: an ad domain that also
                // appears in `geolocation-!cn` must still die.
                rule(
                    RuleMatcher::RuleSet(&["geosite-ads"]),
                    Some(RuleMatcher::DomainSuffix(ADS_DOMAIN_SUFFIXES)),
                    GroupTag::Reject,
                    RuleRenderers::Both,
                ),
                // The purpose groups come before the broad proxy list, or
                // `geolocation-!cn` would swallow the traffic they exist to
                // isolate (it contains `openai.com`, `netflix.com`, `t.me`…).
                rule(
                    RuleMatcher::AiDomains,
                    None,
                    GroupTag::Ai,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-openai"]),
                    Some(RuleMatcher::DomainSuffix(OPENAI_DOMAIN_SUFFIXES)),
                    GroupTag::Ai,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-netflix"]),
                    Some(RuleMatcher::DomainSuffix(NETFLIX_DOMAIN_SUFFIXES)),
                    GroupTag::Stream,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-telegram"]),
                    Some(RuleMatcher::DomainSuffix(TELEGRAM_DOMAIN_SUFFIXES)),
                    GroupTag::Telegram,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-proxy"]),
                    Some(RuleMatcher::DomainSuffix(PROXY_DOMAIN_SUFFIXES)),
                    GroupTag::Proxy,
                    RuleRenderers::Both,
                ),
                // CN: proxied, but in its own group rather than by falling
                // through to `final`, so the verdict is explicit and pin-able.
                // Its twin is domain-only on purpose: `global` has no
                // CN-address verdict, and a CN IP reaching the proxy verdict is
                // the template answering the question instead of dodging it.
                rule(
                    RuleMatcher::RuleSet(&["geosite-cn"]),
                    Some(RuleMatcher::DomainSuffix(CN_DOMAIN_SUFFIXES)),
                    GroupTag::Proxy,
                    RuleRenderers::Both,
                ),
            ],
            // No CN-direct DNS verdict: a globally proxied client should not ask
            // a domestic resolver which CN addresses to use.
            dns: shared_dns(None),
            sniff: true,
            final_group: GroupTag::Selector,
        }
    }

    /// 分流导向: CN and LAN go direct, ads die, the named services keep their
    /// own pin-able group, and only the remainder is proxied.
    fn split(bases: &RuleSetBases) -> Self {
        Self {
            groups: purpose_groups(),
            rule_sets: vec![
                bases.clash_only("geosite-private", RuleSetKind::Domain, "geosite/private"),
                bases.clash_only("geoip-private", RuleSetKind::IpCidr, "geoip/private"),
                bases.both(
                    "geosite-ads",
                    RuleSetKind::Domain,
                    "geosite/category-ads-all",
                ),
                bases.both("geosite-openai", RuleSetKind::Domain, "geosite/openai"),
                bases.both("geosite-netflix", RuleSetKind::Domain, "geosite/netflix"),
                bases.both("geosite-telegram", RuleSetKind::Domain, "geosite/telegram"),
                bases.both(
                    "geosite-proxy",
                    RuleSetKind::Domain,
                    "geosite/geolocation-!cn",
                ),
                bases.both("geosite-cn", RuleSetKind::Domain, "geosite/cn"),
                bases.both("geoip-cn", RuleSetKind::IpCidr, "geoip/cn"),
            ],
            inline_rules: vec![
                direct_rule(RuleMatcher::Private, None, RuleRenderers::SingBox),
                direct_rule(
                    RuleMatcher::RuleSet(&["geosite-private", "geoip-private"]),
                    Some(RuleMatcher::Private),
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-ads"]),
                    Some(RuleMatcher::DomainSuffix(ADS_DOMAIN_SUFFIXES)),
                    GroupTag::Reject,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::AiDomains,
                    None,
                    GroupTag::Ai,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-openai"]),
                    Some(RuleMatcher::DomainSuffix(OPENAI_DOMAIN_SUFFIXES)),
                    GroupTag::Ai,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-netflix"]),
                    Some(RuleMatcher::DomainSuffix(NETFLIX_DOMAIN_SUFFIXES)),
                    GroupTag::Stream,
                    RuleRenderers::Both,
                ),
                rule(
                    RuleMatcher::RuleSet(&["geosite-telegram"]),
                    Some(RuleMatcher::DomainSuffix(TELEGRAM_DOMAIN_SUFFIXES)),
                    GroupTag::Telegram,
                    RuleRenderers::Both,
                ),
                // The explicit "must never be intercepted domestically" list,
                // before the CN rule-set can claim the CDNs it shares with them.
                rule(
                    RuleMatcher::RuleSet(&["geosite-proxy"]),
                    Some(RuleMatcher::DomainSuffix(PROXY_DOMAIN_SUFFIXES)),
                    GroupTag::Proxy,
                    RuleRenderers::Both,
                ),
                // CN last among the direct verdicts: it is the broadest list and
                // must not beat a specific one above it.
                direct_rule(
                    RuleMatcher::RuleSet(&["geosite-cn", "geoip-cn"]),
                    Some(RuleMatcher::Cn),
                    RuleRenderers::Both,
                ),
            ],
            dns: shared_dns(Some("geosite-cn")),
            sniff: true,
            final_group: GroupTag::Selector,
        }
    }
}

/// The manual selector, the automatic group, the direct group, the clash-only
/// failover group, and the four pin-able purpose groups. Eight declarations, of
/// which sing-box renders seven (`⚠️故障转移` has no sing-box outbound type).
fn purpose_groups() -> Vec<GroupSpec> {
    vec![
        group(GroupTag::Selector, GroupRole::Selector, MANUAL_MEMBERS),
        group(GroupTag::Auto, GroupRole::UrlTest, ALL_NODES),
        group(GroupTag::Direct, GroupRole::Direct, &[]),
        // clash has a `fallback` outbound type and sing-box does not, so this
        // group is declared for the format that can honour it.
        group_with_renderers(
            GroupTag::Fallback,
            GroupRole::Fallback,
            RuleRenderers::Clash,
            ALL_NODES,
        ),
        group(GroupTag::Proxy, GroupRole::Selector, MANUAL_MEMBERS),
        group(GroupTag::Ai, GroupRole::Selector, PINNABLE_MEMBERS),
        group(GroupTag::Stream, GroupRole::Selector, PINNABLE_MEMBERS),
        group(GroupTag::Telegram, GroupRole::Selector, PINNABLE_MEMBERS),
    ]
}

/// A group both formats declare; see the four-argument form for the exception.
fn group(tag: GroupTag, role: GroupRole, members: &'static [GroupMember]) -> GroupSpec {
    group_with_renderers(tag, role, RuleRenderers::Both, members)
}

/// Which formats declare a group is part of its identity, not an afterthought:
/// a fallback group exists in clash and nowhere else.
fn group_with_renderers(
    tag: GroupTag,
    role: GroupRole,
    renderers: RuleRenderers,
    members: &'static [GroupMember],
) -> GroupSpec {
    GroupSpec {
        tag,
        role,
        renderers,
        members: members.to_vec(),
    }
}

fn rule(
    matcher: RuleMatcher,
    minimal_twin: Option<RuleMatcher>,
    outbound: GroupTag,
    renderers: RuleRenderers,
) -> InlineRule {
    InlineRule {
        matcher,
        minimal_twin,
        outbound,
        renderers,
    }
}

fn direct_rule(
    matcher: RuleMatcher,
    minimal_twin: Option<RuleMatcher>,
    renderers: RuleRenderers,
) -> InlineRule {
    rule(matcher, minimal_twin, GroupTag::Direct, renderers)
}

/// The resolver identities all three templates share: a domestic UDP resolver,
/// a DoH resolver that dials through the selector, and the fake-ip pool.
fn shared_dns(direct_rule_set: Option<&'static str>) -> DnsSpec {
    DnsSpec {
        direct_tag: "dns-direct",
        direct_server: "223.5.5.5",
        proxy_tag: "dns-proxy",
        proxy_server: "1.1.1.1",
        proxy_url: "https://1.1.1.1/dns-query",
        fake_ip_tag: "dns-fakeip",
        fake_ip_inet4_range: "198.18.0.0/15",
        fake_ip_inet6_range: "fc00::/18",
        direct_rule_set,
    }
}

/// The two rule-set branches of the one CDN knob (`client_rule_set_base_url`).
/// No other URL reaches a client artifact: a second knob would let a mirror
/// serve one format's rule-sets and not the other's.
struct RuleSetBases {
    sing_box: String,
    clash: String,
}

impl RuleSetBases {
    fn for_config(config: &DeploymentConfig) -> Self {
        let root = config.client_rule_set_base_url.trim_end_matches('/');
        Self {
            sing_box: format!("{root}@sing/geo"),
            clash: format!("{root}@meta/geo"),
        }
    }

    /// A rule-set both cores can download. `path` is the file MetaCubeX
    /// meta-rules-dat actually publishes under `geo/` (probed 2026-09-24:
    /// `geosite/{cn,private}`, `geoip/{cn,private}`, `geosite/category-ads-all`,
    /// `geosite/geolocation-!cn`, `geosite/{openai,netflix,telegram}` all
    /// resolve; `geoip/lan` does **not**, which is why LAN stays a built-in
    /// [`RuleMatcher::Private`] instead of a URL that would 404 on every
    /// client).
    fn both(&self, tag: &'static str, kind: RuleSetKind, path: &'static str) -> RuleSetSpec {
        RuleSetSpec {
            tag,
            kind,
            sing_box_url: Some(format!("{}/{}.srs", self.sing_box, path)),
            clash_url: Some(format!("{}/{}.mrs", self.clash, path)),
        }
    }

    /// A rule-set only clash downloads, because the sing-box artifact already
    /// answers that verdict from its own built-in (`ip_is_private`).
    fn clash_only(&self, tag: &'static str, kind: RuleSetKind, path: &'static str) -> RuleSetSpec {
        RuleSetSpec {
            tag,
            kind,
            sing_box_url: None,
            clash_url: Some(format!("{}/{}.mrs", self.clash, path)),
        }
    }
}

const MANUAL_MEMBERS: &[GroupMember] = &[
    GroupMember::Group(GroupTag::Auto),
    GroupMember::BuiltinDirect,
    GroupMember::AllNodes,
];

/// A purpose group holds the automatic group and the nodes: pinning a node for
/// ChatGPT must not silently offer "direct" as a member.
const PINNABLE_MEMBERS: &[GroupMember] =
    &[GroupMember::Group(GroupTag::Auto), GroupMember::AllNodes];

const ALL_NODES: &[GroupMember] = &[GroupMember::AllNodes];

/// Private/LAN names a `minimal` client resolves and routes direct instead of
/// contacting a CDN. Taken from the `private` rule-set's non-reverse entries,
/// plus the resolver suffixes the same list carries.
const PRIVATE_DOMAIN_SUFFIXES: &[&str] = &[
    "lan",
    "local",
    "localdomain",
    "home.arpa",
    "localhost",
    "tplinkwifi.net",
    "plex.direct",
    "my.router",
    "router.ctc",
    "phicomm.me",
    "tendawifi.com",
    "zte.home",
];

/// The ad networks a `minimal` client blocks without a CDN. Deliberately
/// narrower than `category-ads-all`: the full list also names `qq.com` and
/// `baidu.com`, whose first-party traffic a home user would rather keep. A
/// missed ad is a nuisance; a blocked bank is not.
const ADS_DOMAIN_SUFFIXES: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "googleoptimize.com",
    "admob.com",
    "pubmatic.com",
    "taboola.com",
    "moatads.com",
    "adsensecustomsearchads.com",
];

/// The "must never be intercepted domestically" names from
/// `geolocation-!cn`, curated to the services whose certificates clients
/// actually validate. The rule-set is the complete list.
const PROXY_DOMAIN_SUFFIXES: &[&str] = &[
    "google.com",
    "googleapis.com",
    "gstatic.com",
    "youtube.com",
    "ytimg.com",
    "github.com",
    "githubusercontent.com",
    "facebook.com",
    "instagram.com",
    "x.com",
    "twitter.com",
    "twimg.com",
    "wikipedia.org",
    "discord.com",
    "whatsapp.net",
];

/// OpenAI's own domains, from the `openai` rule-set.
const OPENAI_DOMAIN_SUFFIXES: &[&str] = &[
    "chatgpt.com",
    "openai.com",
    "oaistatic.com",
    "oaiusercontent.com",
    "sora.com",
    "chat.com",
];

/// Netflix's streaming and edge domains, from the `netflix` rule-set
/// (`fast.com` is Netflix's own speed test).
const NETFLIX_DOMAIN_SUFFIXES: &[&str] = &[
    "netflix.com",
    "nflxvideo.net",
    "nflxso.net",
    "nflxext.com",
    "nflximg.net",
    "nflximg.com",
    "fast.com",
];

/// Telegram's service and CDN domains, from the `telegram` rule-set.
const TELEGRAM_DOMAIN_SUFFIXES: &[&str] = &[
    "t.me",
    "telegram.org",
    "telegram.me",
    "telegram.dog",
    "telegram.space",
    "telegram-cdn.org",
    "telegra.ph",
    "tdesktop.com",
    "fragment.com",
    "graph.org",
    "tg.dev",
    "ton.org",
];

/// The CN services a `minimal` client still reaches directly. A curated subset
/// of `geosite/cn` (every entry below is in that rule-set), because the
/// compiled-in twin decides what people actually type while the CDN holds the
/// whole database.
const CN_DOMAIN_SUFFIXES: &[&str] = &[
    "baidu.com",
    "qq.com",
    "taobao.com",
    "tmall.com",
    "jd.com",
    "163.com",
    "sohu.com",
    "weibo.com",
    "bilibili.com",
    "youku.com",
    "iqiyi.com",
    "xiaomi.com",
    "huawei.com",
    "meituan.com",
    "dianping.com",
    "ctrip.com",
    "douyin.com",
    "toutiao.com",
    "zhihu.com",
    "alipay.com",
    "aliyun.com",
    "tencent.com",
    "abchina.com",
    "ccb.com",
];

/// The coarsest CN allocations (every `/10` and `/11` block of `geoip/cn`) for a
/// `minimal` client. Restated as explicit CIDRs rather than a geo code because
/// sing-box removed the `geoip`/`geosite` rule fields in 1.12.0, so a compiled-in
/// list is the only form that is valid on all five supported minors.
const CN_IP_CIDRS: &[&str] = &[
    "27.192.0.0/11",
    "36.96.0.0/11",
    "36.160.0.0/11",
    "36.192.0.0/11",
    "39.64.0.0/11",
    "39.128.0.0/10",
    "47.96.0.0/11",
    "49.64.0.0/11",
    "58.32.0.0/11",
    "58.192.0.0/11",
    "59.32.0.0/11",
    "59.192.0.0/10",
    "60.0.0.0/11",
    "60.160.0.0/11",
    "61.128.0.0/11",
    "110.192.0.0/11",
    "111.0.0.0/10",
    "111.128.0.0/11",
    "112.0.0.0/10",
    "112.224.0.0/11",
    "113.64.0.0/10",
    "114.224.0.0/11",
    "115.192.0.0/11",
    "116.128.0.0/10",
    "117.160.0.0/11",
    "120.192.0.0/10",
    "122.64.0.0/11",
    "123.64.0.0/11",
    "175.64.0.0/11",
    "180.96.0.0/11",
    "182.96.0.0/11",
    "183.0.0.0/10",
    "183.128.0.0/11",
    "183.192.0.0/10",
    "218.64.0.0/11",
    "219.128.0.0/11",
    "222.32.0.0/11",
    "222.64.0.0/11",
    "222.192.0.0/11",
    "223.64.0.0/11",
];

#[cfg(test)]
mod tests {
    use super::{
        CN_DOMAIN_SUFFIXES, CN_IP_CIDRS, ClientTemplate, GroupRole, GroupTag, RuleMatcher,
        TemplateSpec,
    };
    use crate::config::{DeploymentConfig, ManagedProtocol, SubscriptionMode};

    fn config() -> DeploymentConfig {
        DeploymentConfig::new(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("a direct VLESS deployment is valid")
    }

    fn spec(template: ClientTemplate) -> TemplateSpec {
        TemplateSpec::for_template(&config(), template)
    }

    /// The seam is only real if `for_template` reads its argument: a stub that
    /// returned one catalog for every template would pass every byte test here.
    #[test]
    fn each_template_resolves_to_its_own_catalog() {
        let standard = spec(ClientTemplate::Standard);
        let global = spec(ClientTemplate::Global);
        let split = spec(ClientTemplate::Split);

        assert_eq!(standard.groups.len(), 3, "standard keeps its three groups");
        assert_eq!(standard.rule_sets.len(), 4);
        assert_eq!(global.groups.len(), 8);
        assert_eq!(split.groups.len(), 8);
        assert_eq!(global.rule_sets.len(), 8);
        assert_eq!(split.rule_sets.len(), 9);
        assert_ne!(standard.inline_rules, global.inline_rules);
        assert_ne!(global.inline_rules, split.inline_rules);
        assert_ne!(global.dns, split.dns, "global has no CN DNS verdict");

        for tag in [
            GroupTag::Proxy,
            GroupTag::Ai,
            GroupTag::Stream,
            GroupTag::Telegram,
            GroupTag::Fallback,
        ] {
            assert!(
                global
                    .groups
                    .iter()
                    .any(|group| group.tag == tag && standard.groups.iter().all(|g| g.tag != tag)),
                "{tag:?} must be declared by the richer templates only"
            );
        }
    }

    /// A `minimal` client that loses a rule-set but keeps no twin stops making
    /// that verdict and silently proxies what it used to send direct. Every
    /// rule-set-backed rule therefore has to arrive with its own twin.
    #[test]
    fn every_rule_set_backed_rule_carries_a_minimal_twin() {
        for template in [
            ClientTemplate::Standard,
            ClientTemplate::Global,
            ClientTemplate::Split,
        ] {
            let spec = spec(template.clone());
            for rule in spec
                .inline_rules
                .iter()
                .filter(|rule| matches!(rule.matcher, RuleMatcher::RuleSet(_)))
            {
                assert!(
                    rule.minimal_twin.is_some(),
                    "{template} rule for {rule:?} has no inline twin"
                );
            }
            // And a twin must never reference a rule-set: that is the CDN again.
            for rule in &spec.inline_rules {
                if let Some(twin) = rule.minimal_twin {
                    assert!(
                        !matches!(twin, RuleMatcher::RuleSet(_)),
                        "{template} minimal twin references a rule-set"
                    );
                }
            }
        }
    }

    /// The CN twin is the one that mattered: it is the only thing standing
    /// between a minimal client and a proxied 银行 App.
    #[test]
    fn the_cn_twin_carries_both_the_domain_and_address_lists_it_needs() {
        assert!(
            CN_DOMAIN_SUFFIXES.len() >= 20,
            "the compiled-in CN domain twin has to be more than a token list"
        );
        assert!(
            CN_IP_CIDRS.iter().all(|cidr| {
                let prefix: u8 = cidr
                    .rsplit('/')
                    .next()
                    .expect("a cidr has a prefix")
                    .parse()
                    .expect("a numeric prefix");
                prefix <= 11
            }),
            "the CN address twin stays at the coarsest allocations"
        );
        assert!(
            !CN_IP_CIDRS.iter().any(|cidr| cidr.starts_with("0.0.0.0")),
            "a 0/0 in the CN list would send everything direct"
        );
    }

    /// A fallback group is a clash concept: sing-box has no such outbound type,
    /// so declaring it for both formats would make the artifact lie.
    #[test]
    fn the_fallback_group_is_declared_for_clash_only() {
        for template in [ClientTemplate::Global, ClientTemplate::Split] {
            let spec = spec(template.clone());
            let fallback = spec
                .groups
                .iter()
                .find(|group| group.role == GroupRole::Fallback)
                .expect("the richer templates declare a fallback group");
            assert_eq!(fallback.tag, GroupTag::Fallback);
            assert!(fallback.renderers.includes_clash());
            assert!(!fallback.renderers.includes_sing_box());
        }
    }
}
