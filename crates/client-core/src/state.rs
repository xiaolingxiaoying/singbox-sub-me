use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::clash_api::{ConnectionsSnapshot, OutboundMode, ProxyGroup};
use crate::settings::{Profile, Settings};
use crate::subscription::SubscriptionUserinfo;
use crate::system_proxy::TrafficMode;

/// How many traffic samples the snapshot keeps for the rate history graph.
/// At the controller's one-second cadence this is a five-minute window.
pub const TRAFFIC_HISTORY: usize = 300;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub name: String,
    pub url: String,
    pub last_updated: u64,
    pub active: bool,
}

impl ProfileSummary {
    pub fn from_profile(profile: &Profile, active: bool) -> Self {
        Self {
            name: profile.name.clone(),
            url: profile.url.clone(),
            last_updated: profile.last_updated,
            active,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyGroupSnapshot {
    pub name: String,
    pub kind: String,
    pub current: String,
    pub members: Vec<String>,
    pub delays: HashMap<String, u64>,
    /// Members whose latest latency test failed or timed out.
    pub failed: Vec<String>,
}

impl ProxyGroupSnapshot {
    /// Whether the group is an automatic `urltest` group rather than a manual
    /// selector. UIs use this to decide whether a member can be picked.
    pub fn is_auto(&self) -> bool {
        self.kind.eq_ignore_ascii_case("urltest")
    }
}

impl From<ProxyGroup> for ProxyGroupSnapshot {
    fn from(group: ProxyGroup) -> Self {
        Self {
            name: group.name,
            kind: group.kind,
            current: group.now,
            members: group.all,
            delays: HashMap::new(),
            failed: Vec::new(),
        }
    }
}

/// The group that holds the user's manual node choice.
///
/// Both clients used to compare against the literal `🚀节点选择`, which is only
/// the tag *this project's own sing-box profile* emits: the Mihomo artifact
/// names the same idea `🌍选择代理节点`, and a third-party subscription names it
/// something else again, so "current node" quietly went stale there. The core
/// already answers the question through `route.final`, so ask it first, then
/// fall back to a group's *shape* rather than to another hard-coded name.
pub fn find_selector_group<'a>(
    groups: &'a [ProxyGroupSnapshot],
    rules: &[RouteRuleSnapshot],
) -> Option<&'a ProxyGroupSnapshot> {
    let final_outbound = rules
        .iter()
        .find(|rule| rule.kind == RuleKind::Final)
        .map(|rule| rule.outbound.as_str());
    groups
        .iter()
        .find(|group| Some(group.name.as_str()) == final_outbound)
        .or_else(|| {
            groups
                .iter()
                .find(|group| group.name == crate::clash_api::SELECTOR_TAG)
        })
        .or_else(|| {
            groups
                .iter()
                .find(|group| group.kind.eq_ignore_ascii_case("selector"))
        })
        .or_else(|| groups.iter().find(|group| !group.is_auto()))
        .or_else(|| groups.first())
}

/// One `{up, down}` traffic sample for the dashboard history graph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficPoint {
    pub up: u64,
    pub down: u64,
}

/// One routing rule of the active configuration, rendered for the UIs' rules
/// views. `outbound` is the target outbound tag, or the rule's `action` when
/// What a route rule matches on. The kind is interface copy, so each client
/// labels it in its own language; the value beside it (domains, ports,
/// protocols) is configuration data and is never translated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleKind {
    DomainSuffix,
    Domain,
    DomainKeyword,
    RuleSet,
    IpCidr,
    Private,
    Protocol,
    Network,
    Port,
    #[default]
    Other,
    /// The `route.final` fall-through.
    Final,
}

