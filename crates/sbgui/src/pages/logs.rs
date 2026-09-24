//! The log page: the level-filtered log tail.

use client_core::ClientCommand;
use client_core::state::{EventLine, LogLevel};
use gpui::{
    ClickEvent, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{inline_empty, log_row, log_table_header, page_head, work_surface};
use crate::lang::Locale;
use crate::state::{FieldSpec, InputField, LogLevelFilter, Sbgui, Tone};
use crate::theme::{BLUE_2, BORDER, CYAN, LABEL, MUTED, RADIUS, RADIUS_CONTROL, SURFACE};
use crate::tr;

/// The one-line form of an event entry in `locale`. A coded record renders from
/// its [`client_core::event_code::EventCode`], so an English interface shows
/// English; a bare string from a call site issue 02 has not converted yet is
/// shown verbatim.
pub(crate) fn localised_event_line(line: &EventLine<'_>, locale: Locale) -> String {
    match (line.record, locale) {
        (Some(record), Locale::En) => record.render_en(),
        (Some(record), Locale::Zh) => record.render_zh(),
        (None, _) => line.text.to_owned(),
    }
}

/// How many lines of each stream the page paints: the newest kernel lines and
/// the newest client events. Both are windows over longer buffers, so the
/// painted row count stops growing here long before the log does.
const KERNEL_ROWS: usize = 180;
const EVENT_ROWS: usize = 60;

/// Which row the log page pinned "自动滚动" to on the previous paint.
///
/// The engine keeps rings (500 kernel lines, 200 events) and the page paints at
/// most [`KERNEL_ROWS`] + [`EVENT_ROWS`] rows, so once either is full the row
/// count can never grow again — a follow test built on the count silently stops
/// firing exactly when the log is busy, which is when tailing matters. The last
/// row's own identity — which stream it came from and which line it carries —
/// keeps telling a new line apart from a repaint, which is how the terminal
/// client's `log_window` renders the newest rows rather than counting them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogTail {
    /// Rows the page paints, i.e. the index of the last one plus one.
    rows: usize,
    /// The stream the last row came from, so an event and a kernel line that
    /// happen to read the same are still different rows.
    source: &'static str,
    /// The last row's text.
    line: String,
}

impl LogTail {
    /// The tail the page is about to paint from these two filtered streams.
    fn new(kernel: &[String], events: &[String], event_source: &'static str) -> Self {
        let shown_kernel = kernel.len().min(KERNEL_ROWS);
        let shown_events = events.len().min(EVENT_ROWS);
        // Events are painted after the kernel rows, so the last row is an event
        // whenever any event shows.
        let (source, line) = if shown_events > 0 {
            (event_source, events.last())
        } else {
            ("sing-box", kernel.last())
        };
        Self {
            rows: shown_kernel + shown_events,
            source,
            line: line.cloned().unwrap_or_default(),
        }
    }

    /// The row to pin the view to, or `None` while nothing is painted.
    fn last_row(&self) -> Option<usize> {
        self.rows.checked_sub(1)
    }
}

/// The row "自动滚动" jumps to on this paint, or `None` when the scroll must be
/// left alone: follow is off, or no new row arrived. The very first paint has
/// nothing pinned yet, so it follows to wherever the newest line is.
fn follow_target(seen: Option<&LogTail>, tail: &LogTail, follow: bool) -> Option<usize> {
    let repaint = seen == Some(tail);
    if !follow || repaint {
        return None;
    }
    tail.last_row()
}

impl Sbgui {
    // ----------------------------------------------------------------- logs

    pub(crate) fn logs(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let locale = self.locale;
        // The source column and the copy/export prefix name the same stream, so
        // they share one label.
        let client_source = tr!(locale, "客户端", "Client");
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
            .event_lines()
            .into_iter()
            .filter(|line| keep(line.text))
            .map(|line| localised_event_line(&line, locale))
            .collect();
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for (index, line) in kernel.iter().rev().take(KERNEL_ROWS).rev().enumerate() {
            rows.push(log_row(index, "sing-box", line, self.log_wrap));
        }
        let event_offset = rows.len();
        for (index, line) in events.iter().rev().take(EVENT_ROWS).rev().enumerate() {
            rows.push(log_row(
                event_offset + index,
                client_source,
                line,
                self.log_wrap,
            ));
        }
        // "自动滚动" pins the view to the newest line whenever a line arrived.
        // Scrolling on a paint that brought nothing new would fight the user's
        // wheel, which is why the guard compares the tail row itself rather than
        // the row count: see [`LogTail`].
        let tail = LogTail::new(&kernel, &events, client_source);
        let seen = self.log_tail.take();
        let pin_to = follow_target(seen.as_ref(), &tail, self.log_follow);
        self.log_tail.set(Some(tail));
        if let Some(row) = pin_to {
            self.log_scroll.scroll_to_item(row);
        }
        let copy_text = kernel
            .iter()
            .map(|line| format!("[sing-box] {line}"))
            .chain(
                events
                    .iter()
                    .map(|line| format!("[{client_source}] {line}")),
            )
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
                    .min_h(px(34.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(RADIUS_CONTROL))
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
                    .child(candidate.label(locale))
                    .into_any_element(),
            );
        }

