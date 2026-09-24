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

use crate::lang::Locale;
use crate::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Dashboard,
    Subscriptions,
    Proxies,
    Rules,
    /// 覆写配置文件内容: the active profile's override file, its switchable rule
    /// fragments, and the redacted outline of the merged configuration. This is
    /// the terminal client's seventh tab; both read the same snapshot fields.
    Overrides,
    Connections,
    Logs,
    Settings,
    About,
}

impl Page {
    /// The navigation label: both languages are written at the same place, so
    /// a new page cannot ship with only one of them.
    pub(crate) fn title(self, locale: Locale) -> &'static str {
        match self {
            Self::Dashboard => tr!(locale, "概览", "Overview"),
            Self::Subscriptions => tr!(locale, "订阅", "Subscriptions"),
            Self::Proxies => tr!(locale, "节点", "Nodes"),
            Self::Rules => tr!(locale, "规则", "Rules"),
            Self::Overrides => tr!(locale, "覆写", "Overrides"),
            Self::Connections => tr!(locale, "连接", "Connections"),
            Self::Logs => tr!(locale, "日志", "Logs"),
            Self::Settings => tr!(locale, "设置", "Settings"),
            Self::About => tr!(locale, "关于", "About"),
        }
    }

    pub(crate) fn all() -> [Self; 9] {
        [
            Self::Dashboard,
            Self::Subscriptions,
            Self::Proxies,
            Self::Rules,
            Self::Overrides,
            Self::Connections,
            Self::Logs,
            Self::Settings,
            Self::About,
        ]
    }
}

/// What the title-bar language control does, written over the pair it could
/// disturb rather than inside the click closure: a switch hands back the page
/// the user was on and the other language, and nothing else. Everything the
/// page keeps in `Sbgui` — the selected group, the filters, the open
/// disclosures — is not in this signature, so it cannot be lost by it.
pub(crate) fn switch_language(locale: Locale, page: Page) -> (Page, Locale) {
    (page, locale.toggle())
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
    /// The optional name the 「添加」 click gives the new profile.
    SubName,
    /// A local sing-box JSON file the 「导入本地 JSON」 click imports.
    SubFile,
    /// One profile's link, opened by that row's 「编辑链接」.
    SubEditUrl,
    Mirror,
    MixedPort,
    TestUrl,
    AutoUpdateMinutes,
    CoreVersion,
}

