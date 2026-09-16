//! sbgui — the desktop sing-box client.
//!
//! The window is a pure renderer over `client_core::ClientController`: a
//! background engine owns the core, the clash_api channel and the persisted
//! settings, publishes a [`ClientSnapshot`] roughly four times a second, and
//! the UI only draws that snapshot and sends [`ClientCommand`]s. This is the
//! same control plane the terminal client uses, so the two clients cannot drift
//! apart.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use client_core::clash_api::Connection;
use client_core::command::SettingsPatch;
use client_core::settings::{self, Profiles, Settings};
use client_core::state::{ClientSnapshot, ProxyGroupSnapshot};
use client_core::system_proxy::TrafficMode;
use client_core::{ClientCommand, ClientController};
use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window, WindowBounds,
    WindowControlArea, WindowOptions, div, px, rgb, size,
};
use gpui_platform::application;

const BG: u32 = 0x0b1422;
const SURFACE: u32 = 0x111f31;
const SURFACE_2: u32 = 0x16273b;
const BORDER: u32 = 0x263b55;
const TEXT: u32 = 0xe6eff8;
const MUTED: u32 = 0x8ea3b9;
const FAINT: u32 = 0x6f879f;
const CYAN: u32 = 0x65d9ef;
const MINT: u32 = 0x68e0ae;
const AMBER: u32 = 0xf3bd62;
const DANGER: u32 = 0xff6861;

const DATA_DIR: &str = "sbgui";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Dashboard,
    Proxies,
    Connections,
    Logs,
    Settings,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "概览",
            Self::Proxies => "节点",
            Self::Connections => "连接",
            Self::Logs => "日志",
            Self::Settings => "设置",
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Self::Dashboard => "确认代理是否正在工作，以及流量经过哪里。",
            Self::Proxies => "选择节点、并发测试延迟，管理当前代理组。",
            Self::Connections => "查看 sing-box 当前接管的连接与分流规则。",
            Self::Logs => "实时查看内核日志与运行事件。",
            Self::Settings => "订阅档案、内核版本、运行模式与自动化选项。",
        }
    }
    fn all() -> [Self; 5] {
        [
            Self::Dashboard,
            Self::Proxies,
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
    Positive,
    Warning,
}

struct Sbgui {
    controller: ClientController,
    snapshot: ClientSnapshot,
    page: Page,
    group_index: usize,
}

impl Sbgui {
    fn new(controller: ClientController) -> Self {
        let snapshot = controller.snapshot();
        Self {
            controller,
            snapshot,
            page: Page::Dashboard,
            group_index: 0,
        }
    }

    fn send(&self, command: ClientCommand) {
        let _ = self.controller.send(command);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        div()
            .size_full()
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .child(self.titlebar(cx))
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
                            .flex()
                            .flex_col()
                            .child(self.header(page, cx))
                            .child(
                                div()
                                    .id("page-scroll")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .px(px(28.0))
                                    .pb(px(24.0))
                                    .child(self.content(cx)),
                            ),
                    ),
            )
            .child(self.status_bar())
    }
}

impl Sbgui {
    fn titlebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.snapshot.core_running;
        div()
            .h(px(46.0))
            .w_full()
            .flex()
            .items_center()
            .bg(rgb(0x0f1b2c))
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
                    .px(px(18.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .window_control_area(WindowControlArea::Drag)
                    .child(div().w(px(9.0)).h(px(9.0)).rounded(px(5.0)).bg(rgb(CYAN)))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .text_color(rgb(TEXT))
                            .child("sbgui"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(MUTED))
                            .child("桌面代理控制台"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(pill(
                if running {
                    "● 内核运行中"
                } else {
                    "○ 内核已停止"
                },
                if running { MINT } else { MUTED },
            ))
            .child(
                self.window_button("—", WindowControlArea::Min, cx, |window, _| {
                    window.minimize_window()
                }),
            )
            .child(
                self.window_button("□", WindowControlArea::Max, cx, |window, _| {
                    window.zoom_window()
                }),
            )
            .child(
                self.window_button("×", WindowControlArea::Close, cx, |window, _| {
                    window.remove_window()
                }),
            )
    }

    fn window_button(
        &self,
        label: &'static str,
        area: WindowControlArea,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(label)
            .w(px(42.0))
            .h(px(46.0))
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .text_size(px(15.0))
            .text_color(rgb(MUTED))
            .hover(|style| style.bg(rgb(0x1a2d43)).text_color(rgb(TEXT)))
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| action(window, cx)))
            .child(label)
    }