impl RuleKind {
    pub fn zh(self) -> &'static str {
        match self {
            Self::DomainSuffix => "域名后缀",
            Self::Domain => "域名",
            Self::DomainKeyword => "域名关键字",
            Self::RuleSet => "规则集",
            Self::IpCidr => "网段",
            Self::Private => "私有地址",
            Self::Protocol => "协议",
            Self::Network => "网络",
            Self::Port => "端口",
            Self::Other => "其他匹配条件",
            Self::Final => "其他未命中流量",
        }
    }

    pub fn en(self) -> &'static str {
        match self {
            Self::DomainSuffix => "Domain suffix",
            Self::Domain => "Domain",
            Self::DomainKeyword => "Domain keyword",
            Self::RuleSet => "Rule set",
            Self::IpCidr => "IP range",
            Self::Private => "Private address",
            Self::Protocol => "Protocol",
            Self::Network => "Network",
            Self::Port => "Port",
            Self::Other => "Other condition",
            Self::Final => "Unmatched traffic",
        }
    }
}

/// the rule uses a non-forward action (sing-box 1.11+).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRuleSnapshot {
    pub kind: RuleKind,
    /// The matched value, verbatim from the configuration.
    pub value: Option<String>,
    pub outbound: String,
}

impl RouteRuleSnapshot {
    /// The Chinese one-line form the terminal client prints.
    pub fn matcher_zh(&self) -> String {
        match &self.value {
            Some(value) => format!("{} · {value}", self.kind.zh()),
            None => self.kind.zh().to_owned(),
        }
    }
}

/// One `route.rule_set` entry of the active configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSetSummary {
    pub tag: String,
    pub url: String,
    /// `remote` or `local`, as declared in the configuration.
    pub kind: String,
}

/// Extracts the routing rules and rule-set summaries the UIs' rules views
/// render. Matching conditions collapse into one human line; both clients
/// previously parsed this themselves from `cache/active-config.json`, so the
/// engine now publishes it once and the UIs stay identical.
/// One inbound of the configuration the core was actually started from.
///
/// The client used to describe only what it creates itself (the mixed or tun
/// inbound it injects), never what the imported subscription declares, so an
/// operator could not see that a profile listens on something unexpected
/// without opening the JSON by hand.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InboundInfo {
    /// `mixed`, `tun`, `shadowsocks`, ...
    pub kind: String,
    pub tag: String,
    /// The literal `listen` value; empty when the inbound binds every address.
    pub listen: String,
    /// 0 when the inbound declares no port (a tun inbound, for instance).
    pub port: u16,
}

/// Reads the top-level `inbounds` array. Bad or absent JSON yields no inbounds
/// rather than an error: this feeds a status view, not a decision.
pub fn parse_inbounds(config_text: &str) -> Vec<InboundInfo> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(config_text) else {
        return Vec::new();
    };
    let Some(inbounds) = value.get("inbounds").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    inbounds
        .iter()
        .map(|inbound| {
            let text = |key: &str| {
                inbound
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned()
            };
            InboundInfo {
                kind: text("type"),
                tag: text("tag"),
                listen: text("listen"),
                port: inbound
                    .get("listen_port")
                    .and_then(|v| v.as_u64())
                    .and_then(|port| u16::try_from(port).ok())
                    .unwrap_or(0),
            }
        })
        .collect()
}

