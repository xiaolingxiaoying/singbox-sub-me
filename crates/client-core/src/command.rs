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
    /// `name` overrides the generated `订阅 N` label; the URL is normalized
    /// the same way either way.
    ImportSubscription {
        name: Option<String>,
        url: String,
    },
    /// Imports a local sing-box JSON configuration file as a new profile and
    /// makes it active. The profile has no URL, so updating it is a no-op
    /// until a link is set with [`ClientCommand::SetProfileUrl`].
    ImportProfileFile(String),
    /// Replaces one profile's subscription link (renormalized). An empty URL
    /// is rejected; keep [`ClientCommand::ImportProfileFile`] for file-only
    /// profiles.
    SetProfileUrl {
        name: String,
        url: String,
    },
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
    /// Empties the published log and event buffers. Clearing has to happen in
    /// the engine: a UI-side "hide the first N lines" marker breaks as soon as
    /// the ring buffer is full, because the cap makes N permanent.
    ClearLogs,
    /// Ends the engine loop, which reaps the core child on its way out without
    /// touching the operating-system proxy — the UI has already decided whether
    /// to keep or restore it. See `ClientController::shutdown`.
    Shutdown,
}

impl ClientCommand {
    /// A human-readable name shown while the command executes and in failure
    /// statuses (`{label} 失败: …`), so both UIs render the same wording
    /// instead of parsing Debug output.
    pub fn label(&self) -> String {
        match self {
            Self::StartCore => "启动内核".to_owned(),
            Self::StopCore => "停止内核".to_owned(),
            Self::RestartCore => "重启内核".to_owned(),
            Self::UpdateSubscription => "更新订阅".to_owned(),
            Self::ImportSubscription { .. } => "导入订阅".to_owned(),
            Self::ImportProfileFile(_) => "导入本地配置".to_owned(),
            Self::SetProfileUrl { .. } => "更新订阅链接".to_owned(),
            Self::RemoveProfile(name) => format!("删除档案 {name}"),
            Self::DownloadCore => "下载内核".to_owned(),
            Self::SwitchProfile(name) => format!("激活档案 {name}"),
            Self::SwitchNode { group, node } => format!("{group} → {node}"),
            Self::TestNode(node) => format!("测试 {node} 延迟"),
            Self::TestGroup(group) => format!("测试 {group} 组延迟"),
            Self::ToggleSystemProxy => "切换系统代理".to_owned(),
            Self::SetTrafficMode(mode) => format!("切换流量模式至{}", mode.label()),
            Self::SetOutboundMode(mode) => format!("切换出站模式至{}", mode.label()),
            Self::CloseConnection(_) => "关闭连接".to_owned(),
            Self::CloseAllConnections => "关闭全部连接".to_owned(),
            Self::UpdateSettings(_) => "保存设置".to_owned(),
            Self::Refresh => "刷新状态".to_owned(),
            Self::ClearLogs => "清空日志".to_owned(),
            // Never rendered: `Shutdown` is intercepted before `apply`.
            Self::Shutdown => String::new(),
        }
    }
}