    fn sidebar(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(216.0))
            .h_full()
            .p(px(16.0))
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(BORDER))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .px(px(10.0))
                    .pt(px(6.0))
                    .pb(px(14.0))
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child("工作区"),
            )
            .children(Page::all().into_iter().map(|item| {
                let active = item == page;
                div()
                    .id(item.title())
                    .w_full()
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .bg(if active { rgb(0x164258) } else { rgb(SURFACE) })
                    .text_color(if active { rgb(CYAN) } else { rgb(0xb9c8d8) })
                    .hover(|style| style.bg(rgb(0x19324a)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.page = item;
                        cx.notify();
                    }))
                    .child(div().w(px(6.0)).h(px(6.0)).rounded(px(3.0)).bg(if active {
                        rgb(CYAN)
                    } else {
                        rgb(0x536a82)
                    }))
                    .child(item.title())
            }))
            .child(div().flex_1())
            .child(
                div()
                    .p(px(12.0))
                    .rounded(px(7.0))
                    .bg(rgb(0x0d1928))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(MUTED))
                            .child("数据目录"),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(10.0))
                            .text_color(rgb(FAINT))
                            .child(format!("%APPDATA%\\{DATA_DIR}")),
                    ),
            )
    }

    fn header(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.snapshot.core_running;
        let starting = self.snapshot.starting;
        let proxy_on = self.snapshot.system_proxy_enabled;
        div()
            .px(px(28.0))
            .pt(px(22.0))
            .pb(px(16.0))
            .flex()
            .items_end()
            .gap(px(10.0))
            .child(
                div()
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(28.0))
                            .text_color(rgb(TEXT))
                            .child(page.title()),
                    )
                    .child(
                        div()
                            .mt(px(6.0))
                            .text_size(px(13.0))
                            .text_color(rgb(MUTED))
                            .child(page.subtitle()),
                    ),
            )
            .child(self.action(
                "update-sub",
                "更新订阅",
                Tone::Neutral,
                cx,
                ClientCommand::UpdateSubscription,
            ))
            .child(self.action(
                "toggle-proxy",
                if proxy_on {
                    "关闭系统代理"
                } else {
                    "开启系统代理"
                },
                if proxy_on {
                    Tone::Positive
                } else {
                    Tone::Neutral
                },
                cx,
                ClientCommand::ToggleSystemProxy,
            ))
            .child(self.action(
                if running { "stop-core" } else { "start-core" },
                if starting {
                    "启动中…"
                } else if running {
                    "停止内核"
                } else {
                    "启动内核"
                },
                if running { Tone::Warning } else { Tone::Accent },
                cx,
                if running {
                    ClientCommand::StopCore
                } else {
                    ClientCommand::StartCore
                },
            ))
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
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(6.0))
            .bg(rgb(bg))
            .border_1()
            .border_color(rgb(edge))
            .text_size(px(12.0))
            .text_color(rgb(fg))
            .hover(|style| style.bg(rgb(edge)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.send(command.clone());
                cx.notify();
            }))
            .child(label)
    }

    fn status_bar(&self) -> impl IntoElement {
        let busy = self.snapshot.busy.clone();
        let status = self.snapshot.status.clone();
        div()
            .h(px(30.0))
            .w_full()
            .px(px(18.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .bg(rgb(0x0d1928))
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(status_color(&status)))
                    .child(status),
            )
            .child(div().flex_1())
            .children(busy.map(|busy| {
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(AMBER))
                    .child(format!("⏳ {busy}"))
            }))
    }

    fn content(&self, cx: &mut Context<Self>) -> gpui::Div {
        match self.page {
            Page::Dashboard => self.dashboard(cx),
            Page::Proxies => self.proxies(cx),
            Page::Connections => self.connections(cx),
            Page::Logs => self.logs(cx),
            Page::Settings => self.settings(cx),
        }
    }

    // ------------------------------------------------------------ dashboard

    fn dashboard(&self, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let current = snapshot
            .current_node
            .clone()
            .unwrap_or_else(|| "未选择".to_owned());
        let proxy_label = if snapshot.system_proxy_enabled {
            format!("已接管 127.0.0.1:{}", snapshot.settings.mixed_port)
        } else {
            "未接管系统网络".to_owned()
        };
        let core_detail = if snapshot.core_installed {
            format!(
                "sing-box {}",
                snapshot.core_version.as_deref().unwrap_or("未知版本")
            )
        } else {
            "未安装内核".to_owned()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(
                div()
                    .flex()
                    .gap(px(16.0))
                    .child(metric(
                        "内核状态",
                        if snapshot.core_running {
                            "运行中"
                        } else {
                            "未运行"
                        },
                        &core_detail,
                        if snapshot.core_running { MINT } else { AMBER },
                    ))
                    .child(metric(
                        "系统代理",
                        if snapshot.system_proxy_enabled {
                            "已开启"
                        } else {
                            "已关闭"
                        },
                        &proxy_label,
                        if snapshot.system_proxy_enabled {
                            MINT
                        } else {
                            MUTED
                        },
                    ))
                    .child(metric("当前节点", &current, "在「节点」页切换", CYAN))
                    .child(metric(
                        "活动连接",
                        &snapshot.active_connections.to_string(),
                        if snapshot.core_running {
                            "实时刷新"
                        } else {
                            "内核未运行"
                        },
                        if snapshot.active_connections > 0 {
                            CYAN
                        } else {
                            MUTED
                        },
                    )),
            )
            .child(network_path(&current, snapshot))
            .child(
                div()
                    .flex()
                    .gap(px(16.0))
                    .child(traffic_panel(snapshot))
                    .child(self.quick_panel(cx)),
            )
    }

    fn quick_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let mode = snapshot.traffic_mode;
        let next_mode = match mode {
            TrafficMode::SystemProxy => TrafficMode::Tun,
            TrafficMode::Tun => TrafficMode::SystemProxy,
        };
        panel("快速操作")
            .child(
                div()
                    .mt(px(14.0))
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(self.action(
                        "quick-download",
                        "下载内核",
                        Tone::Neutral,
                        cx,
                        ClientCommand::DownloadCore,
                    ))
                    .child(self.action(
                        "quick-mode",
                        match mode {
                            TrafficMode::SystemProxy => "切换为 TUN",
                            TrafficMode::Tun => "切换为系统代理",
                        },
                        Tone::Neutral,
                        cx,
                        ClientCommand::UpdateSettings(SettingsPatch {
                            traffic_mode: Some(next_mode),
                            ..Default::default()
                        }),
                    ))
                    .child(self.action(
                        "quick-outbound",
                        "切换出站模式",
                        Tone::Neutral,
                        cx,
                        ClientCommand::SetOutboundMode(snapshot.outbound_mode.next()),
                    )),
            )
            .child(
                div()
                    .mt(px(12.0))
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child(format!(
                        "当前模式 {} · 出站 {} · 订阅 {}",
                        mode.label(),
                        snapshot.outbound_mode.label(),
                        snapshot
                            .active_profile
                            .as_ref()
                            .map(|profile| profile.name.clone())
                            .unwrap_or_else(|| "未导入".to_owned())
                    )),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .text_size(px(11.0))
                    .text_color(rgb(FAINT))
                    .child("系统代理与 TUN 会影响整机网络，切换前请确认当前连接。"),
            )
    }

    // -------------------------------------------------------------- proxies

    fn proxies(&self, cx: &mut Context<Self>) -> gpui::Div {
        let groups = self.snapshot.proxy_groups.clone();
        if groups.is_empty() {
            return div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .child(section_title("代理组", "启动内核后加载选择器与节点"))
                .child(empty_state(
                    "暂无代理组",
                    if self.snapshot.core_running {
                        "正在等待 clash_api 返回代理组。"
                    } else {
                        "导入订阅并启动 sing-box，节点、延迟和分组会显示在这里。"
                    },
                    if self.snapshot.core_running {
                        None
                    } else {
                        Some("去设置导入订阅")
                    },
                    cx,
                ));
        }
        let group = self.selected_group().unwrap_or_else(|| groups[0].clone());
        let member_rows: Vec<gpui::AnyElement> = group
            .members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                let delay = group.delays.get(member).copied();
                let failed = group.failed.contains(member);
                let selected = member == &group.current;
                let color = if failed {
                    DANGER
                } else if let Some(delay) = delay {
                    delay_color(delay)
                } else {
                    MUTED
                };
                let delay_label = if failed {
                    "超时".to_owned()
                } else if let Some(delay) = delay {
                    format!("{delay} ms")
                } else {
                    "未测".to_owned()
                };
                let group_name = group.name.clone();
                let member_name = member.clone();
                div()
                    .id(index)
                    .w_full()
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .bg(if selected {
                        rgb(0x164258)
                    } else {
                        rgb(SURFACE_2)
                    })
                    .hover(|style| style.bg(rgb(0x1b3350)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.send(ClientCommand::SwitchNode {
                            group: group_name.clone(),
                            node: member_name.clone(),
                        });
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(14.0))
                            .text_color(rgb(if selected { CYAN } else { FAINT }))
                            .child(if selected { "●" } else { "○" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.0))
                            .text_color(rgb(if selected { TEXT } else { 0xc3d3e1 }))
                            .truncate()
                            .child(member.clone()),
                    )
                    .child(
                        div()
                            .w(px(70.0))
                            .text_size(px(11.0))
                            .text_color(rgb(color))
                            .child(delay_label),
                    )
                    .child(self.mini_action(
                        index + 100_000,
                        "测",
                        cx,
                        ClientCommand::TestNode(member.clone()),
                    ))
                    .into_any_element()
            })
            .collect();
        let group_tabs: Vec<gpui::AnyElement> = groups
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let active = item.name == group.name;
                div()
                    .id(index)
                    .px(px(12.0))
                    .py(px(7.0))
                    .rounded(px(6.0))
                    .bg(if active { rgb(0x164258) } else { rgb(SURFACE) })
                    .border_1()
                    .border_color(rgb(if active { 0x2d7188 } else { BORDER }))
                    .text_size(px(12.0))
                    .text_color(rgb(if active { CYAN } else { 0xb9c8d8 }))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.group_index = index;
                        cx.notify();
                    }))
                    .child(format!("{} ({})", item.name, item.kind))
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .children(group_tabs)
                    .child(div().flex_1())
                    .child(self.action(
                        "test-group",
                        "并发测延迟",
                        Tone::Accent,
                        cx,
                        ClientCommand::TestGroup(group.name.clone()),
                    )),
            )
            .child(
                panel(format!("{} · {} 个节点", group.name, group.members.len())).child(
                    div()
                        .mt(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .children(member_rows),
                ),
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
            .rounded(px(5.0))
            .bg(rgb(0x1a2b3e))
            .border_1()
            .border_color(rgb(BORDER))
            .text_size(px(11.0))
            .text_color(rgb(MUTED))
            .hover(|style| style.bg(rgb(0x21516a)).text_color(rgb(TEXT)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.send(command.clone());
                cx.notify();
            }))
            .child(label)
    }

    // ---------------------------------------------------------- connections

    fn connections(&self, cx: &mut Context<Self>) -> gpui::Div {
        let connections = self.snapshot.connections.connections.clone();
        let rows: Vec<gpui::AnyElement> = connections
            .iter()
            .enumerate()
            .map(|(index, connection)| connection_row(index, connection, cx))
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .text_color(rgb(TEXT))
                                    .child("活动连接"),
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
            .child(connection_header())
            .child(div().flex().flex_col().gap(px(4.0)).children(rows))
            .children(if connections.is_empty() {
                Some(empty_state(
                    "暂无活动连接",
                    "内核运行后，经过本机代理的连接会实时显示在这里。",
                    Some("启动内核"),
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
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(section_title("内核日志", "实时跟随 cache/core.log"))
            .child(
                panel("sing-box 输出").child(
                    div().mt(px(10.0)).flex().flex_col().gap(px(2.0)).children(
                        if kernel.is_empty() {
                            vec![
                                div()
                                    .text_size(px(12.0))
                                    .text_color(rgb(FAINT))
                                    .child("等待 sing-box 启动……"),
                            ]
                        } else {
                            kernel
                                .iter()
                                .rev()
                                .take(200)
                                .rev()
                                .map(|line| {
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(rgb(level_color(line)))
                                        .child(line.clone())
                                })
                                .collect()
                        },
                    ),
                ),
            )
            .child(
                panel("运行事件").child(
                    div().mt(px(10.0)).flex().flex_col().gap(px(3.0)).children(
                        events
                            .iter()
                            .rev()
                            .take(40)
                            .rev()
                            .map(|line| {
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(MUTED))
                                    .child(line.clone())
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
            )
            .children(self.snapshot.core_logs.is_empty().then(|| {
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(FAINT))
                    .child("提示：日志仅在官方内核以 info 级别输出时可见。")
            }))
            .children(if self.snapshot.core_logs.is_empty() && events.is_empty() {
                Some(empty_state(
                    "暂无日志",
                    "启动内核后这里会显示 sing-box 的输出。",
                    Some("启动内核"),
                    cx,
                ))
            } else {
                None
            })
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
                    .px(px(12.0))
                    .py(px(9.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .bg(if active {
                        rgb(0x164258)
                    } else {
                        rgb(SURFACE_2)
                    })
                    .child(
                        div()
                            .w(px(16.0))
                            .text_color(rgb(if active { MINT } else { FAINT }))
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
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                panel("订阅档案")
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
                                .child("尚未导入订阅。编辑 %APPDATA%\\sbgui\\profiles.toml，或在 TUI 客户端导入后复制该文件。"),
                        )
                    } else {
                        None
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.0))
                    .child(
                        panel("内核")
                            .child(setting_line(
                                "版本",
                                snapshot.core_version.as_deref().unwrap_or("未安装"),
                            ))
                            .child(setting_line(
                                "状态",
                                if snapshot.core_installed { "已安装" } else { "缺失" },
                            ))
                            .child(setting_line(
                                "镜像前缀",
                                if snapshot.settings.mirror.is_empty() {
                                    "直连"
                                } else {
                                    &snapshot.settings.mirror
                                },
                            ))
                            .child(
                                div().mt(px(12.0)).child(self.action(
                                    "download-core",
                                    "下载 / 更新内核",
                                    Tone::Accent,
                                    cx,
                                    ClientCommand::DownloadCore,
                                )),
                            ),
                    )
                    .child(
                        panel("运行时")
                            .child(setting_line("流量模式", snapshot.traffic_mode.label()))
                            .child(setting_line(
                                "混合端口",
                                &snapshot.settings.mixed_port.to_string(),
                            ))
                            .child(setting_line("延迟地址", &snapshot.settings.test_url))
                            .child(setting_line(
                                "出站模式",
                                snapshot.outbound_mode.label(),
                            ))
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
                                        ClientCommand::SetOutboundMode(
                                            snapshot.outbound_mode.next(),
                                        ),
                                    )),
                            ),
                    ),
            )
            .child(
                panel("自动化")
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
            )
    }
}

fn tone_colors(tone: Tone) -> (u32, u32, u32) {
    match tone {
        Tone::Accent => (CYAN, 0x164258, 0x2d7188),
        Tone::Neutral => (0xb9c8d8, 0x1a2b3e, BORDER),
        Tone::Positive => (MINT, 0x18352f, 0x2c6b56),
        Tone::Warning => (AMBER, 0x33280f, 0x7a5a1f),
    }
}

fn pill(label: &str, color: u32) -> impl IntoElement {
    div()
        .mr(px(10.0))
        .px(px(10.0))
        .py(px(5.0))
        .rounded(px(5.0))
        .bg(rgb(SURFACE_2))
        .border_1()
        .border_color(rgb(BORDER))
        .text_size(px(12.0))
        .text_color(rgb(color))
        .child(label.to_owned())
}

fn panel(title: impl Into<String>) -> gpui::Div {
    div()
        .flex_1()
        .p(px(18.0))
        .rounded(px(10.0))
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

fn section_title(title: &'static str, detail: &'static str) -> gpui::Div {
    div().flex().items_center().child(
        div()
            .flex_1()
            .child(div().text_size(px(15.0)).text_color(rgb(TEXT)).child(title))
            .child(
                div()
                    .mt(px(3.0))
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child(detail),
            ),
    )
}

fn metric(title: &str, value: &str, detail: &str, color: u32) -> impl IntoElement {
    div()
        .flex_1()
        .min_h(px(108.0))
        .p(px(16.0))
        .rounded(px(10.0))
        .bg(rgb(SURFACE_2))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(title.to_owned()),
        )
        .child(
            div()
                .mt(px(10.0))
                .text_size(px(20.0))
                .text_color(rgb(color))
                .truncate()
                .child(value.to_owned()),
        )
        .child(
            div()
                .mt(px(5.0))
                .text_size(px(11.0))
                .text_color(rgb(FAINT))
                .truncate()
                .child(detail.to_owned()),
        )
}

fn network_path(current: &str, snapshot: &ClientSnapshot) -> impl IntoElement {
    let proxy = if snapshot.system_proxy_enabled {
        format!("已启用 :{}", snapshot.settings.mixed_port)
    } else if snapshot.traffic_mode == TrafficMode::Tun {
        "TUN 模式".to_owned()
    } else {
        "未启用".to_owned()
    };
    let node = if snapshot.core_running {
        current.to_owned()
    } else {
        "等待内核".to_owned()
    };
    panel("网络路径").child(
        div()
            .mt(px(18.0))
            .flex()
            .items_center()
            .justify_between()
            .child(path_node("本机", "Windows", CYAN))
            .child(path_line())
            .child(path_node(
                "系统代理",
                &proxy,
                if snapshot.system_proxy_enabled {
                    MINT
                } else {
                    MUTED
                },
            ))
            .child(path_line())
            .child(path_node(
                "当前节点",
                &node,
                if snapshot.core_running { CYAN } else { MUTED },
            ))
            .child(path_line())
            .child(path_node(
                "公网出口",
                &format!("{} 条连接", snapshot.active_connections),
                if snapshot.active_connections > 0 {
                    MINT
                } else {
                    MUTED
                },
            )),
    )
}

fn path_node(title: &str, detail: &str, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .w(px(10.0))
                .h(px(10.0))
                .rounded(px(5.0))
                .bg(rgb(color)),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .truncate()
                .child(detail.to_owned()),
        )
}