pub fn parse_route_rules(config_text: &str) -> (Vec<RouteRuleSnapshot>, Vec<RuleSetSummary>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(config_text) else {
        return (Vec::new(), Vec::new());
    };
    let Some(route) = value.get("route") else {
        return (Vec::new(), Vec::new());
    };
    let rule_sets = route
        .get("rule_set")
        .and_then(|sets| sets.as_array())
        .map(|sets| {
            sets.iter()
                .map(|set| RuleSetSummary {
                    tag: set
                        .get("tag")
                        .and_then(|value| value.as_str())
                        .unwrap_or("?")
                        .to_owned(),
                    url: set
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_owned(),
                    kind: set
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("remote")
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut rules = Vec::new();
    if let Some(list) = route.get("rules").and_then(|rules| rules.as_array()) {
        for rule in list {
            let outbound = rule
                .get("outbound")
                .or_else(|| rule.get("action"))
                .and_then(|value| value.as_str())
                .unwrap_or("未指定")
                .to_owned();
            let (kind, value) = rule_matcher(rule);
            rules.push(RouteRuleSnapshot {
                kind,
                value,
                outbound,
            });
        }
    }
    if let Some(final_outbound) = route.get("final").and_then(|value| value.as_str()) {
        rules.push(RouteRuleSnapshot {
            kind: RuleKind::Final,
            value: None,
            outbound: final_outbound.to_owned(),
        });
    }
    (rules, rule_sets)
}

/// One human-readable line describing a rule's matching conditions.
fn rule_matcher(rule: &serde_json::Value) -> (RuleKind, Option<String>) {
    let list_value = |key: &str| {
        rule.get(key)
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join("、")
            })
    };
    let plain_value = |key: &str| {
        rule.get(key)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    };
    if let Some(value) = list_value("domain_suffix") {
        return (RuleKind::DomainSuffix, Some(value));
    }
    if let Some(value) = list_value("domain") {
        return (RuleKind::Domain, Some(value));
    }
    if let Some(value) = list_value("domain_keyword") {
        return (RuleKind::DomainKeyword, Some(value));
    }
    if let Some(value) = list_value("rule_set") {
        return (RuleKind::RuleSet, Some(value));
    }
    if let Some(value) = list_value("ip_cidr") {
        return (RuleKind::IpCidr, Some(value));
    }
    if rule.get("ip_is_private").is_some() {
        return (RuleKind::Private, None);
    }
    if let Some(protocol) = plain_value("protocol") {
        return (RuleKind::Protocol, Some(protocol));
    }
    if let Some(network) = plain_value("network") {
        return (RuleKind::Network, Some(network));
    }
    if let Some(port) = plain_value("port").or_else(|| {
        rule.get("port")
            .and_then(|value| value.as_u64())
            .map(|value| value.to_string())
    }) {
        return (RuleKind::Port, Some(port));
    }
    (RuleKind::Other, None)
}

/// The UI-facing view of the persistent preferences that clients can change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub mirror: String,
    pub core_version: String,
    pub auto_update_minutes: u64,
    pub mixed_port: u16,
    pub test_url: String,
    pub auto_start: bool,
    pub auto_system_proxy: bool,
    pub traffic_mode: TrafficMode,
}

impl From<&Settings> for SettingsSnapshot {
    fn from(settings: &Settings) -> Self {
        Self {
            mirror: settings.mirror.clone(),
            core_version: settings.core_version.clone(),
            auto_update_minutes: settings.auto_update_minutes,
            mixed_port: settings.mixed_port,
            test_url: settings.test_url.clone(),
            auto_start: settings.auto_start,
            auto_system_proxy: settings.auto_system_proxy,
            traffic_mode: settings.traffic_mode,
        }
    }
}

/// A complete, renderable view of the client. Both UIs draw from this value
/// only, so they always agree on what is happening.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientSnapshot {
    pub core_running: bool,
    pub starting: bool,
    pub active_profile: Option<ProfileSummary>,
    pub profiles: Vec<ProfileSummary>,
    pub current_node: Option<String>,
    pub traffic_mode: TrafficMode,
    pub system_proxy_enabled: bool,
    pub upload_speed: u64,
    pub download_speed: u64,
    pub total_upload: u64,
    pub total_download: u64,
    pub traffic_history: VecDeque<TrafficPoint>,
    pub active_connections: usize,
    pub proxy_groups: Vec<ProxyGroupSnapshot>,
    pub connections: ConnectionsSnapshot,
    /// The inbounds of the configuration the running core started from; empty
    /// while nothing has been started and no cached configuration exists.
    pub inbounds: Vec<InboundInfo>,
    pub core_logs: VecDeque<String>,
    pub events: VecDeque<String>,
    /// The structured twin of [`Self::events`]: the authoritative record list a
    /// UI renders in its own language. Same order, same cap, written only by
    /// [`ClientSnapshot::push_record`].
    pub event_records: VecDeque<crate::event_code::EventRecord>,
    pub core_version: Option<String>,
    pub core_installed: bool,
    /// The version the running core reports through clash_api; `None` while
    /// the core is stopped. Distinct from `core_version`, the installed
    /// binary's `sing-box version` string.
    pub core_runtime_version: Option<String>,
    /// The core's current heap usage in bytes, from the clash_api `/memory`
    /// endpoint; 0 while the core is stopped.
    pub memory_used: u64,
    /// The routing rules and rule-set sources of the active configuration,
    /// published by the engine so both UIs render the same rules view.
    pub rules: Vec<RouteRuleSnapshot>,
    pub rule_sets: Vec<RuleSetSummary>,
    pub subscription_usage: Option<SubscriptionUserinfo>,
    pub settings: SettingsSnapshot,
    pub status: String,
    /// How severe the current `status` is, when the engine knows. `None` means
    /// the line came from a call site that has not moved to
    /// [`crate::event_code::EventCode`] yet, and the UIs fall back to their
    /// legacy Chinese-substring guess for it.
    pub status_level: Option<crate::event_code::EventLevel>,
    pub outbound_mode: OutboundMode,
    /// The command currently being applied, if any.
    pub busy: Option<String>,
    /// Scheduler state for an automatic restart after an unexpected exit.
    pub restart_attempts: u32,
}

