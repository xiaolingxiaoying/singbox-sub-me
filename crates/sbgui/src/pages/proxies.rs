//! The node page: strategy groups, members and latency.

use client_core::ClientCommand;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb, rgba,
};

use crate::components::{clean_proxy_label, delay_color, empty_state, icon, pill, status_dot};
use crate::state::{FieldSpec, InputField, Sbgui, Tone};
use crate::theme::{
    BLUE_2, BODY, BORDER, CYAN, DANGER, FAINT, GAP_SECTION, LABEL, META, MUTED, NAV_ACTIVE, RADIUS,
    RADIUS_CONTROL, ROW_X, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_NORMAL,
};
use crate::tr;

impl Sbgui {
    // -------------------------------------------------------------- proxies

    pub(crate) fn proxies(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let groups = &self.snapshot.proxy_groups;
        let locale = self.locale;
        let Some(group) = self.selected_group() else {
            return div().child(empty_state(
                tr!(locale, "暂无代理组", "No proxy groups yet"),
                if self.snapshot.core_running {
                    tr!(
                        locale,
                        "正在等待内核返回代理组，请稍后查看。",
                        "Waiting for the core to report its groups."
                    )
                } else {
                    tr!(
                        locale,
                        "导入订阅并启动内核后，在这里选择节点。",
                        "Import a subscription and start the core to choose a node here."
                    )
                },
                Some(tr!(locale, "去订阅管理", "Open subscriptions")),
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
        let total_nodes = groups.iter().map(|item| item.members.len()).sum::<usize>();
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
                    tr!(locale, "超时", "Timeout").to_owned()
                } else {
                    delay
                        .map(|d| format!("{d} ms"))
                        .unwrap_or_else(|| tr!(locale, "测试", "Test").to_owned())
                };
                let color = if failed { DANGER } else { delay_color(delay) };
                let group_name = group.name.clone();
                let node_name = member.clone();
                let test_name = member.clone();
                let display_name = clean_proxy_label(member);
                div()
                    .id(format!("node-{index}"))
                    .when(card_view, |row| row.w(px(238.0)).flex_grow(1.0))
                    .when(!card_view, |row| row.w_full())
                    .min_w(px(0.0))
                    // Compact on purpose: a node row answers "is this the one,
                    // what is it called, how fast is it" and nothing else.
                    .min_h(px(34.0))
                    .px(px(ROW_X))
                    .py(px(8.0))
                    .rounded(px(if card_view { RADIUS_CONTROL } else { 0.0 }))
                    .bg(rgb(if selected { BLUE_2 } else { SURFACE }))
                    .when(card_view, |row| {
                        row.border_1()
                            .border_color(rgb(if selected { CYAN } else { BORDER }))
                    })
                    .when(!card_view && index > 0, |row| {
                        row.border_t_1().border_color(rgb(BORDER))
                    })
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
                            .text_size(px(BODY))
                            .text_color(rgb(if selected { CYAN } else { TEXT }))
                            .font_weight(if selected {
                                WEIGHT_MEDIUM
                            } else {
                                WEIGHT_NORMAL
                            })
                            .truncate()
                            .child(display_name),
                    )
                    .children(selected.then(|| pill(tr!(locale, "当前", "Active"), CYAN)))
                    .child(
                        div()
                            .id(format!("delay-{index}"))
                            .min_w(px(74.0))
                            .flex_shrink_0()
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(RADIUS_CONTROL))
                            .text_size(px(LABEL))
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
            .gap(px(16.0))
            // Strategy groups are the primary rail: the question a node page
            // answers first is "which policy", and only then "which node".
            .child(
                div()
                    .w(px(176.0))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(groups.iter().enumerate().map(|(index, item)| {
                        let active = item.name == group.name;
                        div()
                            .id(format!("group-{index}"))
                            .w_full()
                            .min_h(px(38.0))
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(px(RADIUS_CONTROL))
                            .bg(rgb(if active { NAV_ACTIVE } else { SURFACE }))
                            .flex()
                            .items_center()
                            .gap(px(9.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(if active { NAV_ACTIVE } else { SURFACE_2 })))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.group_index = index;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(3.0))
                                    .h(px(16.0))
                                    .flex_shrink_0()
                                    .rounded(px(2.0))
                                    .bg(if active { rgb(CYAN) } else { rgba(0x0000_0000) }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .truncate()
                                    .text_size(px(BODY))
                                    .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
                                    .text_color(rgb(if active { CYAN } else { TEXT }))
                                    .child(clean_proxy_label(&item.name)),
                            )
                            .child(
                                div()
                                    .text_size(px(META))
                                    .text_color(rgb(if active { CYAN } else { FAINT }))
                                    .child(item.members.len().to_string()),
                            )
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(GAP_SECTION))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .flex_wrap()
                            .child(div().text_size(px(META)).text_color(rgb(MUTED)).child(tr!(
                                locale,
                                format!("{} 个代理组 · {} 个节点", groups.len(), total_nodes),
                                format!("{} proxy groups · {} nodes", groups.len(), total_nodes)
                            )))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(self.text_field(
                                        FieldSpec {
                                            field: InputField::ProxySearch,
                                            id: "proxy-search",
                                            placeholder: tr!(locale, "搜索节点…", "Search nodes…"),
                                            width: 210.0,
                                        },
                                        window,
                                        cx,
                                    ))
                                    .child(self.action(
                                        "test-group-toolbar",
                                        tr!(locale, "全部测速", "Test all"),
                                        Tone::Neutral,
                                        cx,
                                        ClientCommand::TestGroup(group.name.clone()),
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .rounded(px(RADIUS_CONTROL))
                                            .border_1()
                                            .border_color(rgb(BORDER))
                                            .overflow_hidden()
                                            .child(self.view_mode(
                                                "node-list-view",
                                                tr!(locale, "列表", "List"),
                                                !card_view,
                                                cx,
                                                false,
                                            ))
                                            .child(self.view_mode(
                                                "node-card-view",
                                                tr!(locale, "卡片", "Cards"),
                                                card_view,
                                                cx,
                                                true,
                                            )),
                                    ),
                            ),
                    )
                    // The group summary used to be a card of its own, repeating the
                    // selected chip and a second 测试延迟 button.
                    .child(
                        div()
                            .px(px(4.0))
                            .text_size(px(META))
                            .text_color(rgb(MUTED))
                            .child(tr!(
                                locale,
                                format!(
                                    "{} · {} 个节点 · {}",
                                    clean_proxy_label(&group.name),
                                    group.members.len(),
                                    if automatic {
                                        "自动选择，点击节点不会切换"
                                    } else {
                                        "点击节点即可切换"
                                    }
                                ),
                                format!(
                                    "{} · {} nodes · {}",
                                    clean_proxy_label(&group.name),
                                    group.members.len(),
                                    if automatic {
                                        "Auto select; clicking a node does not switch"
                                    } else {
                                        "Click a node to switch"
                                    }
                                )
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
                            .gap(px(if card_view { 12.0 } else { 0.0 }))
                            .children(members),
                    ),
            )
    }

    /// One half of the list/card segmented control.
    fn view_mode(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        card: bool,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px(px(12.0))
            .py(px(8.0))
            .text_size(px(LABEL))
            .bg(rgb(if active { BLUE_2 } else { SURFACE }))
            .text_color(rgb(if active { CYAN } else { MUTED }))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.node_card_view = card;
                cx.notify();
            }))
            .child(label)
    }
}
