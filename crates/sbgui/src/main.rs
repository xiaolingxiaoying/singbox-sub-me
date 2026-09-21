//! sbgui — the desktop sing-box client.
//!
//! The window is a pure renderer over `client_core::ClientController`: a
//! background engine owns the core, the clash_api channel and the persisted
//! settings, publishes a [`ClientSnapshot`] roughly four times a second, and
//! the UI only draws that snapshot and sends [`ClientCommand`]s. This is the
//! same control plane the terminal client uses, so the two clients cannot drift
//! apart.
//!
//! THESIS: compact proxy control, informed by the Serein prototype and adapted to sing-box.
//! OWN-WORLD: cool neutral canvas, teal actions, pale active navigation and quiet line icons.
//! STORY: find a section in the sidebar, understand its state, and change it without visual noise.
//! FIRST VIEWPORT: 232px control sidebar with navigation, mode, takeover and live traffic.
//! FORM: a compact desktop control surface with a data-first overview.
//! FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use client_core::clash_api::Connection;
use client_core::command::SettingsPatch;
use client_core::settings::{self, Profiles, Settings};
use client_core::state::{ClientSnapshot, LogLevel, ProxyGroupSnapshot, RouteRuleSnapshot};
use client_core::system_proxy;
use client_core::system_proxy::TrafficMode;
use client_core::{ClientCommand, ClientController};
use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext as _, AssetSource, Bounds, ClickEvent, ClipboardItem, Context, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, TitlebarOptions, Window, WindowBounds,
    WindowControlArea, WindowOptions, div, px, rgb, rgba, size, svg,
};
use gpui_platform::application;

// Serein's desktop palette: a quiet neutral canvas, white working surfaces,
// and a cool teal reserved for active state and primary actions.
const BG: u32 = 0xf7f8fa;
const SURFACE: u32 = 0xffffff;
const SURFACE_2: u32 = 0xf2f4f7;
const BORDER: u32 = 0xe4e7ec;
const TEXT: u32 = 0x101828;
const MUTED: u32 = 0x667085;
const FAINT: u32 = 0x98a2b3;
const CYAN: u32 = 0x0f766e;
const CYAN_DARK: u32 = 0x0b5f59;
const BLUE_2: u32 = 0xecf7f5;
const NAV_ACTIVE: u32 = 0xe7f3f1;
const EDGE: u32 = 0x0a514c;
const MINT: u32 = 0x15803d;
const AMBER: u32 = 0xb45309;
const DANGER: u32 = 0xdc2626;
const RADIUS: f32 = 12.0;
const WINDOW_RADIUS: f32 = 16.0;
const CONTENT_PAD: f32 = 24.0;
const SECTION_GAP: f32 = 14.0;

const DATA_DIR: &str = "sbgui";
const BRAND_ICON_PATH: &str = "serein-icon.png";

struct SereinAssets;

impl AssetSource for SereinAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == BRAND_ICON_PATH {
            Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/serein-icon.png"
            ))))
        } else {
            Ok(None)
        }
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![BRAND_ICON_PATH.into()])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Dashboard,
    Subscriptions,
    Proxies,
    Rules,
    Connections,
    Logs,
    Settings,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "概览",
            Self::Subscriptions => "订阅",
            Self::Proxies => "节点",
            Self::Rules => "规则",
            Self::Connections => "连接",
            Self::Logs => "日志",
            Self::Settings => "设置",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Self::Dashboard => "内核状态、当前节点、实时流量与活跃连接。",
            Self::Subscriptions => "管理本地订阅档案，保持配置与节点列表同步。",
            Self::Proxies => "选择代理组与节点，测试延迟并切换出站路径。",
            Self::Rules => "查看当前 sing-box 路由规则与最终出站。",
            Self::Connections => "查看当前连接、分流规则与实时上下行流量。",
            Self::Logs => "跟随 sing-box 输出与客户端运行事件。",
            Self::Settings => "管理内核、流量模式、端口与自动化行为。",
        }
    }
    fn all() -> [Self; 7] {
        [
            Self::Dashboard,
            Self::Subscriptions,
            Self::Proxies,
            Self::Rules,
            Self::Connections,
            Self::Logs,
            Self::Settings,
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Accent,
    Neutral,
    Warning,
}

/// What the user chose to do with the OS proxy when closing the window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExitChoice {
    /// Leave the OS proxy pointing at the local port.
    Keep,
    /// Restore the captured pre-install proxy state, then close.
    Restore,
}

/// The text fields the window renders, indexing `Sbgui::inputs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputField {
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

