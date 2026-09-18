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
use client_core::clash_api::{Connection, OutboundMode};
use client_core::command::SettingsPatch;
use client_core::settings::{self, Profiles, Settings};
use client_core::state::{ClientSnapshot, ProxyGroupSnapshot};
use client_core::system_proxy;
use client_core::system_proxy::TrafficMode;
use client_core::{ClientCommand, ClientController};
use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext as _, AssetSource, Bounds, ClickEvent, Context, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    TitlebarOptions, Window, WindowBounds, WindowControlArea, WindowOptions, div, px, rgb, rgba,
    size, svg,
};
use gpui_platform::application;

// Serein's desktop palette: a quiet neutral canvas, white working surfaces,
// and a cool teal reserved for active state and primary actions.
const BG: u32 = 0xf5f6f7;
const SURFACE: u32 = 0xffffff;
const SURFACE_2: u32 = 0xf0f3f4;
const BORDER: u32 = 0xdfe4e5;
const TEXT: u32 = 0x182022;
const MUTED: u32 = 0x697475;
const FAINT: u32 = 0x8d9899;
const CYAN: u32 = 0x0a7374;
const CYAN_DARK: u32 = 0x075b5d;
const BLUE_2: u32 = 0xe7f3f2;
const NAV_ACTIVE: u32 = 0xe9f3f2;
const EDGE: u32 = 0x17393b;
const MINT: u32 = 0x237c5b;
const AMBER: u32 = 0x9a640f;
const DANGER: u32 = 0xc83e49;
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
}

impl Sbgui {
    fn new(controller: ClientController, data_dir: PathBuf) -> Self {
        let snapshot = controller.snapshot();
        Self {
            controller,
            data_dir,
            snapshot,
            page: Page::Dashboard,
            group_index: 0,
            confirm_exit: false,
            exit_choice: None,
        }
    }

    fn send(&self, command: ClientCommand) {
        let _ = self.controller.send(command);
    }

    /// GPUI asks this before the window closes; both the custom close button
    /// and Alt+F4 end up as `WM_CLOSE`, and a `false` return vetoes the close.
    /// The window may close only once the exit decision is made: closing
    /// kills the core with the process, and while the OS proxy is enabled it
    /// would keep pointing at a dead local port, silently breaking the
    /// user's network.
    fn handle_close_request(&mut self, cx: &mut Context<Self>) -> bool {
        if self.exit_choice.is_some() || !self.snapshot.system_proxy_enabled {
            return true;
        }
        if !self.confirm_exit {
            self.confirm_exit = true;
            cx.notify();
        }
        false
    }

