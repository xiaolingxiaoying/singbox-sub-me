use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::clash_api::{ConnectionsSnapshot, OutboundMode, ProxyGroup};
use crate::settings::Profile;
use crate::system_proxy::TrafficMode;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub name: String,
    pub url: String,
    pub last_updated: u64,
}

impl From<&Profile> for ProfileSummary {
    fn from(profile: &Profile) -> Self {
        Self {
            name: profile.name.clone(),
            url: profile.url.clone(),
            last_updated: profile.last_updated,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyGroupSnapshot {
    pub name: String,
    pub current: String,
    pub members: Vec<String>,
    pub delays: HashMap<String, u64>,
}

impl From<ProxyGroup> for ProxyGroupSnapshot {
    fn from(group: ProxyGroup) -> Self {
        Self {
            name: group.name,
            current: group.now,
            members: group.all,
            delays: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientSnapshot {
    pub core_running: bool,
    pub active_profile: Option<ProfileSummary>,
    pub current_node: Option<String>,
    pub traffic_mode: TrafficMode,
    pub system_proxy_enabled: bool,
    pub upload_speed: u64,
    pub download_speed: u64,
    pub total_upload: u64,
    pub total_download: u64,
    pub proxy_groups: Vec<ProxyGroupSnapshot>,
    pub connections: ConnectionsSnapshot,
    pub core_logs: Vec<String>,
    pub status: String,
    pub outbound_mode: OutboundMode,
}

impl Default for ClientSnapshot {
    fn default() -> Self {
        Self {
            core_running: false,
            active_profile: None,
            current_node: None,
            traffic_mode: TrafficMode::SystemProxy,
            system_proxy_enabled: false,
            upload_speed: 0,
            download_speed: 0,
            total_upload: 0,
            total_download: 0,
            proxy_groups: Vec::new(),
            connections: ConnectionsSnapshot::default(),
            core_logs: Vec::new(),
            status: "就绪。先导入订阅，再启动内核。".to_owned(),
            outbound_mode: OutboundMode::Rule,
        }
    }
}
