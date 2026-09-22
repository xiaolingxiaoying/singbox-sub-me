//! The rule page: route rules and rule sets.

use client_core::ClientCommand;
use client_core::state::RouteRuleSnapshot;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{
    accordion, inline_empty, page_head, rule_row, status_dot, table_col, table_head_row,
    work_surface,
};
use crate::state::{FieldSpec, InputField, Sbgui, Tone};
use crate::theme::{
    BODY, BORDER, CYAN, FAINT, LABEL, LIST_PAGE, META, MINT, MUTED, RADIUS_CONTROL, ROW_HOVER,
    SURFACE, WEIGHT_MEDIUM,
};

impl Sbgui {
    // --------------------------------------------------------------- rules

    pub(crate) fn rules(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
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

        work_surface()
            .child(
                page_head("当前配置里的路由规则：每条决定匹配的流量走哪个出站。").child(
                    self.action(
                        "refresh-rules",
                        "刷新状态",
                        Tone::Neutral,
                        cx,
                        ClientCommand::Refresh,
                    ),
                ),
            )
            // The statement of what is loaded on the left, the tool that narrows
            // it on the right — the kit's rule bar, so the count is never
            // competing with the search box for the same line.
            .child(
                div()
                    .mt(px(18.0))
                    .mb(px(18.0))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_dot(if count > 0 { MINT } else { FAINT }))
                            .child(div().text_size(px(LABEL)).text_color(rgb(MUTED)).child(
                                if count > 0 {
                                    if query.is_empty() {
                                        format!("当前配置 · {count} 条规则")
                                    } else {
                                        format!("匹配 {} / {count} 条规则", matched.len())
                                    }
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
                            width: 280.0,
                        },
                        window,
                        cx,
                    )),
            )
            // Rule sets are the least-read thing on the page, so they disclose
            // on demand instead of pushing the table down.
            .children((!rule_sets.is_empty()).then(|| {
                accordion(
                    "rule-sets-toggle",
                    "规则集",
                    format!("{} 个 · 展开查看来源", rule_sets.len()),
                    self.show_rule_sets,
                    cx,
                    |view| view.show_rule_sets = !view.show_rule_sets,
                    div()
                        .flex()
                        .flex_col()
                        .children(rule_sets.iter().enumerate().map(|(index, set)| {
                            div()
                                .id(format!("rule-set-{index}"))
                                .w_full()
                                .min_h(px(39.0))
                                .py(px(8.0))
                                .flex()
                                .items_center()
                                .gap(px(10.0))
                                .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
                                .child(
                                    div()
                                        .w(px(180.0))
                                        .flex_shrink_0()
                                        .truncate()
                                        .text_size(px(BODY))
                                        .font_weight(WEIGHT_MEDIUM)
                                        .text_color(rgb(CYAN))
                                        .child(set.tag.clone()),
                                )
                                .child(
                                    div()
                                        .w(px(64.0))
                                        .flex_shrink_0()
                                        .text_size(px(META))
                                        .text_color(rgb(FAINT))
                                        .child(set.kind.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .text_size(px(META))
                                        .text_color(rgb(MUTED))
                                        .child(if set.url.is_empty() {
                                            "（本地规则集）".to_owned()
                                        } else {
                                            set.url.clone()
                                        }),
                                )
                        })),
                )
            }))
            .child(
                div()
                    .id("rules-panel")
                    .w_full()
                    .rounded(px(RADIUS_CONTROL + 2.0))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        table_head_row()
                            .child(table_col("#", Some(40.0)))
                            .child(table_col("类型", Some(96.0)))
                            .child(table_col("匹配条件", None))
                            .child(table_col("出站", Some(170.0))),
                    )
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
    pub(crate) fn list_more(
        &self,
        label: String,
        id: &'static str,
        cx: &mut Context<Self>,
        toggle: impl Fn(&mut Sbgui) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .w_full()
            .min_h(px(44.0))
            .flex()
            .items_center()
            .justify_center()
            .border_t_1()
            .border_color(rgb(BORDER))
            .bg(rgb(SURFACE))
            .text_size(px(LABEL))
            .font_weight(WEIGHT_MEDIUM)
            .text_color(rgb(CYAN))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(ROW_HOVER)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                toggle(view);
                cx.notify();
            }))
            .child(label)
    }
}