pub(crate) const INPUT_FIELDS: [InputField; 13] = [
    InputField::ConnFilter,
    InputField::LogQuery,
    InputField::ProxySearch,
    InputField::RuleSearch,
    InputField::SubUrl,
    InputField::SubName,
    InputField::SubFile,
    InputField::SubEditUrl,
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
    /// The chip the logs page filters by. Four of the five are level names the
    /// terminal client shows untranslated, so only the first needs both
    /// languages.
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::All => tr!(locale, "全部", "All"),
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

    pub(crate) fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::General => tr!(locale, "常规", "General"),
            Self::Network => tr!(locale, "网络与端口", "Network & ports"),
            Self::Core => tr!(locale, "内核", "Core"),
            Self::Tun => "TUN",
            Self::Automation => tr!(locale, "自动化", "Automation"),
            Self::Appearance => tr!(locale, "外观", "Appearance"),
            Self::Advanced => tr!(locale, "高级", "Advanced"),
        }
    }

    /// Why the section exists, shown under its name in the settings rail so a
    /// row is 66 px of answer instead of one bare word.
    pub(crate) fn blurb(self, locale: Locale) -> &'static str {
        match self {
            Self::General => tr!(
                locale,
                "订阅与本地配置档案",
                "Subscriptions and local profiles"
            ),
            Self::Network => tr!(
                locale,
                "流量模式、端口与测试地址",
                "Traffic mode, ports and the test address"
            ),
            Self::Core => tr!(
                locale,
                "sing-box 版本、镜像与更新",
                "sing-box version, mirror and updates"
            ),
            Self::Tun => tr!(
                locale,
                "虚拟网卡与系统路由",
                "Virtual adapter and system routes"
            ),
            Self::Automation => tr!(locale, "自动启动与自动更新", "Auto start and auto update"),
            Self::Appearance => tr!(locale, "主题与字体", "Theme and fonts"),
            Self::Advanced => tr!(
                locale,
                "数据目录与配置安全",
                "Data directory and config safety"
            ),
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
    /// Which language the interface is drawn in. Session-level: switching must
    /// not lose the page or the view state the user is working in.
    pub(crate) locale: crate::lang::Locale,
    /// The exit decision, once made; a set choice lets the window close.
    pub(crate) exit_choice: Option<ExitChoice>,
    /// Text fields, indexed by `InputField as usize`.
    pub(crate) inputs: [TextField; INPUT_FIELDS.len()],
    /// The logs-page level filter.
    pub(crate) log_level: LogLevelFilter,
    pub(crate) settings_section: SettingsSection,
    pub(crate) show_subscription_import: bool,
    /// Why the last 「添加」 click did nothing, kept as a code so the panel can
    /// say it in the language it is rendering. It is what stops an empty link
    /// from being a silent no-op; issue 02 item 4.
    pub(crate) subscription_error: Option<crate::parse::ImportReject>,
    /// The same refusal for the path field of the same panel, kept apart because
    /// the two lines sit apart and each names its own blank field.
    pub(crate) import_file_error: Option<crate::parse::ImportReject>,
    /// The profile whose link editor is open, by name: one at a time, since the
    /// editor holds the link it was prefilled from.
    pub(crate) editing_profile_url: Option<String>,
    /// Why the open editor refused to save.
    pub(crate) profile_url_error: Option<crate::parse::ImportReject>,
    /// What the launch path had to work around, told once inside the window it
    /// still managed to open (issue 02 item 5). It cannot ride on
    /// `snapshot.status`, which the engine republishes four times a second and
    /// which would therefore be blank again before the first frame finished
    /// drawing, so it is the window's own message and the user clears it.
    pub(crate) startup_notice: Option<String>,
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
    /// The row the log panel pinned itself to last paint. Comparing it is what
    /// recognises a new line once the ring buffers and the panel's own caps have
    /// saturated the row count; see [`crate::pages::logs::LogTail`].
    pub(crate) log_tail: std::cell::Cell<Option<crate::pages::logs::LogTail>>,
    /// Minute of the last repaint: `age_label` renders relative times from the
    /// wall clock at paint time, so a quiet snapshot still needs one repaint per
    /// minute or those labels freeze.
    pub(crate) painted_minute: u64,
    pub(crate) log_wrap: bool,
    pub(crate) log_follow: bool,
    pub(crate) confirm_close_all: bool,
    /// A profile name whose delete button is armed waiting for a second click.
    pub(crate) confirm_delete_profile: Option<String>,
    /// Set while the override page shows its delete-the-override confirmation.
    /// Arming belongs to that page alone: clearing an override has no undo, so
    /// the click that asks must never also be the click that deletes.
    pub(crate) confirm_clear_override: bool,
}

/// Visual-review seams read once at startup. Ordinary launches never set
/// them; automated screenshot review uses them instead of synthesized mouse
/// input, which cannot reach the window on a locked desktop session.
///
/// - `SBGUI_PAGE=<dashboard|subscriptions|proxies|rules|overrides|connections|logs|settings>`
///   opens the window directly on that page.
/// - `SBGUI_SIZE=<width>x<height>` overrides the window size in logical px.
/// - `SBGUI_SHOW_EXIT_CONFIRM=1` renders the exit-confirmation overlay
///   without enabling the OS proxy.
/// - `SBGUI_SHOW_STOP_CONFIRM=1` renders the stop-the-core confirmation.
/// - `SBGUI_SHOW_IMPORT_PANEL=1` opens the subscription import panel without a
///   click, and presses 「添加」 on the empty link field so the frame carries its
///   refusal too.
/// - `SBGUI_SHOW_URL_EDITOR=1` arms the link editor on the first profile the
///   snapshot holds, the same way the import seam replays its click.
/// - `SBGUI_SHOW_CLEAR_CONFIRM=1` renders the override page's armed
///   delete-the-override bar, which otherwise needs the click the harness
///   cannot make.
/// - `SBGUI_LANG=en` starts the interface in English, so a translation can be
///   screenshotted without a pointer reaching the title-bar control.
pub(crate) fn env_locale() -> crate::lang::Locale {
    match std::env::var("SBGUI_LANG").ok().as_deref() {
        Some("en") => crate::lang::Locale::En,
        _ => crate::lang::Locale::default(),
    }
}

