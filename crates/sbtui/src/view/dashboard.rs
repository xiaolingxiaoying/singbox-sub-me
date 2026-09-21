//! The Dashboard tab: the run-state rail, the network path, the traffic panel and
//! the compact variant a small terminal falls back to.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Sparkline};

use crate::app::App;
use crate::format::{human_bytes, usage_label};
use crate::state::TrafficPoint;
use crate::style::{
    CYAN, EDGE, MINT, MUTED, TEXT, delay_color, meter, panel, short_label, status_color,
};
use crate::view::proxies::selected_node;

pub(crate) fn draw_dashboard(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
    if area.width < 100 || area.height < 28 {
        draw_dashboard_compact(frame, area, app);
        return;
    }
    let columns =
        Layout::horizontal([Constraint::Percentage(31), Constraint::Percentage(69)]).split(area);
    let rail = Layout::vertical([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(columns[0]);
    let content = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(columns[1]);
    let current = selected_node(app);
    let running = app.snapshot.core_running;
    let state_color = if running { MINT } else { MUTED };
    let delay = app.node_delay(&current);
    let health = vec![
        Line::from(Span::styled(
            if running { "● 在线" } else { "○ 离线" },
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("内核  ", Style::default().fg(MUTED)),
            Span::styled(
                if running { "已启动" } else { "未启动" },
                Style::default().fg(state_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("代理  ", Style::default().fg(MUTED)),
            Span::styled(
                if app.snapshot.system_proxy_enabled {
                    format!("已接管 127.0.0.1:{}", app.snapshot.settings.mixed_port)
                } else {
                    "未接管系统网络".to_owned()
                },
                Style::default().fg(if app.snapshot.system_proxy_enabled {
                    MINT
                } else {
                    MUTED
                }),
            ),
        ]),
        Line::from(vec![
            Span::styled("模式  ", Style::default().fg(MUTED)),
            Span::styled(
                format!(
                    "{} · {}",
                    app.snapshot.traffic_mode.label(),
                    app.snapshot.outbound_mode.label()
                ),
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("当前节点", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            short_label(&current, 22),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "延迟 {}",
                delay
                    .map(|d| format!("{d} ms"))
                    .unwrap_or_else(|| "待测".into())
            ),
            Style::default().fg(delay_color(delay)),
        )),
    ];
    frame.render_widget(Paragraph::new(health).block(panel("运行状态")), rail[0]);
    let network = vec![
        Line::from(Span::styled(
            "本机",
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled("  │", Style::default().fg(EDGE))),
        Line::from(Span::styled(
            format!(
                "  ├── 系统代理  {}",
                if app.snapshot.system_proxy_enabled {
                    "已启用"
                } else {
                    "待启用"
                }
            ),
            Style::default().fg(if app.snapshot.system_proxy_enabled {
                MINT
            } else {
                MUTED
            }),
        )),
        Line::from(Span::styled("  │", Style::default().fg(EDGE))),
        Line::from(Span::styled("  ▼", Style::default().fg(CYAN))),
        Line::from(Span::styled(
            format!("  [ {} ]", short_label(&current, 32)),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  │  ├────────── 规则流量",
            Style::default().fg(EDGE),
        )),
        Line::from(Span::styled(
            "  │  └────────── 直连流量",
            Style::default().fg(EDGE),
        )),
        Line::from(Span::styled("  ▼", Style::default().fg(MINT))),
        Line::from(Span::styled(
            format!(
                "  公网出口  ·  {} 个活动连接",
                app.snapshot.connections.connections.len()
            ),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "路径说明：当前节点承载代理链路；规则可将流量直连。",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(network).block(panel("网络路径")), content[0]);
    let metrics = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(content[1]);
    draw_traffic_panel(frame, metrics[0], app);
    let activity: Vec<Line> = app
        .snapshot
        .events
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(|line| {
            Line::from(short_label(line, 38)).style(Style::default().fg(status_color(line)))
        })
        .collect();
    let activity = if activity.is_empty() {
        vec![Line::from("等待新的运行事件").style(Style::default().fg(MUTED))]
    } else {
        activity
    };
    frame.render_widget(
        Paragraph::new(activity).block(panel("最近事件")),
        metrics[1],
    );
    let subscription = vec![
        Line::from(Span::styled("节点摘要", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            short_label(&current, 24),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "{} · {} 个活动连接",
                app.snapshot.outbound_mode.label(),
                app.snapshot.connections.connections.len()
            ),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled("订阅状态", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            usage_label(app.snapshot.subscription_usage.as_ref()),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "内核 {}",
                app.snapshot
                    .core_version
                    .clone()
                    .unwrap_or_else(|| "未安装".into())
            ),
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            if running {
                format!(
                    "运行 {}  ·  内存 {}",
                    app.snapshot
                        .core_runtime_version
                        .clone()
                        .unwrap_or_else(|| "未知".into()),
                    human_bytes(app.snapshot.memory_used)
                )
            } else {
                format!("内存 {}", human_bytes(app.snapshot.memory_used))
            },
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(subscription).block(panel("订阅与核心")),
        rail[1],
    );
}

/// The dashboard traffic panel: live rates plus a sparkline over the recent
/// rate history, scaled to the window's peak.
fn draw_traffic_panel(frame: &mut Frame, area: Rect, app: &App) {
    let history = &app.snapshot.traffic_history;
    let peak = history
        .iter()
        .map(|point: &TrafficPoint| point.up.max(point.down))
        .max()
        .unwrap_or(0)
        .max(1);
    let block = panel(format!(
        "实时流量 · 近 {} 秒",
        // One sample per engine traffic poll, not per render tick.
        history.len() as u64 * client_core::controller::TRAFFIC_EVERY.as_secs()
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 6 {
        frame.render_widget(
            Paragraph::new(format!(
                "↓ {}/s   ↑ {}/s",
                human_bytes(app.snapshot.download_speed),
                human_bytes(app.snapshot.upload_speed)
            ))
            .style(Style::default().fg(CYAN)),
            inner,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↓ ", Style::default().fg(CYAN)),
            Span::styled(
                format!("{}/s", human_bytes(app.snapshot.download_speed)),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[0],
    );
    frame.render_widget(
        Sparkline::default()
            .data(history.iter().map(|point| point.down))
            .max(peak)
            .style(Style::default().fg(CYAN)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑ ", Style::default().fg(MINT)),
            Span::styled(
                format!("{}/s", human_bytes(app.snapshot.upload_speed)),
                Style::default().fg(MINT).add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[2],
    );
    frame.render_widget(
        Sparkline::default()
            .data(history.iter().map(|point| point.up))
            .max(peak)
            .style(Style::default().fg(MINT)),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(
                "累计 ↓ {}   ↑ {}   峰值 {}/s",
                human_bytes(app.snapshot.total_download),
                human_bytes(app.snapshot.total_upload),
                human_bytes(peak)
            ),
            Style::default().fg(MUTED),
        )),
        rows[4],
    );
}

fn draw_dashboard_compact(frame: &mut Frame, area: Rect, app: &App) {
    let current = selected_node(app);
    let running = app.snapshot.core_running;
    let state = if running { "运行中" } else { "已停止" };
    let proxy = if app.snapshot.system_proxy_enabled {
        "系统代理：已启用"
    } else {
        "系统代理：未启用"
    };
    let delay = app
        .node_delay(&current)
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "待测".to_owned());
    let body = vec![
        Line::from(vec![
            Span::styled("状态  ", Style::default().fg(MUTED)),
            Span::styled(
                state,
                Style::default().fg(if running { MINT } else { MUTED }),
            ),
            Span::styled("  ·  ", Style::default().fg(EDGE)),
            Span::styled(
                proxy,
                Style::default().fg(if app.snapshot.system_proxy_enabled {
                    MINT
                } else {
                    MUTED
                }),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("代理路径", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            format!("本机  →  {}  →  公网出口", short_label(&current, 36)),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "{} · {} · {} 个活动连接",
                app.snapshot.outbound_mode.label(),
                delay,
                app.snapshot.connections.connections.len()
            ),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("↓ ", Style::default().fg(CYAN)),
            Span::styled(
                format!("{}/s", human_bytes(app.snapshot.download_speed)),
                Style::default().fg(CYAN),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                meter(app.snapshot.download_speed, 12),
                Style::default().fg(CYAN),
            ),
            Span::styled("     ↑ ", Style::default().fg(MINT)),
            Span::styled(
                format!("{}/s", human_bytes(app.snapshot.upload_speed)),
                Style::default().fg(MINT),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                meter(app.snapshot.upload_speed, 12),
                Style::default().fg(MINT),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "紧凑视图：放大终端可查看完整网络拓扑与事件。",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(body).block(panel("概览 · 紧凑视图")), area);
}
