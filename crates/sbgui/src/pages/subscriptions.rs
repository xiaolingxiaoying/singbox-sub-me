//! The subscription page: profiles, import and updates.

use client_core::ClientCommand;
use client_core::format::human_bytes;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{
    empty_state, icon, page_head, pill, table_col, table_head_row, work_surface,
};
use crate::lang::{Locale, age_label};
use crate::parse::ImportReject;
use crate::state::{FieldSpec, InputField, Sbgui, Tone};
use crate::theme::{
    BODY, BORDER, BORDER_STRONG, CYAN, DANGER, FAINT, LABEL, META, MINT, MUTED, RADIUS_CONTROL,
    ROW_HOVER, ROW_SELECTED, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM,
};
use crate::tr;

/// The name column carries two lines and the usage column a bar, so both get a
/// share of the leftover width instead of a fixed track.
fn grow_col(grow: f32, min_w: f32) -> gpui::Div {
    div().flex_grow(grow).min_w(px(min_w))
}

/// One row's quota bar: two flex children weighted by the used and the unused
/// share, because GPUI has no percentage width.
fn usage_bar(used: u64, total: u64) -> impl IntoElement {
    div()
        .mt(px(6.0))
        .h(px(6.0))
        .w_full()
        .flex()
        .rounded(px(RADIUS_CONTROL))
        .bg(rgb(BORDER))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .flex_grow(used.min(total) as f32)
                .bg(rgb(CYAN)),
        )
        .child(div().h_full().flex_grow(total.saturating_sub(used) as f32))
}

/// One refusal line under the field it refuses. The panel stays open with it,
/// which is what an empty field answers with since issue 02 item 4.
fn rejection(locale: Locale, reject: ImportReject) -> gpui::Div {
    div()
        .mt(px(10.0))
        .text_size(px(LABEL))
        .line_height(px(19.0))
        .text_color(rgb(DANGER))
        .child(reject.label(locale))
}

impl Sbgui {
    // --------------------------------------------------------- subscriptions

    pub(crate) fn subscriptions(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let profiles = self.snapshot.profiles.clone();
        let locale = self.locale;
        // Below roughly 940px of window the fixed "last updated" column and the
        // original minimum track widths no longer leave room for the action
        // buttons, which used to be clipped off the right edge of the table.
        // Drop that column and let the two text tracks shrink; the horizontal
        // scroller around the table is the hard guarantee.
        let narrow = window.viewport_size().width < px(940.0);
        // The action cell is fixed so the header lines up with the rows: its
        // content is wider than the "Actions" label, and an auto width would
        // shift the grow columns per row. The track has to fit the widest *pair*
        // in either language — 设为当前 + 编辑链接 / "Set as current" + "Edit
        // link" — because `flex_wrap` only breaks lines inside the track, and a
        // track narrower than the pair overflows the card and cuts the last
        // glyph off (issue 01 item 8, seen at 860×640).
        let action_width = if narrow { 250.0 } else { 268.0 };
        let usage = self.snapshot.subscription_usage;
        let node_count = self
            .snapshot
            .proxy_groups
            .iter()
            .map(|group| group.members.len())
            .sum::<usize>();

        let head_actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(9.0))
            .child(self.button(
                "add-subscription",
                tr!(locale, "添加订阅", "Add subscription"),
                Tone::Accent,
                Some("plus"),
                cx,
                |view, cx| {
                    view.show_subscription_import = true;
                    // A panel opened afresh starts without the last complaint.
                    view.subscription_error = None;
                    cx.notify();
                },
            ))
            .child(self.button(
                "import-from-clipboard",
                tr!(locale, "从剪贴板导入", "Import from clipboard"),
                Tone::Neutral,
                None,
                cx,
                |view, cx| {
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
                },
            ))
            .child(self.action(
                "update-all-subscriptions",
                tr!(locale, "全部更新", "Update all"),
                Tone::Neutral,
                cx,
                ClientCommand::UpdateSubscription,
            ));

        let mut surface = work_surface().child(
            page_head(tr!(
                locale,
                "管理订阅链接，更新节点列表与切换档案。",
                "Manage subscription links, update their nodes and switch profiles.",
            ))
            .child(head_actions),
        );

