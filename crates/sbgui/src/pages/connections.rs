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
use crate::tr;

impl Sbgui {
    // ---------------------------------------------------------- connections

    pub(crate) fn connections(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let locale = self.locale;
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
        let proxied = connections.iter().filter(|c| !c.is_direct()).count();
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
            .map(|(index, connection)| connection_row(index, connection, locale, cx))
            .collect();

        // The kit gives a management page one working surface: the sentence and
        // the tools on top, the table below. The count line and the filter share
        // a row so the "how many" never competes with the search box.
        work_surface()
            .child(page_head(tr!(
                locale,
                "内核运行期间经过本机代理的连接，实时列在这里。",
                "Connections that the running core is proxying right now.",
            )))
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
                                    tr!(
                                        locale,
                                        format!(
                                            "{total} 条活动连接 · 代理 {proxied} · 直连 {direct} · 按下载流量排序"
                                        ),
                                        format!(
                                            "{total} active · proxied {proxied} · direct {direct} · by download"
                                        )
                                    )
                                } else {
                                    tr!(
                                        locale,
                                        format!(
                                            "匹配 {} / {total} 条 · 按下载流量排序",
                                            connections.len()
                                        ),
                                        format!(
                                            "Matched {} / {total} · by download",
                                            connections.len()
                                        )
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
                                    placeholder: tr!(
                                        locale,
                                        "按主机 / 目标 / 规则筛选…",
                                        "Filter by host / destination / rule…",
                                    ),
                                    width: 240.0,
                                },
                                window,
                                cx,
                            ))
                            .child(self.utility_toggle(
                                "pause-connections",
                                if self.paused_connections.is_some() {
                                    tr!(locale, "继续刷新", "Resume")
                                } else {
                                    tr!(locale, "暂停刷新", "Pause")
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
                                    tr!(locale, "再次点击确认", "Click again to confirm")
                                } else {
                                    tr!(locale, "关闭全部", "Close all")
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
                                        .child(connection_header(locale))
                                        .children(rows)
                                        .children(
                                            (connections.len() > LIST_PAGE
                                                || self.show_all_connections)
                                                .then(|| {
                                                    self.list_more(
                                                        tr!(
                                                            locale,
                                                            if self.show_all_connections {
                                                                format!("收起，先看前 {LIST_PAGE} 条")
                                                            } else {
                                                                format!(
                                                                    "显示全部 {} 条连接",
                                                                    connections.len()
                                                                )
                                                            },
                                                            if self.show_all_connections {
                                                                format!(
                                                                    "Show fewer: the first {LIST_PAGE}"
                                                                )
                                                            } else {
                                                                format!(
                                                                    "Show all {} connections",
                                                                    connections.len()
                                                                )
                                                            },
                                                        ),
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
                                            .child(tr!(locale, "连接详情", "Connection detail")),
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
                            .child(setting_line(tr!(locale, "远程目标", "Destination"), &connection_target(connection)))
                            .child(setting_line(
                                tr!(locale, "命中规则", "Rule"),
                                if connection.rule.is_empty() {
                                    tr!(locale, "未匹配", "No match")
                                } else {
                                    &connection.rule
                                },
                            ))
                            .child(setting_line(tr!(locale, "使用节点", "Node"), &connection_chain(connection, locale)))
                            .child(setting_line(tr!(locale, "协议", "Protocol"), &connection.metadata.network))
                            .child(setting_line(
                                tr!(locale, "累计流量", "Traffic"),
                                &format!(
                                    "↓ {} · ↑ {}",
                                    human_bytes(connection.download),
                                    human_bytes(connection.upload)
                                ),
                            ))
                            .child(setting_line(
                                tr!(locale, "建立时间", "Started"),
                                if connection.start.is_empty() {
                                    tr!(locale, "刚刚", "Just now")
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
                                        .child(tr!(locale, "暂无活动连接", "No active connections")),
                                )
                                .child(
                                    div()
                                        .max_w(px(420.0))
                                        .text_size(px(BODY))
                                        .line_height(px(21.0))
                                        .text_color(rgb(MUTED))
                                        .child(tr!(
                                            locale,
                                            "内核运行后，经过本机代理的连接会实时显示在这里。",
                                            "Once the core runs, proxied connections appear here live.",
                                        )),
                                )
                                .when(
                                    !(self.snapshot.core_running || self.snapshot.starting),
                                    |block| {
                                        block.child(self.button(
                                            "start-core-from-connections",
                                            tr!(locale, "启动内核", "Start core"),
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
                                tr!(locale, "无匹配连接", "No matching connections"),
                                tr!(
                                    locale,
                                    "换个关键字，或按 Esc 清空筛选。",
                                    "Try another keyword, or press Esc to clear the filter.",
                                ),
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
