//! The subscription page: profiles, import and updates.

use client_core::ClientCommand;
use client_core::format::{age_label, usage_label};
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{empty_state, icon, pill};
use crate::state::{FieldSpec, InputField, Sbgui, Tone};
use crate::theme::{
    BLUE_2, BORDER, CYAN, CYAN_DARK, DANGER, GAP_SECTION, LABEL, META, MUTED, PAD_CARD, RADIUS,
    SECTION, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
};

impl Sbgui {
    // --------------------------------------------------------- subscriptions

    pub(crate) fn subscriptions(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
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

    pub(crate) fn mini_action(
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
}