    fn choose_exit(&mut self, choice: ExitChoice, window: &mut Window) {
        if choice == ExitChoice::Restore {
            // Synchronous and fast (registry + refresh broadcast); the engine
            // is about to die with the process, so no command round-trip.
            let _ = system_proxy::disable(&self.data_dir);
            self.snapshot.system_proxy_enabled = false;
        }
        self.exit_choice = Some(choice);
        window.remove_window();
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
        let page = self.page;
        div()
            .size_full()
            .rounded(px(WINDOW_RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(BORDER))
            .font_family("Microsoft YaHei UI")
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
                            .child(
                                div()
                                    .id("page-scroll")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .px(px(CONTENT_PAD))
                                    .pb(px(CONTENT_PAD))
                                    .child(self.content(cx)),
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
                            .text_size(px(15.0))
                            .text_color(rgb(TEXT))
                            .child("退出前确认"),
                    )
                    .child(div().text_size(px(12.0)).text_color(rgb(MUTED)).child(
                        "系统代理仍指向本机 sing-box。退出后内核将随进程停止，\
                                 若不恢复代理设置，依赖系统代理的应用可能无法联网。",
                    ))
                    .child(
                        div()
                            .mt(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(self.exit_button(
                                "exit-restore",
                                "恢复代理并退出",
                                Tone::Accent,
                                cx,
                                ExitChoice::Restore,
                            ))
                            .child(self.exit_button(
                                "exit-keep",
                                "保留代理设置",
                                Tone::Neutral,
                                cx,
                                ExitChoice::Keep,
                            ))
                            .child(
                                div()
                                    .id("exit-cancel")
                                    .px(px(15.0))
                                    .py(px(8.0))
                                    .rounded(px(7.0))
                                    .text_size(px(12.0))
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|style| style.text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.confirm_exit = false;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            ),
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
            .h(px(48.0))
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
                |window, _| window.minimize_window(),
            ))
            .child(self.window_button(
                "maximize",
                maximize_icon,
                WindowControlArea::Max,
                cx,
                |window, _| window.zoom_window(),
            ))
            .child(self.window_button(
                "close",
                "close",
                WindowControlArea::Close,
                cx,
                |window, _| window.remove_window(),
            ))
    }

    fn window_button(
        &self,
        button_id: &'static str,
        icon_name: &'static str,
        area: WindowControlArea,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let is_close = area == WindowControlArea::Close;
        div()
            .id(button_id)
            .w(px(42.0))
            .h(px(48.0))
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
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| action(window, cx)))
            .child(icon(icon_name, MUTED, 14.0))
    }

    fn sidebar(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let profile = snapshot
            .active_profile
            .as_ref()
            .map(|p| p.name.as_str())
            .unwrap_or("添加订阅");
        let side_samples: Vec<(u64, u64)> = snapshot
            .traffic_history
            .iter()
            .map(|point| (point.up, point.down))
            .collect();
        div()
            .id("sidebar-scroll")
            .w(px(232.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(9.0))
            .p(px(10.0))
            .bg(rgb(SURFACE_2))
            .border_r_1()
            .border_color(rgb(BORDER))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .children(Page::all().into_iter().map(|item| {
                        let active = item == page;
                        let (glyph, count) = match item {
                            Page::Dashboard => ("home", None),
                            Page::Subscriptions => ("subscription", Some(snapshot.profiles.len())),
                            Page::Proxies => ("nodes", Some(snapshot.proxy_groups.len())),
                            Page::Rules => ("rules", Some(read_rule_rows(&self.data_dir).len())),
                            Page::Connections => ("network", Some(snapshot.active_connections)),
                            Page::Logs => ("logs", None),
                            Page::Settings => ("settings", None),
                        };
                        div()
                            .id(format!("nav-{}", item.title()))
                            .w_full()
                            .h(px(40.0))
                            .px(px(10.0))
                            .rounded(px(9.0))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .bg(rgb(if active { NAV_ACTIVE } else { SURFACE }))
                            .border_1()
                            .border_color(rgb(if active { 0xb9d4d2 } else { BORDER }))
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
                                    .w(px(22.0))
                                    .h(px(22.0))
                                    .rounded(px(6.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(rgb(if active { CYAN } else { SURFACE_2 }))
                                    .child(icon(glyph, if active { SURFACE } else { MUTED }, 14.0)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.0))
                                    .text_color(rgb(if active { TEXT } else { MUTED }))
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
            .child(self.mode_selector(cx))
            .child(
                div()
                    .p(px(11.0))
                    .rounded(px(10.0))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(self.sidebar_switch(
                        "side-system",
                        "系统代理",
                        "globe",
                        snapshot.system_proxy_enabled,
                        ClientCommand::ToggleSystemProxy,
                        cx,
                    ))
                    .child(
                        div()
                            .mt(px(8.0))
                            .pt(px(8.0))
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .child(self.sidebar_switch(
                                "side-tun",
                                "TUN 模式",
                                "network",
                                snapshot.traffic_mode == TrafficMode::Tun,
                                ClientCommand::UpdateSettings(SettingsPatch {
                                    traffic_mode: Some(
                                        if snapshot.traffic_mode == TrafficMode::Tun {
                                            TrafficMode::SystemProxy
                                        } else {
                                            TrafficMode::Tun
                                        },
                                    ),
                                    ..Default::default()
                                }),
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .id("active-subscription")
                    .p(px(12.0))
                    .rounded(px(10.0))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(BLUE_2)))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.page = Page::Subscriptions;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("subscription", MUTED, 16.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(13.0))
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(profile.to_owned()),
                            )
                            .child(icon("chevron", MUTED, 14.0)),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(usage_label(snapshot.subscription_usage.as_ref())),
                    ),
            )
            .child(
                div().mt_auto().pt(px(2.0)).child(
                    div()
                        .p(px(12.0))
                        .rounded(px(10.0))
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .child(side_rate("下载", snapshot.download_speed, CYAN))
                        .child(side_rate("上传", snapshot.upload_speed, MUTED))
                        .child(
                            div()
                                .mt(px(8.0))
                                .h(px(30.0))
                                .w_full()
                                .child(traffic_chart(side_samples, snapshot.traffic_peak().max(1))),
                        ),
                ),
            )
            .child(
                div()
                    .p(px(11.0))
                    .rounded(px(10.0))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.0))
                            .child(status_dot(if snapshot.core_running { MINT } else { FAINT }))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(TEXT))
                                    .child("sing-box 内核"),
                            )
                            .child(div().flex_1())
                            .child(switch_track(snapshot.core_running)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(MUTED))
                            .child(format!(
                                "{} · {}",
                                snapshot.core_version.as_deref().unwrap_or("未安装"),
                                if snapshot.starting {
                                    "启动中"
                                } else if snapshot.core_running {
                                    "运行中"
                                } else {
                                    "未运行"
                                }
                            )),
                    ),
            )
    }

    fn mode_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p(px(10.0))
            .rounded(px(10.0))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(MUTED))
                    .child("出站模式"),
            )
            .child(
                div()
                    .mt(px(7.0))
                    .flex()
                    .p(px(3.0))
                    .gap(px(2.0))
                    .rounded(px(8.0))
                    .bg(rgb(SURFACE_2))
                    .children(
                        [
                            OutboundMode::Rule,
                            OutboundMode::Global,
                            OutboundMode::Direct,
                        ]
                        .into_iter()
                        .map(|mode| {
                            let active = self.snapshot.outbound_mode == mode;
                            div()
                                .id(format!("outbound-{}", mode.as_str()))
                                .flex_1()
                                .py(px(7.0))
                                .rounded(px(6.0))
                                .flex()
                                .justify_center()
                                .text_size(px(11.0))
                                .cursor_pointer()
                                .bg(rgb(if active { SURFACE } else { SURFACE_2 }))
                                .text_color(rgb(if active { TEXT } else { MUTED }))
                                .when(active, |style| style.border_1().border_color(rgb(BORDER)))
                                .hover(move |s| s.bg(rgb(if active { SURFACE } else { BLUE_2 })))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.send(ClientCommand::SetOutboundMode(mode));
                                    cx.notify();
                                }))
                                .child(mode.label())
                        }),
                    ),
            )
            .child(
                div()
                    .mt(px(7.0))
                    .text_size(px(10.0))
                    .text_color(rgb(MUTED))
                    .child(match self.snapshot.outbound_mode {
                        OutboundMode::Rule => "按配置路由规则分流",
                        OutboundMode::Global => "所有流量通过当前节点",
                        OutboundMode::Direct => "所有流量直连网络",
                    }),
            )
    }

    fn sidebar_switch(
        &self,
        id: &'static str,
        label: &'static str,
        glyph: &'static str,
        on: bool,
        command: ClientCommand,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let locked = label == "TUN 模式" && (self.snapshot.core_running || self.snapshot.starting);
        div()
            .id(id)
            .p(px(2.0))
            .rounded(px(8.0))
            .when(!locked, |s| s.cursor_pointer().hover(|s| s.bg(rgb(BLUE_2))))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if locked {
                    return;
                }
                view.send(command.clone());
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(icon(glyph, TEXT, 17.0))
                    .child(switch_track(on)),
            )
            .child(
                div()
                    .mt(px(10.0))
                    .text_size(px(12.0))
                    .text_color(rgb(TEXT))
                    .child(label),
            )
            .child(
                div()
                    .mt(px(3.0))
                    .text_size(px(10.0))
                    .text_color(rgb(MUTED))
                    .child(if locked {
                        "停核后切换"
                    } else if label == "TUN 模式" {
                        if on { "已选择" } else { "未选择" }
                    } else if on {
                        "已开启"
                    } else {
                        "已关闭"
                    }),
            )
    }

    fn header(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.snapshot.core_running;
        div()
            .px(px(CONTENT_PAD))
            .pt(px(18.0))
            .pb(px(14.0))
            .flex()
            .items_end()
            .gap(px(12.0))
            .flex_shrink_0()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(MUTED))
                            .child(format!("SEREIN  /  {}", page.title().to_uppercase())),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .text_size(px(26.0))
                            .text_color(rgb(TEXT))
                            .child(page.title()),
                    )
                    .child(
                        div()
                            .mt(px(3.0))
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child(page.subtitle()),
                    )
                    .child(self.header_status()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .children((page == Page::Dashboard).then(|| {
                        self.action(
                            "header-restart",
                            "重启内核",
                            Tone::Neutral,
                            cx,
                            ClientCommand::RestartCore,
                        )
                    }))
                    .child(self.action(
                        "header-core",
                        if self.snapshot.starting {
                            "启动中…"
                        } else if running {
                            "停止内核"
                        } else {
                            "启动内核"
                        },
                        Tone::Accent,
                        cx,
                        if running {
                            ClientCommand::StopCore
                        } else {
                            ClientCommand::StartCore
                        },
                    )),
            )
    }

    /// The engine's own status line, shown under every page title: what is
    /// executing right now, whether a crash restart is pending, and the last
    /// operation's outcome. The engine writes these strings; the UI only
    /// renders them, so failures are visible no matter which page is open.
    fn header_status(&self) -> impl IntoElement {
        let (label, color) = if let Some(busy) = self.snapshot.busy.as_deref() {
            (busy_label(busy), CYAN)
        } else if self.snapshot.restart_attempts > 0 {
            (
                format!(
                    "内核异常退出，第 {} 次自动重启等待中…",
                    self.snapshot.restart_attempts
                ),
                AMBER,
            )
        } else {
            let color = if self.snapshot.status.contains("失败") {
                DANGER
            } else {
                MUTED
            };
            (self.snapshot.status.clone(), color)
        };
        div()
            .mt(px(6.0))
            .text_size(px(12.0))
            .text_color(rgb(color))
            .child(label)
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

    fn content(&self, cx: &mut Context<Self>) -> gpui::Div {
        match self.page {
            Page::Dashboard => self.dashboard(cx),
            Page::Subscriptions => self.subscriptions(cx),
            Page::Proxies => self.proxies(cx),
            Page::Rules => self.rules(cx),
            Page::Connections => self.connections(cx),
            Page::Logs => self.logs(cx),
            Page::Settings => self.settings(cx),
        }
    }

    // ------------------------------------------------------------ dashboard

    fn dashboard(&self, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let current = snapshot.current_node.as_deref().unwrap_or("尚未选择节点");
        let upload_peak = snapshot
            .traffic_history
            .iter()
            .map(|point| point.up)
            .max()
            .unwrap_or(0);
        let group = self.selected_group();
        let group_detail = group
            .as_ref()
            .map(|group| {
                format!(
                    "节点选择 · {} · {}",
                    if group.is_auto() {
                        "自动选择组"
                    } else {
                        "手动选择组"
                    },
                    group.kind
                )
            })
            .unwrap_or_else(|| "导入订阅并启动内核后选择节点".to_owned());
        let current_delay = group
            .as_ref()
            .and_then(|group| group.delays.get(current).copied());
        let tcp_count = snapshot
            .connections
            .connections
            .iter()
            .filter(|connection| connection.metadata.network.eq_ignore_ascii_case("tcp"))
            .count();
        let udp_count = snapshot.active_connections.saturating_sub(tcp_count);
        let node_test = (!current.is_empty() && current != "尚未选择节点").then(|| {
            self.action(
                "overview-test-node",
                "测试延迟",
                Tone::Neutral,
                cx,
                ClientCommand::TestNode(current.to_owned()),
            )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(12.0))
                    .child(metric_card(
                        "实时下载",
                        format!("{} /s", human_bytes(snapshot.download_speed)),
                        format!("5 分钟峰值  {} /s", human_bytes(snapshot.traffic_peak())),
                        true,
                    ))
                    .child(metric_card(
                        "实时上传",
                        format!("{} /s", human_bytes(snapshot.upload_speed)),
                        format!("5 分钟峰值  {} /s", human_bytes(upload_peak)),
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
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(420.0))
                            .child(traffic_panel(snapshot)),
                    )
                    .child(
                        div()
                            .w(px(320.0))
                            .flex_grow(1.0)
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(px(14.0))
                            .child(
                                panel("当前节点")
                                    .child(
                                        div()
                                            .mt(px(14.0))
                                            .flex()
                                            .items_center()
                                            .gap(px(10.0))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.0))
                                                    .text_size(px(18.0))
                                                    .text_color(rgb(TEXT))
                                                    .truncate()
                                                    .child(current.to_owned()),
                                            )
                                            .children(node_test)
                                            .children(current_delay.map(|delay| {
                                                div()
                                                    .text_size(px(18.0))
                                                    .text_color(rgb(MINT))
                                                    .child(format!("{} ms", delay))
                                            })),
                                    )
                                    .child(
                                        div()
                                            .mt(px(5.0))
                                            .text_size(px(11.0))
                                            .text_color(rgb(MUTED))
                                            .child(group_detail),
                                    )
                                    .child(setting_line("出站模式", snapshot.outbound_mode.label()))
                                    .child(setting_line(
                                        "内核版本",
                                        snapshot.core_version.as_deref().unwrap_or("未安装"),
                                    ))
                                    .child(setting_line(
                                        "自动重启",
                                        &format!("已触发 {} 次", snapshot.restart_attempts),
                                    )),
                            )
                            .child(panel("客户端事件").children(if snapshot.events.is_empty() {
                                vec![
                                    div()
                                        .mt(px(10.0))
                                        .text_size(px(11.0))
                                        .text_color(rgb(FAINT))
                                        .child("等待客户端事件……"),
                                ]
                            } else {
                                snapshot
                                    .events
                                    .iter()
                                    .rev()
                                    .take(4)
                                    .map(|event| {
                                        div()
                                            .mt(px(9.0))
                                            .flex()
                                            .items_start()
                                            .gap(px(8.0))
                                            .child(status_dot(level_color(event)))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .text_size(px(11.0))
                                                    .text_color(rgb(MUTED))
                                                    .child(event.clone()),
                                            )
                                    })
                                    .collect()
                            })),
                    ),
            )
    }

    // --------------------------------------------------------- subscriptions

    fn subscriptions(&self, cx: &mut Context<Self>) -> gpui::Div {
        let profiles = self.snapshot.profiles.clone();
        let import = div()
            .id("import-from-clipboard")
            .px(px(16.0))
            .py(px(11.0))
            .rounded(px(13.0))
            .bg(rgb(CYAN))
            .text_size(px(12.0))
            .text_color(rgb(0xffffff))
            .hover(|style| style.bg(rgb(CYAN_DARK)))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                view.send(ClientCommand::ImportSubscription(text));
                cx.notify();
            }))
            .child("从剪贴板导入");

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(
                div()
                    .p(px(16.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE_2))
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex_1()
                            .child(div().text_size(px(13.0)).text_color(rgb(TEXT)).child("复制订阅地址后直接导入"))
                            .child(div().mt(px(4.0)).text_size(px(11.0)).text_color(rgb(MUTED)).child("支持 HTTP/HTTPS 的 sing-box JSON 或完整订阅地址。导入后会立即拉取并设为当前档案。")),
                    )
                    .child(import),
            );

        if profiles.is_empty() {
            return root.child(empty_state(
                "还没有订阅",
                "复制订阅链接，然后点击上方「从剪贴板导入」。",
                None,
                cx,
            ));
        }

        let cards = profiles.into_iter().enumerate().map(|(index, profile)| {
            let active = profile.active;
            let name_for_activate = profile.name.clone();
            let name_for_remove = profile.name.clone();
            let updated = age_label(profile.last_updated);
            let usage = if active {
                usage_label(self.snapshot.subscription_usage.as_ref())
            } else {
                format!("更新于 {updated}")
            };
            div()
                .id(format!("subscription-card-{index}"))
                .w(px(320.0))
                .flex_grow(1.0)
                .min_w(px(300.0))
                .min_h(px(168.0))
                .p(px(16.0))
                .rounded(px(RADIUS))
                .bg(if active { rgb(BLUE_2) } else { rgb(SURFACE) })
                .border_1()
                .border_color(rgb(if active { CYAN } else { BORDER }))
                .text_color(rgb(TEXT))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(14.0))
                                .child(profile.name.clone()),
                        )
                        .child(pill(
                            if active {
                                "当前使用"
                            } else {
                                "本地档案"
                            },
                            CYAN,
                        )),
                )
                .child(
                    div()
                        .mt(px(12.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .truncate()
                        .child(profile.url.clone()),
                )
                .child(div().mt(px(10.0)).text_size(px(12.0)).child(usage))
                .child(
                    div()
                        .mt(px(14.0))
                        .flex()
                        .gap(px(8.0))
                        .children((!active).then(|| {
                            div()
                                .id(format!("activate-{index}"))
                                .px(px(12.0))
                                .py(px(7.0))
                                .rounded(px(11.0))
                                .bg(rgb(if active { 0xffffff } else { BLUE_2 }))
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
                            self.action(
                                "refresh-active-profile",
                                "立即更新",
                                Tone::Neutral,
                                cx,
                                ClientCommand::UpdateSubscription,
                            )
                        }))
                        .child(div().flex_1())
                        .child(
                            div()
                                .id(format!("remove-{index}"))
                                .px(px(10.0))
                                .py(px(7.0))
                                .rounded(px(11.0))
                                .text_size(px(11.0))
                                .text_color(rgb(DANGER))
                                .hover(|style| style.bg(rgb(0xffecee)))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.send(ClientCommand::RemoveProfile(
                                        name_for_remove.clone(),
                                    ));
                                    cx.notify();
                                }))
                                .child("删除"),
                        ),
                )
        });
        root = root.child(div().flex().flex_wrap().gap(px(14.0)).children(cards));
        root.child(
            div()
                .p(px(15.0))
                .rounded(px(RADIUS))
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(MUTED))
                        .child("地址归一化"),
                )
                .child(
                    div()
                        .mt(px(7.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child("订阅地址会自动归一化为完整 sing-box 配置，确保入站、路由、代理组与 Clash API 一起可用。"),
                ),
        )
    }

    // -------------------------------------------------------------- proxies

    fn proxies(&self, cx: &mut Context<Self>) -> gpui::Div {
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
        let members = group
            .members
            .iter()
            .enumerate()
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
                div()
                    .id(format!("node-{index}"))
                    .w(px(238.0))
                    .flex_grow(1.0)
                    .min_w(px(0.0))
                    .p(px(12.0))
                    .rounded(px(9.0))
                    .bg(rgb(if selected { BLUE_2 } else { SURFACE }))
                    .border_1()
                    .border_color(rgb(if selected { 0xb8c4fb } else { BORDER }))
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_dot(if selected { CYAN } else { 0xc4c8d3 }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(12.0))
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(member.clone()),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_size(px(10.0)).text_color(rgb(MUTED)).child(
                                if selected {
                                    "当前使用"
                                } else if automatic {
                                    "自动选择"
                                } else {
                                    "点击切换"
                                },
                            ))
                            .child(
                                div()
                                    .id(format!("delay-{index}"))
                                    .px(px(7.0))
                                    .py(px(3.0))
                                    .rounded(px(5.0))
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
                            ),
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
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgb(MUTED))
                                    .child("测试地址"),
                            )
                            .child(
                                div()
                                    .max_w(px(320.0))
                                    .px(px(10.0))
                                    .py(px(7.0))
                                    .rounded(px(8.0))
                                    .bg(rgb(SURFACE))
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(self.snapshot.settings.test_url.clone()),
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
                            .child(item.name.clone())
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
                                    .child(group.name.clone()),
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
            .child(div().flex().flex_wrap().gap(px(8.0)).children(members))
    }

    // --------------------------------------------------------------- rules

    fn rules(&self, cx: &mut Context<Self>) -> gpui::Div {
        let rules = read_rule_rows(&self.data_dir);
        let count = rules.len();
        let rows: Vec<gpui::AnyElement> = rules
            .iter()
            .enumerate()
            .map(|(index, rule)| rule_row(index, rule))
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
                    .child(self.action(
                        "refresh-rules",
                        "刷新状态",
                        Tone::Neutral,
                        cx,
                        ClientCommand::Refresh,
                    )),
            )
            .child(
                div()
                    .id("rules-panel")
                    .w_full()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .children(if rows.is_empty() {
                        vec![
                            empty_state(
                                "暂无可读规则",
                                "启动内核或激活订阅后，这里会读取 cache/active-config.json。",
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

    // ---------------------------------------------------------- connections

    fn connections(&self, cx: &mut Context<Self>) -> gpui::Div {
        let mut connections = self.snapshot.connections.connections.clone();
        // The header advertises download-first ordering, so the rows must
        // actually follow it: the biggest current bandwidth users lead.
        connections.sort_by_key(|connection| std::cmp::Reverse(connection.download));
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
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(format!(
                                        "{} 条活动连接 · 按下载流量排序",
                                        connections.len()
                                    )),
                            )
                            .child(
                                div()
                                    .mt(px(3.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child("行右侧按钮可断开单条连接"),
                            ),
                    )
                    .child(self.action(
                        "close-all",
                        "关闭全部",
                        Tone::Warning,
                        cx,
                        ClientCommand::CloseAllConnections,
                    )),
            )
            .child(
                div()
                    .id("connections-horizontal")
                    .w_full()
                    .overflow_x_scroll()
                    .child(
                        div()
                            .min_w(px(1040.0))
                            .child(connection_header())
                            .child(div().flex().flex_col().gap(px(4.0)).children(rows)),
                    ),
            )
            .children(if connections.is_empty() {
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
            } else {
                None
            })
    }

    // ----------------------------------------------------------------- logs

    fn logs(&self, cx: &mut Context<Self>) -> gpui::Div {
        let kernel: Vec<String> = self.snapshot.core_logs.iter().cloned().collect();
        let events: Vec<String> = self.snapshot.events.iter().cloned().collect();
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for (index, line) in kernel.iter().rev().take(180).rev().enumerate() {
            rows.push(log_row(index, "sing-box", line));
        }
        let event_offset = rows.len();
        for (index, line) in events.iter().rev().take(60).rev().enumerate() {
            rows.push(log_row(event_offset + index, "客户端", line));
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
                            .gap(px(8.0))
                            .child(pill("内核日志", CYAN))
                            .child(pill("客户端事件", MUTED)),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child(format!("{} 行 · cache/core.log", rows.len())),
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

    fn settings(&self, cx: &mut Context<Self>) -> gpui::Div {
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
                    .child(self.mini_action(
                        index + 200_000,
                        "激活",
                        cx,
                        ClientCommand::SwitchProfile(name),
                    ))
                    .into_any_element()
            })
            .collect();

        let settings_panel =
            |title: &'static str| panel(title).w(px(360.0)).min_w(px(320.0)).flex_grow(1.0);
        let tun_on = snapshot.traffic_mode == TrafficMode::Tun;

        div()
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(
                settings_panel("外观")
                    .w_full()
                    .child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(16.0))
                            .child(
                                div()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(rgb(TEXT))
                                            .child("界面主题"),
                                    )
                                    .child(
                                        div()
                                            .mt(px(3.0))
                                            .text_size(px(11.0))
                                            .text_color(rgb(MUTED))
                                            .child("浅灰工作区、白色工作面与冷青强调色"),
                                    ),
                            )
                            .child(pill("亮色", CYAN)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(SECTION_GAP))
                    .child(
                        settings_panel("订阅档案")
                            .child(
                                div()
                                    .mt(px(12.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.0))
                                    .children(profile_rows),
                            )
                            .children(if profiles.is_empty() {
                                Some(
                                    div()
                                        .mt(px(10.0))
                                        .text_size(px(11.0))
                                        .text_color(rgb(MUTED))
                                        .child("尚未导入订阅。前往「订阅」页，从剪贴板导入地址。"),
                                )
                            } else {
                                None
                            }),
                    )
                    .child(
                        settings_panel("sing-box 内核")
                            .child(setting_line(
                                "版本",
                                snapshot.core_version.as_deref().unwrap_or("未安装"),
                            ))
                            .child(setting_line(
                                "状态",
                                if snapshot.core_installed {
                                    "已安装"
                                } else {
                                    "缺失"
                                },
                            ))
                            .child(setting_line(
                                "镜像前缀",
                                if snapshot.settings.mirror.is_empty() {
                                    "直连"
                                } else {
                                    &snapshot.settings.mirror
                                },
                            ))
                            .child(div().mt(px(12.0)).child(self.action(
                                "download-core",
                                "检查并更新内核",
                                Tone::Accent,
                                cx,
                                ClientCommand::DownloadCore,
                            ))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(SECTION_GAP))
                    .child(
                        settings_panel("端口与出站")
                            .child(setting_line("流量模式", snapshot.traffic_mode.label()))
                            .child(setting_line(
                                "混合端口",
                                &snapshot.settings.mixed_port.to_string(),
                            ))
                            .child(setting_line("延迟地址", &snapshot.settings.test_url))
                            .child(setting_line("出站模式", snapshot.outbound_mode.label()))
                            .child(
                                div()
                                    .mt(px(12.0))
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(8.0))
                                    .child(self.action(
                                        "toggle-mode",
                                        "切换流量模式",
                                        Tone::Neutral,
                                        cx,
                                        ClientCommand::UpdateSettings(SettingsPatch {
                                            traffic_mode: Some(match snapshot.traffic_mode {
                                                TrafficMode::SystemProxy => TrafficMode::Tun,
                                                TrafficMode::Tun => TrafficMode::SystemProxy,
                                            }),
                                            ..Default::default()
                                        }),
                                    ))
                                    .child(self.action(
                                        "cycle-outbound",
                                        "循环出站模式",
                                        Tone::Neutral,
                                        cx,
                                        ClientCommand::SetOutboundMode(snapshot.outbound_mode.next()),
                                    )),
                            ),
                    )
                    .child(
                        settings_panel("TUN 模式")
                            .child(toggle_line(
                                "当前 TUN 状态",
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
                            .child(
                                div()
                                    .mt(px(10.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child("Windows 需要管理员权限与 wintun.dll；切换模式后通常需要重启内核。"),
                            )
                            .child(setting_line(
                                "当前说明",
                                if tun_on { "虚拟网卡接管流量" } else { "使用系统代理端口" },
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(SECTION_GAP))
                    .child(
                        settings_panel("自动化与更新")
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
                            .child(setting_line(
                                "订阅自动更新",
                                &if snapshot.settings.auto_update_minutes == 0 {
                                    "关闭".to_owned()
                                } else {
                                    format!("每 {} 分钟", snapshot.settings.auto_update_minutes)
                                },
                            ))
                            .child(
                                div()
                                    .mt(px(10.0))
                                    .text_size(px(11.0))
                                    .text_color(rgb(FAINT))
                                    .child(format!(
                                        "配置写入 {}\\settings.toml，与终端客户端共用同一套模型。",
                                        DATA_DIR
                                    )),
                            ),
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

fn switch_track(on: bool) -> impl IntoElement {
    div()
        .w(px(30.0))
        .h(px(18.0))
        .p(px(3.0))
        .rounded(px(9.0))
        .flex()
        .items_center()
        .when(on, |s| s.justify_end())
        .bg(rgb(if on { CYAN } else { 0xd6d9e2 }))
        .child(div().size(px(12.0)).rounded(px(6.0)).bg(rgb(SURFACE)))
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

/// Renders the engine's `busy` label (a `ClientCommand` variant name, with
/// any payload arguments attached) as a readable progress line.
fn busy_label(busy: &str) -> String {
    let name = busy.split('(').next().unwrap_or(busy).trim();
    let label = match name {
        "StartCore" | "AutoStart" => "正在启动内核…",
        "StopCore" => "正在停止内核…",
        "RestartCore" => "正在重启内核…",
        "UpdateSubscription" => "正在更新订阅…",
        "ImportSubscription" => "正在导入订阅…",
        "RemoveProfile" => "正在删除订阅…",
        "DownloadCore" => "正在下载内核…",
        "TestNode" => "正在测试节点延迟…",
        "TestGroup" => "正在测试整组延迟…",
        "CloseConnection" => "正在关闭连接…",
        "CloseAllConnections" => "正在关闭全部连接…",
        "Refresh" => "正在刷新状态…",
        other => return format!("正在执行 {other}…"),
    };
    label.to_owned()
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

fn pill(label: &str, color: u32) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(7.0))
        .bg(rgb(BLUE_2))
        .text_size(px(12.0))
        .text_color(rgb(color))
        .child(label.to_owned())
}

fn metric_card(label: &str, value: String, detail: String, accent: bool) -> impl IntoElement {
    div()
        .w(px(270.0))
        .flex_grow(1.0)
        .min_w(px(260.0))
        .min_h(px(112.0))
        .p(px(14.0))
        .rounded(px(12.0))
        .bg(rgb(if accent { CYAN } else { SURFACE }))
        .border_1()
        .border_color(rgb(if accent { EDGE } else { BORDER }))
        .when(accent, |style| style.border_b_1().border_color(rgb(EDGE)))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(if accent { SURFACE } else { MUTED }))
                .child(label.to_owned()),
        )
        .child(
            div()
                .mt(px(10.0))
                .text_size(px(25.0))
                .text_color(rgb(if accent { SURFACE } else { TEXT }))
                .child(value),
        )
        .child(
            div()
                .mt(px(7.0))
                .text_size(px(10.0))
                .text_color(rgb(if accent { 0xd4eeee } else { MUTED }))
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
    panel("本机 sing-box 流量")
        .child(
            div()
                .mt(px(3.0))
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .child("最近 5 分钟 · 每 1 秒采样 · 非 VPS 网卡流量"),
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

#[derive(Clone, Debug)]
struct RuleRow {
    name: String,
    matcher: String,
    outbound: String,
}

fn read_rule_rows(data_dir: &Path) -> Vec<RuleRow> {
    let active = data_dir.join("cache/active-config.json");
    let Ok(text) = std::fs::read_to_string(active) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(route) = value.get("route") else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if let Some(rules) = route.get("rules").and_then(|rules| rules.as_array()) {
        for (index, rule) in rules.iter().enumerate() {
            let outbound = rule
                .get("outbound")
                .or_else(|| rule.get("action"))
                .and_then(|value| value.as_str())
                .unwrap_or("未指定")
                .to_owned();
            let matcher = rule_matcher(rule);
            rows.push(RuleRow {
                name: format!("路由规则 {:02}", index + 1),
                matcher,
                outbound,
            });
        }
    }
    if let Some(final_outbound) = route.get("final").and_then(|value| value.as_str()) {
        rows.push(RuleRow {
            name: "兜底规则".to_owned(),
            matcher: "其他未命中流量".to_owned(),
            outbound: final_outbound.to_owned(),
        });
    }
    rows
}

fn rule_matcher(rule: &serde_json::Value) -> String {
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
    if let Some(value) = list_value("domain_suffix") {
        return format!("域名后缀 · {value}");
    }
    if let Some(value) = list_value("domain") {
        return format!("域名 · {value}");
    }
    if let Some(value) = list_value("rule_set") {
        return format!("规则集 · {value}");
    }
    if rule.get("ip_is_private").is_some() {
        return "私有地址".to_owned();
    }
    if let Some(protocol) = rule.get("protocol").and_then(|value| value.as_str()) {
        return format!("协议 · {protocol}");
    }
    if let Some(network) = rule.get("network").and_then(|value| value.as_str()) {
        return format!("网络 · {network}");
    }
    if let Some(port) = rule.get("port").and_then(|value| value.as_str()) {
        return format!("端口 · {port}");
    }
    "其他匹配条件".to_owned()
}

fn rule_row(index: usize, rule: &RuleRow) -> gpui::AnyElement {
    div()
        .id(format!("rule-row-{index}"))
        .w_full()
        .px(px(15.0))
        .py(px(12.0))
        .flex()
        .items_center()
        .gap(px(14.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .hover(|style| style.bg(rgb(BLUE_2)))
        .child(
            div()
                .w(px(24.0))
                .flex_shrink_0()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(format!("{:02}", index + 1)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(7.0))
                .flex_1()
                .min_w(px(0.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(rgb(TEXT))
                        .child(rule.name.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(pill(&rule.matcher, MUTED))
                        .child(pill(&format!("→ {}", rule.outbound), CYAN)),
                ),
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

fn log_row(index: usize, source: &str, line: &str) -> gpui::AnyElement {
    let color = level_color(line);
    let level = if color == DANGER {
        "error"
    } else if color == AMBER {
        "warn"
    } else if color == FAINT {
        "debug"
    } else {
        "info"
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
                .text_color(rgb(if color == TEXT { TEXT } else { color }))
                .child(line.to_owned()),
        )
        .into_any_element()
}

fn connection_header() -> impl IntoElement {
    div()
        .min_w(px(1040.0))
        .px(px(12.0))
        .py(px(8.0))
        .flex()
        .gap(px(10.0))
        .text_size(px(11.0))
        .text_color(rgb(CYAN))
        .child(div().w(px(62.0)).child("状态"))
        .child(div().w(px(100.0)).child("建立时间"))
        .child(div().w(px(54.0)).child("类型"))
        .child(div().w(px(180.0)).child("主机"))
        .child(div().w(px(140.0)).child("规则"))
        .child(div().w(px(160.0)).child("代理链"))
        .child(div().w(px(124.0)).child("来源 IP"))
        .child(div().w(px(150.0)).child("远程目标"))
        .child(div().w(px(82.0)).child("上传"))
        .child(div().w(px(82.0)).child("下载"))
        .child(div().w(px(48.0)).child(""))
}

fn connection_row(
    index: usize,
    connection: &Connection,
    cx: &mut Context<Sbgui>,
) -> gpui::AnyElement {
    let id = connection.id.clone();
    div()
        .id(index)
        .min_w(px(1040.0))
        .px(px(12.0))
        .py(px(9.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .bg(rgb(SURFACE))
        .border_b_1()
        .border_color(rgb(BORDER))
        .text_size(px(11.0))
        .text_color(rgb(TEXT))
        .hover(|style| style.bg(rgb(BLUE_2)))
        .child(
            div()
                .w(px(62.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(status_dot(MINT))
                .child("活动"),
        )
        .child(div().w(px(100.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.start.is_empty() {
                "刚刚".to_owned()
            } else {
                connection.start.clone()
            },
        ))
        .child(div().w(px(54.0)).child(connection.metadata.network.clone()))
        .child(div().w(px(180.0)).truncate().text_color(rgb(TEXT)).child(
            if connection.metadata.destination_host.is_empty() {
                connection.metadata.destination_ip.clone()
            } else {
                connection.metadata.destination_host.clone()
            },
        ))
        .child(div().w(px(140.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.rule.is_empty() {
                "未匹配".to_owned()
            } else {
                connection.rule.clone()
            },
        ))
        .child(div().w(px(160.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.chains.is_empty() {
                "直连".to_owned()
            } else {
                connection.chains.join(" → ")
            },
        ))
        .child(div().w(px(124.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.metadata.inbound_ip.is_empty() {
                "-".to_owned()
            } else {
                connection.metadata.inbound_ip.clone()
            },
        ))
        .child(
            div()
                .w(px(150.0))
                .truncate()
                .text_color(rgb(MUTED))
                .child(format!(
                    "{}:{}",
                    connection.metadata.destination_ip, connection.metadata.destination_port
                )),
        )
        .child(
            div()
                .w(px(82.0))
                .text_color(rgb(MINT))
                .child(human_bytes(connection.upload)),
        )
        .child(
            div()
                .w(px(82.0))
                .text_color(rgb(CYAN))
                .child(human_bytes(connection.download)),
        )
        .child(
            div()
                .id(index + 300_000)
                .w(px(48.0))
                .text_color(rgb(DANGER))
                .hover(|style| style.text_color(rgb(0xffb0ab)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
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
    let upper = line.to_ascii_uppercase();
    if upper.contains("ERROR") || upper.contains("FATAL") {
        DANGER
    } else if upper.contains("WARN") {
        AMBER
    } else if upper.contains("DEBUG") {
        FAINT
    } else {
        TEXT
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let dir = settings::data_dir_for(DATA_DIR).expect("data directory");
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
            let bounds = Bounds::centered(None, size(px(1080.0), px(760.0)), cx);
            let _runtime = runtime;
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
                        let view = Sbgui::new(controller, ui_data_dir);
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
                    // One hook covers the custom close button and Alt+F4:
                    // both arrive as WM_CLOSE, and a `false` return vetoes it
                    // so the exit confirmation can be shown first.
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