const INPUT_FIELDS: [InputField; 10] = [
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
struct TextField {
    focus: FocusHandle,
    text: String,
}

/// The render-time identity of one field, bundled so field helpers stay
/// readable (`text_field(spec, window, cx)`).
struct FieldSpec {
    field: InputField,
    id: &'static str,
    placeholder: &'static str,
    width: f32,
}

/// The logs-page level filter, mirroring the TUI's `info+`/`warn+`/`error`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum LogLevelFilter {
    #[default]
    All,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevelFilter {
    fn label(self) -> &'static str {
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
enum SettingsSection {
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
    fn all() -> [Self; 7] {
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

    fn label(self) -> &'static str {
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
}

struct Sbgui {
    controller: ClientController,
    data_dir: PathBuf,
    snapshot: ClientSnapshot,
    page: Page,
    group_index: usize,
    /// Set while the exit confirmation overlay is visible.
    confirm_exit: bool,
    /// The exit decision, once made; a set choice lets the window close.
    exit_choice: Option<ExitChoice>,
    /// Text fields, indexed by `InputField as usize`.
    inputs: [TextField; INPUT_FIELDS.len()],
    /// The logs-page level filter.
    log_level: LogLevelFilter,
    settings_section: SettingsSection,
    show_subscription_import: bool,
    show_rule_sets: bool,
    core_menu_open: bool,
    node_card_view: bool,
    paused_connections: Option<Vec<Connection>>,
    selected_connection: Option<String>,
    /// Scroll position of the log panel, so "自动滚动" can pin the view to the
    /// newest line instead of being a label that does nothing.
    log_scroll: ScrollHandle,
    /// Row count of the last rendered log panel; a change means new lines.
    log_rows: std::cell::Cell<usize>,
    log_wrap: bool,
    log_follow: bool,
    confirm_close_all: bool,
    /// A profile name whose delete button is armed waiting for a second click.
    confirm_delete_profile: Option<String>,
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
fn env_page() -> Option<Page> {
    let name = std::env::var("SBGUI_PAGE").ok()?;
    Some(match name.to_ascii_lowercase().as_str() {
        "dashboard" | "概览" => Page::Dashboard,
        "subscriptions" | "订阅" => Page::Subscriptions,
        "proxies" | "节点" => Page::Proxies,
        "rules" | "规则" => Page::Rules,
        "connections" | "连接" => Page::Connections,
        "logs" | "日志" => Page::Logs,
        "settings" | "设置" => Page::Settings,
        _ => return None,
    })
}

fn env_window_size() -> (f32, f32) {
    std::env::var("SBGUI_SIZE")
        .ok()
        .and_then(|text| {
            let (w, h) = text.split_once('x')?;
            Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
        })
        .filter(|(w, h)| *w >= 400.0 && *h >= 300.0)
        .unwrap_or((1080.0, 760.0))
}

fn env_show_exit_confirm() -> bool {
    std::env::var("SBGUI_SHOW_EXIT_CONFIRM").is_ok_and(|value| value == "1")
}

impl Sbgui {
    fn new(controller: ClientController, data_dir: PathBuf, cx: &mut Context<Self>) -> Self {
        let snapshot = controller.snapshot();
        Self {
            controller,
            data_dir,
            snapshot,
            page: env_page().unwrap_or(Page::Dashboard),
            group_index: 0,
            confirm_exit: env_show_exit_confirm(),
            exit_choice: None,
            inputs: INPUT_FIELDS.map(|_| TextField {
                focus: cx.focus_handle(),
                text: String::new(),
            }),
            log_level: LogLevelFilter::default(),
            settings_section: SettingsSection::default(),
            show_subscription_import: false,
            show_rule_sets: false,
            core_menu_open: false,
            node_card_view: false,
            paused_connections: None,
            selected_connection: None,
            log_scroll: ScrollHandle::default(),
            log_rows: std::cell::Cell::new(0),
            log_wrap: true,
            log_follow: true,
            confirm_close_all: false,
            confirm_delete_profile: None,
        }
    }

    fn field(&self, field: InputField) -> &TextField {
        &self.inputs[field as usize]
    }

    fn field_mut(&mut self, field: InputField) -> &mut TextField {
        &mut self.inputs[field as usize]
    }

    fn send(&self, command: ClientCommand) {
        let _ = self.controller.send(command);
    }

    /// Fields that mirror persisted settings re-sync from the snapshot
    /// whenever they are not being edited, so the UI never shows a stale
    /// value after the engine applies (or rejects) a change.
    fn sync_fields(&mut self, window: &Window) {
        let settings = &self.snapshot.settings;
        let expected = [
            (InputField::Mirror, settings.mirror.clone()),
            (InputField::MixedPort, settings.mixed_port.to_string()),
            (InputField::TestUrl, settings.test_url.clone()),
            (
                InputField::AutoUpdateMinutes,
                settings.auto_update_minutes.to_string(),
            ),
            (InputField::CoreVersion, settings.core_version.clone()),
        ];
        for (field, value) in expected {
            let state = self.field_mut(field);
            if !state.focus.is_focused(window) && state.text != value {
                state.text = value;
            }
        }
    }

    fn handle_field_key(
        &mut self,
        field: InputField,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifier_held = keystroke.modifiers.control
            || keystroke.modifiers.alt
            || keystroke.modifiers.platform
            || keystroke.modifiers.function;
        if keystroke.modifiers.control || keystroke.modifiers.platform {
            // These fields have no menu and no right-click, so Ctrl+V is the
            // only way to paste a subscription link copied elsewhere; every
            // other modified key stays ignored rather than inserting control
            // characters.
            if keystroke.key.as_str() == "v"
                && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            {
                self.field_mut(field)
                    .text
                    .push_str(&text.replace(['\r', '\n'], ""));
            }
            return;
        }
        if !modifier_held {
            if let Some(typed) = keystroke
                .key_char
                .as_deref()
                .filter(|typed| !typed.contains('\n') && !typed.contains('\r'))
            {
                self.field_mut(field).text.push_str(typed);
            } else {
                match keystroke.key.as_str() {
                    "backspace" => {
                        self.field_mut(field).text.pop();
                    }
                    "space" => self.field_mut(field).text.push(' '),
                    _ => {}
                }
            }
        }
        match keystroke.key.as_str() {
            "enter"
                if matches!(
                    field,
                    InputField::ConnFilter
                        | InputField::LogQuery
                        | InputField::ProxySearch
                        | InputField::RuleSearch
                        | InputField::SubUrl
                ) =>
            {
                self.commit_field(field, cx)
            }
            "escape" => self.reset_field(field),
            _ => {}
        }
        cx.notify();
    }

    /// Applies a committed field. The two filters already apply live on every
    /// keystroke; the settings fields send a partial update, and the engine
    /// reports the outcome (including rejection while the core runs) in the
    /// page header's status line.
    fn commit_field(&mut self, field: InputField, cx: &mut Context<Self>) {
        let text = self.field(field).text.trim().to_owned();
        match field {
            InputField::ConnFilter
            | InputField::LogQuery
            | InputField::ProxySearch
            | InputField::RuleSearch => {}
            InputField::SubUrl => self.submit_sub_url(cx),
            InputField::Mirror => self.send(ClientCommand::UpdateSettings(SettingsPatch {
                mirror: Some(text),
                ..Default::default()
            })),
            InputField::MixedPort => match parse_port(&text) {
                Some(port) => self.send(ClientCommand::UpdateSettings(SettingsPatch {
                    mixed_port: Some(port),
                    ..Default::default()
                })),
                None => self.reset_field(field),
            },
            InputField::TestUrl => {
                if !text.is_empty() {
                    self.send(ClientCommand::UpdateSettings(SettingsPatch {
                        test_url: Some(text),
                        ..Default::default()
                    }));
                }
            }
            InputField::AutoUpdateMinutes => match parse_count(&text) {
                Some(minutes) => self.send(ClientCommand::UpdateSettings(SettingsPatch {
                    auto_update_minutes: Some(minutes),
                    ..Default::default()
                })),
                None => self.reset_field(field),
            },
            InputField::CoreVersion => {
                // Empty means "track the latest release" again.
                self.send(ClientCommand::UpdateSettings(SettingsPatch {
                    core_version: Some(text.trim_start_matches('v').to_owned()),
                    ..Default::default()
                }));
            }
        }
    }

    fn submit_sub_url(&mut self, cx: &mut Context<Self>) {
        let text = self.field(InputField::SubUrl).text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.send(ClientCommand::ImportSubscription {
            name: None,
            url: text.clone(),
        });
        // A non-HTTP line stays in the field so it can be fixed; the engine's
        // rejection shows up in the header status line either way.
        if text.starts_with("https://") || text.starts_with("http://") {
            self.field_mut(InputField::SubUrl).text.clear();
        }
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        for field in [
            InputField::Mirror,
            InputField::MixedPort,
            InputField::TestUrl,
            InputField::AutoUpdateMinutes,
            InputField::CoreVersion,
        ] {
            self.commit_field(field, cx);
        }
        cx.notify();
    }

    /// Esc: filters and the import field clear; settings fields restore the
    /// persisted value, so a half-typed edit never pretends to be saved.
    fn reset_field(&mut self, field: InputField) {
        let value = match field {
            InputField::ConnFilter
            | InputField::LogQuery
            | InputField::ProxySearch
            | InputField::RuleSearch
            | InputField::SubUrl => String::new(),
            InputField::Mirror => self.snapshot.settings.mirror.clone(),
            InputField::MixedPort => self.snapshot.settings.mixed_port.to_string(),
            InputField::TestUrl => self.snapshot.settings.test_url.clone(),
            InputField::AutoUpdateMinutes => self.snapshot.settings.auto_update_minutes.to_string(),
            InputField::CoreVersion => self.snapshot.settings.core_version.clone(),
        };
        self.field_mut(field).text = value;
    }

    /// The veto GPUI consults for `WM_CLOSE` (Alt+F4, taskbar close). A `false`
    /// return keeps the window open so the exit decision can be asked for
    /// first; the custom close button routes through [`Self::request_close`]
    /// because a client-area click never reaches this hook.
    fn handle_close_request(&mut self, cx: &mut Context<Self>) -> bool {
        if self.exit_choice.is_none() && self.snapshot.system_proxy_enabled {
            if !self.confirm_exit {
                self.confirm_exit = true;
                cx.notify();
            }
            return false;
        }
        self.teardown();
        true
    }

    fn choose_exit(&mut self, choice: ExitChoice, window: &mut Window) {
        if choice == ExitChoice::Restore {
            // Synchronous and fast (registry + refresh broadcast); the engine
            // is about to die with the process, so no command round-trip.
            let _ = system_proxy::disable(&self.data_dir);
            self.snapshot.system_proxy_enabled = false;
        }
        self.exit_choice = Some(choice);
        self.teardown();
        window.remove_window();
    }

    /// Stops the core and waits for the engine to reap it, before the window
    /// disappears. The engine's runtime is leaked for the process lifetime, so
    /// nothing would ever drop the child handle and `kill_on_drop` would not
    /// run: sing-box would outlive the client holding the mixed port and, in
    /// TUN mode, the routes it took over.
    fn teardown(&self) {
        self.controller.shutdown();
    }

    fn selected_group(&self) -> Option<ProxyGroupSnapshot> {
        self.snapshot
            .proxy_groups
            .get(self.group_index)
            .or_else(|| self.snapshot.proxy_groups.first())
            .cloned()
    }
}

impl Render for Sbgui {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_fields(window);
        let page = self.page;
        div()
            .size_full()
            .rounded(px(WINDOW_RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(BORDER))
            .font_family("Segoe UI Variable, Microsoft YaHei UI, Noto Sans SC")
            .text_color(rgb(TEXT))
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .child(self.titlebar(window, cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(self.sidebar(page, cx))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .min_h(px(0.0))
                            .min_w(px(0.0))
                            .bg(rgb(BG))
                            .flex()
                            .flex_col()
                            .child(self.header(page, cx))
                            .child(self.global_controls(cx))
                            .child(
                                div()
                                    .id("page-scroll")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .px(px(CONTENT_PAD))
                                    .pb(px(CONTENT_PAD))
                                    .child(
                                        div()
                                            .w_full()
                                            .max_w(px(1320.0))
                                            .mx_auto()
                                            .child(self.content(window, cx)),
                                    ),
                            ),
                    ),
            )
            .children(
                (self.confirm_exit && self.exit_choice.is_none()).then(|| self.exit_overlay(cx)),
            )
    }
}

impl Sbgui {
    /// Modal confirmation shown when the window closes while the OS proxy is
    /// still enabled. Nothing is restored silently: the user picks.
    fn exit_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            // A hitbox only exists for an identified element; without this the
            // overlay looks modal while the buttons underneath still answer
            // clicks.
            .id("exit-modal")
            .occlude()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .bg(rgba(0x00000073))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(470.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .p(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .text_color(rgb(TEXT))
                            .child("退出 Serein？"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child("退出会停止 sing-box。若保留系统代理，其他应用可能无法联网。"),
                    )
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_size(px(11.0))
                            .text_color(rgb(DANGER))
                            .child("选择「仅退出」后，系统代理设置将保留。"),
                    )
                    .child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .id("exit-cancel")
                                    .px(px(15.0))
                                    .py(px(8.0))
                                    .rounded(px(7.0))
                                    .text_size(px(12.0))
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(SURFACE_2)).text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.confirm_exit = false;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(self.exit_button(
                                "exit-keep",
                                "仅退出",
                                Tone::Neutral,
                                cx,
                                ExitChoice::Keep,
                            ))
                            .child(self.exit_button(
                                "exit-restore",
                                "关闭系统代理并退出",
                                Tone::Accent,
                                cx,
                                ExitChoice::Restore,
                            )),
                    ),
            )
    }

    fn exit_button(
        &self,
        id: &'static str,
        label: &'static str,
        tone: Tone,
        cx: &mut Context<Self>,
        choice: ExitChoice,
    ) -> impl IntoElement {
        let (fg, bg, edge) = tone_colors(tone);
        div()
            .id(id)
            .px(px(15.0))
            .py(px(8.0))
            .rounded(px(7.0))
            .bg(rgb(bg))
            .border_1()
            .border_color(rgb(edge))
            .text_size(px(12.0))
            .text_color(rgb(fg))
            .cursor_pointer()
            .hover(move |style| {
                style
                    .bg(rgb(if matches!(tone, Tone::Accent) {
                        CYAN_DARK
                    } else {
                        edge
                    }))
                    .text_color(rgb(fg))
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, window, _| {
                view.choose_exit(choice, window);
            }))
            .child(label)
    }
}

impl Sbgui {
    fn titlebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let maximize_icon = if window.is_maximized() {
            "restore"
        } else {
            "maximize"
        };
        div()
            .h(px(54.0))
            .w_full()
            .flex()
            .items_center()
            .bg(rgb(SURFACE))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                // `WindowControlArea::Drag` is the portable GPUI way to mark a
                // draggable region. On Windows gpui answers `WM_NCHITTEST` with
                // `HTCAPTION`, which also gives native snap and double-click
                // maximize. The old code used `window.start_window_move()`,
                // which is a no-op on Windows (only macOS/Linux implement it),
                // so the custom titlebar could not be dragged at all.
                div()
                    .px(px(14.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(9.0))
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded(px(8.0))
                            .bg(rgb(CYAN))
                            .border_1()
                            .border_color(rgb(EDGE))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("serein", SURFACE, 14.0)),
                    )
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(rgb(TEXT))
                            .child("Serein"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(self.window_button(
                "minimize",
                "minimize",
                WindowControlArea::Min,
                cx,
                |_, window, _| window.minimize_window(),
            ))
            .child(self.window_button(
                "maximize",
                maximize_icon,
                WindowControlArea::Max,
                cx,
                |_, window, _| window.zoom_window(),
            ))
            .child(self.window_button(
                "close",
                "close",
                WindowControlArea::Close,
                cx,
                Self::request_close,
            ))
    }

    /// The custom close button is a click inside the client area, not a
    /// `WM_CLOSE`, so `on_window_should_close` never vetoes it. It has to run
    /// the same decision Alt+F4 goes through, or a click would drop the window
    /// while the OS proxy still points at the dying core's mixed port.
    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_close_request(cx) {
            window.remove_window();
        }
    }

    fn window_button(
        &self,
        button_id: &'static str,
        icon_name: &'static str,
        area: WindowControlArea,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        let is_close = area == WindowControlArea::Close;
        div()
            .id(button_id)
            .w(px(42.0))
            .h(px(54.0))
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .text_color(rgb(MUTED))
            .hover(move |style| {
                if is_close {
                    style.bg(rgb(DANGER)).text_color(rgb(SURFACE))
                } else {
                    style.bg(rgb(SURFACE_2)).text_color(rgb(TEXT))
                }
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| action(view, window, cx)))
            .child(icon(icon_name, MUTED, 14.0))
    }

    fn sidebar(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let side_samples: Vec<(u64, u64)> = snapshot
            .traffic_history
            .iter()
            .map(|point| (point.up, point.down))
            .collect();
        div()
            .id("sidebar-scroll")
            .w(px(224.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(16.0))
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(BORDER))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(Page::all().into_iter().map(|item| {
                        let active = item == page;
                        let (glyph, count) = match item {
                            Page::Dashboard => ("home", None),
                            Page::Subscriptions => ("subscription", Some(snapshot.profiles.len())),
                            Page::Proxies => ("nodes", Some(snapshot.proxy_groups.len())),
                            Page::Rules => ("rules", Some(snapshot.rules.len())),
                            Page::Connections => ("network", Some(snapshot.active_connections)),
                            Page::Logs => ("logs", None),
                            Page::Settings => ("settings", None),
                        };
                        div()
                            .id(format!("nav-{}", item.title()))
                            .w_full()
                            .h(px(42.0))
                            .px(px(12.0))
                            .rounded(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(11.0))
                            .bg(rgb(if active { NAV_ACTIVE } else { SURFACE }))
                            .when(active, |style| style.border_l_2().border_color(rgb(CYAN)))
                            .cursor_pointer()
                            .hover(move |style| {
                                style.bg(rgb(if active { NAV_ACTIVE } else { SURFACE_2 }))
                            })
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.page = item;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(20.0))
                                    .h(px(20.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(glyph, if active { CYAN } else { MUTED }, 16.0)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(14.0))
                                    .text_color(rgb(if active { CYAN } else { TEXT }))
                                    .child(item.title()),
                            )
                            .children(count.map(|value| {
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(if active { TEXT } else { MUTED }))
                                    .child(value.to_string())
                            }))
                    })),
            )
            .child(
                div().mt_auto().pt(px(16.0)).child(
                    div()
                        .pt(px(14.0))
                        .border_t_1()
                        .border_color(rgb(BORDER))
                        .child(side_rate("下载", snapshot.download_speed, CYAN))
                        .child(side_rate("上传", snapshot.upload_speed, AMBER))
                        .child(
                            div()
                                .mt(px(8.0))
                                .h(px(34.0))
                                .w_full()
                                .child(traffic_chart(side_samples, snapshot.traffic_peak().max(1))),
                        ),
                ),
            )
    }

    fn header(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = cx;
        div()
            .px(px(CONTENT_PAD))
            .pt(px(14.0))
            .pb(px(10.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .flex_shrink_0()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(28.0))
                            .text_color(rgb(TEXT))
                            .child(page.title()),
                    )
                    .child(
                        div()
                            .mt(px(3.0))
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child(page.subtitle()),
                    ),
            )
    }

    fn global_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let running = snapshot.core_running;
        let tun_on = snapshot.traffic_mode == TrafficMode::Tun;
        let current = clean_proxy_label(snapshot.current_node.as_deref().unwrap_or("未选择节点"));
        let delay = self.selected_group().and_then(|group| {
            group
                .delays
                .get(snapshot.current_node.as_deref().unwrap_or(""))
                .copied()
        });
        let current_detail = delay
            .map(|value| format!("{current} · {value} ms"))
            .unwrap_or(current);
        let status_text = snapshot
            .busy
            .as_deref()
            .map(|busy| format!("{busy}…"))
            .unwrap_or_else(|| snapshot.status.clone());
        let status_color = if status_text.contains("失败") || status_text.contains("错误") {
            DANGER
        } else if snapshot.busy.is_some() {
            CYAN
        } else {
            MUTED
        };

        div()
            .mx(px(CONTENT_PAD))
            .mb(px(4.0))
            .min_h(px(62.0))
            .px(px(14.0))
            .py(px(9.0))
            .rounded(px(10.0))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(BORDER))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .child(Self::global_control_item(
                "global-core",
                "内核",
                if snapshot.busy.is_some() {
                    if running {
                        "停止并取消"
                    } else {
                        "取消操作"
                    }
                } else if snapshot.starting {
                    "启动中"
                } else if running {
                    "运行中"
                } else {
                    "未运行"
                },
                "nodes",
                if running { MINT } else { FAINT },
                cx,
                if snapshot.busy.is_some() || snapshot.starting {
                    Some(ClientCommand::StopCore)
                } else {
                    (!running).then_some(ClientCommand::StartCore)
                },
            ))
            .child(Self::global_control_item(
                "global-mode",
                "出站模式",
                snapshot.outbound_mode.label(),
                "rules",
                CYAN,
                cx,
                Some(ClientCommand::SetOutboundMode(
                    snapshot.outbound_mode.next(),
                )),
            ))
            .child(
                div()
                    .id("global-node")
                    .w(px(190.0))
                    .flex_shrink_0()
                    .px(px(12.0))
                    .py(px(7.0))
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SURFACE_2)))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.page = Page::Proxies;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("globe", CYAN, 17.0))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(rgb(MUTED))
                                            .child("当前节点"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(rgb(TEXT))
                                            .truncate()
                                            .child(current_detail),
                                    ),
                            ),
                    ),
            )
            .child(Self::global_control_item(
                "global-system-proxy",
                "系统代理",
                if snapshot.system_proxy_enabled {
                    "已开启"
                } else {
                    "已关闭"
                },
                "network",
                if snapshot.system_proxy_enabled {
                    MINT
                } else {
                    MUTED
                },
                cx,
                Some(ClientCommand::ToggleSystemProxy),
            ))
            .child(Self::global_control_item(
                "global-tun",
                "TUN",
                if tun_on { "已开启" } else { "已关闭" },
                "network",
                if tun_on { MINT } else { MUTED },
                cx,
                (!running && !snapshot.starting).then_some(ClientCommand::UpdateSettings(
                    SettingsPatch {
                        traffic_mode: Some(if tun_on {
                            TrafficMode::SystemProxy
                        } else {
                            TrafficMode::Tun
                        }),
                        ..Default::default()
                    },
                )),
            ))
            .child(
                div()
                    .id("global-core-menu")
                    .size(px(36.0))
                    .flex_shrink_0()
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(18.0))
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SURFACE_2)).text_color(rgb(TEXT)))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.core_menu_open = !view.core_menu_open;
                        cx.notify();
                    }))
                    .child("···"),
            )
            .children((self.core_menu_open && running).then(|| {
                div()
                    .w_full()
                    .pt(px(8.0))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.0))
                    .child(self.action(
                        "global-restart",
                        "重启内核",
                        Tone::Neutral,
                        cx,
                        ClientCommand::RestartCore,
                    ))
                    .child(self.action(
                        "global-stop",
                        "停止内核",
                        Tone::Warning,
                        cx,
                        ClientCommand::StopCore,
                    ))
            }))
            .children((!status_text.is_empty()).then(|| {
                div()
                    .w_full()
                    .text_size(px(11.0))
                    .text_color(rgb(status_color))
                    .child(status_text)
            }))
    }

    fn global_control_item(
        id: &'static str,
        label: &'static str,
        value: impl Into<String>,
        glyph: &'static str,
        color: u32,
        cx: &mut Context<Self>,
        command: Option<ClientCommand>,
    ) -> impl IntoElement {
        let value = value.into();
        let clickable = command.is_some();
        div()
            .id(id)
            .w(px(126.0))
            .flex_shrink_0()
            .px(px(12.0))
            .py(px(7.0))
            .rounded(px(7.0))
            .when(clickable, |s| {
                s.cursor_pointer().hover(|s| s.bg(rgb(SURFACE_2)))
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if let Some(command) = command.clone() {
                    view.send(command);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(icon(glyph, color, 17.0))
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgb(MUTED))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(color))
                                    .child(value),
                            ),
                    ),
            )
    }

    /// One header/row action button. The command is built at click time so the
    /// buttons always carry the freshest view state.
    fn action(
        &self,
        id: &'static str,
        label: &'static str,
        tone: Tone,
        cx: &mut Context<Self>,
        command: ClientCommand,
    ) -> impl IntoElement {
        let (fg, bg, edge) = tone_colors(tone);
        div()
            .id(id)
            .px(px(15.0))
            .py(px(8.0))
            .rounded(px(7.0))
            .bg(rgb(bg))
            .border_1()
            .border_color(rgb(edge))
            .text_size(px(12.0))
            .text_color(rgb(fg))
            .cursor_pointer()
            .hover(move |style| {
                style
                    .bg(rgb(if matches!(tone, Tone::Accent) {
                        CYAN_DARK
                    } else {
                        edge
                    }))
                    .text_color(rgb(fg))
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if matches!(command, ClientCommand::StartCore) && view.snapshot.starting {
                    return;
                }
                view.send(command.clone());
                cx.notify();
            }))
            .child(label)
    }

    fn content(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        match self.page {
            Page::Dashboard => self.dashboard(cx),
            Page::Subscriptions => self.subscriptions(window, cx),
            Page::Proxies => self.proxies(window, cx),
            Page::Rules => self.rules(window, cx),
            Page::Connections => self.connections(window, cx),
            Page::Logs => self.logs(window, cx),
            Page::Settings => self.settings(window, cx),
        }
    }

    /// One single-line text field. Click focuses; typing edits live; Enter
    /// commits (`commit_field`); Esc resets (`reset_field`).
    fn text_field(
        &self,
        spec: FieldSpec,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let FieldSpec {
            field,
            id,
            placeholder,
            width,
        } = spec;
        let state = self.field(field);
        let focused = state.focus.is_focused(window);
        let empty = state.text.is_empty();
        div()
            .id(id)
            .w(px(width))
            .min_w(px(120.0))
            .px(px(10.0))
            .py(px(7.0))
            .rounded(px(7.0))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(if focused { CYAN } else { BORDER }))
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor_pointer()
            .text_size(px(12.0))
            .text_color(rgb(if empty { FAINT } else { TEXT }))
            .track_focus(&state.focus)
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.field(field).focus.focus(window, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
                view.handle_field_key(field, event, cx);
            }))
            .child(if empty {
                placeholder.to_owned()
            } else {
                state.text.clone()
            })
            .children(focused.then(|| div().w(px(1.5)).h(px(15.0)).bg(rgb(CYAN))))
    }

    /// A label with an editable value line for the settings page.
    fn edit_line(
        &self,
        label: &'static str,
        spec: FieldSpec,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .mt(px(10.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .child(div().text_size(px(12.0)).text_color(rgb(TEXT)).child(label))
            .child(self.text_field(spec, window, cx))
    }

    // ------------------------------------------------------------ dashboard

    fn dashboard(&self, _cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let current = clean_proxy_label(snapshot.current_node.as_deref().unwrap_or("尚未选择节点"));
        let current_delay = self.selected_group().as_ref().and_then(|group| {
            group
                .delays
                .get(snapshot.current_node.as_deref().unwrap_or(""))
                .copied()
        });
        let tcp_count = snapshot
            .connections
            .connections
            .iter()
            .filter(|connection| connection.metadata.network.eq_ignore_ascii_case("tcp"))
            .count();
        let udp_count = snapshot.active_connections.saturating_sub(tcp_count);
        let profile = snapshot
            .active_profile
            .as_ref()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "尚未添加订阅".to_owned());
        let connected = snapshot.core_running;
        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .children(snapshot.profiles.is_empty().then(|| {
                div()
                    .p(px(16.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(rgb(TEXT))
                            .child("完成首次连接"),
                    )
                    .child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.0))
                            .children(
                                ["添加订阅", "选择节点", "启动内核", "开启系统代理"]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, step)| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .size(px(24.0))
                                                    .rounded(px(12.0))
                                                    .bg(rgb(BLUE_2))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(11.0))
                                                    .text_color(rgb(CYAN))
                                                    .child((index + 1).to_string()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(rgb(MUTED))
                                                    .child(step),
                                            )
                                            .children(
                                                (index < 3).then(|| icon("chevron", FAINT, 13.0)),
                                            )
                                    }),
                            ),
                    )
            }))
            .child(
                div()
                    .id("overview-connection")
                    .min_h(px(76.0))
                    .px(px(18.0))
                    .py(px(14.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .size(px(38.0))
                            .rounded(px(19.0))
                            .bg(rgb(if connected { 0xeaf7ee } else { SURFACE_2 }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                if connected { "check" } else { "network" },
                                if connected { MINT } else { MUTED },
                                20.0,
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(div().text_size(px(15.0)).text_color(rgb(TEXT)).child(
                                if connected {
                                    "内核已连接"
                                } else {
                                    "内核未连接"
                                },
                            ))
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(format!("{profile} · {current}")),
                            ),
                    )
                    .children(
                        current_delay.map(|delay| pill(format!("{delay} ms"), delay_color(delay))),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(snapshot.outbound_mode.label()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(12.0))
                    .child(metric_card(
                        "下载",
                        format!("{} /s", human_bytes(snapshot.download_speed)),
                        format!("5 分钟峰值  {} /s", human_bytes(snapshot.download_peak())),
                        false,
                    ))
                    .child(metric_card(
                        "上传",
                        format!("{} /s", human_bytes(snapshot.upload_speed)),
                        format!("5 分钟峰值  {} /s", human_bytes(snapshot.upload_peak())),
                        false,
                    ))
                    .child(metric_card(
                        "活跃连接",
                        format!("{} 条", snapshot.active_connections),
                        format!("TCP {} · UDP {}", tcp_count, udp_count),
                        false,
                    ))
                    .child(metric_card(
                        "累计流量",
                        human_bytes(snapshot.total_download + snapshot.total_upload),
                        format!(
                            "↓ {}  ·  ↑ {}",
                            human_bytes(snapshot.total_download),
                            human_bytes(snapshot.total_upload)
                        ),
                        false,
                    )),
            )
            .child(div().min_h(px(270.0)).child(traffic_panel(snapshot)))
            .child(
                panel("运行状态").child(
                    div().mt(px(12.0)).flex().flex_wrap().children(
                        [
                            (
                                "内存占用",
                                if snapshot.core_running && snapshot.memory_used > 0 {
                                    human_bytes(snapshot.memory_used)
                                } else {
                                    "—".to_owned()
                                },
                            ),
                            (
                                "内核版本",
                                snapshot
                                    .core_runtime_version
                                    .clone()
                                    .or_else(|| snapshot.core_version.clone())
                                    .unwrap_or_else(|| "未安装".to_owned()),
                            ),
                            ("自动重启", format!("{} 次", snapshot.restart_attempts)),
                            ("连接协议", format!("TCP {tcp_count} · UDP {udp_count}")),
                        ]
                        .into_iter()
                        .map(|(label, value)| {
                            div()
                                .w(px(210.0))
                                .flex_grow(1.0)
                                .py(px(8.0))
                                .px(px(12.0))
                                .border_r_1()
                                .border_color(rgb(BORDER))
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(rgb(MUTED))
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .mt(px(4.0))
                                        .text_size(px(14.0))
                                        .text_color(rgb(TEXT))
                                        .child(value),
                                )
                        }),
                    ),
                ),
            )
    }

    // --------------------------------------------------------- subscriptions

    fn subscriptions(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let profiles = self.snapshot.profiles.clone();
        let mut root = div().flex().flex_col().gap(px(SECTION_GAP)).child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap(px(8.0))
                .child(
                    div()
                        .id("add-subscription")
                        .px(px(14.0))
                        .py(px(8.0))
                        .rounded(px(7.0))
                        .bg(rgb(CYAN))
                        .text_size(px(12.0))
                        .text_color(rgb(SURFACE))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(CYAN_DARK)))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.show_subscription_import = true;
                            cx.notify();
                        }))
                        .child("+ 添加订阅"),
                )
                .child(
                    div()
                        .id("import-from-clipboard")
                        .px(px(14.0))
                        .py(px(8.0))
                        .rounded(px(7.0))
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_size(px(12.0))
                        .text_color(rgb(TEXT))
                        .cursor_pointer()
                        .hover(|style| style.border_color(rgb(CYAN)).text_color(rgb(CYAN)))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            let text = cx
                                .read_from_clipboard()
                                .and_then(|item| item.text())
                                .unwrap_or_default();
                            if !text.trim().is_empty() {
                                view.send(ClientCommand::ImportSubscription {
                                    name: None,
                                    url: text,
                                });
                            }
                            cx.notify();
                        }))
                        .child("从剪贴板导入"),
                )
                .child(self.action(
                    "update-all-subscriptions",
                    "全部更新",
                    Tone::Neutral,
                    cx,
                    ClientCommand::UpdateSubscription,
                ))
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(format!("{} 个订阅档案", profiles.len())),
                ),
        );

        if self.show_subscription_import {
            root = root.child(
                div()
                    .p(px(16.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(CYAN))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(14.0))
                                    .text_color(rgb(TEXT))
                                    .child("添加订阅"),
                            )
                            .child(
                                div()
                                    .id("close-subscription-import")
                                    .size(px(36.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(SURFACE_2)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.show_subscription_import = false;
                                        cx.notify();
                                    }))
                                    .child(icon("close", MUTED, 14.0)),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child("粘贴 HTTP/HTTPS 订阅地址或 sing-box JSON 地址。"),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::SubUrl,
                                    id: "sub-url",
                                    placeholder: "https://…",
                                    width: 520.0,
                                },
                                window,
                                cx,
                            ))
                            .child(
                                div()
                                    .id("import-manual")
                                    .px(px(14.0))
                                    .py(px(8.0))
                                    .rounded(px(7.0))
                                    .bg(rgb(CYAN))
                                    .text_size(px(12.0))
                                    .text_color(rgb(SURFACE))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(CYAN_DARK)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.submit_sub_url(cx);
                                        view.show_subscription_import = false;
                                    }))
                                    .child("添加"),
                            ),
                    ),
            );
        }

        if profiles.is_empty() {
            return root.child(empty_state(
                "还没有订阅",
                "点击「添加订阅」，或从剪贴板导入订阅链接。",
                None,
                cx,
            ));
        }

        let rows = profiles.into_iter().enumerate().map(|(index, profile)| {
            let active = profile.active;
            let name_for_activate = profile.name.clone();
            let name_for_remove = profile.name.clone();
            let updated = age_label(profile.last_updated);
            let usage = if active {
                usage_label(self.snapshot.subscription_usage.as_ref())
            } else {
                "用量信息仅在当前订阅可用".to_owned()
            };
            div()
                .id(format!("subscription-row-{index}"))
                .w_full()
                .min_h(px(68.0))
                .px(px(14.0))
                .py(px(10.0))
                .bg(if active { rgb(BLUE_2) } else { rgb(SURFACE) })
                .border_b_1()
                .border_color(rgb(BORDER))
                .flex()
                .items_center()
                .gap(px(12.0))
                .text_color(rgb(TEXT))
                .child(div().w(px(22.0)).child(if active {
                    icon("check", CYAN, 17.0).into_any_element()
                } else {
                    status_dot(FAINT).into_any_element()
                }))
                .child(
                    div()
                        .w(px(250.0))
                        .min_w(px(160.0))
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(rgb(TEXT))
                                .truncate()
                                .child(profile.name.clone()),
                        )
                        .child(
                            div()
                                .mt(px(3.0))
                                .text_size(px(10.0))
                                .text_color(rgb(MUTED))
                                .truncate()
                                .child(if active { "当前" } else { "可用" }),
                        ),
                )
                .child(
                    div()
                        .w(px(250.0))
                        .flex_grow(1.0)
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .truncate()
                        .child(usage),
                )
                .child(
                    div()
                        .w(px(100.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(updated),
                )
                .child(
                    div()
                        .w(px(72.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(if active {
                            self.snapshot
                                .proxy_groups
                                .iter()
                                .map(|group| group.members.len())
                                .sum::<usize>()
                                .to_string()
                        } else {
                            "—".to_owned()
                        }),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .children((!active).then(|| {
                            div()
                                .id(format!("activate-{index}"))
                                .px(px(12.0))
                                .py(px(7.0))
                                .rounded(px(7.0))
                                .bg(rgb(BLUE_2))
                                .text_size(px(11.0))
                                .text_color(rgb(CYAN))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.send(ClientCommand::SwitchProfile(
                                        name_for_activate.clone(),
                                    ));
                                    cx.notify();
                                }))
                                .child("设为当前")
                        }))
                        .children(active.then(|| {
                            self.mini_action(
                                index + 100_000,
                                "更新",
                                cx,
                                ClientCommand::UpdateSubscription,
                            )
                        }))
                        .child({
                            let armed = self
                                .confirm_delete_profile
                                .as_deref()
                                .is_some_and(|pending| pending == name_for_remove);
                            div()
                                .id(format!("remove-{index}"))
                                .px(px(10.0))
                                .py(px(7.0))
                                .rounded(px(7.0))
                                .text_size(px(11.0))
                                .text_color(rgb(DANGER))
                                .hover(|style| style.bg(rgb(0xffecee)))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    // Deleting also drops the cached node list,
                                    // so the first click only arms the button —
                                    // the same shape as 关闭全部.
                                    if view.confirm_delete_profile.as_deref()
                                        == Some(name_for_remove.as_str())
                                    {
                                        view.send(ClientCommand::RemoveProfile(
                                            name_for_remove.clone(),
                                        ));
                                        view.confirm_delete_profile = None;
                                    } else {
                                        view.confirm_delete_profile = Some(name_for_remove.clone());
                                    }
                                    cx.notify();
                                }))
                                .child(if armed { "确认删除？" } else { "删除" })
                        }),
                )
        });
        root.child(
            div()
                .rounded(px(RADIUS))
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .min_h(px(38.0))
                        .px(px(14.0))
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .bg(rgb(SURFACE_2))
                        .text_size(px(10.0))
                        .text_color(rgb(MUTED))
                        .child(div().w(px(22.0)).child(""))
                        .child(div().w(px(250.0)).min_w(px(160.0)).child("订阅名称"))
                        .child(div().w(px(250.0)).flex_grow(1.0).child("用量"))
                        .child(div().w(px(100.0)).child("上次更新"))
                        .child(div().w(px(72.0)).child("节点"))
                        .child(div().w(px(126.0)).child("操作")),
                )
                .children(rows),
        )
    }

    // -------------------------------------------------------------- proxies

    fn proxies(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let groups = &self.snapshot.proxy_groups;
        let Some(group) = self.selected_group() else {
            return div().child(empty_state(
                "暂无代理组",
                if self.snapshot.core_running {
                    "正在等待内核返回代理组，请稍后查看。"
                } else {
                    "导入订阅并启动内核后，在这里选择节点。"
                },
                Some("去订阅管理"),
                cx,
            ));
        };
        let automatic = group.is_auto();
        let query = self
            .field(InputField::ProxySearch)
            .text
            .trim()
            .to_lowercase();
        let card_view = self.node_card_view;
        let members = group
            .members
            .iter()
            .enumerate()
            .filter(|(_, member)| query.is_empty() || member.to_lowercase().contains(&query))
            .map(|(index, member)| {
                let selected = member == &group.current;
                let failed = group.failed.contains(member);
                let delay = group.delays.get(member).copied();
                let label = if failed {
                    "超时".to_owned()
                } else {
                    delay
                        .map(|d| format!("{d} ms"))
                        .unwrap_or_else(|| "测试".to_owned())
                };
                let color = if failed {
                    DANGER
                } else {
                    delay.map(delay_color).unwrap_or(CYAN)
                };
                let group_name = group.name.clone();
                let node_name = member.clone();
                let test_name = member.clone();
                let display_name = clean_proxy_label(member);
                div()
                    .id(format!("node-{index}"))
                    .w(if card_view { px(238.0) } else { px(720.0) })
                    .when(card_view, |row| row.flex_grow(1.0))
                    .min_w(px(0.0))
                    .min_h(px(52.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .rounded(px(if card_view { 9.0 } else { 0.0 }))
                    .bg(rgb(if selected { BLUE_2 } else { SURFACE }))
                    .when(card_view, |row| {
                        row.border_1()
                            .border_color(rgb(if selected { CYAN } else { BORDER }))
                    })
                    .when(!card_view, |row| row.border_b_1().border_color(rgb(BORDER)))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .when(!automatic, |row| {
                        row.cursor_pointer().hover(|s| s.bg(rgb(BLUE_2)))
                    })
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        if !automatic {
                            view.send(ClientCommand::SwitchNode {
                                group: group_name.clone(),
                                node: node_name.clone(),
                            });
                            cx.notify();
                        }
                    }))
                    .child(if selected {
                        icon("check", CYAN, 16.0).into_any_element()
                    } else {
                        status_dot(FAINT).into_any_element()
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(display_name),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .text_size(px(10.0))
                                    .text_color(rgb(MUTED))
                                    .child(if automatic {
                                        "自动选择组"
                                    } else {
                                        "手动选择"
                                    }),
                            ),
                    )
                    .children(selected.then(|| pill("当前", CYAN)))
                    .child(
                        div()
                            .id(format!("delay-{index}"))
                            .min_w(px(68.0))
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .text_size(px(11.0))
                            .text_color(rgb(color))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(SURFACE_2)))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                view.send(ClientCommand::TestNode(test_name.clone()));
                                cx.notify();
                            }))
                            .child(label),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .flex_wrap()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "{} 个代理组 · {} 个节点",
                                groups.len(),
                                groups.iter().map(|item| item.members.len()).sum::<usize>()
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::ProxySearch,
                                    id: "proxy-search",
                                    placeholder: "搜索节点…",
                                    width: 210.0,
                                },
                                window,
                                cx,
                            ))
                            .child(self.action(
                                "test-group-toolbar",
                                "全部测速",
                                Tone::Neutral,
                                cx,
                                ClientCommand::TestGroup(group.name.clone()),
                            ))
                            .child(
                                div()
                                    .id("node-list-view")
                                    .px(px(10.0))
                                    .py(px(8.0))
                                    .rounded(px(7.0))
                                    .bg(rgb(if !card_view { BLUE_2 } else { SURFACE }))
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .text_size(px(11.0))
                                    .text_color(rgb(if !card_view { CYAN } else { MUTED }))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.node_card_view = false;
                                        cx.notify();
                                    }))
                                    .child("列表"),
                            )
                            .child(
                                div()
                                    .id("node-card-view")
                                    .px(px(10.0))
                                    .py(px(8.0))
                                    .rounded(px(7.0))
                                    .bg(rgb(if card_view { BLUE_2 } else { SURFACE }))
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .text_size(px(11.0))
                                    .text_color(rgb(if card_view { CYAN } else { MUTED }))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.node_card_view = true;
                                        cx.notify();
                                    }))
                                    .child("卡片"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .children(groups.iter().enumerate().map(|(index, item)| {
                        let active = item.name == group.name;
                        div()
                            .id(format!("group-{index}"))
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(8.0))
                            .bg(rgb(if active { BLUE_2 } else { SURFACE }))
                            .text_size(px(12.0))
                            .text_color(rgb(if active { CYAN } else { MUTED }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(BLUE_2)))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.group_index = index;
                                cx.notify();
                            }))
                            .child(clean_proxy_label(&item.name))
                    })),
            )
            .child(
                div()
                    .p(px(14.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon("nodes", CYAN, 18.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(clean_proxy_label(&group.name)),
                            )
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(format!(
                                        "{} · {} 个节点",
                                        if automatic {
                                            "自动选择，由内核决定当前节点"
                                        } else {
                                            "手动选择"
                                        },
                                        group.members.len()
                                    )),
                            ),
                    )
                    .child(self.action(
                        "test-group",
                        "测试延迟",
                        Tone::Neutral,
                        cx,
                        ClientCommand::TestGroup(group.name.clone()),
                    )),
            )
            .child(
                div()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .flex()
                    .flex_wrap()
                    .gap(px(if card_view { 8.0 } else { 0.0 }))
                    .children(members),
            )
    }

    // --------------------------------------------------------------- rules

    fn rules(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let rules = &self.snapshot.rules;
        let rule_sets = &self.snapshot.rule_sets;
        let count = rules.len();
        let query = self
            .field(InputField::RuleSearch)
            .text
            .trim()
            .to_lowercase();
        let rows: Vec<gpui::AnyElement> = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                query.is_empty()
                    || rule.matcher.to_lowercase().contains(&query)
                    || rule.outbound.to_lowercase().contains(&query)
            })
            .map(|(index, rule)| rule_row(index, rule))
            .collect();
        let rule_set_rows: Vec<gpui::AnyElement> = rule_sets
            .iter()
            .map(|set| {
                div()
                    .w_full()
                    .px(px(15.0))
                    .py(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(CYAN))
                            .child(set.tag.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(FAINT))
                            .child(set.kind.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(if set.url.is_empty() {
                                "（本地规则集）".to_owned()
                            } else {
                                set.url.clone()
                            }),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .flex_wrap()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_dot(if count > 0 { MINT } else { FAINT }))
                            .child(div().text_size(px(12.0)).text_color(rgb(MUTED)).child(
                                if count > 0 {
                                    format!("当前配置 · {count} 条规则")
                                } else {
                                    "等待激活配置".to_owned()
                                },
                            )),
                    )
                    .child(self.text_field(
                        FieldSpec {
                            field: InputField::RuleSearch,
                            id: "rule-search",
                            placeholder: "搜索匹配条件或出站…",
                            width: 240.0,
                        },
                        window,
                        cx,
                    ))
                    .child(self.action(
                        "refresh-rules",
                        "刷新状态",
                        Tone::Neutral,
                        cx,
                        ClientCommand::Refresh,
                    )),
            )
            .children((!rule_set_rows.is_empty()).then(|| {
                div()
                    .id("rule-sets-panel")
                    .w_full()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("rule-sets-toggle")
                            .px(px(15.0))
                            .py(px(11.0))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(SURFACE_2)))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.show_rule_sets = !view.show_rule_sets;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.0))
                                    .text_color(rgb(TEXT))
                                    .child(format!("规则集 · {} 个", rule_set_rows.len())),
                            )
                            .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child(
                                if self.show_rule_sets {
                                    "收起"
                                } else {
                                    "展开"
                                },
                            )),
                    )
                    .children(if self.show_rule_sets {
                        rule_set_rows
                    } else {
                        Vec::new()
                    })
            }))
            .child(
                div()
                    .id("rules-panel")
                    .w_full()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .child(
                        div()
                            .px(px(15.0))
                            .py(px(9.0))
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .bg(rgb(SURFACE_2))
                            .text_size(px(10.0))
                            .text_color(rgb(MUTED))
                            .child(div().w(px(58.0)).child("优先级"))
                            .child(div().flex_1().child("匹配条件"))
                            .child(div().w(px(180.0)).child("出站"))
                            .child(div().w(px(72.0)).child("状态")),
                    )
                    .children(if rows.is_empty() {
                        vec![
                            empty_state(
                                "暂无可读规则",
                                "启动内核或激活订阅后，这里会显示当前配置的路由规则。",
                                None,
                                cx,
                            )
                            .into_any_element(),
                        ]
                    } else {
                        rows
                    }),
            )
    }

    fn mini_action(
        &self,
        id: usize,
        label: &'static str,
        cx: &mut Context<Self>,
        command: ClientCommand,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px(px(8.0))
            .py(px(3.0))
            .rounded(px(12.0))
            .bg(rgb(SURFACE_2))
            .border_1()
            .border_color(rgb(BORDER))
            .text_size(px(11.0))
            .text_color(rgb(MUTED))
            .hover(|style| style.bg(rgb(BLUE_2)).text_color(rgb(CYAN)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.send(command.clone());
                cx.notify();
            }))
            .child(label)
    }

    fn utility_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        toggle: impl Fn(&mut Sbgui) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px(px(10.0))
            .py(px(7.0))
            .rounded(px(7.0))
            .bg(rgb(if active { BLUE_2 } else { SURFACE }))
            .border_1()
            .border_color(rgb(if active { CYAN } else { BORDER }))
            .text_size(px(11.0))
            .text_color(rgb(if active { CYAN } else { TEXT }))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                toggle(view);
                cx.notify();
            }))
            .child(label)
    }

    // ---------------------------------------------------------- connections

    fn connections(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let query = self.field(InputField::ConnFilter).text.trim().to_owned();
        let mut connections = self
            .paused_connections
            .clone()
            .unwrap_or_else(|| self.snapshot.connections.connections.clone());
        // The header advertises download-first ordering, so the rows must
        // actually follow it: the biggest current bandwidth users lead.
        connections.sort_by_key(|connection| std::cmp::Reverse(connection.download));
        let total = connections.len();
        if !query.is_empty() {
            connections.retain(|connection| connection.matches(&query));
        }
        let rows: Vec<gpui::AnyElement> = connections
            .iter()
            .enumerate()
            .map(|(index, connection)| connection_row(index, connection, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .child(div().text_size(px(11.0)).text_color(rgb(MUTED)).child(
                                if query.is_empty() {
                                    format!("{total} 条活动连接 · 按下载流量排序")
                                } else {
                                    format!(
                                        "匹配 {} / {total} 条 · 按下载流量排序",
                                        connections.len()
                                    )
                                },
                            ))
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child("行右侧按钮可断开单条连接"),
                            ),
                    )
                    .child(self.text_field(
                        FieldSpec {
                            field: InputField::ConnFilter,
                            id: "conn-filter",
                            placeholder: "按主机 / 目标 / 规则筛选…",
                            width: 240.0,
                        },
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .id("pause-connections")
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(7.0))
                            .bg(rgb(if self.paused_connections.is_some() {
                                BLUE_2
                            } else {
                                SURFACE
                            }))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(12.0))
                            .text_color(rgb(if self.paused_connections.is_some() {
                                CYAN
                            } else {
                                TEXT
                            }))
                            .cursor_pointer()
                            .hover(|s| s.border_color(rgb(CYAN)))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.paused_connections = if view.paused_connections.is_some() {
                                    None
                                } else {
                                    Some(view.snapshot.connections.connections.clone())
                                };
                                cx.notify();
                            }))
                            .child(if self.paused_connections.is_some() {
                                "继续刷新"
                            } else {
                                "暂停刷新"
                            }),
                    )
                    .child(
                        div()
                            .id("close-all")
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(7.0))
                            .bg(rgb(SURFACE))
                            .border_1()
                            .border_color(rgb(DANGER))
                            .text_size(px(12.0))
                            .text_color(rgb(DANGER))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0xfff1f1)))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                if view.confirm_close_all {
                                    view.send(ClientCommand::CloseAllConnections);
                                    view.confirm_close_all = false;
                                } else {
                                    view.confirm_close_all = true;
                                }
                                cx.notify();
                            }))
                            .child(if self.confirm_close_all {
                                "再次点击确认"
                            } else {
                                "关闭全部"
                            }),
                    ),
            )
            .child(
                // Same white bordered container as the log list, so the table
                // does not float on the canvas next to the empty-state card.
                div()
                    .id("connections-panel")
                    .w_full()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("connections-horizontal")
                            .w_full()
                            .overflow_x_scroll()
                            .child(
                                div()
                                    .min_w(px(1080.0))
                                    .child(connection_header())
                                    .child(div().flex().flex_col().gap(px(4.0)).children(rows)),
                            ),
                    ),
            )
            .children(self.selected_connection.as_ref().and_then(|selected| {
                connections
                    .iter()
                    .find(|connection| &connection.id == selected)
                    .map(|connection| {
                        div()
                            .p(px(16.0))
                            .rounded(px(RADIUS))
                            .bg(rgb(SURFACE))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(14.0))
                                            .text_color(rgb(TEXT))
                                            .child("连接详情"),
                                    )
                                    .child(
                                        div()
                                            .id("close-connection-detail")
                                            .size(px(36.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(rgb(SURFACE_2)))
                                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                                view.selected_connection = None;
                                                cx.notify();
                                            }))
                                            .child(icon("close", MUTED, 14.0)),
                                    ),
                            )
                            .child(setting_line("远程目标", &connection_target(connection)))
                            .child(setting_line(
                                "命中规则",
                                if connection.rule.is_empty() {
                                    "未匹配"
                                } else {
                                    &connection.rule
                                },
                            ))
                            .child(setting_line("使用节点", &connection_chain(connection)))
                            .child(setting_line("协议", &connection.metadata.network))
                    })
            }))
            .children(if total == 0 {
                Some(empty_state(
                    "暂无活动连接",
                    "内核运行后，经过本机代理的连接会实时显示在这里。",
                    if self.snapshot.core_running || self.snapshot.starting {
                        None
                    } else {
                        Some("启动内核")
                    },
                    cx,
                ))
            } else if connections.is_empty() {
                Some(empty_state(
                    "无匹配连接",
                    "换个关键字，或按 Esc 清空筛选。",
                    None,
                    cx,
                ))
            } else {
                None
            })
    }

    // ----------------------------------------------------------------- logs

    fn logs(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let query = self.field(InputField::LogQuery).text.trim().to_lowercase();
        let level = self.log_level;
        let keep = |line: &str| -> bool {
            if !query.is_empty() && !line.to_lowercase().contains(&query) {
                return false;
            }
            match level {
                // Threshold semantics shared with the terminal client: "Info"
                // keeps the client's own unmarked events rather than hiding
                // them, and "Debug" is the whole buffer.
                LogLevelFilter::All => client_core::state::log_level_shown(None, line),
                LogLevelFilter::Debug => {
                    client_core::state::log_level_shown(Some(LogLevel::Debug), line)
                }
                LogLevelFilter::Info => {
                    client_core::state::log_level_shown(Some(LogLevel::Info), line)
                }
                LogLevelFilter::Warn => {
                    client_core::state::log_level_shown(Some(LogLevel::Warn), line)
                }
                LogLevelFilter::Error => {
                    client_core::state::log_level_shown(Some(LogLevel::Error), line)
                }
            }
        };
        let kernel: Vec<String> = self
            .snapshot
            .core_logs
            .iter()
            .filter(|line| keep(line))
            .cloned()
            .collect();
        let events: Vec<String> = self
            .snapshot
            .events
            .iter()
            .filter(|line| keep(line))
            .cloned()
            .collect();
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for (index, line) in kernel.iter().rev().take(180).rev().enumerate() {
            rows.push(log_row(index, "sing-box", line, self.log_wrap));
        }
        let event_offset = rows.len();
        for (index, line) in events.iter().rev().take(60).rev().enumerate() {
            rows.push(log_row(event_offset + index, "客户端", line, self.log_wrap));
        }
        // "自动滚动" pins the view to the newest line whenever the panel grew;
        // scrolling a list that did not change would fight the user's wheel.
        let rows_seen = self.log_rows.replace(rows.len());
        if self.log_follow && rows.len() > rows_seen {
            self.log_scroll.scroll_to_item(rows.len() - 1);
        }
        let copy_text = kernel
            .iter()
            .map(|line| format!("[sing-box] {line}"))
            .chain(events.iter().map(|line| format!("[客户端] {line}")))
            .collect::<Vec<_>>()
            .join("\n");
        let mut level_chips: Vec<gpui::AnyElement> = Vec::new();
        for candidate in [
            LogLevelFilter::All,
            LogLevelFilter::Debug,
            LogLevelFilter::Info,
            LogLevelFilter::Warn,
            LogLevelFilter::Error,
        ] {
            let active = self.log_level == candidate;
            level_chips.push(
                div()
                    .id(format!("log-level-{:?}", candidate))
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(12.0))
                    .text_size(px(11.0))
                    .cursor_pointer()
                    .bg(rgb(if active { BLUE_2 } else { SURFACE_2 }))
                    .border_1()
                    .border_color(rgb(if active { CYAN } else { BORDER }))
                    .text_color(rgb(if active { CYAN } else { MUTED }))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.log_level = candidate;
                        cx.notify();
                    }))
                    .child(candidate.label())
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .flex_wrap()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .children(level_chips),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::LogQuery,
                                    id: "log-query",
                                    placeholder: "按关键字过滤日志…",
                                    width: 240.0,
                                },
                                window,
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(format!("{} 行", rows.len())),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.utility_toggle(
                        "log-follow",
                        if self.log_follow {
                            "自动滚动：开"
                        } else {
                            "自动滚动：关"
                        },
                        self.log_follow,
                        cx,
                        |view| view.log_follow = !view.log_follow,
                    ))
                    .child(self.utility_toggle(
                        "log-wrap",
                        if self.log_wrap {
                            "自动换行：开"
                        } else {
                            "自动换行：关"
                        },
                        self.log_wrap,
                        cx,
                        |view| view.log_wrap = !view.log_wrap,
                    ))
                    .child(
                        div()
                            .id("copy-logs")
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(SURFACE_2)))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                            }))
                            .child("复制"),
                    )
                    .child(
                        div()
                            .id("clear-logs")
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(SURFACE_2)))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                // The engine owns the buffers: hiding lines by
                                // count stops working once the ring is full.
                                view.send(ClientCommand::ClearLogs);
                                view.log_rows.set(0);
                                cx.notify();
                            }))
                            .child("清空"),
                    )
                    .child(
                        div()
                            .id("export-logs")
                            .px(px(10.0))
                            .py(px(7.0))
                            .rounded(px(7.0))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .text_size(px(11.0))
                            .text_color(rgb(TEXT))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(SURFACE_2)))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                let text = view
                                    .snapshot
                                    .core_logs
                                    .iter()
                                    .map(|line| format!("[sing-box] {line}"))
                                    .chain(
                                        view.snapshot
                                            .events
                                            .iter()
                                            .map(|line| format!("[客户端] {line}")),
                                    )
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                let path = view.data_dir.join("serein-logs.txt");
                                view.snapshot.status = match std::fs::write(&path, text) {
                                    Ok(()) => format!("日志已导出到 {}", path.display()),
                                    Err(error) => format!("导出日志失败：{error}"),
                                };
                                cx.notify();
                            }))
                            .child("导出"),
                    ),
            )
            .child(
                div()
                    .id("log-panel")
                    .w_full()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .child(
                        div()
                            .p(px(15.0))
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(rgb(TEXT))
                                    .child("日志输出"),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .text_size(px(10.0))
                                    .text_color(rgb(MUTED))
                                    .child("内核日志与客户端运行事件按来源合并展示"),
                            ),
                    )
                    .child(log_table_header())
                    .child(
                        div()
                            .id("log-view")
                            .h(px(420.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.log_scroll)
                            .children(if rows.is_empty() {
                                vec![
                                    empty_state(
                                        "暂无日志",
                                        "启动内核后这里会显示 sing-box 的输出。",
                                        if self.snapshot.core_running || self.snapshot.starting {
                                            None
                                        } else {
                                            Some("启动内核")
                                        },
                                        cx,
                                    )
                                    .into_any_element(),
                                ]
                            } else {
                                rows
                            }),
                    ),
            )
    }

    // ------------------------------------------------------------- settings

    fn settings(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let profiles = snapshot.profiles.clone();
        let profile_rows: Vec<gpui::AnyElement> = profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let name = profile.name.clone();
                let active = profile.active;
                div()
                    .id(index)
                    .w_full()
                    .px(px(14.0))
                    .py(px(11.0))
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .bg(if active { rgb(BLUE_2) } else { rgb(SURFACE) })
                    .border_1()
                    .border_color(rgb(if active { 0xb9ceff } else { BORDER }))
                    .child(
                        div()
                            .w(px(16.0))
                            .text_color(rgb(if active { CYAN } else { FAINT }))
                            .child(if active { "✓" } else { " " }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(rgb(TEXT))
                            .truncate()
                            .child(profile.name.clone()),
                    )
                    .child(
                        div()
                            .w(px(96.0))
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(age_label(profile.last_updated)),
                    )
                    .child(if active {
                        // The active archive is already in use; a second
                        // "激活" button here would be a no-op.
                        div()
                            .px(px(8.0))
                            .text_size(px(11.0))
                            .text_color(rgb(FAINT))
                            .child("使用中")
                            .into_any_element()
                    } else {
                        self.mini_action(
                            index + 200_000,
                            "激活",
                            cx,
                            ClientCommand::SwitchProfile(name),
                        )
                        .into_any_element()
                    })
                    .into_any_element()
            })
            .collect();

        let tun_on = snapshot.traffic_mode == TrafficMode::Tun;
        let dirty_count = [
            self.field(InputField::Mirror).text != snapshot.settings.mirror,
            self.field(InputField::MixedPort).text != snapshot.settings.mixed_port.to_string(),
            self.field(InputField::TestUrl).text != snapshot.settings.test_url,
            self.field(InputField::AutoUpdateMinutes).text
                != snapshot.settings.auto_update_minutes.to_string(),
            self.field(InputField::CoreVersion).text != snapshot.settings.core_version,
        ]
        .into_iter()
        .filter(|dirty| *dirty)
        .count();

        let content = match self.settings_section {
            SettingsSection::General => panel("订阅档案")
                .w_full()
                .child(
                    div()
                        .mt(px(6.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child("当前订阅与本地配置档案。订阅的添加、更新和删除请前往订阅页。"),
                )
                .child(
                    div()
                        .mt(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .children(profile_rows),
                )
                .children(profiles.is_empty().then(|| {
                    div()
                        .mt(px(12.0))
                        .text_size(px(12.0))
                        .text_color(rgb(MUTED))
                        .child("尚未导入订阅。")
                })),
            SettingsSection::Network => panel("网络与端口")
                .w_full()
                .child(setting_row_intro(
                    "流量模式",
                    "选择系统代理或 TUN 接管方式。",
                ))
                .child(setting_line("当前模式", snapshot.traffic_mode.label()))
                .child(self.edit_line(
                    "混合端口",
                    FieldSpec {
                        field: InputField::MixedPort,
                        id: "mixed-port-field",
                        placeholder: "2080",
                        width: 180.0,
                    },
                    window,
                    cx,
                ))
                .child(self.edit_line(
                    "延迟测试地址",
                    FieldSpec {
                        field: InputField::TestUrl,
                        id: "test-url-field",
                        placeholder: "https://…",
                        width: 320.0,
                    },
                    window,
                    cx,
                ))
                .child(setting_line("出站模式", snapshot.outbound_mode.label())),
            SettingsSection::Core => panel("sing-box 内核")
                .w_full()
                .child(setting_line(
                    "安装版本",
                    snapshot.core_version.as_deref().unwrap_or("未安装"),
                ))
                .child(setting_line(
                    "运行状态",
                    if snapshot.core_running {
                        "运行中"
                    } else if snapshot.starting {
                        "启动中"
                    } else {
                        "未运行"
                    },
                ))
                .child(self.edit_line(
                    "固定版本",
                    FieldSpec {
                        field: InputField::CoreVersion,
                        id: "core-version-field",
                        placeholder: "留空跟随最新",
                        width: 220.0,
                    },
                    window,
                    cx,
                ))
                .child(self.edit_line(
                    "镜像前缀",
                    FieldSpec {
                        field: InputField::Mirror,
                        id: "mirror-field",
                        placeholder: "直连",
                        width: 320.0,
                    },
                    window,
                    cx,
                ))
                .child(div().mt(px(14.0)).child(self.action(
                    "download-core",
                    "检查并更新内核",
                    Tone::Neutral,
                    cx,
                    ClientCommand::DownloadCore,
                ))),
            SettingsSection::Tun => panel("TUN")
                .w_full()
                .child(toggle_line(
                    "启用 TUN 模式",
                    tun_on,
                    "toggle-tun-settings",
                    cx,
                    SettingsPatch {
                        traffic_mode: Some(if tun_on {
                            TrafficMode::SystemProxy
                        } else {
                            TrafficMode::Tun
                        }),
                        ..Default::default()
                    },
                ))
                .child(setting_row_intro(
                    "需要重启",
                    "Windows 需要管理员权限与 wintun.dll；修改后请重启内核使配置生效。",
                ))
                .child(setting_line(
                    "当前说明",
                    if tun_on {
                        "虚拟网卡接管流量"
                    } else {
                        "使用系统代理端口"
                    },
                )),
            SettingsSection::Automation => panel("自动化")
                .w_full()
                .child(toggle_line(
                    "启动时自动启动内核",
                    snapshot.settings.auto_start,
                    "toggle-autostart",
                    cx,
                    SettingsPatch {
                        auto_start: Some(!snapshot.settings.auto_start),
                        ..Default::default()
                    },
                ))
                .child(toggle_line(
                    "内核就绪后自动开启系统代理",
                    snapshot.settings.auto_system_proxy,
                    "toggle-autoproxy",
                    cx,
                    SettingsPatch {
                        auto_system_proxy: Some(!snapshot.settings.auto_system_proxy),
                        ..Default::default()
                    },
                ))
                .child(self.edit_line(
                    "自动更新间隔（分钟）",
                    FieldSpec {
                        field: InputField::AutoUpdateMinutes,
                        id: "auto-update-field",
                        placeholder: "0 = 关闭",
                        width: 180.0,
                    },
                    window,
                    cx,
                )),
            SettingsSection::Appearance => panel("外观")
                .w_full()
                .child(setting_row_intro(
                    "界面主题",
                    "浅灰工作区、白色工作面与冷青强调色。",
                ))
                .child(setting_line("当前主题", "亮色"))
                .child(setting_line(
                    "字体",
                    "Segoe UI Variable / Microsoft YaHei UI",
                )),
            SettingsSection::Advanced => panel("高级")
                .w_full()
                .child(setting_row_intro(
                    "配置目录",
                    "GUI 与终端客户端共用同一套设置模型。",
                ))
                .child(setting_line(
                    "数据目录",
                    &self.data_dir.display().to_string(),
                ))
                .child(setting_line(
                    "设置文件",
                    // The real path: joining DATA_DIR with a Windows separator
                    // was wrong on Linux and was never an absolute path.
                    &self.data_dir.join("settings.toml").display().to_string(),
                ))
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(px(11.0))
                        .text_color(rgb(AMBER))
                        .child("修改高级配置前请停止内核，并保留可恢复的配置副本。"),
                ),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(div().flex().flex_wrap().gap(px(4.0)).children(
                SettingsSection::all().into_iter().map(|section| {
                    let active = section == self.settings_section;
                    div()
                        .id(format!("settings-{:?}", section))
                        .px(px(12.0))
                        .py(px(8.0))
                        .rounded(px(7.0))
                        .bg(rgb(if active { BLUE_2 } else { SURFACE }))
                        .border_1()
                        .border_color(rgb(if active { CYAN } else { BORDER }))
                        .text_size(px(12.0))
                        .text_color(rgb(if active { CYAN } else { MUTED }))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(SURFACE_2)))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.settings_section = section;
                            cx.notify();
                        }))
                        .child(section.label())
                }),
            ))
            .child(content)
            .children((dirty_count > 0).then(|| {
                div()
                    .px(px(14.0))
                    .py(px(10.0))
                    .rounded(px(8.0))
                    .bg(rgb(0xfff7ed))
                    .border_1()
                    .border_color(rgb(0xfed7aa))
                    .text_size(px(11.0))
                    .text_color(rgb(AMBER))
                    .child(format!(
                        "有 {dirty_count} 项更改尚未保存；端口或内核配置可能需要重启后生效。"
                    ))
            }))
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("save-settings")
                        .px(px(16.0))
                        .py(px(9.0))
                        .rounded(px(7.0))
                        .bg(rgb(CYAN))
                        .text_size(px(12.0))
                        .text_color(rgb(SURFACE))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(CYAN_DARK)))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_settings(cx)))
                        .child("保存更改"),
                ),
            )
    }
}

