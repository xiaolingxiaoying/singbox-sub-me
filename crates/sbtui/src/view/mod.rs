//! The frame every tab is drawn into: the outer chrome (header, footer), the three
//! overlays, and [`draw`], which picks the page for the active tab.

mod connections;
mod dashboard;
pub(crate) mod logs;
pub(crate) mod proxies;
pub(crate) mod settings;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs};

use crate::app::{App, InputGoal, Tab};
use crate::format::human_bytes;
use crate::style::{AMBER, CYAN, EDGE, MINT, MUTED, TEXT, panel, short_label, status_color};
use crate::system_proxy::TrafficMode;
use crate::view::connections::draw_connections;
use crate::view::dashboard::draw_dashboard;
use crate::view::logs::{draw_logs, tail_within_width};
use crate::view::proxies::{draw_proxies, selected_node};
use crate::view::settings::draw_settings;

const TAB_TITLES: [&str; 5] = ["概览", "节点", "连接", "日志", "设置"];

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
    };
    let line = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(&app.status, Style::default().fg(status_color(&app.status))),
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
