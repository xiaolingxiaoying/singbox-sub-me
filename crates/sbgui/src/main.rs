//! sbgui — the desktop sing-box client.
//!
//! The window is a pure renderer over `client_core::ClientController`: a
//! background engine owns the core, the clash_api channel and the persisted
//! settings, publishes a [`ClientSnapshot`] roughly four times a second, and
//! the UI only draws that snapshot and sends [`ClientCommand`]s. This is the
//! same control plane the terminal client uses, so the two clients cannot drift
//! apart.
//!
//! THESIS: calm proxy control, informed by the Serein prototype and adapted to sing-box.
//! OWN-WORLD: cool neutral canvas, teal actions, pale active navigation, quiet line icons.
//! STORY: find a section in the sidebar, understand its state, and change it without visual noise.
//! FIRST VIEWPORT: 216px navigation sidebar plus one toolbar holding the page name and the
//! kernel, mode, node, system-proxy and TUN state.
//! FORM: a desktop control surface that separates blocks with whitespace, not with ink.
//! FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod chrome;
mod components;
mod overlay;
mod parse;
mod state;
mod theme;

use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use client_core::command::SettingsPatch;
use client_core::format::{age_label, human_bytes, usage_label};
use client_core::settings::{Profiles, Settings};
use client_core::state::{LogLevel, RouteRuleSnapshot};
use client_core::system_proxy::TrafficMode;
use client_core::{ClientCommand, ClientController, settings};
use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext as _, AssetSource, Bounds, ClickEvent, ClipboardItem, Context,
    InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement,
    Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, rgb, size,
};
use gpui_platform::application;

use crate::components::{
    clean_proxy_label, connection_chain, connection_header, connection_row, connection_target,
    delay_color, empty_state, icon, info_cell, inline_empty, log_row, log_table_header, panel,
    pill, rule_row, setting_line, setting_row_intro, status_dot, toggle_line, traffic_panel,
};
use crate::state::{
    FieldSpec, InputField, LogLevelFilter, Sbgui, SettingsSection, Tone, env_window_size,
};
use crate::theme::{
    AMBER, BLUE_2, BODY, BORDER, BRAND_ICON_PATH, CYAN, CYAN_DARK, DANGER, DATA_DIR, FAINT,
    GAP_SECTION, LABEL, LIST_PAGE, META, MINT, MUTED, PAD_CARD, RADIUS, ROW_X, ROW_Y, SECTION,
    SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_NORMAL, WEIGHT_SEMIBOLD,
};

pub(crate) struct SereinAssets;

impl AssetSource for SereinAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == BRAND_ICON_PATH {
            Ok(Some(Cow::Borrowed(include_bytes!("../assets/serein.ico"))))
        } else {
            Ok(None)
        }
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(vec![BRAND_ICON_PATH.into()])
    }
}

impl Sbgui {
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

    // --------------------------------------------------------- subscriptions