pub(crate) fn env_page() -> Option<Page> {
    let name = std::env::var("SBGUI_PAGE").ok()?;
    Some(match name.to_ascii_lowercase().as_str() {
        "dashboard" | "概览" => Page::Dashboard,
        "subscriptions" | "订阅" => Page::Subscriptions,
        "proxies" | "节点" => Page::Proxies,
        "rules" | "规则" => Page::Rules,
        "overrides" | "覆写" => Page::Overrides,
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

/// Review seam for the subscription import panel, which only opens on a click.
/// With it set, the window also presses 「添加」 once on an empty field — see
/// `crate::app`, so the frame shows the panel *and* its refusal, and a panel
/// that vanished on that click is visible as a missing frame.
pub(crate) fn env_show_import_panel() -> bool {
    std::env::var("SBGUI_SHOW_IMPORT_PANEL").is_ok_and(|value| value == "1")
}

/// Review seam for one profile row's link editor, which only opens on a click.
/// With it set the window arms the editor on its first profile through the same
/// handler the button calls, so the frame shows the prefilled field rather than
/// a hand-drawn mock of it.
pub(crate) fn env_show_url_editor() -> bool {
    std::env::var("SBGUI_SHOW_URL_EDITOR").is_ok_and(|value| value == "1")
}

/// Review seam for the launch notice band (issue 02 item 5), which the real
/// launch path only fills when a persisted file cannot be read. See `crate::app`
/// for what it renders: the same sentence the real outcome produces.
pub(crate) fn env_startup_notice() -> bool {
    std::env::var("SBGUI_STARTUP_NOTICE").is_ok_and(|value| value == "1")
}

/// Review seam for the override page's armed delete bar, same reason. It arms
/// only the confirmation: the seam cannot delete anything, because the send
/// lives on the bar's own second click.
pub(crate) fn env_show_clear_confirm() -> bool {
    std::env::var("SBGUI_SHOW_CLEAR_CONFIRM").is_ok_and(|value| value == "1")
}

#[cfg(test)]
mod tests {
    use super::{INPUT_FIELDS, InputField, Locale, Page, switch_language};

    /// `Sbgui::field` indexes `inputs` by `field as usize`, so the enum order and
    /// this array are one list written twice. Getting them out of step would not
    /// fail to compile: it would type a subscription link into the name field.
    #[test]
    fn every_input_field_indexes_the_array_at_its_own_slot() {
        for (index, field) in INPUT_FIELDS.iter().enumerate() {
            assert_eq!(
                *field as usize, index,
                "{field:?} sits at slot {index}, which is another field's text"
            );
        }
        assert_eq!(
            InputField::CoreVersion as usize + 1,
            INPUT_FIELDS.len(),
            "`CoreVersion` is the last variant, so a new one needs a new array slot too"
        );
    }

    /// The title-bar control goes through [`switch_language`], so this covers
    /// the rule it exists to keep: whatever page you are on, asking for the
    /// other language leaves you on it.
    #[test]
    fn switching_the_language_keeps_the_page() {
        for page in Page::all() {
            for locale in [Locale::Zh, Locale::En] {
                let (kept, next) = switch_language(locale, page);
                assert_eq!(kept, page);
                assert_eq!(next, locale.toggle());
                let (kept, back) = switch_language(next, kept);
                assert_eq!(kept, page);
                assert_eq!(back, locale);
            }
        }
    }
}