// Small embedded SVGs keep icon weight consistent and survive standalone packaging.
fn icon(name: &str, color: u32, size: f32) -> impl IntoElement {
    let geometry = match name {
        "serein" => "<path d='M5 7h14M5 12h9M5 17h14'/>",
        "home" => "<path d='m3 10 9-7 9 7v10H3zM9 20v-7h6v7'/>",
        "subscription" => {
            "<rect x='5' y='3' width='14' height='18' rx='2'/><path d='M9 8h6M9 12h6M9 16h4'/>"
        }
        "nodes" => {
            "<rect x='8' y='8' width='8' height='8' rx='2'/><path d='M8 3H4v5m12-5h4v5M4 16v4h4m12-4v4h-4'/>"
        }
        "rules" => {
            "<path d='M4 5h16M4 12h16M4 19h16'/><circle cx='8' cy='5' r='2'/><circle cx='15' cy='12' r='2'/><circle cx='10' cy='19' r='2'/>"
        }
        "network" => "<rect x='3' y='4' width='18' height='12' rx='2'/><path d='M8 21h8m-4-5v5'/>",
        "logs" => "<path d='M5 4h14v16H5zM8 8h8m-8 4h8m-8 4h5'/>",
        "settings" => {
            "<path d='M4 7h16M4 17h16'/><circle cx='9' cy='7' r='3'/><circle cx='15' cy='17' r='3'/>"
        }
        "minimize" => "<path d='M5 12h14'/>",
        "maximize" => "<rect x='5' y='5' width='14' height='14' rx='1'/>",
        "restore" => "<path d='M8 8V5h11v11h-3'/><rect x='5' y='8' width='11' height='11' rx='1'/>",
        "close" => "<path d='m6 6 12 12M18 6 6 18'/>",
        "globe" => {
            "<circle cx='12' cy='12' r='9'/><ellipse cx='12' cy='12' rx='4' ry='9'/><path d='M3 12h18'/>"
        }
        "chevron" => "<path d='m9 5 7 7-7 7'/>",
        "check" => "<path d='m5 12 4 4L19 6'/>",
        "clock" => "<circle cx='12' cy='12' r='9'/><path d='M12 7v5l3 2'/>",
        "download" => "<path d='M12 3v12m-5-5 5 5 5-5M5 21h14'/>",
        "upload" => "<path d='M12 21V9m-5 5 5-5 5 5M5 3h14'/>",
        _ => "<circle cx='12' cy='12' r='8'/>",
    };
    let data = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24' fill='none' stroke='#{color:06x}' stroke-width='1.7' stroke-linecap='round' stroke-linejoin='round'>{geometry}</svg>"
    );
    svg()
        .data(data.as_bytes())
        .size(px(size))
        .flex_shrink_0()
        .text_color(rgb(color))
}

