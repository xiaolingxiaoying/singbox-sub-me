//! The window's view state: which page is showing, which fields are being
//! edited, and the enums that name them.
//!
//! The state itself is dumb. It owns no behaviour beyond the startup seams
//! at the bottom; the commit path that changes it lives in `crate::app`.

use std::path::PathBuf;

use client_core::ClientController;
use client_core::clash_api::Connection;
use client_core::state::ClientSnapshot;
use gpui::{FocusHandle, ScrollHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Dashboard,
    Subscriptions,
    Proxies,
    Rules,
    Connections,
    Logs,
    Settings,
    About,
}

impl Page {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "概览",
            Self::Subscriptions => "订阅",
            Self::Proxies => "节点",
            Self::Rules => "规则",
            Self::Connections => "连接",
            Self::Logs => "日志",
            Self::Settings => "设置",
            Self::About => "关于",
        }
    }

    pub(crate) fn all() -> [Self; 8] {
        [
            Self::Dashboard,
            Self::Subscriptions,
            Self::Proxies,
            Self::Rules,
            Self::Connections,
            Self::Logs,
            Self::Settings,
            Self::About,
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Accent,
    Neutral,
    Warning,
    /// A control that destroys something: the kit keeps it quiet (danger text
    /// on a white control) rather than a filled red block.
    Danger,
}

/// What the user chose to do with the OS proxy when closing the window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitChoice {
    /// Leave the OS proxy pointing at the local port.
    Keep,
    /// Restore the captured pre-install proxy state, then close.
    Restore,
}

/// The text fields the window renders, indexing `Sbgui::inputs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputField {
    ConnFilter,
    LogQuery,
    ProxySearch,
    RuleSearch,
    SubUrl,
    Mirror,
    MixedPort,
    TestUrl,
    AutoUpdateMinutes,
    CoreVersion,
}

pub(crate) const INPUT_FIELDS: [InputField; 10] = [
    InputField::ConnFilter,
    InputField::LogQuery,
    InputField::ProxySearch,
    InputField::RuleSearch,
    InputField::SubUrl,
    InputField::Mirror,
    InputField::MixedPort,
    InputField::TestUrl,
    InputField::AutoUpdateMinutes,
    InputField::CoreVersion,
];

/// A minimal single-line text field: click to focus, type to edit, Enter to
/// commit, Esc to reset. Editing is append/backspace with the caret always at
/// the end — the same model the TUI's input overlay uses — and the typed
/// character comes from the keystroke's `key_char`. IME composition (typing
/// Chinese into a field) is not handled yet; filters and settings are ASCII.
pub(crate) struct TextField {
    pub(crate) focus: FocusHandle,
    pub(crate) text: String,
}

/// The render-time identity of one field, bundled so field helpers stay
/// readable (`text_field(spec, window, cx)`).
pub(crate) struct FieldSpec {
    pub(crate) field: InputField,
    pub(crate) id: &'static str,
    pub(crate) placeholder: &'static str,
    pub(crate) width: f32,
}

/// The logs-page level filter, mirroring the TUI's `info+`/`warn+`/`error`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LogLevelFilter {
    #[default]
    All,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevelFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            // The chips are thresholds, matching the terminal client.
            Self::Debug => "Debug+",
            Self::Info => "Info+",
            Self::Warn => "Warn+",
            Self::Error => "Error",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    #[default]
    General,
    Network,
    Core,
    Tun,
    Automation,
    Appearance,
    Advanced,
}

impl SettingsSection {
    pub(crate) fn all() -> [Self; 7] {
        [
            Self::General,
            Self::Network,
            Self::Core,
            Self::Tun,
            Self::Automation,
            Self::Appearance,
            Self::Advanced,
        ]
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "常规",
            Self::Network => "网络与端口",
            Self::Core => "内核",
            Self::Tun => "TUN",
            Self::Automation => "自动化",
            Self::Appearance => "外观",
            Self::Advanced => "高级",
        }
    }

    /// Why the section exists, shown under its name in the settings rail so a
    /// row is 66 px of answer instead of one bare word.
    pub(crate) fn blurb(self) -> &'static str {
        match self {
            Self::General => "订阅与本地配置档案",
            Self::Network => "流量模式、端口与测试地址",
            Self::Core => "sing-box 版本、镜像与更新",
            Self::Tun => "虚拟网卡与系统路由",
            Self::Automation => "自动启动与自动更新",
            Self::Appearance => "主题与字体",
            Self::Advanced => "数据目录与配置安全",
        }
    }

    /// The rail icon, from the kit's own inventory.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Self::General => "stack",
            Self::Network => "network",
            Self::Core => "power",
            Self::Tun => "globe",
            Self::Automation => "clock",
            Self::Appearance => "home",
            Self::Advanced => "settings",
        }
    }
}

