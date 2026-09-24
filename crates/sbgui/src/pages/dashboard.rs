//! The overview page: connection state, traffic and usage.
//!
//! The page follows the kit's three-surface rhythm: the state you act on, the
//! numbers you watch, and the account you are spending. Each is one working
//! surface separated by 12 px, and the lower-frequency detail sits behind a
//! disclosure inside the first one rather than as a fourth card.

use client_core::ClientCommand;
use client_core::command::SettingsPatch;
use client_core::format::human_bytes;
use client_core::system_proxy::TrafficMode;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, rgb,
};

use crate::components::{
    accordion, clean_proxy_label, delay_color, detail_item, health_dot, icon, info_cell, legend,
    metric_cell, pill, switch, traffic_chart, work_surface,
};
use crate::lang::{Locale, outbound_mode, usage_label};
use crate::pages::logs::localised_event_line;
use crate::state::{Page, Sbgui};
use crate::theme::{
    AMBER, BLUE, BLUE_2, BODY, BORDER, BORDER_STRONG, CYAN, CYAN_DARK, DANGER, DISPLAY, FAINT,
    GAP_SECTION, LABEL, META, MINT, MUTED, RADIUS_CONTROL, SECTION, SECTION_LG, SURFACE, SURFACE_2,
    TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
};
use crate::tr;

/// The quota's reset-or-expiry half of the overview's usage line. The count of
/// days is data; only the words around it belong to a language.
fn expiry_text(
    usage: Option<&client_core::subscription::SubscriptionUserinfo>,
    locale: Locale,
) -> String {
    let Some(item) = usage else {
        return tr!(locale, "无到期信息", "No expiry reported").to_owned();
    };
    let Some(expire) = item.expire else {
        return tr!(locale, "配额不设到期", "Quota without an expiry").to_owned();
    };
    let now = client_core::format::now_epoch();
    if expire > now {
        tr!(
            locale,
            format!("重置或到期 · {} 天后", (expire - now) / 86_400),
            format!("resets or expires in {} days", (expire - now) / 86_400)
        )
    } else {
        tr!(locale, "已到期", "expired").to_owned()
    }
}

impl Sbgui {
    // ------------------------------------------------------------ dashboard