        if self.show_subscription_import {
            surface = surface.child(
                div()
                    .mt(px(18.0))
                    .p(px(14.0))
                    .rounded(px(RADIUS_CONTROL + 2.0))
                    .bg(rgb(SURFACE_2))
                    .border_1()
                    .border_color(rgb(BORDER_STRONG))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(BODY))
                                    .font_weight(WEIGHT_MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .child(tr!(locale, "添加订阅", "Add subscription")),
                            )
                            .child(
                                div()
                                    .id("close-subscription-import")
                                    .size(px(34.0))
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .rounded(px(RADIUS_CONTROL))
                                    .hover(|s| s.bg(rgb(ROW_HOVER)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.show_subscription_import = false;
                                        view.subscription_error = None;
                                        cx.notify();
                                    }))
                                    .child(icon("close", MUTED, 14.0)),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_size(px(LABEL))
                            .text_color(rgb(MUTED))
                            .child(tr!(
                                locale,
                                "粘贴 HTTP/HTTPS 订阅地址，或写一个本地 sing-box JSON 文件的路径。",
                                "Paste an HTTP/HTTPS subscription or sing-box JSON URL, or write the path of a local sing-box JSON file.",
                            )),
                    )
                    .child(
                        div()
                            .mt(px(14.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.0))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::SubUrl,
                                    id: "sub-url",
                                    placeholder: "https://…",
                                    width: 460.0,
                                },
                                window,
                                cx,
                            ))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::SubName,
                                    id: "sub-name",
                                    placeholder: tr!(locale, "档案名称（可选）", "Profile name (optional)"),
                                    width: 200.0,
                                },
                                window,
                                cx,
                            ))
                            .child(self.button(
                                "import-manual",
                                tr!(locale, "添加", "Add"),
                                Tone::Accent,
                                None,
                                cx,
                                |view, cx| {
                                    // The panel closes when the link was taken and
                                    // stays open when it was not: an empty field
                                    // used to close it too, which read as the
                                    // client eating what the user typed.
                                    if view.submit_sub_url(cx) {
                                        view.show_subscription_import = false;
                                    }
                                },
                            )),
                    )
                    .children(self.subscription_error.map(|reject| rejection(locale, reject)))
                    .child(
                        div()
                            .mt(px(10.0))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.0))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::SubFile,
                                    id: "sub-file",
                                    placeholder: tr!(
                                        locale,
                                        "本地 JSON 文件路径，如 C:\\sbctl\\local.json",
                                        "Local JSON path, e.g. C:\\sbctl\\local.json"
                                    ),
                                    width: 460.0,
                                },
                                window,
                                cx,
                            ))
                            .child(self.button(
                                "import-local-file",
                                tr!(locale, "导入本地 JSON", "Import local JSON"),
                                Tone::Neutral,
                                None,
                                cx,
                                |view, cx| {
                                    // Taken-or-not is the same rule as the link's:
                                    // a blank path keeps the panel open and says so.
                                    if view.submit_sub_file(cx) {
                                        view.show_subscription_import = false;
                                    }
                                },
                            )),
                    )
                    .children(self.import_file_error.map(|reject| rejection(locale, reject))),
            );
        }

        // The link editor belongs above the table it came out of, so the row that
        // opened it and the field that edits it stay in one glance.
        surface = surface.children(self.profile_url_editor(window, cx));

        if profiles.is_empty() {
            return surface.child(empty_state(
                tr!(locale, "还没有订阅", "No subscriptions yet"),
                tr!(
                    locale,
                    "点击「添加订阅」，或从剪贴板导入订阅链接。",
                    "Use Add subscription, or import a link from the clipboard.",
                ),
                None,
                cx,
            ));
        }

        let rows: Vec<gpui::AnyElement> = profiles
            .into_iter()
            .enumerate()
            .map(|(index, profile)| {
                let active = profile.active;
                let name_for_activate = profile.name.clone();
                let name_for_remove = profile.name.clone();
                let name_for_edit = profile.name.clone();
                let armed = self
                    .confirm_delete_profile
                    .as_deref()
                    .is_some_and(|pending| pending == name_for_remove);
                let used = usage.as_ref().map_or(0, |item| item.used());
                let total = usage.as_ref().map_or(0, |item| item.total);
                let usage_cell: Vec<gpui::AnyElement> = if active {
                    let quota: Vec<gpui::AnyElement> = vec![
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.0))
                            .text_size(px(LABEL))
                            .child(
                                div()
                                    .font_weight(WEIGHT_MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .truncate()
                                    .child(if total > 0 {
                                        format!("{} / {}", human_bytes(used), human_bytes(total))
                                    } else {
                                        tr!(locale, "未设置配额", "No quota set").to_owned()
                                    }),
                            )
                            .children((total > 0).then(|| {
                                div()
                                    .text_color(rgb(MUTED))
                                    .child(format!("{:.1}%", used as f64 / total as f64 * 100.0))
                            }))
                            .into_any_element(),
                        div()
                            .mt(px(5.0))
                            .text_size(px(META))
                            .text_color(rgb(MUTED))
                            .child(tr!(
                                locale,
                                format!("{} 个节点", node_count),
                                format!("{} nodes", node_count)
                            ))
                            .into_any_element(),
                    ];
                    // The bar only earns its row when the provider actually
                    // sent a quota; otherwise it would be an empty track under
                    // "未设置配额".
                    let mut cell = quota;
                    if total > 0 {
                        cell.insert(1, usage_bar(used, total).into_any_element());
                    }
                    cell
                } else {
                    vec![
                        div()
                            .text_size(px(META))
                            .text_color(rgb(MUTED))
                            .child(tr!(
                                locale,
                                "未启用时无用量信息",
                                "No usage reported while inactive"
                            ))
                            .into_any_element(),
                    ]
                };
                div()
                    .id(format!("subscription-row-{index}"))
                    .w_full()
                    .min_h(px(72.0))
                    .px(px(14.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .text_size(px(LABEL))
                    .bg(rgb(if active { ROW_SELECTED } else { SURFACE }))
                    .hover(|row| row.bg(rgb(if active { ROW_SELECTED } else { ROW_HOVER })))
                    .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
                    // Name over its link: the URL is what a user checks when a
                    // subscription stops updating, and it never fits in one
                    // line, so it truncates rather than wrapping.
                    .child(
                        grow_col(1.4, if narrow { 120.0 } else { 200.0 })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .text_size(px(BODY))
                                            .font_weight(WEIGHT_MEDIUM)
                                            .text_color(rgb(if active { CYAN } else { TEXT }))
                                            .truncate()
                                            .child(profile.name.clone()),
                                    )
                                    .children(
                                        active.then(|| pill(tr!(locale, "当前", "Active"), CYAN)),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .text_size(px(META))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(profile.url.clone()),
                            ),
                    )
                    .child(
                        div()
                            .w(px(if narrow { 64.0 } else { 72.0 }))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_size(px(META))
                            .text_color(rgb(if active { MINT } else { MUTED }))
                            .child(
                                div()
                                    .size(px(7.0))
                                    .flex_shrink_0()
                                    .rounded(px(4.0))
                                    .bg(rgb(if active { MINT } else { FAINT })),
                            )
                            .child(if active {
                                tr!(locale, "使用中", "In use")
                            } else {
                                tr!(locale, "未启用", "Inactive")
                            }),
                    )
                    .child(grow_col(1.2, if narrow { 130.0 } else { 170.0 }).children(usage_cell))
                    .when(!narrow, |row| {
                        row.child(
                            div()
                                .w(px(72.0))
                                .flex_shrink_0()
                                .text_size(px(META))
                                .text_color(rgb(MUTED))
                                .child(age_label(profile.last_updated, locale)),
                        )
                    })
                    .child(
                        div()
                            .w(px(action_width))
                            .flex()
                            .flex_shrink_0()
                            .flex_wrap()
                            .items_center()
                            .gap(px(9.0))
                            .children((!active).then(|| {
                                self.mini_action(
                                    index + 300_000,
                                    tr!(locale, "设为当前", "Use this"),
                                    cx,
                                    ClientCommand::SwitchProfile(name_for_activate.clone()),
                                )
                                .into_any_element()
                            }))
                            .children(active.then(|| {
                                self.mini_action(
                                    index + 100_000,
                                    tr!(locale, "更新", "Update"),
                                    cx,
                                    ClientCommand::UpdateSubscription,
                                )
                                .into_any_element()
                            }))
                            .child(self.button(
                                "edit-profile-url",
                                tr!(locale, "编辑链接", "Edit link"),
                                Tone::Neutral,
                                None,
                                cx,
                                move |view, cx| {
                                    // Arming is the whole click: the editor panel
                                    // above the table holds the link, and the same
                                    // button on the open row closes it again.
                                    view.arm_url_editor(&name_for_edit, cx);
                                },
                            ))
                            .child(self.button(
                                if armed {
                                    "confirm-remove-profile"
                                } else {
                                    "remove-profile"
                                },
                                if armed {
                                    tr!(locale, "确认删除？", "Confirm delete?")
                                } else {
                                    tr!(locale, "删除", "Delete")
                                },
                                Tone::Danger,
                                None,
                                cx,
                                move |view, cx| {
                                    // Deleting also drops the cached node list,
                                    // so the first click only arms the button —
                                    // the same shape as 关闭全部.
                                    if view.confirm_delete_profile.as_deref()
                                        == Some(name_for_remove.as_str())
                                    {
                                        view.send(ClientCommand::RemoveProfile(
                                            name_for_remove.clone(),
                                        ));
                                        view.confirm_delete_profile = None;
                                    } else {
                                        view.confirm_delete_profile = Some(name_for_remove.clone());
                                    }
                                    cx.notify();
                                },
                            )),
                    )
                    .into_any_element()
            })
            .collect();

        surface.child(
            div()
                .mt(px(18.0))
                .w_full()
                .rounded(px(RADIUS_CONTROL + 2.0))
                .border_1()
                .border_color(rgb(BORDER))
                .overflow_hidden()
                .child(
                    div()
                        .id("subscriptions-horizontal")
                        .w_full()
                        .overflow_x_scroll()
                        .child(
                            div()
                                .min_w(px(600.0))
                                .child(
                                    table_head_row()
                                        .child(
                                            grow_col(1.4, if narrow { 120.0 } else { 200.0 })
                                                .child(tr!(locale, "名称", "Name")),
                                        )
                                        .child(table_col(
                                            tr!(locale, "状态", "Status"),
                                            Some(if narrow { 64.0 } else { 72.0 }),
                                        ))
                                        .child(
                                            grow_col(1.2, if narrow { 130.0 } else { 170.0 })
                                                .child(tr!(locale, "流量与节点", "Usage & nodes")),
                                        )
                                        .when(!narrow, |head| {
                                            head.child(table_col(
                                                tr!(locale, "上次更新", "Last updated"),
                                                Some(72.0),
                                            ))
                                        })
                                        .child(
                                            div()
                                                .w(px(action_width))
                                                .flex_shrink_0()
                                                .child(tr!(locale, "操作", "Actions")),
                                        ),
                                )
                                .children(rows),
                        ),
                ),
        )
    }

    /// The panel one row's 「编辑链接」 opens: that profile's subscription link,
    /// prefilled with what is stored, committed through
    /// [`ClientCommand::SetProfileUrl`] instead of importing a second profile.
    /// It renders only while the profile it was armed on still exists, so a
    /// profile deleted elsewhere takes its own editor with it.
    fn profile_url_editor(&self, window: &Window, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let name = self.editing_profile_url.as_deref()?;
        if !self
            .snapshot
            .profiles
            .iter()
            .any(|profile| profile.name == name)
        {
            return None;
        }
        let locale = self.locale;
        Some(
            div()
                .mt(px(18.0))
                .p(px(14.0))
                .rounded(px(RADIUS_CONTROL + 2.0))
                .bg(rgb(SURFACE_2))
                .border_1()
                .border_color(rgb(BORDER_STRONG))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .w_full()
                                .text_size(px(BODY))
                                .font_weight(WEIGHT_MEDIUM)
                                .text_color(rgb(TEXT))
                                .child(tr!(
                                    locale,
                                    format!("编辑订阅链接：{name}"),
                                    format!("Edit subscription link: {name}")
                                )),
                        )
                        .child(self.text_field(
                            FieldSpec {
                                field: InputField::SubEditUrl,
                                id: "sub-edit-url",
                                placeholder: tr!(locale, "新的订阅链接", "New subscription link"),
                                width: 460.0,
                            },
                            window,
                            cx,
                        ))
                        .child(self.button(
                            "save-profile-url",
                            tr!(locale, "保存链接", "Save link"),
                            Tone::Accent,
                            None,
                            cx,
                            |view, cx| {
                                // Taken-or-not is the import panel's rule again: a
                                // refused link keeps the editor open with its
                                // reason, so nothing the user typed disappears.
                                if view.submit_profile_url(cx) {
                                    view.close_url_editor();
                                }
                            },
                        ))
                        .child(self.button(
                            "cancel-profile-url",
                            tr!(locale, "取消", "Cancel"),
                            Tone::Neutral,
                            None,
                            cx,
                            |view, cx| {
                                view.close_url_editor();
                                cx.notify();
                            },
                        )),
                )
                .child(
                    div()
                        .mt(px(8.0))
                        .text_size(px(LABEL))
                        .text_color(rgb(MUTED))
                        .child(tr!(
                            locale,
                            "只改这一个档案的链接；要清空请删除该档案。",
                            "This rewrites only this profile's link; delete the profile to drop it."
                        )),
                )
                .children(
                    self.profile_url_error
                        .map(|reject| rejection(locale, reject)),
                ),
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
            .min_h(px(34.0))
            .px(px(12.0))
            .rounded(px(RADIUS_CONTROL))
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(BORDER_STRONG))
            .flex()
            .items_center()
            .text_size(px(LABEL))
            .font_weight(WEIGHT_MEDIUM)
            .text_color(rgb(TEXT))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(ROW_HOVER)).border_color(rgb(CYAN)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.send(command.clone());
                cx.notify();
            }))
            .child(label)
    }
}