fn status_dot(color: u32) -> impl IntoElement {
    div()
        .w(px(7.0))
        .h(px(7.0))
        .flex_shrink_0()
        .rounded(px(4.0))
        .bg(rgb(color))
}

fn side_rate(label: &str, value: u64, color: u32) -> impl IntoElement {
    div()
        .py(px(3.0))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(color))
                .child(format!("{}/s", human_bytes(value))),
        )
}

fn tone_colors(tone: Tone) -> (u32, u32, u32) {
    match tone {
        Tone::Accent => (0xffffff, CYAN, EDGE),
        Tone::Neutral => (TEXT, SURFACE_2, BORDER),
        Tone::Warning => (0x985c08, 0xfff5e5, 0xf2dfbf),
    }
}

fn usage_label(usage: Option<&client_core::subscription::SubscriptionUserinfo>) -> String {
    let Some(usage) = usage else {
        return "用量信息待更新".to_owned();
    };
    let used = usage.used();
    match usage.remaining() {
        Some(remaining) => format!(
            "已用 {} / {} · 剩余 {}",
            human_bytes(used),
            human_bytes(usage.total),
            human_bytes(remaining)
        ),
        None => format!("已用 {} · 未设置配额", human_bytes(used)),
    }
}

fn pill(label: impl Into<String>, color: u32) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(7.0))
        .bg(rgb(BLUE_2))
        .text_size(px(12.0))
        .text_color(rgb(color))
        .child(label.into())
}

