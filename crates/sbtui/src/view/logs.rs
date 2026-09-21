//! The Logs tab: the kernel tail with its level and keyword filters, the paged
//! window over it, and the rules view that replaces it while `r` is on.

use ratatui::Frame;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::style::panel;
use crate::view::settings::rules_lines;

pub(crate) fn draw_logs(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    // The bordered panel leaves its inner rows to the text.
    let inner_height = area.height.saturating_sub(2);
    app.log_view_height = inner_height;
    if app.show_rules {
        let lines: Vec<Line> = rules_lines(app).into_iter().map(Line::from).collect();
        frame.render_widget(
            Paragraph::new(lines).block(panel("分流规则 · r 返回日志")),
            area,
        );
        return;
    }
    let all = log_page_lines(app);
    let lines: Vec<Line> = log_window(&all, inner_height, app.log_scroll)
        .iter()
        .map(|line| Line::from(line.clone()))
        .collect();
    let mut title = format!(
        "日志 [{}]{}（Space 暂停，l 级别，/ 关键字，c 复制，r 规则）",
        app.log_filter.label(),
        if app.paused_logs.is_some() {
            " · 已暂停"
        } else {
            ""
        }
    );
    if app.log_scroll > 0 {
        title.push_str(&format!(" · 已回看 {} 行（End 回到最新）", app.log_scroll));
    }
    if !app.log_query.is_empty() {
        title = format!("{title} · 关键字「{}」", app.log_query);
    }
    frame.render_widget(Paragraph::new(lines).block(panel(title)), area);
}

/// Whether a kernel log line contains the active keyword filter.
pub(crate) fn log_query_matches(query: &str, line: &str) -> bool {
    query.is_empty() || line.to_lowercase().contains(&query.to_lowercase())
}

/// The end of `text` that fits in `width` display columns.
pub(crate) fn tail_within_width(text: &str, width: usize) -> String {
    let mut used = 0usize;
    let mut taken = String::new();
    for ch in text.chars().rev() {
        let columns = Line::from(ch.to_string()).width();
        if used + columns > width {
            break;
        }
        used += columns;
        taken.push(ch);
    }
    taken.chars().rev().collect()
}

/// The log lines the Logs page currently shows: the frozen copy while paused,
/// otherwise the engine's live kernel tail.
pub(crate) fn current_log_lines(app: &App) -> Vec<String> {
    match &app.paused_logs {
        Some(frozen) => frozen.clone(),
        None => app.snapshot.core_logs.iter().cloned().collect(),
    }
}

/// The Logs page body: the active configuration's routing rules, or the
/// filtered log tail (falling back to the client's own event stream when the
/// kernel has not logged anything matching).
fn log_page_lines(app: &App) -> Vec<String> {
    if app.show_rules {
        return rules_lines(app);
    }
    let lines = current_log_lines(app);
    let filtered: Vec<String> = lines
        .into_iter()
        .filter(|line| app.log_filter.matches(line) && log_query_matches(&app.log_query, line))
        .collect();
    if filtered.is_empty() {
        app.snapshot
            .events
            .iter()
            .filter(|line| log_query_matches(&app.log_query, line))
            .cloned()
            .collect()
    } else {
        filtered
    }
}

/// The window of log rows shown by the Logs panel: the newest `height` lines by
/// default, walked back `scroll` rows at the user's request. The engine keeps a
/// 500-line ring, so rendering the whole list from the top would leave the
/// recent lines permanently below the screen.
fn log_window(lines: &[String], height: u16, scroll: usize) -> &[String] {
    let height = (height as usize).min(lines.len()).max(1);
    if lines.len() <= height {
        return lines;
    }
    let max_scroll = lines.len() - height;
    let end = lines.len() - scroll.min(max_scroll);
    &lines[end - height..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LogFilter;

    #[test]
    fn log_query_matches_are_case_insensitive() {
        assert!(log_query_matches("DNS", "[123] inbound/dns: lookup"));
        assert!(!log_query_matches("DNS", "[123] outbound/tcp: connect"));
        assert!(log_query_matches("", "anything"));
    }

    #[test]
    fn the_log_filter_keeps_unmarked_client_events_at_info_and_above() {
        assert!(
            LogFilter::Info.matches("内核已启动"),
            "the client's own events are informational, not debug"
        );
        assert!(LogFilter::Info.matches("ERROR boom"));
        assert!(!LogFilter::Info.matches("DEBUG detail"));
        assert!(LogFilter::Error.matches("导入订阅失败: timeout"));
        assert!(LogFilter::All.matches("DEBUG detail"));
    }

    #[test]
    fn the_input_box_keeps_the_tail_of_a_long_value() {
        let url = "https://sub.example.test/sub/credential-that-is-quite-long/sing-box.json";
        assert_eq!(tail_within_width(url, 20), &url[url.len() - 20..]);
        assert_eq!(tail_within_width("short", 40), "short");
        assert!(tail_within_width("", 10).is_empty());
        assert_eq!(
            tail_within_width("节点节点节点", 4),
            "节点",
            "wide characters take two columns each"
        );
    }

    #[test]
    fn the_log_window_keeps_the_newest_rows_and_pages_backwards() {
        fn first(rows: &[String]) -> Option<&str> {
            rows.first().map(String::as_str)
        }
        fn last(rows: &[String]) -> Option<&str> {
            rows.last().map(String::as_str)
        }

        let lines: Vec<String> = (1..=500).map(|number| format!("line {number}")).collect();
        assert_eq!(
            last(log_window(&lines, 10, 0)),
            Some("line 500"),
            "the newest line has to be on screen without any scrolling"
        );
        assert_eq!(first(log_window(&lines, 10, 0)), Some("line 491"));
        assert_eq!(last(log_window(&lines, 10, 5)), Some("line 495"));
        assert_eq!(
            first(log_window(&lines, 10, 10_000)),
            Some("line 1"),
            "paging past the top clamps at the oldest line"
        );

        let few: Vec<String> = (1..=3).map(|number| format!("line {number}")).collect();
        assert_eq!(log_window(&few, 10, 0).len(), 3, "a short list shows whole");
        assert!(log_window(&[], 10, 0).is_empty());
    }
}