fn path_line() -> impl IntoElement {
    div().h(px(1.0)).flex_1().mx(px(10.0)).bg(rgb(BORDER))
}

fn traffic_panel(snapshot: &ClientSnapshot) -> impl IntoElement {
    let history: Vec<(u64, u64)> = snapshot
        .traffic_history
        .iter()
        .map(|point| (point.up, point.down))
        .collect();
    let peak = snapshot.traffic_peak().max(1);
    panel(format!("实时流量 · 近 {} 秒", history.len()))
        .child(
            div()
                .mt(px(14.0))
                .flex()
                .gap(px(32.0))
                .child(rate("↓ 下载", snapshot.download_speed, CYAN))
                .child(rate("↑ 上传", snapshot.upload_speed, MINT)),
        )
        .child(
            div()
                .mt(px(14.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child("↓ 下载"),
        )
        .child(traffic_chart(&history, peak, CYAN, false))
        .child(
            div()
                .mt(px(10.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child("↑ 上传"),
        )
        .child(traffic_chart(&history, peak, MINT, true))
        .child(
            div()
                .mt(px(14.0))
                .text_size(px(11.0))
                .text_color(rgb(FAINT))
                .child(format!(
                    "累计 ↓ {}   ↑ {}   峰值 {}/s",
                    human_bytes(snapshot.total_download),
                    human_bytes(snapshot.total_upload),
                    human_bytes(peak)
                )),
        )
}

fn traffic_chart(history: &[(u64, u64)], peak: u64, color: u32, upload: bool) -> impl IntoElement {
    const HEIGHT: f32 = 40.0;
    let samples: Vec<u64> = history
        .iter()
        .rev()
        .take(96)
        .rev()
        .map(|(up, down)| if upload { *up } else { *down })
        .collect();
    div()
        .id(if upload { "chart-up" } else { "chart-down" })
        .mt(px(6.0))
        .h(px(HEIGHT))
        .w_full()
        .flex()
        .items_end()
        .gap(px(2.0))
        .children(samples.into_iter().map(move |value| {
            let ratio = value as f32 / peak as f32;
            div()
                .flex_1()
                .min_w(px(2.0))
                .h(px((HEIGHT * ratio).max(1.0)))
                .rounded_t(px(2.0))
                .bg(rgb(color))
        }))
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

fn connection_header() -> impl IntoElement {
    div()
        .px(px(12.0))
        .py(px(6.0))
        .flex()
        .gap(px(10.0))
        .text_size(px(11.0))
        .text_color(rgb(CYAN))
        .child(div().w(px(170.0)).child("目标"))
        .child(div().flex_1().child("主机"))
        .child(div().w(px(50.0)).child("网络"))
        .child(div().w(px(110.0)).child("规则"))
        .child(div().w(px(80.0)).child("上传"))
        .child(div().w(px(80.0)).child("下载"))
        .child(div().w(px(40.0)).child(""))
}

fn connection_row(
    index: usize,
    connection: &Connection,
    cx: &mut Context<Sbgui>,
) -> gpui::AnyElement {
    let id = connection.id.clone();
    div()
        .id(index)
        .px(px(12.0))
        .py(px(7.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .bg(rgb(SURFACE_2))
        .text_size(px(11.0))
        .text_color(rgb(0xc3d3e1))
        .child(div().w(px(170.0)).truncate().child(format!(
            "{}:{}",
            connection.metadata.destination_ip, connection.metadata.destination_port
        )))
        .child(div().flex_1().truncate().child(
            if connection.metadata.destination_host.is_empty() {
                "-".to_owned()
            } else {
                connection.metadata.destination_host.clone()
            },
        ))
        .child(div().w(px(50.0)).child(connection.metadata.network.clone()))
        .child(div().w(px(110.0)).truncate().text_color(rgb(MUTED)).child(
            if connection.rule.is_empty() {
                "-".to_owned()
            } else {
                connection.rule.clone()
            },
        ))
        .child(
            div()
                .w(px(80.0))
                .text_color(rgb(MINT))
                .child(human_bytes(connection.upload)),
        )
        .child(
            div()
                .w(px(80.0))
                .text_color(rgb(CYAN))
                .child(human_bytes(connection.download)),
        )
        .child(
            div()
                .id(index + 300_000)
                .w(px(40.0))
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
        .mt(px(8.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .w(px(120.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .text_color(rgb(TEXT))
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
        .mt(px(8.0))
        .flex()
        .items_center()
        .gap(px(12.0))
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
                .px(px(12.0))
                .py(px(5.0))
                .rounded(px(5.0))
                .bg(rgb(if on { 0x18352f } else { 0x1a2b3e }))
                .border_1()
                .border_color(rgb(if on { 0x2c6b56 } else { BORDER }))
                .text_size(px(11.0))
                .text_color(rgb(if on { MINT } else { MUTED }))
                .hover(|style| style.bg(rgb(0x21516a)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.send(ClientCommand::UpdateSettings(patch.clone()));
                    cx.notify();
                }))
                .child(if on { "已开启" } else { "已关闭" }),
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
            .rounded(px(6.0))
            .bg(rgb(0x164258))
            .border_1()
            .border_color(rgb(0x2d7188))
            .text_size(px(12.0))
            .text_color(rgb(CYAN))
            .hover(|style| style.bg(rgb(0x21516a)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if label.contains("启动") {
                    view.send(ClientCommand::StartCore);
                } else {
                    view.page = Page::Settings;
                }
                cx.notify();
            }))
            .child(label)
    });
    div()
        .min_h(px(200.0))
        .p(px(28.0))
        .rounded(px(10.0))
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
        0xb9c8d8
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

fn status_color(status: &str) -> u32 {
    if status.contains("失败") || status.contains("错误") || status.contains("崩溃") {
        DANGER
    } else if status.contains("需要") || status.contains("未") {
        AMBER
    } else if status.contains("成功")
        || status.contains("启动")
        || status.contains("已")
        || status.contains("完成")
    {
        MINT
    } else {
        TEXT
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
    let controller = {
        let _guard = runtime.enter();
        ClientController::start(dir)
    };

    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.0), px(780.0)), cx);
        let _runtime = runtime;
        cx.open_window(
            WindowOptions {
                titlebar: None,
                is_movable: true,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|cx| {
                    let view = Sbgui::new(controller);
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
                })
            },
        )
        .unwrap();
    });
}