impl Default for ClientSnapshot {
    fn default() -> Self {
        Self {
            core_running: false,
            starting: false,
            active_profile: None,
            profiles: Vec::new(),
            current_node: None,
            traffic_mode: TrafficMode::SystemProxy,
            system_proxy_enabled: false,
            upload_speed: 0,
            download_speed: 0,
            total_upload: 0,
            total_download: 0,
            traffic_history: VecDeque::new(),
            active_connections: 0,
            proxy_groups: Vec::new(),
            connections: ConnectionsSnapshot::default(),
            core_logs: VecDeque::new(),
            events: VecDeque::new(),
            event_records: VecDeque::new(),
            core_version: None,
            core_installed: false,
            core_runtime_version: None,
            memory_used: 0,
            rules: Vec::new(),
            rule_sets: Vec::new(),
            inbounds: Vec::new(),
            subscription_usage: None,
            settings: SettingsSnapshot::default(),
            status: "就绪。先导入订阅，再启动内核。".to_owned(),
            status_level: None,
            outbound_mode: OutboundMode::Rule,
            busy: None,
            restart_attempts: 0,
        }
    }
}

impl ClientSnapshot {
    /// The group the user's node choice actually lives in. See
    /// [`find_selector_group`].
    pub fn selector_group(&self) -> Option<&ProxyGroupSnapshot> {
        find_selector_group(&self.proxy_groups, &self.rules)
    }

    /// The most recent engine event a UI renders in its own language.
    pub fn latest_event(&self) -> Option<&crate::event_code::EventRecord> {
        self.event_records.back()
    }

    /// Pushes a UI-level event line, keeping a bounded history.
    pub fn push_event(&mut self, message: impl Into<String>) {
        self.events.push_back(message.into());
        while self.events.len() > 200 {
            self.events.pop_front();
        }
    }

    /// Records one engine event in both shapes: the structured record is the
    /// authoritative one a UI renders in its own language, and the Chinese line
    /// keeps today's string-only consumers working until every call site has
    /// moved. Both lists are written here and nowhere else, so they cannot
    /// drift apart. See [`crate::event_code`] and issue 02.
    pub fn push_record(&mut self, record: crate::event_code::EventRecord) {
        let line = record.render_zh();
        while self.event_records.len() >= 200 {
            self.event_records.pop_front();
        }
        self.event_records.push_back(record);
        self.push_event(line);
    }

    /// Pushes a kernel log line, keeping a bounded history.
    pub fn push_log(&mut self, line: impl Into<String>) {
        self.core_logs.push_back(line.into());
        while self.core_logs.len() > 500 {
            self.core_logs.pop_front();
        }
    }

    /// Records one traffic sample for the history graph.
    pub fn push_traffic(&mut self, up: u64, down: u64) {
        self.upload_speed = up;
        self.download_speed = down;
        self.traffic_history.push_back(TrafficPoint { up, down });
        while self.traffic_history.len() > TRAFFIC_HISTORY {
            self.traffic_history.pop_front();
        }
    }

    /// The largest rate in the history window, used to scale the graph.
    pub fn traffic_peak(&self) -> u64 {
        self.traffic_history
            .iter()
            .map(|point| point.up.max(point.down))
            .max()
            .unwrap_or(0)
    }