pub(crate) struct Sbgui {
    pub(crate) controller: ClientController,
    pub(crate) data_dir: PathBuf,
    pub(crate) snapshot: ClientSnapshot,
    pub(crate) page: Page,
    pub(crate) group_index: usize,
    /// Set while the exit confirmation overlay is visible.
    pub(crate) confirm_exit: bool,
    /// Set while the stop-the-core confirmation is visible.
    pub(crate) confirm_stop_core: bool,
    /// The exit decision, once made; a set choice lets the window close.
    pub(crate) exit_choice: Option<ExitChoice>,
    /// Text fields, indexed by `InputField as usize`.
    pub(crate) inputs: [TextField; INPUT_FIELDS.len()],
    /// The logs-page level filter.
    pub(crate) log_level: LogLevelFilter,
    pub(crate) settings_section: SettingsSection,
    pub(crate) show_subscription_import: bool,
    pub(crate) show_rule_sets: bool,
    /// Rules pages are walls of text on a real subscription: the list starts
    /// capped and the user opens the rest on demand.
    pub(crate) show_all_rules: bool,
    /// Same cap for the connection list, which grows without bound.
    pub(crate) show_all_connections: bool,
    pub(crate) core_menu_open: bool,
    /// Whether the overview's lower-frequency numbers are showing. It starts
    /// open so the page never hides a value the previous layout always drew.
    pub(crate) advanced_open: bool,
    pub(crate) node_card_view: bool,
    pub(crate) paused_connections: Option<Vec<Connection>>,
    pub(crate) selected_connection: Option<String>,
    /// Scroll position of the log panel, so "自动滚动" can pin the view to the
    /// newest line instead of being a label that does nothing.
    pub(crate) log_scroll: ScrollHandle,
    /// Row count of the last rendered log panel; a change means new lines.
    pub(crate) log_rows: std::cell::Cell<usize>,
    /// Minute of the last repaint: `age_label` renders relative times from the
    /// wall clock at paint time, so a quiet snapshot still needs one repaint per
    /// minute or those labels freeze.
    pub(crate) painted_minute: u64,
    pub(crate) log_wrap: bool,
    pub(crate) log_follow: bool,
    pub(crate) confirm_close_all: bool,
    /// A profile name whose delete button is armed waiting for a second click.
    pub(crate) confirm_delete_profile: Option<String>,
}

/// Visual-review seams read once at startup. Ordinary launches never set
/// them; automated screenshot review uses them instead of synthesized mouse
/// input, which cannot reach the window on a locked desktop session.
///
/// - `SBGUI_PAGE=<dashboard|subscriptions|proxies|rules|connections|logs|settings>`
///   opens the window directly on that page.
/// - `SBGUI_SIZE=<width>x<height>` overrides the window size in logical px.
/// - `SBGUI_SHOW_EXIT_CONFIRM=1` renders the exit-confirmation overlay
///   without enabling the OS proxy.
pub(crate) fn env_page() -> Option<Page> {
    let name = std::env::var("SBGUI_PAGE").ok()?;
    Some(match name.to_ascii_lowercase().as_str() {
        "dashboard" | "概览" => Page::Dashboard,
        "subscriptions" | "订阅" => Page::Subscriptions,
        "proxies" | "节点" => Page::Proxies,
        "rules" | "规则" => Page::Rules,
        "connections" | "连接" => Page::Connections,
        "logs" | "日志" => Page::Logs,
        "settings" | "设置" => Page::Settings,
        "about" | "关于" => Page::About,
        _ => return None,
    })
}

/// `SBGUI_SETTINGS_SECTION=<general|network|core|tun|automation|appearance|advanced>`
/// opens one settings section directly, so the screenshot harness can look at
/// the widest rows instead of only the one that happens to be the default.
pub(crate) fn env_settings_section() -> Option<SettingsSection> {
    match std::env::var("SBGUI_SETTINGS_SECTION")
        .ok()?
        .to_ascii_lowercase()
        .as_str()
    {
        "general" => Some(SettingsSection::General),
        "network" => Some(SettingsSection::Network),
        "core" => Some(SettingsSection::Core),
        "tun" => Some(SettingsSection::Tun),
        "automation" => Some(SettingsSection::Automation),
        "appearance" => Some(SettingsSection::Appearance),
        "advanced" => Some(SettingsSection::Advanced),
        _ => None,
    }
}

pub(crate) fn env_window_size() -> (f32, f32) {
    std::env::var("SBGUI_SIZE")
        .ok()
        .and_then(|text| {
            let (w, h) = text.split_once('x')?;
            Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
        })
        .filter(|(w, h)| *w >= 400.0 && *h >= 300.0)
        .unwrap_or((1080.0, 760.0))
}

pub(crate) fn env_show_exit_confirm() -> bool {
    std::env::var("SBGUI_SHOW_EXIT_CONFIRM").is_ok_and(|value| value == "1")
}

/// Review seam for the stop-the-core confirmation, which otherwise needs a
/// click the screenshot harness cannot make.
pub(crate) fn env_show_stop_confirm() -> bool {
    std::env::var("SBGUI_SHOW_STOP_CONFIRM").is_ok_and(|value| value == "1")
}
