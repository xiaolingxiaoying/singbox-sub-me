//! The chrome around the page: titlebar, sidebar, toolbar, the control
//! chips, the content shell and the two shared field widgets.

use client_core::ClientCommand;
use client_core::command::SettingsPatch;
use client_core::system_proxy::TrafficMode;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, WindowControlArea, div, img, px, rgb, rgba,
};

use crate::components::{clean_proxy_label, icon, side_rate};
use crate::state::{FieldSpec, Page, Sbgui, Tone};
use crate::theme::{
    BLUE, BODY, BORDER, BRAND_ICON_PATH, CONTENT_MAX, CONTENT_PAD, CYAN, CYAN_DARK, DANGER, FAINT,
    GAP_ITEM, LABEL, META, MINT, MUTED, NAV_ACTIVE, RADIUS_CONTROL, SECTION, SIDEBAR_W, SURFACE,
    SURFACE_2, TEXT, TITLE, TITLEBAR_H, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD, tone_colors,
};

impl Sbgui {
    pub(crate) fn titlebar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let maximize_icon = if window.is_maximized() {
            "restore"
        } else {
            "maximize"
        };
        div()
            .h(px(TITLEBAR_H))
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
                    .px(px(16.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .window_control_area(WindowControlArea::Drag)
                    .child(img(BRAND_ICON_PATH).size(px(22.0)).flex_shrink_0())
                    .child(
                        div()
                            .text_size(px(SECTION))
                            .font_weight(WEIGHT_MEDIUM)
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
            .w(px(44.0))
            .h(px(TITLEBAR_H))
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

    pub(crate) fn sidebar(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        div()
            .id("sidebar-scroll")
            .w(px(SIDEBAR_W))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .px(px(14.0))
            .py(px(18.0))
            .bg(rgb(SURFACE))
            .border_r_1()
            .border_color(rgb(BORDER))
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
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
                            .h(px(44.0))
                            .px(px(12.0))
                            .rounded(px(RADIUS_CONTROL))
                            .flex()
                            .items_center()
                            .gap(px(11.0))
                            // The selected destination carries a 3 px inset bar.
                            // It is laid out for every row and only painted when
                            // selected, so the labels stay in one column.
                            .child(
                                div()
                                    .w(px(3.0))
                                    .h(px(18.0))
                                    .flex_shrink_0()
                                    .rounded(px(2.0))
                                    .bg(if active { rgb(CYAN) } else { rgba(0x0000_0000) }),
                            )
                            .bg(rgb(if active { NAV_ACTIVE } else { SURFACE }))
                            .cursor_pointer()
                            .hover(move |style| {
                                style.bg(rgb(if active { NAV_ACTIVE } else { SURFACE_2 }))
                            })
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.page = item;
                                // Armed one-click confirmations belong to the page
                                // that shows them; carrying them over left a
                                // button labelled 确认 without the user having
                                // armed it on this page.
                                view.confirm_close_all = false;
                                view.confirm_delete_profile = None;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(20.0))
                                    .h(px(20.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(glyph, if active { CYAN } else { FAINT }, 17.0)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(BODY))
                                    .when(active, |style| style.font_weight(WEIGHT_MEDIUM))
                                    .text_color(rgb(if active { CYAN } else { TEXT }))
                                    .child(item.title()),
                            )
                            .children(count.map(|value| {
                                div()
                                    .text_size(px(META))
                                    .text_color(rgb(if active { CYAN } else { FAINT }))
                                    .child(value.to_string())
                            }))
                    })),
            )
            // The live rates the sidebar used to show twice: two labelled rows
            // and a mini chart that repeated the overview's traffic panel.
            .child(
                div()
                    .mt_auto()
                    .pt(px(18.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .text_size(px(META))
                    .child(side_rate("下载", snapshot.download_speed, CYAN))
                    .child(side_rate("上传", snapshot.upload_speed, BLUE)),
            )
    }

    /// The page's one permanent band: what page this is on the left, the
    /// controls that used to own a full-width bar of their own on the right.
    /// Title, subtitle and status line used to stack into three.
    pub(crate) fn toolbar(&self, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
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
            .w_full()
            .flex_shrink_0()
            .px(px(CONTENT_PAD))
            .pt(px(18.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(CONTENT_MAX))
                    .mx_auto()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap(px(GAP_ITEM))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(240.0))
                            .child(
                                div()
                                    .text_size(px(TITLE))
                                    .font_weight(WEIGHT_SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(page.title()),
                            )
                            .when(!status_text.is_empty(), |column| {
                                column.child(
                                    div()
                                        .mt(px(4.0))
                                        .text_size(px(META))
                                        .text_color(rgb(status_color))
                                        .child(status_text),
                                )
                            }),
                    )
                    .child(self.control_chips(cx))
                    // Secondary core actions stay behind the ··· until asked
                    // for; 停止 and 重启 never share the row with 启动.
                    .children((self.core_menu_open && snapshot.core_running).then(|| {
                        div()
                            .w_full()
                            .pt(px(6.0))
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
                    })),
            )
    }

    fn control_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .child(Self::status_chip(
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
                if running { MINT } else { MUTED },
                cx,
                if snapshot.busy.is_some() || snapshot.starting {
                    Some(ClientCommand::StopCore)
                } else {
                    (!running).then_some(ClientCommand::StartCore)
                },
            ))
            .child(Self::status_chip(
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
                    .max_w(px(280.0))
                    .flex_shrink_0()
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(RADIUS_CONTROL))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SURFACE_2)).border_color(rgb(CYAN)))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.page = Page::Proxies;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("globe", CYAN, 15.0))
                            .child(
                                div()
                                    .text_size(px(META))
                                    .text_color(rgb(FAINT))
                                    .child("当前节点"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(LABEL))
                                    .font_weight(WEIGHT_MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(current_detail),
                            ),
                    ),
            )
            .child(Self::status_chip(
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
            .child(Self::status_chip(
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
                    .size(px(34.0))
                    .flex_shrink_0()
                    .rounded(px(RADIUS_CONTROL))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(15.0))
                    .text_color(rgb(MUTED))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(SURFACE_2)).text_color(rgb(TEXT)))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.core_menu_open = !view.core_menu_open;
                        cx.notify();
                    }))
                    .child("···"),
            )
    }

    /// One control-chip in the toolbar: icon, what it reports, and its state
    /// as text on a single line.
    fn status_chip(
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
            .flex_shrink_0()
            .min_h(px(34.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(RADIUS_CONTROL))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(BORDER))
            .flex()
            .items_center()
            .gap(px(8.0))
            .when(clickable, |style| {
                style
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(SURFACE_2)).border_color(rgb(CYAN)))
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if let Some(command) = command.clone() {
                    view.send(command);
                    cx.notify();
                }
            }))
            .child(icon(glyph, color, 15.0))
            .child(
                div()
                    .text_size(px(META))
                    .text_color(rgb(FAINT))
                    .child(label.to_owned()),
            )
            .child(
                div()
                    .text_size(px(LABEL))
                    .font_weight(WEIGHT_MEDIUM)
                    .text_color(rgb(color))
                    .child(value),
            )
    }

    /// One header/row action button. The command is built at click time so the
    /// buttons always carry the freshest view state.
    pub(crate) fn action(
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
            // The kit keeps every clickable control at 34 px or taller; padding
            // alone used to leave these at 31 px.
            .min_h(px(34.0))
            .px(px(14.0))
            .py(px(8.0))
            .rounded(px(RADIUS_CONTROL))
            .bg(rgb(bg))
            .border_1()
            .border_color(rgb(edge))
            .flex()
            .items_center()
            .text_size(px(LABEL))
            .font_weight(WEIGHT_MEDIUM)
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

    pub(crate) fn content(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
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
    pub(crate) fn text_field(
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
            .min_h(px(36.0))
            .px(px(12.0))
            .py(px(9.0))
            .rounded(px(RADIUS_CONTROL))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(if focused { CYAN } else { BORDER }))
            .flex()
            .items_center()
            .overflow_hidden()
            .cursor_pointer()
            .text_size(px(LABEL))
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
    pub(crate) fn edit_line(
        &self,
        label: &'static str,
        spec: FieldSpec,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .w_full()
            .py(px(14.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(div().text_size(px(BODY)).text_color(rgb(TEXT)).child(label))
            .child(self.text_field(spec, window, cx))
    }
}