    /// The download peak alone: labelling this as the download peak when it is
    /// `max(up, down)` overstates the download axis whenever upload leads.
    pub fn download_peak(&self) -> u64 {
        self.traffic_history
            .iter()
            .map(|point| point.down)
            .max()
            .unwrap_or(0)
    }

    pub fn upload_peak(&self) -> u64 {
        self.traffic_history
            .iter()
            .map(|point| point.up)
            .max()
            .unwrap_or(0)
    }
}

/// How insistent a published log line is. Both clients filter and colour on
/// this so the same line is treated identically in either UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    #[default]
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "Debug",
            Self::Info => "Info",
            Self::Warn => "Warn",
            Self::Error => "Error",
        }
    }
}

/// The severity of one log line.
///
/// Kernel lines carry sing-box's own marker; the client's own events are plain
/// Chinese sentences, and those are informational rather than debug — a filter
/// of "Info and above" must keep them, which is what the two clients disagreed
/// on while each classified them its own way.
pub fn log_level_of(line: &str) -> LogLevel {
    let lower = line.to_lowercase();
    if lower.contains("error")
        || lower.contains("fatal")
        || line.contains("失败")
        || line.contains("错误")
    {
        LogLevel::Error
    } else if lower.contains("warn") || line.contains("警告") {
        LogLevel::Warn
    } else if lower.contains("trace") || lower.contains("debug") {
        LogLevel::Debug
    } else {
        LogLevel::Info
    }
}

