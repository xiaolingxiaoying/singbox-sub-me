//! The log page: the level-filtered log tail.

use client_core::ClientCommand;
use client_core::state::LogLevel;
use gpui::{
    ClickEvent, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{inline_empty, log_row, log_table_header};
use crate::state::{FieldSpec, InputField, LogLevelFilter, Sbgui};
use crate::theme::{
    BLUE_2, BORDER, CYAN, GAP_SECTION, LABEL, MUTED, RADIUS, ROW_X, SURFACE, SURFACE_2, TEXT,
};

impl Sbgui {
    // ----------------------------------------------------------------- logs

    pub(crate) fn logs(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
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