fn metric_card(label: &str, value: String, detail: String, _accent: bool) -> impl IntoElement {
    div()
        .w(px(190.0))
        .flex_grow(1.0)
        .min_w(px(170.0))
        .min_h(px(118.0))
        .p(px(16.0))
        .rounded(px(RADIUS))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(label.to_owned()),
        )
        .child(
            div()
                .mt(px(9.0))
                .text_size(px(28.0))
                .text_color(rgb(TEXT))
                .child(value),
        )
        .child(
            div()
                .mt(px(6.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
}

fn panel(title: impl Into<String>) -> gpui::Div {
    div()
        .flex_1()
        .p(px(16.0))
        .rounded(px(RADIUS))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(TEXT))
                .child(title.into()),
        )
}

fn traffic_panel(snapshot: &ClientSnapshot) -> impl IntoElement {
    let samples: Vec<(u64, u64)> = snapshot
        .traffic_history
        .iter()
        .map(|p| (p.up, p.down))
        .collect();
    let peak = snapshot.traffic_peak();
    panel("流量趋势")
        .child(
            div()
                .mt(px(8.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(10.0))
                        .text_color(rgb(MUTED))
                        .child("本机 sing-box · 最近 5 分钟实时采样"),
                )
                .child(range_chip("5 分钟", true))
                .child(range_chip("1 小时", false))
                .child(range_chip("24 小时", false)),
        )
        .child(
            div()
                .mt(px(14.0))
                .flex()
                .gap(px(32.0))
                .child(rate("下载", snapshot.download_speed, CYAN))
                .child(rate("上传", snapshot.upload_speed, MINT)),
        )
        .child(
            div()
                .mt(px(16.0))
                .h(px(170.0))
                .w_full()
                .child(traffic_chart(samples, peak.max(1))),
        )
        .child(
            div()
                .mt(px(8.0))
                .flex()
                .justify_between()
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .child(if snapshot.traffic_history.is_empty() {
                    "等待流量数据".to_owned()
                } else {
                    format!("{} 个采样点", snapshot.traffic_history.len())
                })
                .child("现在"),
        )
        .child(
            div()
                .mt(px(14.0))
                .pt(px(10.0))
                .border_t_1()
                .border_color(rgb(BORDER))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(format!(
                    "累计下载 {}   ·   累计上传 {}   ·   峰值 {}/s",
                    human_bytes(snapshot.total_download),
                    human_bytes(snapshot.total_upload),
                    human_bytes(peak)
                )),
        )
}

