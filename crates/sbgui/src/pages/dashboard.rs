//! The overview page: connection state, traffic and usage.

use client_core::format::human_bytes;
use gpui::{Context, InteractiveElement, ParentElement, Styled, div, px, rgb};

use crate::components::{
    clean_proxy_label, delay_color, icon, info_cell, panel, pill, traffic_panel,
};
use crate::state::Sbgui;
use crate::theme::{
    BLUE_2, BODY, BORDER, CYAN, FAINT, GAP_SECTION, LABEL, META, MINT, MUTED, PAD_CARD, RADIUS,
    SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
};

impl Sbgui {
    // ------------------------------------------------------------ dashboard

    pub(crate) fn dashboard(&self, _cx: &mut Context<Self>) -> gpui::Div {
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
            .gap(px(GAP_SECTION))
            .child(
                div()
                    .id("overview-connection")
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_h(px(84.0))
                            .px(px(PAD_CARD))
                            .py(px(18.0))
                            .flex()
                            .items_center()
                            .gap(px(14.0))
                            .child(
                                div()
                                    .size(px(44.0))
                                    .rounded(px(22.0))
                                    .bg(rgb(if connected { 0xeaf7ee } else { SURFACE_2 }))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(
                                        if connected { "check" } else { "network" },
                                        if connected { MINT } else { FAINT },
                                        22.0,
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(
                                        div()
                                            .text_size(px(16.0))
                                            .font_weight(WEIGHT_SEMIBOLD)
                                            .text_color(rgb(TEXT))
                                            .child(if connected {
                                                "内核已连接"
                                            } else {
                                                "内核未连接"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .mt(px(5.0))
                                            .text_size(px(LABEL))
                                            .text_color(rgb(MUTED))
                                            .truncate()
                                            .child(format!("{profile} · {current}")),
                                    ),
                            )
                            .children(current_delay.map(|delay| {
                                pill(format!("{delay} ms"), delay_color(Some(delay)))
                            })),
                    )
                    // First-run guidance lives inside the same card rather than
                    // stacking a second one above the state it explains.
                    .children(snapshot.profiles.is_empty().then(|| {
                        div()
                            .px(px(PAD_CARD))
                            .pb(px(18.0))
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .pt(px(16.0))
                            .child(
                                div()
                                    .text_size(px(BODY))
                                    .font_weight(WEIGHT_MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .child("完成首次连接"),
                            )
                            .child(
                                div()
                                    .mt(px(12.0))
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(10.0))
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
                                                            .text_size(px(META))
                                                            .font_weight(WEIGHT_MEDIUM)
                                                            .text_color(rgb(CYAN))
                                                            .child((index + 1).to_string()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(BODY))
                                                            .text_color(rgb(MUTED))
                                                            .child(step),
                                                    )
                                                    .children(
                                                        (index < 3)
                                                            .then(|| icon("chevron", FAINT, 14.0)),
                                                    )
                                            }),
                                    ),
                            )
                    })),
            )
            .child(traffic_panel(snapshot))
            .child(
                panel("运行状态").child(
                    div()
                        .mt(px(18.0))
                        .flex()
                        .flex_wrap()
                        .gap(px(32.0))
                        .children([
                            info_cell(
                                "内存占用",
                                if snapshot.core_running && snapshot.memory_used > 0 {
                                    human_bytes(snapshot.memory_used)
                                } else {
                                    "—".to_owned()
                                },
                                None,
                            ),
                            info_cell(
                                "内核版本",
                                snapshot
                                    .core_runtime_version
                                    .clone()
                                    .or_else(|| snapshot.core_version.clone())
                                    .unwrap_or_else(|| "未安装".to_owned()),
                                None,
                            ),
                            info_cell(
                                "自动重启",
                                format!("{} 次", snapshot.restart_attempts),
                                None,
                            ),
                            info_cell(
                                "活跃连接",
                                format!("{} 条", snapshot.active_connections),
                                Some(format!("TCP {tcp_count} · UDP {udp_count}")),
                            ),
                        ]),
                ),
            )
    }
}