/// Whether `line` belongs in a view that shows `minimum` and above; `None`
/// shows everything.
pub fn log_level_shown(minimum: Option<LogLevel>, line: &str) -> bool {
    minimum.is_none_or(|minimum| log_level_of(line) >= minimum)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, kind: &str) -> ProxyGroupSnapshot {
        ProxyGroupSnapshot {
            name: name.to_owned(),
            kind: kind.to_owned(),
            current: "东京-A".to_owned(),
            members: vec!["东京-A".to_owned()],
            delays: HashMap::new(),
            failed: Vec::new(),
        }
    }

    fn final_rule(outbound: &str) -> RouteRuleSnapshot {
        RouteRuleSnapshot {
            kind: RuleKind::Final,
            value: None,
            outbound: outbound.to_owned(),
        }
    }

    /// A Mihomo subscription has no `🚀节点选择` anywhere, so the literal
    /// comparison both clients used made "current node" go stale there. The
    /// core's `route.final` names the group, so it has to win.
    #[test]
    fn the_selector_group_follows_route_final_not_a_hardcoded_name() {
        let groups = vec![
            group("♻️自动选择", "urltest"),
            group("🌍选择代理节点", "select"),
        ];
        assert_eq!(
            find_selector_group(&groups, &[final_rule("🌍选择代理节点")]).map(|g| g.name.as_str()),
            Some("🌍选择代理节点")
        );
        // With no final rule to follow, the manual group still wins over the
        // automatic one — the ordering here is what keeps the old behaviour for
        // this project's own profile.
        assert_eq!(
            find_selector_group(&groups, &[]).map(|g| g.name.as_str()),
            Some("🌍选择代理节点")
        );
    }

    /// Two manual groups, and only `route.final` says which one the user's
    /// choice lives in. Without that arm the fallbacks pick the first non-auto
    /// group, which is the wrong one — so this is the case that makes the
    /// ordering above actually constrained: deleting the `route.final` arm
    /// turns it red, which the test before it does not.
    #[test]
    fn route_final_decides_when_more_than_one_manual_group_exists() {
        let groups = vec![
            group("🌍选择代理节点", "select"),
            group("地区选择", "select"),
        ];
        assert_eq!(
            find_selector_group(&groups, &[final_rule("地区选择")]).map(|g| g.name.as_str()),
            Some("地区选择"),
            "`route.final` must outrank group order"
        );
        assert_eq!(
            find_selector_group(&groups, &[]).map(|g| g.name.as_str()),
            Some("🌍选择代理节点"),
            "with nothing to go on, the first manual group is the guess"
        );
    }

    #[test]
    fn the_selector_group_still_finds_this_projects_own_profile() {
        let groups = vec![
            group("🚀节点选择", "selector"),
            group("♻️自动选择", "urltest"),
            group("direct", "direct"),
        ];
        assert_eq!(
            find_selector_group(&groups, &[final_rule("🚀节点选择")]).map(|g| g.name.as_str()),
            Some("🚀节点选择")
        );
        assert_eq!(
            find_selector_group(&groups, &[]).map(|g| g.name.as_str()),
            Some("🚀节点选择")
        );
    }

    /// Only automatic groups, or none at all: it must answer with *something*
    /// rather than dropping the current-node display.
    #[test]
    fn the_selector_group_degrades_to_the_first_group_it_has() {
        let auto_only = vec![group("♻️自动选择", "urltest")];
        assert_eq!(
            find_selector_group(&auto_only, &[]).map(|g| g.name.as_str()),
            Some("♻️自动选择")
        );
        assert_eq!(find_selector_group(&[], &[]), None);
    }

    #[test]
    fn traffic_history_is_bounded_and_peaks_over_both_directions() {
        let mut snapshot = ClientSnapshot::default();
        for index in 0..(TRAFFIC_HISTORY + 10) {
            snapshot.push_traffic(index as u64, (index * 2) as u64);
        }
        assert_eq!(snapshot.traffic_history.len(), TRAFFIC_HISTORY);
        assert_eq!(snapshot.download_speed, (TRAFFIC_HISTORY + 9) as u64 * 2);
        assert_eq!(snapshot.traffic_peak(), (TRAFFIC_HISTORY + 9) as u64 * 2);
    }

    #[test]
    fn events_and_logs_are_bounded() {
        let mut snapshot = ClientSnapshot::default();
        for index in 0..250 {
            snapshot.push_event(format!("event {index}"));
        }
        assert_eq!(snapshot.events.len(), 200);
        assert_eq!(snapshot.events.front().unwrap(), "event 50");
        for index in 0..520 {
            snapshot.push_log(format!("log {index}"));
        }
        assert_eq!(snapshot.core_logs.len(), 500);
    }

    #[test]
    fn auto_groups_are_recognized() {
        let group = ProxyGroupSnapshot {
            kind: "URLTest".to_owned(),
            ..Default::default()
        };
        assert!(group.is_auto());
        let selector = ProxyGroupSnapshot {
            kind: "Selector".to_owned(),
            ..Default::default()
        };
        assert!(!selector.is_auto());
    }

    #[test]
    fn parse_inbounds_reports_what_the_started_configuration_listens_on() {
        let config = r#"{"inbounds":[
            {"type":"mixed","tag":"mixed-in","listen":"127.0.0.1","listen_port":2080},
            {"type":"tun","tag":"tun-in","address":["172.19.0.1/30"],"stack":"mixed"},
            {"type":"shadowsocks","tag":"ss","listen":"::","listen_port":8443}
        ]}"#;
        let parsed = parse_inbounds(config);
        assert_eq!(parsed.len(), 3, "every declared inbound is listed");
        assert_eq!(parsed[0].kind, "mixed");
        assert_eq!(parsed[0].port, 2080);
        assert_eq!(parsed[0].listen, "127.0.0.1");
        assert_eq!(
            parsed[1].port, 0,
            "a tun inbound declares no port, and 0 is how that is shown"
        );
        assert_eq!(parsed[1].listen, "", "no listen means every address");
        assert_eq!(parsed[2].listen, "::", "an IPv6 literal survives verbatim");
    }

    #[test]
    fn parse_inbounds_tolerates_a_missing_or_unparsable_configuration() {
        for junk in [
            "not json",
            "",
            "{}",
            r#"{"inbounds":null}"#,
            r#"{"inbounds":{}}"#,
        ] {
            assert!(
                parse_inbounds(junk).is_empty(),
                "{junk:?} has no inbounds to report"
            );
        }
        let odd = r#"{"inbounds":[{"type":"mixed","listen_port":"2080"},{"listen_port":99999}],"route":{}}"#;
        let parsed = parse_inbounds(odd);
        assert_eq!(parsed.len(), 2, "an unmapped entry still occupies its row");
        assert_eq!(
            parsed[0].port, 0,
            "a string port is not a number, and guessing would misreport what is listening"
        );
        assert_eq!(
            parsed[1].port, 0,
            "a port above u16 range is not shown as a port"
        );
        assert_eq!(parsed[1].kind, "", "an entry with no type is still counted");
    }

    #[test]
    fn parse_route_rules_covers_matchers_rule_sets_and_final() {
        let config = serde_json::json!({
            "route": {
                "rule_set": [
                    {"type": "remote", "tag": "geoip-cn", "url": "https://example/srs"},
                    {"type": "local", "tag": "lan"}
                ],
                "rules": [
                    {"rule_set": ["geoip-cn"], "outbound": "🚀节点选择"},
                    {"domain_suffix": [".cn"], "outbound": "🎯直连"},
                    {"ip_is_private": "always", "outbound": "🎯直连"},
                    {"protocol": "dns", "action": "hijack-dns"},
                    {"port": 443, "outbound": "🚀节点选择"}
                ],
                "final": "🚀节点选择"
            }
        })
        .to_string();
        let (rules, rule_sets) = parse_route_rules(&config);
        assert_eq!(rule_sets.len(), 2);
        assert_eq!(rule_sets[0].tag, "geoip-cn");
        assert_eq!(rule_sets[0].kind, "remote");
        assert_eq!(rule_sets[1].kind, "local");
        assert_eq!(rules.len(), 6);
        assert_eq!(rules[0].matcher_zh(), "规则集 · geoip-cn");
        assert_eq!(rules[0].outbound, "🚀节点选择");
        assert!(rules[1].matcher_zh().starts_with("域名后缀"));
        assert_eq!(rules[2].matcher_zh(), "私有地址");
        assert_eq!(rules[3].matcher_zh(), "协议 · dns");
        assert_eq!(rules[3].outbound, "hijack-dns");
        assert_eq!(rules[4].matcher_zh(), "端口 · 443");
        assert_eq!(rules[5].matcher_zh(), "其他未命中流量");
        assert_eq!(rules[5].outbound, "🚀节点选择");
    }

    #[test]
    fn parse_route_rules_tolerates_invalid_or_empty_configs() {
        assert_eq!(parse_route_rules("not json"), (Vec::new(), Vec::new()));
        assert_eq!(parse_route_rules("{}"), (Vec::new(), Vec::new()));
        assert_eq!(
            parse_route_rules(r#"{"route":{}}"#),
            (Vec::new(), Vec::new())
        );
    }

    #[test]
    fn log_levels_order_by_marker_and_keep_unmarked_client_events() {
        assert_eq!(log_level_of("ERROR[0001] inbound broken"), LogLevel::Error);
        assert_eq!(log_level_of("导入订阅失败: timeout"), LogLevel::Error);
        assert_eq!(log_level_of("写入配置错误"), LogLevel::Error);
        assert_eq!(log_level_of("WARN[0002] slow dial"), LogLevel::Warn);
        assert_eq!(log_level_of("level=warning msg=x"), LogLevel::Warn);
        assert_eq!(log_level_of("DEBUG[0000] cache miss"), LogLevel::Debug);
        assert_eq!(
            log_level_of("内核已启动"),
            LogLevel::Info,
            "the client's own events are informational, not debug"
        );

        assert!(log_level_shown(Some(LogLevel::Info), "内核已启动"));
        assert!(!log_level_shown(Some(LogLevel::Info), "TRACE trace detail"));
        assert!(log_level_shown(
            Some(LogLevel::Error),
            "导入订阅失败: timeout"
        ));
        assert!(log_level_shown(None, "TRACE trace detail"));
    }

    #[test]
    fn direction_peaks_stay_below_the_combined_peak() {
        let mut snapshot = ClientSnapshot::default();
        snapshot.push_traffic(100, 40);
        snapshot.push_traffic(10, 90);
        assert_eq!(snapshot.upload_peak(), 100);
        assert_eq!(snapshot.download_peak(), 90);
        assert_eq!(snapshot.traffic_peak(), 100);
    }
}
