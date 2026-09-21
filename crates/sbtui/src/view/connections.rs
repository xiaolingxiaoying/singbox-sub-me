//! The Connections tab: one row per live connection, in the order the engine
//! published them after the table filter and sort.

use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Cell, Row, Table};

use crate::app::App;
use crate::format::human_bytes;
use crate::input::{selected_connection_index, visible_connections};
use crate::style::{CYAN, panel};

pub(crate) fn draw_connections(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
    let visible = visible_connections(app);
    let rows = visible.iter().enumerate().map(|(index, connection)| {
        let chains = if connection.chains.is_empty() {
            "-".to_owned()
        } else {
            connection.chains.join(" → ")
        };
        let rule = if connection.rule.is_empty() {
            "-".to_owned()
        } else {
            connection.rule.clone()
        };
        Row::new(vec![
            Cell::from(if index == selected_connection_index(app) {
                "●"
            } else {
                ""
            }),
            Cell::from(format!(
                "{}:{}",
                connection.metadata.destination_ip, connection.metadata.destination_port
            )),
            Cell::from(connection.metadata.destination_host.clone()),
            Cell::from(connection.metadata.network.clone()),
            Cell::from(rule),
            Cell::from(chains),
            Cell::from(human_bytes(connection.upload)),
            Cell::from(human_bytes(connection.download)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(23),
            Constraint::Min(18),
            Constraint::Length(6),
            Constraint::Length(12),
            Constraint::Min(14),
            Constraint::Length(9),
            Constraint::Length(9),
        ],
    )
    .header(
        Row::new(vec![
            "", "目标", "主机", "网络", "规则", "链路", "上传", "下载",
        ])
        .style(Style::default().fg(CYAN).bold()),
    )
    .block(panel(format!(
        "活动连接 · {} · {} 条{}{}",
        app.conn_sort.label(),
        visible.len(),
        if app.conn_filter.is_empty() {
            String::new()
        } else {
            format!(" · 过滤「{}」", app.conn_filter)
        },
        if visible.is_empty() && !app.snapshot.connections.connections.is_empty() {
            " · 无匹配"
        } else {
            ""
        }
    )));
    frame.render_widget(table, area);
}
