//! The frame every tab is drawn into: the outer chrome (header, footer), the three
//! overlays, and [`draw`], which picks the page for the active tab.

mod connections;
mod dashboard;
pub(crate) mod logs;
pub(crate) mod proxies;
mod rules;
pub(crate) mod settings;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs};

use crate::app::{App, InputGoal, Tab};
use crate::format::human_bytes;
use crate::style::{AMBER, CYAN, EDGE, MINT, MUTED, TEXT, panel, short_label, status_color_at};
use crate::system_proxy::TrafficMode;
use crate::view::connections::draw_connections;
use crate::view::dashboard::draw_dashboard;
use crate::view::logs::{draw_logs, tail_within_width};
use crate::view::proxies::{draw_proxies, selected_node};
use crate::view::rules::draw_rules;
use crate::view::settings::draw_settings;

const TAB_TITLES: [&str; 6] = ["概览", "节点", "连接", "日志", "设置", "入站"];

pub(crate) fn draw(frame: &mut Frame, app: &mut App) {
    const INK: Color = Color::Rgb(8, 18, 28);
    frame.render_widget(
        Block::default().style(Style::default().bg(INK)),
        frame.area(),
    );
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(12),
        Constraint::Length(3),
    ])
    .margin(1)
    .split(frame.area());
    draw_header(frame, outer[0], app);
    match app.tab {
        Tab::Dashboard => draw_dashboard(frame, outer[1], app),
        Tab::Proxies => draw_proxies(frame, outer[1], app),
        Tab::Connections => draw_connections(frame, outer[1], app),
        Tab::Logs => draw_logs(frame, outer[1], app),
        Tab::Settings => draw_settings(frame, outer[1], app),
        Tab::Rules => draw_rules(frame, outer[1], app),
    }
    draw_footer(frame, outer[2], app);
    if app.input.is_some() {
        draw_input_overlay(frame, app);
    } else if app.show_help {
        draw_help_overlay(frame, app);
    } else if app.confirm_mode || app.confirm_quit {
        draw_confirmation_overlay(frame, app);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).split(area);
    let state = if app.snapshot.core_running {
        "● 运行中"
    } else if app.snapshot.starting {
        "◐ 启动中"
    } else {
        "○ 已停止"
    };
    let state_color = if app.snapshot.core_running || app.snapshot.starting {
        MINT
    } else {
        MUTED
    };
    let current = selected_node(app);
    let header = Line::from(vec![
        Span::styled(
            " sbtui ",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled("网络控制台", Style::default().fg(MUTED)),
        Span::styled("  /  ", Style::default().fg(EDGE)),
        Span::styled(
            state,
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::default().fg(EDGE)),
        Span::styled(short_label(&current, 24), Style::default().fg(TEXT)),
        Span::styled("  ·  ", Style::default().fg(EDGE)),
        Span::styled(
            format!("↓ {}/s", human_bytes(app.snapshot.download_speed)),
            Style::default().fg(CYAN),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("↑ {}/s", human_bytes(app.snapshot.upload_speed)),
            Style::default().fg(MINT),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), rows[0]);
    let titles: Vec<Line> = TAB_TITLES
        .iter()
        .enumerate()
        .map(|(i, title)| Line::from(format!(" {} {} ", i + 1, title)))
        .collect();
    frame.render_widget(
        Tabs::new(titles)
            .select(app.tab.index())
            .divider(Span::styled("│", Style::default().fg(EDGE)))
            .highlight_style(
                Style::default()
                    .fg(Color::Rgb(8, 18, 28))
                    .bg(CYAN)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().fg(MUTED)),
        rows[1],
    );
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hint = match app.tab {
        Tab::Dashboard => "s 启动/停止  ·  u 更新订阅  ·  p 系统代理  ·  m 切换模式",
        Tab::Proxies => "↑↓ 选节点  ·  ←→ 切组  ·  Enter 切换  ·  t 测当前  ·  T 测全组",
        Tab::Connections => "↑↓ 选择  ·  x 关闭连接  ·  X 关闭全部  ·  S 排序  ·  / 过滤",
        Tab::Logs => {
            "↑↓/PgUp PgDn 回看  ·  End 回到最新  ·  Space 暂停  ·  l 级别  ·  / 关键字  ·  c 复制  ·  r 分流规则"
        }
        Tab::Settings => "n 新增  ·  e 编辑  ·  Delete 删除  ·  d 下载内核  ·  a/P/U 选项",
        Tab::Rules => "入站与规则来自内核正在使用的配置  ·  r 返回日志",
    };
    let line = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            &app.status,
            Style::default().fg(status_color_at(app.status_level, &app.status)),
        ),
        Span::styled("  │  ", Style::default().fg(EDGE)),
        Span::styled(hint, Style::default().fg(MUTED)),
        Span::styled(
            "  │  Tab 切换  ·  ? 帮助  ·  q 退出",
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(EDGE)),
        ),
        area,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height.min(area.height)),
        Constraint::Fill(1),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width.min(vertical[1].width)),
        Constraint::Fill(1),
    ])
    .split(vertical[1])[1]
}

