use crate::clash_api::OutboundMode;
use crate::system_proxy::TrafficMode;

/// A partial update to the persisted settings. `None` leaves a field alone, so
/// a UI can save a single control without clobbering the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsPatch {
    pub mirror: Option<String>,
    pub core_version: Option<String>,
    pub auto_update_minutes: Option<u64>,
    pub mixed_port: Option<u16>,
    pub test_url: Option<String>,
    pub auto_start: Option<bool>,
    pub auto_system_proxy: Option<bool>,
    pub traffic_mode: Option<TrafficMode>,
}

impl SettingsPatch {
    pub fn apply(self, settings: &mut crate::settings::Settings) {
        if let Some(value) = self.mirror {
            settings.mirror = value;
        }
        if let Some(value) = self.core_version {
            settings.core_version = value;
        }
        if let Some(value) = self.auto_update_minutes {
            settings.auto_update_minutes = value;
        }
        if let Some(value) = self.mixed_port {
            settings.mixed_port = value;
        }
        if let Some(value) = self.test_url {
            settings.test_url = value;
        }
        if let Some(value) = self.auto_start {
            settings.auto_start = value;
        }
        if let Some(value) = self.auto_system_proxy {
            settings.auto_system_proxy = value;
        }
        if let Some(value) = self.traffic_mode {
            settings.traffic_mode = value;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientCommand {
    StartCore,
    StopCore,
    RestartCore,
    UpdateSubscription,
    /// Imports a subscription URL as a new local profile and makes it active.
    ImportSubscription(String),
    /// Removes a local subscription profile and its cached configuration.
    RemoveProfile(String),
    DownloadCore,
    SwitchProfile(String),
    /// Selects a member inside one proxy group (the group name matters because
    /// subscriptions can define several selectors).
    SwitchNode {
        group: String,
        node: String,
    },
    TestNode(String),
    /// Tests every member of one group concurrently.
    TestGroup(String),
    ToggleSystemProxy,
    SetTrafficMode(TrafficMode),
    SetOutboundMode(OutboundMode),
    CloseConnection(String),
    CloseAllConnections,
    UpdateSettings(SettingsPatch),
    /// Forces an immediate state refresh from the core.
    Refresh,
}