        // The view controls (follow / wrap / copy / clear / export) are what the
        // page does, so they sit in the head beside the sentence; the level chips
        // and the search are how the list is narrowed, so they share one row
        // above the table — the same spine the rules page uses.
        work_surface()
            .child(
                page_head(tr!(
                    locale,
                    "内核运行日志与客户端事件，按级别与关键字筛选。",
                    "Core log lines and client events, filtered by level and keyword.",
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(self.utility_toggle(
                            "log-follow",
                            if self.log_follow {
                                tr!(locale, "自动滚动：开", "Auto-scroll: on")
                            } else {
                                tr!(locale, "自动滚动：关", "Auto-scroll: off")
                            },
                            self.log_follow,
                            cx,
                            |view| view.log_follow = !view.log_follow,
                        ))
                        .child(self.utility_toggle(
                            "log-wrap",
                            if self.log_wrap {
                                tr!(locale, "自动换行：开", "Word wrap: on")
                            } else {
                                tr!(locale, "自动换行：关", "Word wrap: off")
                            },
                            self.log_wrap,
                            cx,
                            |view| view.log_wrap = !view.log_wrap,
                        ))
                        .child(self.button(
                            "copy-logs",
                            tr!(locale, "复制", "Copy"),
                            Tone::Neutral,
                            None,
                            cx,
                            move |view, cx| {
                                let _ = view;
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                            },
                        ))
                        .child(self.button(
                            "clear-logs",
                            tr!(locale, "清空", "Clear"),
                            Tone::Neutral,
                            None,
                            cx,
                            |view, cx| {
                                // The engine owns the buffers: hiding lines
                                // by count stops working once the ring is full.
                                view.send(ClientCommand::ClearLogs);
                                view.log_tail.set(None);
                                cx.notify();
                            },
                        ))
                        .child(self.button(
                            "export-logs",
                            tr!(locale, "导出", "Export"),
                            Tone::Neutral,
                            None,
                            cx,
                            move |view, cx| {
                                let events: Vec<String> = view
                                    .snapshot
                                    .event_lines()
                                    .into_iter()
                                    .map(|line| localised_event_line(&line, view.locale))
                                    .collect();
                                let text = view
                                    .snapshot
                                    .core_logs
                                    .iter()
                                    .map(|line| format!("[sing-box] {line}"))
                                    .chain(
                                        events
                                            .iter()
                                            .map(|line| format!("[{client_source}] {line}")),
                                    )
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                let path = view.data_dir.join("serein-logs.txt");
                                let locale = view.locale;
                                view.snapshot.status = match std::fs::write(&path, text) {
                                    Ok(()) => tr!(
                                        locale,
                                        format!("日志已导出到 {}", path.display()),
                                        format!("Logs exported to {}", path.display())
                                    ),
                                    Err(error) => tr!(
                                        locale,
                                        format!("导出日志失败：{error}"),
                                        format!("Export failed: {error}")
                                    ),
                                };
                                cx.notify();
                            },
                        )),
                ),
            )
            .child(
                div()
                    .mt(px(18.0))
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.0))
                            .children(level_chips),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.0))
                            .child(self.text_field(
                                FieldSpec {
                                    field: InputField::LogQuery,
                                    id: "log-query",
                                    placeholder: tr!(
                                        locale,
                                        "按关键字过滤日志…",
                                        "Filter the log by keyword…",
                                    ),
                                    width: 240.0,
                                },
                                window,
                                cx,
                            ))
                            .child(div().text_size(px(LABEL)).text_color(rgb(MUTED)).child(tr!(
                                locale,
                                format!("{} 行", rows.len()),
                                format!("{} lines", rows.len())
                            ))),
                    ),
            )
            .child(
                div()
                    .id("log-panel")
                    .mt(px(18.0))
                    .w_full()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .children(if rows.is_empty() {
                        Vec::new()
                    } else {
                        vec![log_table_header(locale).into_any_element()]
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
                                        tr!(locale, "暂无日志", "No log lines"),
                                        tr!(
                                            locale,
                                            "启动内核后这里会显示 sing-box 的输出。",
                                            "sing-box writes here once the core starts.",
                                        ),
                                    )
                                    .into_any_element(),
                                ]
                            } else {
                                rows
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel's window over a saturated kernel ring: exactly `KERNEL_ROWS`
    /// lines, the newest one being `newest`.
    fn full_kernel(newest: usize) -> Vec<String> {
        (newest - KERNEL_ROWS + 1..=newest)
            .map(|number| format!("INFO line {number}"))
            .collect()
    }

    #[test]
    fn following_keeps_working_after_the_row_count_saturates() {
        let seen = LogTail::new(&full_kernel(500), &[], "Client");
        assert_eq!(seen.rows, KERNEL_ROWS);

        // The core writes one more line: the ring drops its oldest entry, so the
        // painted row count is unchanged — the count-based guard that used to
        // decide this stopped firing here, and the view silently stopped tailing.
        let tail = LogTail::new(&full_kernel(501), &[], "Client");
        assert_eq!(
            tail.rows, seen.rows,
            "the whole point: a saturated window cannot report growth by count"
        );
        assert_eq!(
            follow_target(Some(&seen), &tail, true),
            Some(KERNEL_ROWS - 1),
            "a new last row is a new line, so the view pins to it"
        );

        // And it keeps working line after line, not just once.
        let next = LogTail::new(&full_kernel(502), &[], "Client");
        assert_eq!(
            follow_target(Some(&tail), &next, true),
            Some(KERNEL_ROWS - 1)
        );
    }

    #[test]
    fn a_repaint_of_an_unchanged_tail_leaves_the_scroll_alone() {
        let kernel = full_kernel(500);
        let seen = LogTail::new(&kernel, &[], "Client");
        let tail = LogTail::new(&kernel, &[], "Client");
        assert_eq!(
            follow_target(Some(&seen), &tail, true),
            None,
            "scrolling on a paint that brought nothing new fights the wheel"
        );
        assert_eq!(follow_target(Some(&seen), &tail, false), None);
    }

    #[test]
    fn follow_switches_off_and_an_empty_panel_stop_the_jump() {
        let kernel = ["INFO one".to_owned(), "INFO two".to_owned()];
        let tail = LogTail::new(&kernel, &[], "Client");
        assert_eq!(follow_target(None, &tail, true), Some(1));
        assert_eq!(
            follow_target(None, &tail, false),
            None,
            "自动滚动 off means off, even on a first paint"
        );
        assert_eq!(
            follow_target(None, &LogTail::new(&[], &[], "Client"), true),
            None,
            "an empty panel has no row to pin to; the old code underflowed here"
        );
    }

    #[test]
    fn the_newest_event_is_the_tail_even_when_the_kernel_ring_is_full() {
        let kernel = full_kernel(500);
        let events = vec!["节点已切换".to_owned()];
        let tail = LogTail::new(&kernel, &events, "Client");
        assert_eq!(tail.rows, KERNEL_ROWS + 1);
        assert_eq!(follow_target(None, &tail, true), Some(KERNEL_ROWS));

        // Two rows that read the same from different streams are still different
        // rows, so a repaint of one as the other cannot pass for "nothing new".
        let kernel_last = LogTail::new(&kernel, &[], "Client");
        let trimmed: Vec<String> = kernel.iter().take(KERNEL_ROWS - 1).cloned().collect();
        let event_last = LogTail::new(&trimmed, &["INFO line 500".to_owned()], "Client");
        assert_eq!(kernel_last.rows, event_last.rows);
        assert_eq!(kernel_last.line, event_last.line);
        assert_ne!(
            kernel_last, event_last,
            "the source column is part of the last row's identity"
        );
    }

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

    #[test]
    fn coded_events_render_in_the_interface_language_and_bare_lines_verbatim() {
        use client_core::event_code::{EventCode, EventRecord};
        let record = EventRecord::new(EventCode::CoreNotInstalledHint, Vec::new());
        let text = record.render_zh();
        let coded = EventLine {
            text: &text,
            record: Some(&record),
        };
        assert_eq!(
            localised_event_line(&coded, Locale::En),
            "Ready. Download the sing-box core first, then import a subscription."
        );
        assert_eq!(
            localised_event_line(&coded, Locale::Zh),
            "就绪。先下载 sing-box 内核，再导入订阅。"
        );

        let bare = EventLine {
            text: "尚未迁移的普通事件",
            record: None,
        };
        assert_eq!(
            localised_event_line(&bare, Locale::En),
            "尚未迁移的普通事件",
            "an unpaired line has no translation and is shown verbatim"
        );
        assert_eq!(
            localised_event_line(&bare, Locale::Zh),
            "尚未迁移的普通事件"
        );
    }
}
