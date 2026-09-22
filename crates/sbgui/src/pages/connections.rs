//! The connection page: the live connection table.

use client_core::ClientCommand;
use client_core::format::human_bytes;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{
    connection_chain, connection_header, connection_row, connection_target, inline_empty,
    page_head, setting_line, status_dot, work_surface,
};
use crate::state::{FieldSpec, InputField, Sbgui, Tone};
use crate::theme::{
    BLUE_2, BODY, BORDER, BORDER_STRONG, CYAN, FAINT, LABEL, LIST_PAGE, MINT, MUTED, PAD_CARD,
    RADIUS_CONTROL, SECTION, SURFACE, SURFACE_2, TEXT, WEIGHT_SEMIBOLD,
};

impl Sbgui {
    // ---------------------------------------------------------- connections

    pub(crate) fn connections(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let query = self.field(InputField::ConnFilter).text.trim().to_owned();
        let mut connections = self
            .paused_connections
            .clone()
            .unwrap_or_else(|| self.snapshot.connections.connections.clone());
        // The header advertises download-first ordering, so the rows must
        // actually follow it: the biggest current bandwidth users lead.
        connections.sort_by_key(|connection| std::cmp::Reverse(connection.download));
        // The proxy/direct split is read from the same list the table draws, so
        // the metric strip and the rows can never disagree.
        let total = connections.len();
        let proxied = connections.iter().filter(|c| !c.chains.is_empty()).count();
        let direct = total - proxied;
        if !query.is_empty() {
            connections.retain(|connection| connection.matches(&query));
        }
        let visible = if self.show_all_connections {
            connections.len()
        } else {
            connections.len().min(LIST_PAGE)
        };
        let rows: Vec<gpui::AnyElement> = connections
            .iter()
            .take(visible)
            .enumerate()
            .map(|(index, connection)| connection_row(index, connection, cx))
            .collect();

        // The kit gives a management page one working surface: the sentence and
        // the tools on top, the table below. The count line and the filter share
        // a row so the "how many" never competes with the search box.
        work_surface()
            .child(page_head("内核运行期间经过本机代理的连接，实时列在这里。"))
            .child(
                div()
                    .mt(px(18.0))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(220.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_dot(if total > 0 { MINT } else { FAINT }))
                            .child(div().text_size(px(LABEL)).text_color(rgb(MUTED)).child(
                                if query.is_empty() {
                                    format!(
                                        "{total} 条活动连接 · 代理 {proxied} · 直连 {direct} · 按下载流量排序"
                                    )
                                } else {
                                    format!(
                                        "匹配 {} / {total} 条 · 按下载流量排序",
                                        connections.len()
                                    )
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.0))
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
                            .child(self.utility_toggle(
                                "pause-connections",
                                if self.paused_connections.is_some() {
                                    "继续刷新"
                                } else {
                                    "暂停刷新"
                                },
                                self.paused_connections.is_some(),
                                cx,
                                |view| {
                                    let paused = match view.paused_connections.take() {
                                        Some(_) => None,
                                        None => Some(view.snapshot.connections.connections.clone()),
                                    };
                                    view.paused_connections = paused;
                                },
                            ))
                            .child(self.button(
                                "close-all",
                                if self.confirm_close_all {
                                    "再次点击确认"
                                } else {
                                    "关闭全部"
                                },
                                Tone::Danger,
                                None,
                                cx,
                                |view, cx| {
                                    // A first click only arms the button; the
                                    // second one actually drops every socket.
                                    if view.confirm_close_all {
                                        view.send(ClientCommand::CloseAllConnections);
                                        view.confirm_close_all = false;
                                    } else {
                                        view.confirm_close_all = true;
                                    }
                                    cx.notify();
                                },
                            )),
                    ),
            )
            // With rows to list there is a table; with none there is a borderless
            // note inside the same surface, so the two boxes never nest.
            .when(!connections.is_empty(), |surface| {
                surface.child(
                    div()
                        .id("connections-panel")
                        .mt(px(18.0))
                        .w_full()
                        .rounded(px(RADIUS_CONTROL + 2.0))
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
                                        .min_w(px(920.0))
                                        .child(connection_header())
                                        .children(rows)
                                        .children(
                                            (connections.len() > LIST_PAGE
                                                || self.show_all_connections)
                                                .then(|| {
                                                    self.list_more(
                                                        if self.show_all_connections {
                                                            format!("收起，先看前 {LIST_PAGE} 条")
                                                        } else {
                                                            format!(
                                                                "显示全部 {} 条连接",
                                                                connections.len()
                                                            )
                                                        },
                                                        "connections-more",
                                                        cx,
                                                        |view| {
                                                            view.show_all_connections =
                                                                !view.show_all_connections
                                                        },
                                                    )
                                                }),
                                        ),
                                ),
                        ),
                )
            })
            .children(self.selected_connection.as_ref().and_then(|selected| {
                connections
                    .iter()
                    .find(|connection| &connection.id == selected)
                    .map(|connection| {
                        div()
                            .mt(px(18.0))
                            .p(px(PAD_CARD))
                            .rounded(px(RADIUS_CONTROL + 2.0))
                            .bg(rgb(SURFACE_2))
                            .border_1()
                            .border_color(rgb(BORDER_STRONG))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(SECTION))
                                            .font_weight(WEIGHT_SEMIBOLD)
                                            .text_color(rgb(TEXT))
                                            .child("连接详情"),
                                    )
                                    .child(
                                        div()
                                            .id("close-connection-detail")
                                            .size(px(34.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .cursor_pointer()
                                            .rounded(px(RADIUS_CONTROL))
                                            .hover(|s| s.bg(rgb(BLUE_2)))
                                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                                view.selected_connection = None;
                                                cx.notify();
                                            }))
                                            .child(crate::components::icon(
                                                "close", MUTED, 14.0,
                                            )),
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
                            .child(setting_line(
                                "累计流量",
                                &format!(
                                    "↓ {} · ↑ {}",
                                    human_bytes(connection.download),
                                    human_bytes(connection.upload)
                                ),
                            ))
                            .child(setting_line(
                                "建立时间",
                                if connection.start.is_empty() {
                                    "刚刚"
                                } else {
                                    &connection.start
                                },
                            ))
                    })
            }))
            .when(connections.is_empty(), |surface| {
                surface.child(
                    div()
                        .mt(px(18.0))
                        .w_full()
                        .min_h(px(220.0))
                        .py(px(40.0))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(10.0))
                        .when(total == 0, |block| {
                            block
                                .child(
                                    div()
                                        .text_size(px(SECTION))
                                        .font_weight(WEIGHT_SEMIBOLD)
                                        .text_color(rgb(TEXT))
                                        .child("暂无活动连接"),
                                )
                                .child(
                                    div()
                                        .max_w(px(420.0))
                                        .text_size(px(BODY))
                                        .line_height(px(21.0))
                                        .text_color(rgb(MUTED))
                                        .child(
                                            "内核运行后，经过本机代理的连接会实时显示在这里。",
                                        ),
                                )
                                .when(
                                    !(self.snapshot.core_running || self.snapshot.starting),
                                    |block| {
                                        block.child(self.button(
                                            "start-core-from-connections",
                                            "启动内核",
                                            Tone::Accent,
                                            Some("power"),
                                            cx,
                                            |view, cx| {
                                                view.send(ClientCommand::StartCore);
                                                cx.notify();
                                            },
                                        ))
                                    },
                                )
                        })
                        .when(total > 0, |block| {
                            block.child(inline_empty(
                                "无匹配连接",
                                "换个关键字，或按 Esc 清空筛选。",
                            ))
                        }),
                )
            })
    }

    pub(crate) fn utility_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        toggle: impl Fn(&mut Sbgui) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .min_h(px(34.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(RADIUS_CONTROL))
            .bg(rgb(if active { BLUE_2 } else { SURFACE }))
            .border_1()
            .border_color(rgb(if active { CYAN } else { BORDER }))
            .text_size(px(LABEL))
            .text_color(rgb(if active { CYAN } else { TEXT }))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                toggle(view);
                cx.notify();
            }))
            .child(label)
    }
}