    fn subscriptions(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let profiles = self.snapshot.profiles.clone();
        let mut root = div().flex().flex_col().gap(px(GAP_SECTION)).child(
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
                        .rounded(px(9.0))
                        .bg(rgb(CYAN))
                        .text_size(px(LABEL))
                        .font_weight(WEIGHT_MEDIUM)
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
                        .rounded(px(9.0))
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .text_size(px(LABEL))
                        .font_weight(WEIGHT_MEDIUM)
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
                )),
        );

        if self.show_subscription_import {
            root = root.child(
                div()
                    .p(px(PAD_CARD))
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
                                    .text_size(px(SECTION))
                                    .font_weight(WEIGHT_SEMIBOLD)
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
                            .mt(px(6.0))
                            .text_size(px(LABEL))
                            .text_color(rgb(MUTED))
                            .child("粘贴 HTTP/HTTPS 订阅地址或 sing-box JSON 地址。"),
                    )
                    .child(
                        div()
                            .mt(px(16.0))
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
                                    .px(px(16.0))
                                    .py(px(9.0))
                                    .rounded(px(9.0))
                                    .bg(rgb(CYAN))
                                    .text_size(px(LABEL))
                                    .font_weight(WEIGHT_MEDIUM)
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
            // Usage and node counts only exist for the profile the core has
            // loaded, so the two cases get their own sentence rather than a
            // column of dashes.
            let detail = if active {
                let nodes = self
                    .snapshot
                    .proxy_groups
                    .iter()
                    .map(|group| group.members.len())
                    .sum::<usize>();
                format!(
                    "{} · {} 个节点 · {}",
                    age_label(profile.last_updated),
                    nodes,
                    usage_label(self.snapshot.subscription_usage.as_ref())
                )
            } else {
                format!("{} · 未启用", age_label(profile.last_updated))
            };
            div()
                .id(format!("subscription-row-{index}"))
                .w_full()
                .px(px(PAD_CARD))
                .py(px(16.0))
                .bg(if active { rgb(BLUE_2) } else { rgb(SURFACE) })
                // Rules, logs and this list all separate rows with a hairline
                // above every row but the first, so the last row does not draw
                // a second line on the container's own edge.
                .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
                .flex()
                .items_center()
                .gap(px(16.0))
                .text_color(rgb(TEXT))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .text_size(px(15.0))
                                        .font_weight(WEIGHT_MEDIUM)
                                        .text_color(rgb(TEXT))
                                        .truncate()
                                        .child(profile.name.clone()),
                                )
                                .children(active.then(|| pill("当前", CYAN))),
                        )
                        .child(
                            div()
                                .mt(px(6.0))
                                .text_size(px(META))
                                .text_color(rgb(MUTED))
                                .truncate()
                                .child(detail),
                        ),
                )
                .children((!active).then(|| {
                    self.mini_action(
                        index + 300_000,
                        "设为当前",
                        cx,
                        ClientCommand::SwitchProfile(name_for_activate.clone()),
                    )
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
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(px(9.0))
                        .text_size(px(LABEL))
                        .text_color(rgb(DANGER))
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(0xffecee)))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            // Deleting also drops the cached node list,
                            // so the first click only arms the button —
                            // the same shape as 关闭全部.
                            if view.confirm_delete_profile.as_deref()
                                == Some(name_for_remove.as_str())
                            {
                                view.send(ClientCommand::RemoveProfile(name_for_remove.clone()));
                                view.confirm_delete_profile = None;
                            } else {
                                view.confirm_delete_profile = Some(name_for_remove.clone());
                            }
                            cx.notify();
                        }))
                        .child(if armed { "确认删除？" } else { "删除" })
                })
        });
        root.child(
            div()
                .rounded(px(RADIUS))
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(rgb(BORDER))
                .overflow_hidden()
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
                let color = if failed { DANGER } else { delay_color(delay) };
                let group_name = group.name.clone();
                let node_name = member.clone();
                let test_name = member.clone();
                let display_name = clean_proxy_label(member);
                div()
                    .id(format!("node-{index}"))
                    .w(if card_view { px(238.0) } else { px(720.0) })
                    .when(card_view, |row| row.flex_grow(1.0))
                    .min_w(px(0.0))
                    .min_h(px(50.0))
                    .px(px(ROW_X))
                    .py(px(ROW_Y))
                    .rounded(px(if card_view { 10.0 } else { 0.0 }))
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
                    .children(selected.then(|| pill("当前", CYAN)))
                    .child(
                        div()
                            .id(format!("delay-{index}"))
                            .min_w(px(74.0))
                            .flex_shrink_0()
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(9.0))
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
            .flex_col()
            .gap(px(GAP_SECTION))
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
                                    .flex()
                                    .items_center()
                                    .rounded(px(9.0))
                                    .border_1()
                                    .border_color(rgb(BORDER))
                                    .overflow_hidden()
                                    .child(self.view_mode(
                                        "node-list-view",
                                        "列表",
                                        !card_view,
                                        cx,
                                        false,
                                    ))
                                    .child(self.view_mode(
                                        "node-card-view",
                                        "卡片",
                                        card_view,
                                        cx,
                                        true,
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .children(groups.iter().enumerate().map(|(index, item)| {
                        let active = item.name == group.name;
                        div()
                            .id(format!("group-{index}"))
                            .px(px(14.0))
                            .py(px(9.0))
                            .rounded(px(10.0))
                            .bg(rgb(if active { BLUE_2 } else { SURFACE }))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(SURFACE_2)))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.group_index = index;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(px(LABEL))
                                    .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
                                    .text_color(rgb(if active { CYAN } else { MUTED }))
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
            // The group summary used to be a card of its own, repeating the
            // selected chip and a second 测试延迟 button.
            .child(
                div()
                    .px(px(4.0))
                    .text_size(px(META))
                    .text_color(rgb(MUTED))
                    .child(format!(
                        "{} · {} 个节点 · {}",
                        clean_proxy_label(&group.name),
                        group.members.len(),
                        if automatic {
                            "自动选择，点击节点不会切换"
                        } else {
                            "点击节点即可切换"
                        }
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
        let matched: Vec<(usize, &RouteRuleSnapshot)> = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                query.is_empty()
                    || rule.matcher.to_lowercase().contains(&query)
                    || rule.outbound.to_lowercase().contains(&query)
            })
            .collect();
        let visible = if self.show_all_rules {
            matched.len()
        } else {
            matched.len().min(LIST_PAGE)
        };
        let rows: Vec<gpui::AnyElement> = matched
            .iter()
            .take(visible)
            .map(|(index, rule)| rule_row(*index, rule))
            .collect();
        let rule_set_rows: Vec<gpui::AnyElement> = rule_sets
            .iter()
            .map(|set| {
                div()
                    .w_full()
                    .px(px(ROW_X))
                    .py(px(ROW_Y))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_size(px(BODY))
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(rgb(CYAN))
                            .child(set.tag.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(META))
                            .text_color(rgb(FAINT))
                            .child(set.kind.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(META))
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
            .gap(px(GAP_SECTION))
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
                    .children(if rows.is_empty() {
                        Vec::new()
                    } else {
                        vec![
                            div()
                                .px(px(ROW_X))
                                .py(px(10.0))
                                .flex()
                                .items_center()
                                .gap(px(16.0))
                                .bg(rgb(SURFACE_2))
                                .text_size(px(META))
                                .text_color(rgb(MUTED))
                                .child(div().w(px(44.0)).child("#"))
                                .child(div().flex_1().child("匹配条件"))
                                .child(div().w(px(200.0)).child("出站"))
                                .into_any_element(),
                        ]
                    })
                    .children(if rows.is_empty() {
                        vec![
                            inline_empty(
                                "暂无可读规则",
                                "启动内核或激活订阅后，这里会显示当前配置的路由规则。",
                            )
                            .into_any_element(),
                        ]
                    } else {
                        rows
                    })
                    // A real subscription ships hundreds of rules; the list
                    // starts capped and opens on demand.
                    .children((matched.len() > LIST_PAGE || self.show_all_rules).then(|| {
                        self.list_more(
                            if self.show_all_rules {
                                format!("收起，先看前 {LIST_PAGE} 条")
                            } else {
                                format!("显示全部 {} 条规则", matched.len())
                            },
                            "rules-more",
                            cx,
                            |view| view.show_all_rules = !view.show_all_rules,
                        )
                    })),
            )
    }

    /// The "show the rest" footer shared by the capped rules and connection
    /// lists.
    fn list_more(
        &self,
        label: String,
        id: &'static str,
        cx: &mut Context<Self>,
        toggle: impl Fn(&mut Sbgui) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .w_full()
            .py(px(14.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(LABEL))
            .font_weight(WEIGHT_MEDIUM)
            .text_color(rgb(CYAN))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                toggle(view);
                cx.notify();
            }))
            .child(label)
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
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(9.0))
            .bg(rgb(SURFACE_2))
            .border_1()
            .border_color(rgb(BORDER))
            .text_size(px(LABEL))
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
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(9.0))
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
        div()
            .flex()
            .flex_col()
            .gap(px(GAP_SECTION))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .text_size(px(LABEL))
                            .text_color(rgb(MUTED))
                            .child(if query.is_empty() {
                                format!("{total} 条活动连接 · 按下载流量排序")
                            } else {
                                format!("匹配 {} / {total} 条 · 按下载流量排序", connections.len())
                            }),
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
                    .child(
                        div()
                            .id("close-all")
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(9.0))
                            .bg(rgb(SURFACE))
                            .border_1()
                            .border_color(rgb(DANGER))
                            .text_size(px(LABEL))
                            .font_weight(WEIGHT_MEDIUM)
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
            // With nothing to list there is no table: the empty-state card
            // below carries the hint and the 启动内核 action.
            .when(!connections.is_empty(), |page| {
                page.child(
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
                            .p(px(PAD_CARD))
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
                                            .text_size(px(SECTION))
                                            .font_weight(WEIGHT_SEMIBOLD)
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
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(9.0))
                    .text_size(px(LABEL))
                    .cursor_pointer()
                    .bg(rgb(if active { BLUE_2 } else { SURFACE }))
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
            .gap(px(GAP_SECTION))
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
                            .children(level_chips),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
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
                                    .text_size(px(LABEL))
                                    .text_color(rgb(MUTED))
                                    .child(format!("{} 行", rows.len())),
                            ),
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
                    // The view controls used to be a third band of their own,
                    // under a panel header that only repeated the page title.
                    .child(
                        div()
                            .px(px(ROW_X))
                            .py(px(12.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_end()
                            .gap(px(8.0))
                            .border_b_1()
                            .border_color(rgb(BORDER))
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
                            .child(self.quiet_action("copy-logs", "复制", cx, move |view, cx| {
                                let _ = view;
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                            }))
                            .child(self.quiet_action("clear-logs", "清空", cx, |view, cx| {
                                // The engine owns the buffers: hiding lines
                                // by count stops working once the ring is
                                // full.
                                view.send(ClientCommand::ClearLogs);
                                view.log_rows.set(0);
                                cx.notify();
                            }))
                            .child(self.quiet_action("export-logs", "导出", cx, |view, cx| {
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
                            })),
                    )
                    .children(if rows.is_empty() {
                        Vec::new()
                    } else {
                        vec![log_table_header().into_any_element()]
                    })
                    .child(
                        div()
                            .id("log-view")
                            .h(px(if rows.is_empty() { 260.0 } else { 430.0 }))
                            .overflow_y_scroll()
                            .track_scroll(&self.log_scroll)
                            .children(if rows.is_empty() {
                                vec![
                                    inline_empty(
                                        "暂无日志",
                                        "启动内核后这里会显示 sing-box 的输出。",
                                    )
                                    .into_any_element(),
                                ]
                            } else {
                                rows
                            }),
                    ),
            )
    }

    /// A quiet bordered action, used where a full `action` button would shout.
    fn quiet_action(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(rgb(BORDER))
            .text_size(px(LABEL))
            .text_color(rgb(TEXT))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| action(view, cx)))
            .child(label)
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
                    .py(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(BODY))
                            .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
                            .text_color(rgb(TEXT))
                            .truncate()
                            .child(profile.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(META))
                            .text_color(rgb(MUTED))
                            .child(age_label(profile.last_updated)),
                    )
                    .child(if active {
                        // The active archive is already in use; a second
                        // "激活" button here would be a no-op.
                        div()
                            .px(px(8.0))
                            .text_size(px(META))
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
                        .mt(px(8.0))
                        .text_size(px(LABEL))
                        .line_height(px(19.0))
                        .text_color(rgb(MUTED))
                        .child("当前订阅与本地配置档案。订阅的添加、更新和删除请前往订阅页。"),
                )
                .child(div().mt(px(16.0)).flex().flex_col().children(profile_rows))
                .children(profiles.is_empty().then(|| {
                    div()
                        .mt(px(16.0))
                        .text_size(px(LABEL))
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
                .child(div().mt(px(20.0)).child(self.action(
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
                        .mt(px(16.0))
                        .text_size(px(LABEL))
                        .line_height(px(19.0))
                        .text_color(rgb(AMBER))
                        .child("修改高级配置前请停止内核，并保留可恢复的配置副本。"),
                ),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(GAP_SECTION))
            // A settings row is a label and its value; the measure is capped
            // so both stay inside one eye sweep instead of drifting apart.
            .max_w(px(780.0))
            .child(div().flex().flex_wrap().gap(px(8.0)).children(
                SettingsSection::all().into_iter().map(|section| {
                    let active = section == self.settings_section;
                    div()
                        .id(format!("settings-{:?}", section))
                        .px(px(14.0))
                        .py(px(9.0))
                        .rounded(px(10.0))
                        .bg(rgb(if active { BLUE_2 } else { SURFACE }))
                        .text_size(px(LABEL))
                        .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
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
                    .px(px(16.0))
                    .py(px(12.0))
                    .rounded(px(10.0))
                    .bg(rgb(0xfff7ed))
                    .border_1()
                    .border_color(rgb(0xfed7aa))
                    .text_size(px(LABEL))
                    .text_color(rgb(AMBER))
                    .child(format!(
                        "有 {dirty_count} 项更改尚未保存；端口或内核配置可能需要重启后生效。"
                    ))
            }))
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("save-settings")
                        .px(px(18.0))
                        .py(px(10.0))
                        .rounded(px(9.0))
                        .bg(rgb(CYAN))
                        .text_size(px(LABEL))
                        .font_weight(WEIGHT_MEDIUM)
                        .text_color(rgb(SURFACE))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(CYAN_DARK)))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_settings(cx)))
                        .child("保存更改"),
                ),
            )
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
                                    let snapshot = view.controller.snapshot();
                                    // Repainting unconditionally kept a window
                                    // redrawing, decoding icons and resampling
                                    // the graph four times a second while nothing
                                    // had changed — and while it was hidden.
                                    let changed = snapshot != view.snapshot;
                                    if changed {
                                        view.snapshot = snapshot;
                                    }
                                    let minute = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|elapsed| elapsed.as_secs() / 60)
                                        .unwrap_or(view.painted_minute);
                                    if changed || minute != view.painted_minute {
                                        view.painted_minute = minute;
                                        cx.notify();
                                    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