fn traffic_chart(samples: Vec<(u64, u64)>, peak: u64) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let left = bounds.origin.x;
            let top = bounds.origin.y + px(2.0);
            let height = bounds.size.height - px(4.0);
            for fraction in [0.0, 0.5, 1.0] {
                let mut path = gpui::PathBuilder::stroke(px(1.0));
                let y = top + height * fraction;
                path.move_to(gpui::point(left, y));
                path.line_to(gpui::point(left + bounds.size.width, y));
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(BORDER));
                }
            }
            if samples.len() < 2 {
                return;
            }
            for upload in [false, true] {
                let mut path = gpui::PathBuilder::stroke(px(1.8));
                for (index, (up, down)) in samples.iter().enumerate() {
                    let value = if upload { *up } else { *down };
                    let x = left + bounds.size.width * (index as f32 / (samples.len() - 1) as f32);
                    let y = top + height * (1.0 - (value as f32 / peak as f32).clamp(0.0, 1.0));
                    if index == 0 {
                        path.move_to(gpui::point(x, y));
                    } else {
                        path.line_to(gpui::point(x, y));
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(if upload { MINT } else { CYAN }));
                }
            }
        },
    )
    .size_full()
}

fn rate(label: &str, value: u64, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(px(18.0))
                .text_color(rgb(color))
                .child(format!("{}/s", human_bytes(value))),
        )
}

