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
}
