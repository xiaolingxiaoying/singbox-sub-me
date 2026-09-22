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
    pub core_logs: VecDeque<String>,
    pub events: VecDeque<String>,
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
            core_version: None,
            core_installed: false,
            core_runtime_version: None,
            memory_used: 0,
            rules: Vec::new(),
            rule_sets: Vec::new(),
            subscription_usage: None,
            settings: SettingsSnapshot::default(),
            status: "就绪。先导入订阅，再启动内核。".to_owned(),
            outbound_mode: OutboundMode::Rule,
            busy: None,
            restart_attempts: 0,
        }
    }
}

impl ClientSnapshot {
    /// Pushes a UI-level event line, keeping a bounded history.
    pub fn push_event(&mut self, message: impl Into<String>) {
        self.events.push_back(message.into());
        while self.events.len() > 200 {
            self.events.pop_front();
        }
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