    pub(crate) fn dashboard(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(GAP_SECTION))
            .child(self.connection_surface(cx))
            .child(self.traffic_surface(cx))
            .child(self.usage_surface(cx))
    }

    /// What the core is doing, the three switches that change it, and the
    /// numbers that used to fill a fourth card.
    fn connection_surface(&self, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let locale = self.locale;
        let running = snapshot.core_running;
        let busy = snapshot.busy.is_some();
        let starting = snapshot.starting;
        let profile = snapshot
            .active_profile
            .as_ref()
            .map(|item| item.name.clone())
            .unwrap_or_else(|| tr!(locale, "尚未添加订阅", "No subscription yet").to_owned());
        let current = clean_proxy_label(snapshot.current_node.as_deref().unwrap_or(tr!(
            locale,
            "尚未选择节点",
            "No node selected"
        )));
        let group = self
            .selected_group()
            .map(|item| clean_proxy_label(&item.name))
            .unwrap_or_else(|| tr!(locale, "无策略组", "No policy group").to_owned());
        let delay = self.selected_group().and_then(|item| {
            item.delays
                .get(snapshot.current_node.as_deref().unwrap_or(""))
                .copied()
        });
        let core_label = if busy {
            tr!(locale, "停止并取消", "Stop & cancel")
        } else if starting {
            tr!(locale, "启动中", "Starting")
        } else if running {
            tr!(locale, "停止内核", "Stop core")
        } else {
            tr!(locale, "启动内核", "Start core")
        };
        let core_command = if busy || starting || running {
            ClientCommand::StopCore
        } else {
            ClientCommand::StartCore
        };
        work_surface()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(14.0))
                    .child(health_dot(running))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(12.0))
                                    .child(
                                        div()
                                            .text_size(px(SECTION_LG))
                                            .font_weight(WEIGHT_SEMIBOLD)
                                            .text_color(rgb(TEXT))
                                            .child(tr!(locale, "连接状态", "Connection status")),
                                    )
                                    .child(
                                        div()
                                            .id("dashboard-core-power")
                                            .min_h(px(34.0))
                                            .px(px(12.0))
                                            .rounded(px(RADIUS_CONTROL))
                                            .border_1()
                                            .border_color(rgb(BORDER_STRONG))
                                            .bg(rgb(SURFACE))
                                            .flex()
                                            .items_center()
                                            .gap(px(6.0))
                                            .text_size(px(LABEL))
                                            .font_weight(WEIGHT_MEDIUM)
                                            .text_color(rgb(if running { DANGER } else { CYAN }))
                                            .cursor_pointer()
                                            .hover(|style| style.bg(rgb(SURFACE_2)))
                                            .on_click(cx.listener(
                                                move |view, _: &ClickEvent, _, cx| {
                                                    view.request(core_command.clone(), cx);
                                                },
                                            ))
                                            .child(icon(
                                                "power",
                                                if running { DANGER } else { CYAN },
                                                15.0,
                                            ))
                                            .child(core_label),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(6.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.0))
                                    .child(
                                        div()
                                            .text_size(px(DISPLAY))
                                            .font_weight(WEIGHT_SEMIBOLD)
                                            .text_color(rgb(if running { MINT } else { MUTED }))
                                            .child(if busy || starting {
                                                tr!(locale, "处理中", "Busy")
                                            } else if running {
                                                tr!(locale, "已连接", "Connected")
                                            } else {
                                                tr!(locale, "已停止", "Stopped")
                                            }),
                                    )
                                    .children(delay.map(|value| {
                                        pill(format!("{value} ms"), delay_color(Some(value)))
                                    })),
                            )
                            .child(
                                div()
                                    .mt(px(8.0))
                                    .text_size(px(BODY))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(if running {
                                        tr!(
                                            locale,
                                            format!("{profile} · {group} · 当前节点 {current}"),
                                            format!("{profile} · {group} · active {current}")
                                        )
                                    } else {
                                        tr!(
                                            locale,
                                            "内核未运行；启动后系统代理与 TUN 才会接管流量。",
                                            "The core is stopped; the proxy and TUN take traffic only once it runs"
                                        )
                                        .to_owned()
                                    }),
                            ),
                    ),
            )
            .child(self.quick_controls(cx))
            .child(accordion(
                "dashboard-advanced",
                tr!(locale, "进阶设置", "Advanced settings"),
                tr!(locale, "路由 / 内核 / 自动重启", "Routing / core / auto restart"),
                self.advanced_open,
                cx,
                |view| view.advanced_open = !view.advanced_open,
                div().flex().flex_wrap().gap(px(24.0)).children([
                    detail_item(
                        tr!(locale, "内核版本", "Core version"),
                        snapshot.core_version.clone().unwrap_or_else(|| tr!(
                            locale,
                            "未安装",
                            "Not installed"
                        ).to_owned()),
                    ),
                    detail_item(
                        tr!(locale, "运行版本", "Running version"),
                        snapshot
                            .core_runtime_version
                            .clone()
                            .unwrap_or_else(|| "—".to_owned()),
                    ),
                    detail_item(
                        tr!(locale, "内存占用", "Memory used"),
                        if running && snapshot.memory_used > 0 {
                            human_bytes(snapshot.memory_used)
                        } else {
                            "—".to_owned()
                        },
                    ),
                    detail_item(
                        tr!(locale, "路由规则", "Route rules"),
                        tr!(
                            locale,
                            format!("{} 条", snapshot.rules.len()),
                            format!("{} rules", snapshot.rules.len())
                        ),
                    ),
                    detail_item(
                        tr!(locale, "规则集", "Rule sets"),
                        tr!(
                            locale,
                            format!("{} 个", snapshot.rule_sets.len()),
                            format!("{} sets", snapshot.rule_sets.len())
                        ),
                    ),
                    detail_item(
                        tr!(locale, "出站模式", "Outbound mode"),
                        outbound_mode(snapshot.outbound_mode, locale).to_owned(),
                    ),
                    detail_item(
                        tr!(locale, "自动重启", "Auto restart"),
                        tr!(
                            locale,
                            format!("{} 次", snapshot.restart_attempts),
                            format!("{} times", snapshot.restart_attempts)
                        ),
                    ),
                    detail_item(
                        tr!(locale, "混合端口", "Mixed port"),
                        snapshot.settings.mixed_port.to_string(),
                    ),
                ]),
            ))
            // First-run guidance lives inside the same surface rather than
            // stacking a second one above the state it explains.
            .children(snapshot.profiles.is_empty().then(|| {
                div()
                    .mt(px(18.0))
                    .pt(px(16.0))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_size(px(BODY))
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(rgb(TEXT))
                            .child(tr!(locale, "完成首次连接", "Finish the first connection")),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.0))
                            .children(
                                [
                                    tr!(locale, "添加订阅", "Add a subscription"),
                                    tr!(locale, "选择节点", "Pick a node"),
                                    tr!(locale, "启动内核", "Start the core"),
                                    tr!(locale, "开启系统代理", "Turn on the system proxy"),
                                ]
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
                                                    .text_color(rgb(CYAN_DARK))
                                                    .child((index + 1).to_string()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(BODY))
                                                    .text_color(rgb(MUTED))
                                                    .child(step),
                                            )
                                            .children(
                                                (index < 3).then(|| icon("chevron", FAINT, 14.0)),
                                            )
                                    }),
                            ),
                    )
            }))
    }

    /// The three controls that answer "is traffic actually being taken over":
    /// system proxy, TUN, and the outbound mode. They sit in one bordered strip
    /// divided by hairlines, which is how the kit keeps them from reading as
    /// three floating cards.
    fn quick_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let locale = self.locale;
        let running = snapshot.core_running || snapshot.starting;
        let tun_on = snapshot.traffic_mode == TrafficMode::Tun;
        div()
            .mt(px(20.0))
            .pt(px(16.0))
            .border_t_1()
            .border_color(rgb(BORDER))
            .flex()
            .flex_wrap()
            .child(self.control_item(
                tr!(locale, "系统代理", "System proxy"),
                true,
                if snapshot.system_proxy_enabled {
                    tr!(
                        locale,
                        format!("已开启 · 127.0.0.1:{}", snapshot.settings.mixed_port),
                        format!("Enabled · 127.0.0.1:{}", snapshot.settings.mixed_port)
                    )
                } else {
                    tr!(locale, "已关闭", "Disabled").to_owned()
                },
                switch(
                    "dashboard-system-proxy",
                    snapshot.system_proxy_enabled,
                    Some(ClientCommand::ToggleSystemProxy),
                    cx,
                ),
            ))
            .child(
                self.control_item(
                    tr!(locale, "TUN 模式", "TUN mode"),
                    false,
                    if tun_on {
                        if running {
                            tr!(
                                locale,
                                "已启用 · 改动需重启内核",
                                "Enabled · a core restart applies it"
                            )
                        } else {
                            tr!(locale, "已启用", "Enabled")
                        }
                    } else if running {
                        tr!(
                            locale,
                            "已关闭 · 改动需重启内核",
                            "Disabled · a core restart applies it"
                        )
                    } else {
                        tr!(locale, "已关闭", "Disabled")
                    }
                    .to_owned(),
                    switch(
                        "dashboard-tun",
                        tun_on,
                        (!running).then(|| {
                            ClientCommand::UpdateSettings(SettingsPatch {
                                traffic_mode: Some(if tun_on {
                                    TrafficMode::SystemProxy
                                } else {
                                    TrafficMode::Tun
                                }),
                                ..Default::default()
                            })
                        }),
                        cx,
                    ),
                ),
            )
            .child(self.control_item(
                tr!(locale, "出站模式", "Outbound mode"),
                false,
                tr!(
                    locale,
                    format!("{}接管匹配流量", snapshot.outbound_mode.label()),
                    format!(
                        "{} takes matched traffic",
                        outbound_mode(snapshot.outbound_mode, locale)
                    )
                ),
                self.outbound_segments(cx),
            ))
    }

    fn control_item(
        &self,
        label: &'static str,
        first: bool,
        value: String,
        control: impl IntoElement,
    ) -> impl IntoElement {
        div()
            .min_w(px(230.0))
            .flex_1()
            .px(px(20.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(px(18.0))
            .when(!first, |cell| cell.border_l_1().border_color(rgb(BORDER)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(SECTION))
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(rgb(TEXT))
                            .child(label),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .text_size(px(LABEL))
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(value),
                    ),
            )
            .child(control)
    }

    /// Outbound mode as the kit draws it: the three states side by side, the
    /// current one filled, so switching is a click on the name rather than a
    /// cycle through a chip.
    fn outbound_segments(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.snapshot.outbound_mode;
        let locale = self.locale;
        div()
            .flex()
            .flex_shrink_0()
            .rounded(px(RADIUS_CONTROL))
            .border_1()
            .border_color(rgb(BORDER))
            .overflow_hidden()
            .children(
                [
                    client_core::clash_api::OutboundMode::Rule,
                    client_core::clash_api::OutboundMode::Global,
                    client_core::clash_api::OutboundMode::Direct,
                ]
                .into_iter()
                .map(|mode| {
                    let active = mode == current;
                    div()
                        .id(format!("dashboard-mode-{}", mode.label()))
                        .min_h(px(34.0))
                        .px(px(12.0))
                        .flex()
                        .items_center()
                        .text_size(px(LABEL))
                        .font_weight(if active {
                            WEIGHT_SEMIBOLD
                        } else {
                            WEIGHT_MEDIUM
                        })
                        .bg(rgb(if active { CYAN } else { SURFACE }))
                        .text_color(rgb(if active { SURFACE } else { MUTED }))
                        .cursor_pointer()
                        .when(!active, |item| item.hover(|style| style.bg(rgb(SURFACE_2))))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.send(ClientCommand::SetOutboundMode(mode));
                            cx.notify();
                        }))
                        .child(outbound_mode(mode, locale))
                }),
            )
    }

    /// The node in use, the four live numbers, and the five-minute curve.
    fn traffic_surface(&self, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let locale = self.locale;
        let current = clean_proxy_label(snapshot.current_node.as_deref().unwrap_or(tr!(
            locale,
            "尚未选择节点",
            "No node selected"
        )));
        let group = self
            .selected_group()
            .map(|item| clean_proxy_label(&item.name))
            .unwrap_or_else(|| tr!(locale, "无策略组", "No policy group").to_owned());
        let delay = self.selected_group().and_then(|item| {
            item.delays
                .get(snapshot.current_node.as_deref().unwrap_or(""))
                .copied()
        });
        let samples: Vec<(u64, u64)> = snapshot
            .traffic_history
            .iter()
            .map(|point| (point.up, point.down))
            .collect();
        let peak = snapshot.traffic_peak();
        // Two points make a line; before that the plot said "等待流量数据"
        // under an empty box, which was the loudest thing on the page.
        let has_curve = samples.len() > 1;
        let tcp_count = snapshot
            .connections
            .connections
            .iter()
            .filter(|connection| connection.metadata.network.eq_ignore_ascii_case("tcp"))
            .count();
        let udp_count = snapshot.active_connections.saturating_sub(tcp_count);
        work_surface()
            .child(
                div()
                    .text_size(px(SECTION_LG))
                    .font_weight(WEIGHT_SEMIBOLD)
                    .text_color(rgb(TEXT))
                    .child(tr!(locale, "当前节点与流量", "Current node & traffic")),
            )
            .child(
                div()
                    .mt(px(15.0))
                    .flex()
                    .flex_wrap()
                    .items_stretch()
                    .gap(px(26.0))
                    .child(
                        div()
                            .id("dashboard-node-summary")
                            .w(px(340.0))
                            .flex_shrink_0()
                            .px(px(12.0))
                            .py(px(12.0))
                            .rounded(px(RADIUS_CONTROL + 2.0))
                            .border_1()
                            .border_color(rgb(BORDER_STRONG))
                            .bg(rgb(SURFACE))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(BLUE_2)))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.page = Page::Proxies;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .size(px(40.0))
                                    .flex_shrink_0()
                                    .rounded(px(RADIUS_CONTROL + 2.0))
                                    .bg(rgb(CYAN))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("globe", 0xffffff, 22.0)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(
                                        div()
                                            .text_size(px(SECTION))
                                            .font_weight(WEIGHT_SEMIBOLD)
                                            .text_color(rgb(TEXT))
                                            .truncate()
                                            .child(current),
                                    )
                                    .child(
                                        div()
                                            .mt(px(4.0))
                                            .text_size(px(LABEL))
                                            .text_color(rgb(MUTED))
                                            .truncate()
                                            .child(tr!(
                                                locale,
                                                format!("策略组 {group}"),
                                                format!("Group {group}")
                                            )),
                                    )
                                    .child(
                                        div()
                                            .mt(px(3.0))
                                            .text_size(px(META))
                                            .text_color(rgb(MUTED))
                                            .truncate()
                                            .child(delay.map_or_else(
                                                || {
                                                    tr!(
                                                        locale,
                                                        "延迟未测，点击节点行可单独测速",
                                                        "Latency not measured yet; test a node there"
                                                    )
                                                    .to_owned()
                                                },
                                                |value| {
                                                    tr!(
                                                        locale,
                                                        format!("{value} ms · 点击可切换节点"),
                                                        format!("{value} ms · pick another node")
                                                    )
                                                },
                                            )),
                                    ),
                            )
                            .child(icon("chevron", FAINT, 17.0)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(320.0))
                            .flex()
                            .flex_wrap()
                            .children([
                                metric_cell(
                                    "download",
                                    tr!(locale, "下载速度", "Download"),
                                    format!("{}/s", human_bytes(snapshot.download_speed)),
                                    None,
                                    true,
                                ),
                                metric_cell(
                                    "upload",
                                    tr!(locale, "上传速度", "Upload"),
                                    format!("{}/s", human_bytes(snapshot.upload_speed)),
                                    None,
                                    false,
                                ),
                                metric_cell(
                                    "connections",
                                    tr!(locale, "活跃连接", "Active connections"),
                                    tr!(
                                        locale,
                                        format!("{} 条", snapshot.active_connections),
                                        snapshot.active_connections.to_string()
                                    ),
                                    Some(format!("TCP {tcp_count} · UDP {udp_count}")),
                                    false,
                                ),
                                metric_cell(
                                    "database",
                                    tr!(locale, "总流量", "Total traffic"),
                                    human_bytes(snapshot.total_download + snapshot.total_upload),
                                    None,
                                    false,
                                ),
                            ]),
                    ),
            )
            .child(
                div()
                    .mt(px(18.0))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(BODY))
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(rgb(TEXT))
                            .child(tr!(locale, "近 5 分钟流量", "Traffic · last 5 min")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(20.0))
                            .text_size(px(META))
                            .text_color(rgb(MUTED))
                            .child(legend(CYAN, tr!(locale, "下载", "Download"), snapshot.download_speed))
                            .child(legend(BLUE, tr!(locale, "上传", "Upload"), snapshot.upload_speed))
                            .child(tr!(locale, "本机 sing-box 实时采样", "Live samples from this sing-box")),
                    ),
            )
            .child(
                div()
                    .mt(px(6.0))
                    .h(px(if has_curve { 184.0 } else { 96.0 }))
                    .w_full()
                    .when(has_curve, |plot| {
                        plot.child(traffic_chart(samples, peak.max(1)))
                    })
                    .when(!has_curve, |plot| {
                        plot.flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(RADIUS_CONTROL + 1.0))
                            .bg(rgb(SURFACE_2))
                            .child(
                                div()
                                    .text_size(px(LABEL))
                                    .text_color(rgb(MUTED))
                                    .child(tr!(
                                    locale,
                                    "内核运行并产生上下行后，这里绘制曲线。",
                                    "The curve is drawn once the core runs and moves traffic."
                                )),
                            )
                    }),
            )
            .children(has_curve.then(|| {
                div()
                    .mt(px(10.0))
                    .flex()
                    .justify_between()
                    .text_size(px(META))
                    .text_color(rgb(MUTED))
                    .child(tr!(
                        locale,
                        format!("{} 个采样点", snapshot.traffic_history.len()),
                        format!("{} samples", snapshot.traffic_history.len())
                    ))
                    .child(tr!(locale, "现在", "Now"))
            }))
            .child(
                div()
                    .mt(px(18.0))
                    .pt(px(16.0))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .flex()
                    .flex_wrap()
                    .gap(px(32.0))
                    .children([
                        info_cell(
                            tr!(locale, "累计下载", "Total download"),
                            human_bytes(snapshot.total_download),
                            None,
                        ),
                        info_cell(
                            tr!(locale, "累计上传", "Total upload"),
                            human_bytes(snapshot.total_upload),
                            None,
                        ),
                        info_cell(
                            tr!(locale, "5 分钟峰值", "5-minute peak"),
                            format!("{}/s", human_bytes(peak)),
                            None,
                        ),
                    ]),
            )
    }

    /// The account being spent and what the core said last.
    fn usage_surface(&self, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let locale = self.locale;
        let usage = snapshot.subscription_usage.as_ref();
        let used = usage.map_or(0, |item| item.used());
        let total = usage.map_or(0, |item| item.total);
        let node_count = snapshot
            .proxy_groups
            .iter()
            .map(|item| item.members.len())
            .sum::<usize>();
        let events: Vec<(client_core::state::EventLine<'_>, String)> = snapshot
            .event_lines()
            .into_iter()
            .rev()
            .take(5)
            .map(|line| {
                let display = localised_event_line(&line, locale);
                (line, display)
            })
            .collect();
        work_surface().child(
            div()
                .flex()
                .flex_wrap()
                .items_stretch()
                .gap(px(26.0))
                .child(
                    div()
                        .w(px(330.0))
                        .min_w(px(280.0))
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(SECTION_LG))
                                .font_weight(WEIGHT_SEMIBOLD)
                                .text_color(rgb(TEXT))
                                .child(tr!(locale, "订阅与事件", "Subscriptions & events")),
                        )
                        .child(
                            div()
                                .mt(px(18.0))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(icon("stack", CYAN, 22.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_size(px(SECTION))
                                        .font_weight(WEIGHT_MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .truncate()
                                        .child(
                                            snapshot
                                                .active_profile
                                                .as_ref()
                                                .map(|item| item.name.clone())
                                                .unwrap_or_else(|| {
                                                    tr!(
                                                        locale,
                                                        "尚未添加订阅",
                                                        "No subscription yet"
                                                    )
                                                    .to_owned()
                                                }),
                                        ),
                                )
                                .children(
                                    snapshot
                                        .active_profile
                                        .as_ref()
                                        .map(|_| pill(tr!(locale, "使用中", "In use"), CYAN)),
                                ),
                        )
                        // Two flex children whose grow weights are the used
                        // and the unused share: GPUI has no percentage
                        // width, and this keeps the bar honest at any
                        // window size.
                        .children((total > 0).then(|| {
                            div()
                                .mt(px(14.0))
                                .h(px(8.0))
                                .w_full()
                                .flex()
                                .rounded(px(RADIUS_CONTROL))
                                .bg(rgb(SURFACE_2))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .h_full()
                                        .flex_grow(used.min(total) as f32)
                                        .bg(rgb(CYAN)),
                                )
                                .child(div().h_full().flex_grow(total.saturating_sub(used) as f32))
                        }))
                        .child(
                            div()
                                .mt(px(8.0))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .text_size(px(META))
                                .child(
                                    div()
                                        .font_weight(WEIGHT_MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .child(if total > 0 {
                                            format!(
                                                "{} / {}",
                                                human_bytes(used),
                                                human_bytes(total)
                                            )
                                        } else {
                                            usage_label(usage, locale)
                                        }),
                                )
                                .children((total > 0).then(|| {
                                    div().text_color(rgb(MUTED)).child(format!(
                                        "{:.1}%",
                                        used as f64 / total as f64 * 100.0
                                    ))
                                })),
                        )
                        .child(
                            div()
                                .mt(px(8.0))
                                .text_size(px(META))
                                .text_color(rgb(MUTED))
                                .truncate()
                                .child(tr!(
                                    locale,
                                    format!(
                                        "{} · {} 个节点",
                                        expiry_text(usage, locale),
                                        node_count
                                    ),
                                    format!(
                                        "{} · {} nodes",
                                        expiry_text(usage, locale),
                                        node_count
                                    )
                                )),
                        ),
                )
                .child(
                    div()
                        .w(px(1.0))
                        .self_stretch()
                        .flex_shrink_0()
                        .bg(rgb(BORDER)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(300.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(icon("logs", TEXT, 19.0))
                                        .child(
                                            div()
                                                .text_size(px(SECTION_LG))
                                                .font_weight(WEIGHT_SEMIBOLD)
                                                .text_color(rgb(TEXT))
                                                .child(tr!(locale, "最近事件", "Recent events")),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("dashboard-open-logs")
                                        .min_h(px(34.0))
                                        .px(px(8.0))
                                        .rounded(px(RADIUS_CONTROL))
                                        .flex()
                                        .items_center()
                                        .text_size(px(LABEL))
                                        .font_weight(WEIGHT_MEDIUM)
                                        .text_color(rgb(CYAN_DARK))
                                        .cursor_pointer()
                                        .hover(|style| style.bg(rgb(SURFACE_2)))
                                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                            view.page = Page::Logs;
                                            cx.notify();
                                        }))
                                        .child(tr!(locale, "查看全部", "View all")),
                                ),
                        )
                        .children(if events.is_empty() {
                            vec![
                                div()
                                    .mt(px(12.0))
                                    .text_size(px(BODY))
                                    .text_color(rgb(MUTED))
                                    .child(tr!(
                                        locale,
                                        "客户端还没有产生事件记录。",
                                        "The client has not recorded an event yet."
                                    ))
                                    .into_any_element(),
                            ]
                        } else {
                            events
                                .into_iter()
                                .enumerate()
                                .map(|(index, (line, display))| {
                                    let level = client_core::state::log_level_of(line.text);
                                    div()
                                        .id(format!("dashboard-event-{index}"))
                                        .w_full()
                                        .min_h(px(29.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(10.0))
                                        .text_size(px(BODY))
                                        .text_color(rgb(TEXT))
                                        .child(
                                            div()
                                                .size(px(7.0))
                                                .flex_shrink_0()
                                                .rounded(px(4.0))
                                                .bg(rgb(match level {
                                                    client_core::state::LogLevel::Error => DANGER,
                                                    client_core::state::LogLevel::Warn => AMBER,
                                                    _ => MINT,
                                                })),
                                        )
                                        .child(
                                            div().flex_1().min_w(px(0.0)).truncate().child(display),
                                        )
                                        .into_any_element()
                                })
                                .collect()
                        }),
                ),
        )
    }
}
