//! The Proxies tab: the group list, its members with their delays, and the node the
//! rest of the UI names as "current".

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::app::App;
use crate::clash_api::SELECTOR_TAG;
use crate::style::{CYAN, DANGER, MUTED, delay_color, panel};

pub(crate) fn draw_proxies(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(area);
    let group_items: Vec<ListItem> = app
        .snapshot
        .proxy_groups
        .iter()
        .map(|group| ListItem::new(format!("{} ({})", group.name, group.kind)))
        .collect();
    frame.render_stateful_widget(
        List::new(group_items)
            .block(panel("代理组"))
            .highlight_style(
                Style::default()
                    .fg(Color::Rgb(8, 18, 28))
                    .bg(CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        columns[0],
        &mut app.group_list,
    );
    if let Some(group) = app.selected_group_snapshot().cloned() {
        let member_items: Vec<ListItem> = group
            .members
            .iter()
            .map(|member| {
                let delay = group.delays.get(member).copied();
                let failed = group.failed.contains(member);
                let marker = if member == &group.current {
                    "● "
                } else {
                    "○ "
                };
                let delay_text = if failed {
                    "超时".to_owned()
                } else {
                    delay
                        .map(|d| format!("{d}ms"))
                        .unwrap_or_else(|| "-".to_owned())
                };
                let color = if failed { DANGER } else { delay_color(delay) };
                ListItem::new(Span::styled(
                    format!("{marker}{member}  [{delay_text}]"),
                    Style::default().fg(color),
                ))
            })
            .collect();
        let mut member_state = ListState::default().with_selected(Some(app.selected_member));
        frame.render_stateful_widget(
            List::new(member_items)
                .block(panel(format!("{} 节点 · Enter 切换", group.name)))
                .highlight_style(
                    Style::default()
                        .fg(Color::Rgb(8, 18, 28))
                        .bg(CYAN)
                        .add_modifier(Modifier::BOLD),
                ),
            columns[1],
            &mut member_state,
        );
    } else {
        frame.render_widget(
            Paragraph::new("启动内核后，节点组与延迟将在这里出现。\n\n按 s 启动内核")
                .style(Style::default().fg(MUTED))
                .block(panel("节点")),
            columns[1],
        );
    }
}

pub(crate) fn selected_node(app: &App) -> String {
    app.snapshot
        .proxy_groups
        .iter()
        .find(|group| group.name == SELECTOR_TAG)
        .map(|group| group.current.clone())
        .or_else(|| app.snapshot.current_node.clone())
        .unwrap_or_else(|| "等待选择节点".to_owned())
}