fn rule_row(index: usize, rule: &RouteRuleSnapshot) -> gpui::AnyElement {
    div()
        .id(format!("rule-row-{index}"))
        .w_full()
        .px(px(15.0))
        .py(px(11.0))
        .flex()
        .items_center()
        .gap(px(14.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .hover(|style| style.bg(rgb(BLUE_2)))
        .child(
            div()
                .w(px(58.0))
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(format!("{:02}", index + 1)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
                .child(rule.matcher.clone()),
        )
        .child(
            div()
                .w(px(180.0))
                .truncate()
                .text_size(px(12.0))
                .text_color(rgb(CYAN))
                .child(rule.outbound.clone()),
        )
        .child(
            div()
                .w(px(72.0))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child(pill("已启用", MINT)),
        )
        .into_any_element()
}

fn log_table_header() -> impl IntoElement {
    div()
        .px(px(15.0))
        .py(px(8.0))
        .flex()
        .gap(px(12.0))
        .bg(rgb(SURFACE_2))
        .border_b_1()
        .border_color(rgb(BORDER))
        .text_size(px(10.0))
        .text_color(rgb(MUTED))
        .child(div().w(px(62.0)).child("序号"))
        .child(div().w(px(72.0)).child("级别"))
        .child(div().w(px(120.0)).child("来源"))
        .child(div().flex_1().child("内容"))
}

fn log_row(index: usize, source: &str, line: &str, wrap: bool) -> gpui::AnyElement {
    let color = level_color(line);
    let level = if color == DANGER {
        "Error"
    } else if color == AMBER {
        "Warn"
    } else if color == FAINT {
        "Debug"
    } else {
        "Info"
    };
    div()
        .id(format!("log-row-{index}"))
        .px(px(15.0))
        .py(px(8.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .hover(|style| style.bg(rgb(BLUE_2)))
        .text_size(px(11.0))
        .child(
            div()
                .w(px(62.0))
                .text_color(rgb(MUTED))
                .child(format!("{:03}", index + 1)),
        )
        .child(div().w(px(72.0)).child(pill(level, color)))
        .child(
            div()
                .w(px(120.0))
                .truncate()
                .text_color(rgb(MUTED))
                .child(source.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .when(!wrap, |row| {
                    row.whitespace_nowrap().overflow_hidden().text_ellipsis()
                })
                .text_color(rgb(if color == TEXT { TEXT } else { color }))
                .child(line.to_owned()),
        )
        .into_any_element()
}

fn connection_header() -> impl IntoElement {
    div()
        .min_w(px(1080.0))
        .px(px(12.0))
        .py(px(8.0))
        .flex()
        .gap(px(10.0))
        .text_size(px(11.0))
        .text_color(rgb(MUTED))
        .child(div().w(px(62.0)).child("状态"))
        .child(div().w(px(140.0)).child("应用 / 入口"))
        .child(div().w(px(230.0)).child("远程目标"))
        .child(div().w(px(72.0)).child("协议"))
        .child(div().w(px(150.0)).child("命中规则"))
        .child(div().w(px(160.0)).child("使用节点"))
        .child(div().w(px(140.0)).child("累计流量"))
        .child(div().w(px(120.0)).child("建立时间"))
        .child(div().w(px(48.0)).child(""))
}

fn range_chip(label: &'static str, active: bool) -> impl IntoElement {
    div()
        .px(px(9.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .bg(rgb(if active { CYAN } else { SURFACE_2 }))
        .text_size(px(10.0))
        .text_color(rgb(if active { SURFACE } else { FAINT }))
        .child(label)
}

fn connection_row(
    index: usize,
    connection: &Connection,
    cx: &mut Context<Sbgui>,
) -> gpui::AnyElement {
    let id = connection.id.clone();
    let detail_id = connection.id.clone();
    let application = if !connection.metadata.process.is_empty() {
        connection.metadata.process.clone()
    } else if !connection.metadata.process_path.is_empty() {
        connection.metadata.process_path.clone()
    } else {
        "系统代理".to_owned()
    };
    div()
        .id(index)
        .min_w(px(1080.0))
        .px(px(12.0))
        .py(px(9.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .bg(rgb(SURFACE))
        .border_b_1()
        .border_color(rgb(BORDER))
        .font_family("Cascadia Mono, Consolas")
        .text_size(px(11.0))
        .text_color(rgb(TEXT))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BLUE_2)))
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
            view.selected_connection = Some(detail_id.clone());
            cx.notify();
        }))
        .child(
            div()
                .w(px(62.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(status_dot(MINT))
                .child("活动"),
        )
        .child(
            div()
                .w(px(140.0))
                .truncate()
                .text_color(rgb(TEXT))
                .child(application),
        )
        .child(
            div()
                .w(px(230.0))
                .truncate()
                .text_color(rgb(TEXT))
                .child(connection_target(connection)),
        )
        .child(
            div()
                .w(px(72.0))
                .child(connection.metadata.network.to_uppercase()),
        )
        .child(div().w(px(150.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.rule.is_empty() {
                "未匹配".to_owned()
            } else {
                connection.rule.clone()
            },
        ))
        .child(
            div()
                .w(px(160.0))
                .truncate()
                .text_color(rgb(MUTED))
                .child(connection_chain(connection)),
        )
        .child(div().w(px(140.0)).text_color(rgb(CYAN)).child(format!(
            "↓ {}  ↑ {}",
            human_bytes(connection.download),
            human_bytes(connection.upload)
        )))
        .child(div().w(px(120.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.start.is_empty() {
                "刚刚".to_owned()
            } else {
                connection.start.clone()
            },
        ))
        .child(
            div()
                .id(index + 300_000)
                .w(px(48.0))
                .text_color(rgb(DANGER))
                .hover(|style| style.text_color(rgb(0xffb0ab)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    view.send(ClientCommand::CloseConnection(id.clone()));
                    cx.notify();
                }))
                .child("断开"),
        )
        .into_any_element()
}

fn setting_line(label: &str, value: &str) -> impl IntoElement {
    div()
        .mt(px(4.0))
        .min_h(px(42.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .w(px(148.0))
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .truncate()
                .child(value.to_owned()),
        )
}

fn setting_row_intro(label: &str, detail: &str) -> impl IntoElement {
    div()
        .mt(px(10.0))
        .px(px(12.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
                .child(label.to_owned()),
        )
        .child(
            div()
                .mt(px(3.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(detail.to_owned()),
        )
}

fn toggle_line(
    label: &'static str,
    on: bool,
    id: &'static str,
    cx: &mut Context<Sbgui>,
    patch: SettingsPatch,
) -> impl IntoElement {
    div()
        .mt(px(4.0))
        .min_h(px(48.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
                .child(label),
        )
        .child(
            div()
                .id(id)
                .w(px(42.0))
                .h(px(24.0))
                .p(px(3.0))
                .rounded(px(12.0))
                .flex()
                .items_center()
                .when(on, |control| control.justify_end())
                .when(!on, |control| control.justify_start())
                .bg(rgb(if on { CYAN } else { 0xc7cbd1 }))
                .border_1()
                .border_color(rgb(if on { CYAN } else { 0xc7cbd1 }))
                .hover(|style| style.border_color(rgb(CYAN)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.send(ClientCommand::UpdateSettings(patch.clone()));
                    cx.notify();
                }))
                .child(
                    div()
                        .w(px(16.0))
                        .h(px(16.0))
                        .rounded(px(8.0))
                        .bg(rgb(SURFACE)),
                ),
        )
}

fn empty_state(
    title: &'static str,
    detail: &'static str,
    action: Option<&'static str>,
    cx: &mut Context<Sbgui>,
) -> impl IntoElement {
    let button = action.map(|label| {
        let label: &'static str = label;
        div()
            .id(label)
            .mt(px(12.0))
            .px(px(12.0))
            .py(px(7.0))
            .rounded(px(7.0))
            .bg(rgb(BLUE_2))
            .border_1()
            .border_color(rgb(BLUE_2))
            .text_size(px(12.0))
            .text_color(rgb(CYAN))
            .hover(|style| style.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if label.contains("启动") {
                    view.send(ClientCommand::StartCore);
                } else {
                    view.page = if label.contains("订阅") {
                        Page::Subscriptions
                    } else {
                        Page::Settings
                    };
                }
                cx.notify();
            }))
            .child(label)
    });
    div()
        .min_h(px(200.0))
        .p(px(28.0))
        .rounded(px(RADIUS))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .child(div().text_size(px(15.0)).text_color(rgb(TEXT)).child(title))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
        .children(button)
}

fn level_color(line: &str) -> u32 {
    // Derived from the shared level judgement, so a Chinese client event that
    // the filter treats as an error is not painted as plain info text.
    match client_core::state::log_level_of(line) {
        LogLevel::Error => DANGER,
        LogLevel::Warn => AMBER,
        LogLevel::Debug => FAINT,
        LogLevel::Info => TEXT,
    }
}

fn delay_color(delay: u64) -> u32 {
    if delay < 200 {
        MINT
    } else if delay < 500 {
        AMBER
    } else {
        DANGER
    }
}

fn clean_proxy_label(value: &str) -> String {
    value
        .trim_start_matches(|character: char| {
            !character.is_ascii_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&character)
        })
        .trim()
        .to_owned()
}

fn connection_target(connection: &Connection) -> String {
    let host = if connection.metadata.destination_host.is_empty() {
        connection.metadata.destination_ip.as_str()
    } else {
        connection.metadata.destination_host.as_str()
    };
    if host.is_empty() {
        "—".to_owned()
    } else if connection.metadata.destination_port.is_empty() {
        host.to_owned()
    } else {
        format!("{host}:{}", connection.metadata.destination_port)
    }
}

fn connection_chain(connection: &Connection) -> String {
    if connection.chains.is_empty() {
        "直连".to_owned()
    } else {
        connection
            .chains
            .iter()
            .map(|value| clean_proxy_label(value))
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

fn human_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn age_label(epoch_seconds: u64) -> String {
    if epoch_seconds == 0 {
        return "从未更新".to_owned();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = now.saturating_sub(epoch_seconds);
    if age < 3600 {
        format!("{} 分钟前", age / 60)
    } else if age < 86_400 {
        format!("{} 小时前", age / 3600)
    } else {
        format!("{} 天前", age / 86_400)
    }
}

/// Loads one settings file the same way the engine does, so the window can
/// open before the engine's first poll completes.
fn load_settings(dir: &Path) -> Settings {
    Settings::load_or_create(dir).unwrap_or_default()
}

fn main() {
    // The engine (controller task, tokio::fs, reqwest) runs on this runtime for
    // the whole process lifetime. It is leaked on purpose: moved into the GPUI
    // launch closure it would be dropped the moment that closure returns, the
    // runtime would shut down, and every engine await — starting with the
    // auto-start subscription read — would fail with "background task failed".
    let runtime: &'static tokio::runtime::Runtime = Box::leak(Box::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime"),
    ));
    let dir = settings::data_dir_for(DATA_DIR).expect("data directory");
    // The mixed port, the OS proxy and the runtime configuration are all
    // per-directory or machine-global, so a second instance would fight the
    // first over all three. Held for as long as the event loop runs.
    let _instance = match settings::acquire_instance_lock(&dir) {
        Ok(lock) => lock,
        Err(error) => {
            eprintln!("无法锁定数据目录: {error}");
            return;
        }
    };
    if _instance.is_none() {
        return;
    }
    let _ = load_settings(&dir);
    let _ = Profiles::load_or_create(&dir);
    let ui_data_dir = dir.clone();
    let controller = {
        let _guard = runtime.enter();
        ClientController::start(dir)
    };

    application()
        .with_assets(SereinAssets)
        .run(move |cx: &mut App| {
            let (win_w, win_h) = env_window_size();
            let bounds = Bounds::centered(None, size(px(win_w), px(win_h)), cx);
            cx.open_window(
                WindowOptions {
                    // The operating-system titlebar is intentionally transparent/hidden.
                    // The complete titlebar, including branding and window controls, is
                    // rendered by `Sbgui::titlebar` through GPUI.
                    titlebar: Some(TitlebarOptions {
                        title: Some("Serein".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    is_movable: true,
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(860.0), px(640.0))),
                    ..Default::default()
                },
                move |window, cx| {
                    #[cfg(windows)]
                    apply_windows_window_chrome(window);

                    let view = cx.new(|cx| {
                        let view = Sbgui::new(controller, ui_data_dir, cx);
                        let refresh = cx.spawn(async move |this, cx| {
                            loop {
                                cx.background_executor()
                                    .timer(Duration::from_millis(400))
                                    .await;
                                let Some(entity) = this.upgrade() else {
                                    break;
                                };
                                entity.update(cx, |view: &mut Sbgui, cx| {
                                    view.snapshot = view.controller.snapshot();
                                    cx.notify();
                                });
                            }
                        });
                        refresh.detach();
                        view
                    });
                    // Alt+F4 and the taskbar close arrive as WM_CLOSE and are
                    // vetoed here; the custom close button is a client-area
                    // click that bypasses this hook and calls
                    // `Sbgui::request_close` instead.
                    let close_view = view.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        close_view
                            .update(cx, |view, cx| view.handle_close_request(cx))
                            .unwrap_or(true)
                    });
                    view
                },
            )
            .unwrap();
        });
}

#[cfg(windows)]
fn apply_windows_window_chrome(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };

    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    let preference = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&preference) as u32,
        )
    };
}

/// Parses the mixed-inbound port. 0 would point the system proxy nowhere, so
/// it is rejected; the engine rejects the change anyway while the core runs.
fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

/// Parses the auto-update interval in minutes; 0 ("off") is a valid value.
fn parse_count(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_accepts_real_ports_and_rejects_zero_or_garbage() {
        assert_eq!(parse_port("2080"), Some(2080));
        assert_eq!(parse_port(" 7890 "), Some(7890));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("abc"), None);
        assert_eq!(parse_port("99999"), None);
    }

    #[test]
    fn parse_count_accepts_zero_for_off() {
        assert_eq!(parse_count("0"), Some(0));
        assert_eq!(parse_count("30"), Some(30));
        assert_eq!(parse_count("-1"), None);
        assert_eq!(parse_count(""), None);
    }

    #[test]
    fn the_info_chip_keeps_unmarked_client_events() {
        assert!(client_core::state::log_level_shown(
            Some(LogLevel::Info),
            "内核已启动"
        ));
        assert!(!client_core::state::log_level_shown(
            Some(LogLevel::Info),
            "TRACE detail"
        ));
        assert!(client_core::state::log_level_shown(
            Some(LogLevel::Error),
            "导入订阅失败: timeout"
        ));
    }
}
