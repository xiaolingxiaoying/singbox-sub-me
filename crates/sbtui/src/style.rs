//! The terminal client's own design tokens and the shared drawing helpers that
//! turn app state into a colour, a glyph run or a bordered panel.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};

use crate::format::{DelayLevel, delay_level};

const PANEL: Color = Color::Rgb(11, 28, 42);
pub(crate) const EDGE: Color = Color::Rgb(37, 92, 116);
pub(crate) const TEXT: Color = Color::Rgb(217, 235, 242);
pub(crate) const MUTED: Color = Color::Rgb(123, 157, 172);
pub(crate) const CYAN: Color = Color::Rgb(42, 213, 235);
pub(crate) const MINT: Color = Color::Rgb(101, 235, 157);
pub(crate) const AMBER: Color = Color::Rgb(255, 190, 81);
pub(crate) const DANGER: Color = Color::Rgb(255, 104, 97);

pub(crate) fn panel<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(EDGE))
        .style(Style::default().bg(PANEL))
        .title(
            title
                .into()
                .style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        )
}

pub(crate) fn short_label(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_owned()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max.saturating_sub(1))
                .collect::<String>()
        )
    }
}

pub(crate) fn meter(value: u64, width: usize) -> String {
    let filled = if value == 0 {
        0
    } else {
        ((value.ilog10() as usize + 1) * width / 10).clamp(1, width)
    };
    format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(width.saturating_sub(filled))
    )
}

pub(crate) fn status_color(status: &str) -> Color {
    if status.contains("失败") || status.contains("错误") || status.contains("崩溃") {
        DANGER
    } else if status.contains("需要") || status.contains("未") {
        AMBER
    } else if status.contains("成功") || status.contains("启动") || status.contains("已") {
        MINT
    } else {
        TEXT
    }
}

/// The footer colour, preferring the engine's own severity over a guess at its
/// wording. `None` means the status came from a call site that has not moved to
/// an event code yet, so the Chinese-substring fallback still applies to it.
pub(crate) fn status_color_at(
    level: Option<client_core::event_code::EventLevel>,
    status: &str,
) -> Color {
    use client_core::event_code::EventLevel;
    match level {
        Some(EventLevel::Error) => DANGER,
        Some(EventLevel::Warn) => AMBER,
        Some(EventLevel::Info) => MINT,
        None => status_color(status),
    }
}

pub(crate) fn delay_color(delay: Option<u64>) -> Color {
    match delay_level(delay) {
        DelayLevel::Fast => MINT,
        DelayLevel::Slow => AMBER,
        DelayLevel::Timeout => DANGER,
        DelayLevel::Unknown => MUTED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_keeps_the_requested_visual_width() {
        assert_eq!(meter(0, 12).chars().count(), 12);
        assert_eq!(meter(1024 * 1024, 12).chars().count(), 12);
    }

    /// The engine's severity must win over the wording, or an English status
    /// line keeps its colour only by accident of translation.
    #[test]
    fn an_engine_severity_beats_the_word_guess() {
        use client_core::event_code::EventLevel;
        assert_eq!(status_color_at(Some(EventLevel::Error), "一切正常"), DANGER);
        assert_eq!(status_color_at(Some(EventLevel::Warn), "一切正常"), AMBER);
        assert_eq!(
            status_color_at(Some(EventLevel::Info), "导入订阅失败"),
            MINT
        );
        assert_eq!(
            status_color_at(None, "导入订阅失败"),
            DANGER,
            "an uncoded status still falls back to reading the wording"
        );
    }

    #[test]
    fn status_color_reads_the_chinese_status_vocabulary() {
        assert_eq!(status_color("导入订阅失败"), DANGER);
        assert_eq!(status_color("需要先安装内核"), AMBER);
        assert_eq!(status_color("内核已启动"), MINT);
        assert_eq!(
            status_color("从未更新"),
            AMBER,
            "\"never\" is deliberately a warning: there is no data yet"
        );
        assert_eq!(
            status_color("就绪"),
            TEXT,
            "an unknown status stays neutral"
        );
    }

    #[test]
    fn short_label_counts_characters_and_leaves_room_for_the_ellipsis() {
        assert_eq!(short_label("订阅管理", 6), "订阅管理");
        assert_eq!(short_label("订阅订阅订阅订阅", 5), "订阅订阅…");
        assert_eq!(short_label("abcdefgh", 5).chars().count(), 5);
        assert_eq!(short_label("abcd", 1), "…");
    }

    #[test]
    fn delay_color_uses_the_shared_bands_and_stays_quiet_without_a_measurement() {
        assert_eq!(delay_color(None), MUTED);
        assert_eq!(delay_color(Some(199)), MINT);
        assert_eq!(delay_color(Some(200)), AMBER);
        assert_eq!(delay_color(Some(500)), DANGER);
    }
}
