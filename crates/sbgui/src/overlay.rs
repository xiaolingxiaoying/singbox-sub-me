//! The exit-confirmation modal. Nothing about the OS proxy is restored
//! silently: the user picks before the window goes away.

use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, rgb, rgba,
};

use crate::state::{ExitChoice, Sbgui, Tone};
use crate::theme::{
    BODY, BORDER, CYAN_DARK, DANGER, LABEL, MUTED, RADIUS, RADIUS_CONTROL, SURFACE, SURFACE_2,
    TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD, tone_colors,
};

impl Sbgui {
    /// Modal confirmation shown when the window closes while the OS proxy is
    /// still enabled. Nothing is restored silently: the user picks.
    pub(crate) fn exit_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            // A hitbox only exists for an identified element; without this the
            // overlay looks modal while the buttons underneath still answer
            // clicks.
            .id("exit-modal")
            .occlude()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .bg(rgba(0x00000073))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(480.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .p(px(26.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(WEIGHT_SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("退出 Serein？"),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(BODY))
                            .line_height(px(21.0))
                            .text_color(rgb(MUTED))
                            .child("退出会停止 sing-box。若保留系统代理，其他应用可能无法联网。"),
                    )
                    .child(
                        div()
                            .mt(px(6.0))
                            .text_size(px(LABEL))
                            .text_color(rgb(DANGER))
                            .child("选择「仅退出」后，系统代理设置将保留。"),
                    )
                    .child(
                        div()
                            .mt(px(16.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .id("exit-cancel")
                                    .px(px(16.0))
                                    .py(px(9.0))
                                    .rounded(px(9.0))
                                    .text_size(px(LABEL))
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(SURFACE_2)).text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.confirm_exit = false;
                                        cx.notify();
                                    }))
                                    .child("取消"),
                            )
                            .child(self.exit_button(
                                "exit-keep",
                                "仅退出",
                                Tone::Neutral,
                                cx,
                                ExitChoice::Keep,
                            ))
                            .child(self.exit_button(
                                "exit-restore",
                                "关闭系统代理并退出",
                                Tone::Accent,
                                cx,
                                ExitChoice::Restore,
                            )),
                    ),
            )
    }

    fn exit_button(
        &self,
        id: &'static str,
        label: &'static str,
        tone: Tone,
        cx: &mut Context<Self>,
        choice: ExitChoice,
    ) -> impl IntoElement {
        let (fg, bg, edge) = tone_colors(tone);
        div()
            .id(id)
            .px(px(16.0))
            .py(px(9.0))
            .rounded(px(9.0))
            .bg(rgb(bg))
            .border_1()
            .border_color(rgb(edge))
            .text_size(px(LABEL))
            .font_weight(WEIGHT_MEDIUM)
            .text_color(rgb(fg))
            .cursor_pointer()
            .hover(move |style| {
                style
                    .bg(rgb(if matches!(tone, Tone::Accent) {
                        CYAN_DARK
                    } else {
                        edge
                    }))
                    .text_color(rgb(fg))
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, window, _| {
                view.choose_exit(choice, window);
            }))
            .child(label)
    }

    /// Modal confirmation for stopping the core. The kit requires an explicit
    /// confirmation before this state change: proxied connections drop, and
    /// with the system proxy on, other applications lose their outlet too.
    pub(crate) fn stop_core_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let detail = if self.snapshot.system_proxy_enabled {
            "系统代理仍开着，其他应用会一起失去代理出口。"
        } else {
            "出站模式、节点与订阅设置都会保留，随时可以重新启动。"
        };
        div()
            .id("stop-core-modal")
            // Without an identified, occluding root the page underneath would
            // still answer clicks while the dialog looks modal.
            .occlude()
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .bg(rgba(0x00000073))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(480.0))
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .p(px(26.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(WEIGHT_SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child("停止内核？"),
                    )
                    .child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(BODY))
                            .line_height(px(21.0))
                            .text_color(rgb(MUTED))
                            .child(format!("停止会断开当前所有代理连接。{detail}")),
                    )
                    .child(
                        div()
                            .mt(px(16.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .id("stop-core-cancel")
                                    .min_h(px(34.0))
                                    .px(px(16.0))
                                    .py(px(9.0))
                                    .rounded(px(RADIUS_CONTROL))
                                    .text_size(px(LABEL))
                                    .text_color(rgb(MUTED))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(SURFACE_2)).text_color(rgb(TEXT)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.answer_stop_core(false, cx);
                                    }))
                                    .child("取消"),
                            )
                            .child(
                                div()
                                    .id("stop-core-confirm")
                                    .min_h(px(34.0))
                                    .px(px(16.0))
                                    .py(px(9.0))
                                    .rounded(px(RADIUS_CONTROL))
                                    .bg(rgb(DANGER))
                                    .text_size(px(LABEL))
                                    .font_weight(WEIGHT_MEDIUM)
                                    .text_color(rgb(SURFACE))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(0x8c3232)))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.answer_stop_core(true, cx);
                                    }))
                                    .child("停止内核"),
                            ),
                    ),
            )
    }
}
