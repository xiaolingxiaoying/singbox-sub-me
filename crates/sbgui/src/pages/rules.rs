//! The rule page: route rules and rule sets.

use client_core::ClientCommand;
use client_core::state::RouteRuleSnapshot;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{inline_empty, rule_row, status_dot};
use crate::state::{FieldSpec, InputField, Sbgui, Tone};
use crate::theme::{
    BODY, BORDER, CYAN, FAINT, GAP_SECTION, LABEL, LIST_PAGE, META, MINT, MUTED, RADIUS, ROW_X,
    ROW_Y, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM,
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
}