fn draw_input_overlay(frame: &mut Frame, app: &App) {
    let area = centered_rect(70, 7, frame.area());
    let goal = app
        .input
        .as_ref()
        .expect("input overlay requires an input goal");
    // Editing a long subscription URL blind is the usual way people lose a
    // link, so the box keeps the tail visible and puts a real caret after it.
    let room = area.width.saturating_sub(3) as usize;
    let shown = tail_within_width(&app.input_text, room);
    let caret_x = 1 + Line::from(shown.clone()).width();
    let body = vec![
        Line::from(input_label(goal)).style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from(shown).style(Style::default().fg(TEXT)),
        Line::from(""),
        Line::from("Enter 保存  ·  Esc 取消").style(Style::default().fg(MUTED)),
    ];
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(body)
            .alignment(Alignment::Left)
            .block(panel("输入")),
        area,
    );
    // Row 2 of the body, inside the panel's top border.
    frame.set_cursor_position(Position::new(area.left() + caret_x as u16, area.top() + 3));
}

fn draw_help_overlay(frame: &mut Frame, app: &App) {
    let page_keys = match app.tab {
        Tab::Dashboard => "s 启动/停止 · u 更新订阅 · p 开/关系统代理 · m 切换模式",
        Tab::Proxies => "↑↓ 选节点 · ←→ 切组 · Enter 切换 · t 测当前 · T 测全组",
        Tab::Connections => "↑↓ 选择 · x 关闭连接 · X 关闭全部 · S 切换排序 · / 关键字过滤",
        Tab::Logs => {
            "↑↓/PgUp PgDn 回看 · End 回到最新 · Space 暂停 · l 切换级别 · / 关键字过滤 · c 复制 · r 查看规则"
        }
        Tab::Settings => "n 新增 · e 编辑 · Delete 删除 · d 下载内核 · a/P/U/g/y 选项",
        Tab::Rules => "只读视图 · r 返回日志",
    };
    let body = vec![
        Line::from(Span::styled(
            "键盘操作",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("全局  ", Style::default().fg(MUTED)),
            Span::styled("Tab / 1–5", Style::default().fg(TEXT)),
            Span::styled(" 切换页面  ·  ", Style::default().fg(MUTED)),
            Span::styled("o", Style::default().fg(TEXT)),
            Span::styled(" 切换出站  ·  ", Style::default().fg(MUTED)),
            Span::styled("q", Style::default().fg(TEXT)),
            Span::styled(" 退出", Style::default().fg(MUTED)),
        ]),
        Line::from(vec![
            Span::styled("当前页  ", Style::default().fg(MUTED)),
            Span::styled(page_keys, Style::default().fg(TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("提示  ", Style::default().fg(AMBER)),
            Span::styled(
                "模式切换与保留系统代理退出，均需再次按对应按键确认。",
                Style::default().fg(TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled("Esc 或 ? 返回", Style::default().fg(MUTED))),
    ];
    let area = centered_rect(82, 11, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(body).block(panel("帮助")), area);
}

fn draw_confirmation_overlay(frame: &mut Frame, app: &App) {
    let (title, lines) = if app.confirm_quit {
        (
            "退出确认",
            vec![
                Line::from(Span::styled(
                    "系统代理仍处于开启状态。",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                )),
                Line::from("再按 q 退出并保留代理；按 p 先关闭代理。"),
                Line::from(Span::styled(
                    "Esc 或其它按键取消",
                    Style::default().fg(MUTED),
                )),
            ],
        )
    } else {
        let target = match app.snapshot.traffic_mode {
            TrafficMode::SystemProxy => TrafficMode::Tun,
            TrafficMode::Tun => TrafficMode::SystemProxy,
        };
        (
            "模式切换确认",
            vec![
                Line::from(Span::styled(
                    format!("准备切换至 {} 模式", target.label()),
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                )),
                Line::from("再按 m 确认；下次启动内核时生效。"),
                Line::from(Span::styled(
                    "Esc 或其它按键取消",
                    Style::default().fg(MUTED),
                )),
            ],
        )
    };
    let area = centered_rect(64, 7, frame.area());
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(panel(title)), area);
}

fn input_label(goal: &InputGoal) -> &'static str {
    match goal {
        InputGoal::ProfileName => "档案名",
        InputGoal::ProfileUrl => "订阅链接",
        InputGoal::ProfileFile => "本地订阅文件路径",
        InputGoal::ProfileEditUrl => "新的订阅链接（留空取消）",
        InputGoal::CoreVersion => "内核版本（留空 = 最新）",
        InputGoal::Mirror => "镜像前缀（留空 = 直连）",
        InputGoal::AutoUpdate => "自动更新间隔（分钟，0 = 关闭）",
        InputGoal::MixedPort => "混合代理端口",
        InputGoal::TestUrl => "延迟测试地址",
        InputGoal::ConnFilter => "连接过滤关键字（留空 = 全部）",
        InputGoal::LogQuery => "日志关键字（留空 = 不过滤）",
    }
}

/// Every tab is drawn from a `ClientSnapshot`, so a golden frame per tab pins
/// what the user actually sees. Until now the client crates had only pure
/// function tests: nothing asserted that a published snapshot reaches the
/// screen, which is exactly the class of bug the connections poll starvation
/// was (`.scratch/sbgui-progressive-workspace/issues/01-connections-poll-flake.md`).
#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::clash_api::{Connection, ConnectionMetadata, ConnectionsSnapshot};
    use crate::state::ProxyGroupSnapshot;
    use crate::{ClientController, ClientSnapshot};
    use std::collections::HashMap;

    /// A snapshot dense enough that every tab has real content to draw: a
    /// running core, two proxy groups, live connections, traffic in both
    /// directions, and log lines.
    fn dense_snapshot() -> ClientSnapshot {
        let mut delays = HashMap::new();
        delays.insert("东京-A".to_owned(), 42_u64);
        ClientSnapshot {
            inbounds: vec![
                client_core::state::InboundInfo {
                    kind: "mixed".to_owned(),
                    tag: "mixed-in".to_owned(),
                    listen: "127.0.0.1".to_owned(),
                    port: 2080,
                },
                client_core::state::InboundInfo {
                    kind: "tun".to_owned(),
                    tag: "tun-in".to_owned(),
                    listen: String::new(),
                    port: 0,
                },
            ],
            core_running: true,
            current_node: Some("东京-A".to_owned()),
            traffic_mode: TrafficMode::Tun,
            upload_speed: 237,
            download_speed: 196_918,
            total_upload: 1_300_234,
            total_download: 19_789_432,
            active_connections: 2,
            connections: ConnectionsSnapshot {
                upload_total: 1_300_234,
                download_total: 19_789_432,
                connections: vec![
                    Connection {
                        id: "1".to_owned(),
                        upload: 4_096,
                        download: 196_608,
                        start: "2026-09-23T01:00:00Z".to_owned(),
                        rule: "ip_is_private=true".to_owned(),
                        chains: vec!["DIRECT".to_owned()],
                        metadata: ConnectionMetadata::default(),
                    },
                    Connection {
                        id: "2".to_owned(),
                        upload: 1_024,
                        download: 81_920,
                        start: "2026-09-23T01:00:05Z".to_owned(),
                        rule: "MATCH".to_owned(),
                        chains: vec!["🚀节点选择".to_owned(), "东京-A".to_owned()],
                        metadata: ConnectionMetadata::default(),
                    },
                ],
            },
            proxy_groups: vec![
                ProxyGroupSnapshot {
                    name: "🚀节点选择".to_owned(),
                    kind: "selector".to_owned(),
                    current: "东京-A".to_owned(),
                    members: vec!["东京-A".to_owned(), "洛杉矶-B".to_owned()],
                    delays,
                    failed: vec!["洛杉矶-B".to_owned()],
                },
                ProxyGroupSnapshot {
                    name: "♻️自动选择".to_owned(),
                    kind: "urltest".to_owned(),
                    current: "东京-A".to_owned(),
                    members: vec!["东京-A".to_owned()],
                    delays: HashMap::new(),
                    failed: Vec::new(),
                },
            ],
            core_version: Some("sing-box 1.14.1".to_owned()),
            core_runtime_version: Some("1.14.1".to_owned()),
            core_installed: true,
            memory_used: 33_554_432,
            core_logs: ["info: inbound connection to 127.0.0.1:8099".to_owned()].into(),
            events: ["已启动内核".to_owned()].into(),
            status: "内核已启动".to_owned(),
            ..ClientSnapshot::default()
        }
    }

    fn frame_for(tab: Tab, snapshot: ClientSnapshot) -> String {
        let dir = tempfile::tempdir().expect("temporary data directory");
        let controller = ClientController::start(dir.path().to_path_buf());
        let mut app = App::new(controller, dir.path().to_path_buf());
        app.snapshot = snapshot;
        app.tab = tab;
        // The settings page prints `<dir>/core/sing-box`. Leaving the real
        // temporary path in place would make every golden depend on how long
        // this machine's temp directory name happens to be.
        app.dir = std::path::PathBuf::from("<DATA_DIR>");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 32)).expect("terminal");
        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("frame draws");
        let rendered = terminal.backend().to_string();
        app.controller.shutdown();
        // Three things in the Settings frame are properties of the machine, not
        // of the snapshot, and CI runs on Linux while development happens on
        // Windows: the data directory's own name, the separator in the core
        // path (which also changes how far the line truncates), and the
        // OS-proxy backend label. See `snapshot_name` for how the last of those
        // three is handled.
        rendered
    }

    /// The Settings page prints the core path and the compiled-in OS-proxy
    /// backend. Both vary in *length* per platform, which changes where the
    /// frame truncates — so no post-hoc replacement can make one golden fit
    /// every runner. The data directory is replaced with a fixed-length
    /// stand-in at the source (see `frame_for`), and the one remaining
    /// platform-dependent frame is scoped per OS instead of pretending to be
    /// portable. Every other tab renders only snapshot state and shares one
    /// golden across platforms.
    fn snapshot_name(tab: Tab) -> String {
        match tab {
            Tab::Settings => format!("tab-{}-{}", tab.index(), std::env::consts::OS),
            other => format!("tab-{}", other.index()),
        }
    }

    #[tokio::test]
    async fn every_tab_renders_the_published_snapshot() {
        for tab in [
            Tab::Dashboard,
            Tab::Proxies,
            Tab::Connections,
            Tab::Logs,
            Tab::Settings,
            Tab::Rules,
        ] {
            let rendered = frame_for(tab, dense_snapshot());
            insta::assert_snapshot!(snapshot_name(tab), rendered);
        }
    }

    /// The rules view is a toggle inside the Logs tab rather than its own page,
    /// which makes it the easiest thing in the client to lose by accident.
    #[tokio::test]
    async fn the_logs_tab_switches_to_the_rules_view() {
        let mut snapshot = dense_snapshot();
        snapshot.rules = vec![
            crate::state::RouteRuleSnapshot {
                kind: crate::state::RuleKind::DomainSuffix,
                value: Some("example.com".to_owned()),
                outbound: "🚀节点选择".to_owned(),
            },
            crate::state::RouteRuleSnapshot {
                kind: crate::state::RuleKind::Private,
                value: None,
                outbound: "direct".to_owned(),
            },
        ];
        insta::assert_snapshot!("logs-tail", frame_for(Tab::Logs, snapshot.clone()));
        // What used to be the panel behind `r` on the Logs page; it is a tab now.
        insta::assert_snapshot!("rules-tab", frame_for(Tab::Rules, snapshot));
    }
}
