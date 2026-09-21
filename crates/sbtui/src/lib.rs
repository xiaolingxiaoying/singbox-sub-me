//! sbtui — a terminal UI sing-box proxy client.
//!
//! Tabs (Tab / number keys): dashboard, proxies, connections, logs, settings.
//! Everything runs on the keyboard: start/stop the core, switch nodes, run
//! latency tests, toggle the system proxy or TUN mode, and manage
//! subscription profiles.
//!
//! The terminal client is a pure renderer over `client_core::ClientController`
//! — the exact control plane the desktop client uses. A background engine owns
//! the sing-box core, the clash_api channel and the persisted settings; this
//! UI only draws [`ClientSnapshot`] and sends [`ClientCommand`]s, so the two
//! clients cannot drift apart.

pub use client_core::{ClientCommand, ClientController, ClientError, ClientEvent, ClientSnapshot};
/// The shared control plane lives in `client-core` so the desktop client can
/// reuse exactly the same clash_api client, core manager, settings store,
/// subscription handling and OS-proxy integration. Re-exported at the crate
/// root so existing `crate::clash_api::…` paths keep working.
pub use client_core::{
    clash_api, command, core, format, settings, state, subscription, system_proxy,
};

mod app;
mod input;
mod style;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Sparkline, Table, Tabs,
};

use crate::app::{App, InputGoal, Tab};
use crate::clash_api::SELECTOR_TAG;
use crate::format::{age_label, human_bytes, usage_label};
use crate::input::{handle_key, selected_connection_index, visible_connections};
use crate::state::TrafficPoint;
use crate::style::{
    AMBER, CYAN, DANGER, EDGE, MINT, MUTED, TEXT, delay_color, meter, panel, short_label,
    status_color,
};
use crate::system_proxy::TrafficMode;

const TAB_TITLES: [&str; 5] = ["概览", "节点", "连接", "日志", "设置"];
const TICK_MS: u64 = 500;

#[derive(Parser)]
#[command(name = "sbtui", version, about = "终端 sing-box 代理客户端")]
struct Cli {
    /// Print the resolved data directory and exit (for debugging).
    #[arg(long)]
    print_dir: bool,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    let dir = settings::data_dir()?;
    if cli.print_dir {
        println!("{}", dir.display());
        return Ok(());
    }
    // The mixed port and the OS proxy are machine-global, so a second instance
    // would fight the first over both. Held for the whole function.
    let _instance = match settings::acquire_instance_lock(&dir)? {
        Some(lock) => lock,
        None => {
            eprintln!("另一个 sbtui 实例正在使用 {}，请先退出它。", dir.display());
            return Ok(());
        }
    };
    let mut terminal = ratatui::init();
    // The engine starts here (including the `auto_start` bring-up), so the
    // terminal client and the desktop client boot the core identically.
    let controller = ClientController::start(dir.clone());
    let result = run_app(&mut terminal, controller, dir).await;
    ratatui::restore();
    result
}

async fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    controller: ClientController,
    dir: PathBuf,
) -> Result<()> {
    let mut app = App::new(controller, dir);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
    // Every exit path (including a failed read or draw) has to reach the
    // teardown below, so the loop records the error and breaks instead.
    let mut loop_error: Option<anyhow::Error> = None;

    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { break };
                match event {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        // The quit keys only quit outside the help and input
                        // overlays; inside them `q` is a plain character for
                        // the search box or the profile-name editor.
                        let quit_requested = !app.show_help
                            && app.input.is_none()
                            && ((key.code == KeyCode::Char('q') && key.modifiers.is_empty())
                                || (key.code == KeyCode::Char('c')
                                    && key.modifiers.contains(KeyModifiers::CONTROL)));
                        if quit_requested {
                            if app.snapshot.system_proxy_enabled && !app.confirm_quit {
                                app.confirm_quit = true;
                                app.status =
                                    "系统代理仍开启：再按 q 退出并保留代理设置，或按 p 关闭后退出"
                                        .to_owned();
                            } else {
                                break;
                            }
                        } else {
                            handle_key(&mut app, key);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        loop_error = Some(error.into());
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                app.refresh_snapshot();
            }
        }
        if let Err(error) = terminal.draw(|frame| draw(frame, &mut app)) {
            loop_error = Some(error.into());
            break;
        }
    }
    // The OS proxy is a machine-wide setting: clear it unless the user
    // explicitly chose to keep it. Then wait for the engine to reap the core —
    // `kill_on_drop` only fires if that task is scheduled again, and returning
    // from here ends the process.
    app.refresh_snapshot();
    if app.snapshot.system_proxy_enabled && !app.confirm_quit {
        let _ = system_proxy::disable(&app.dir);
    }
    app.controller.shutdown();
    match loop_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// The log lines the Logs page currently shows: the frozen copy while paused,
/// otherwise the engine's live kernel tail.
fn current_log_lines(app: &App) -> Vec<String> {
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

/// Renders the engine-published routing rules and rule-set sources. The rules
/// arrive with the snapshot, so this view works before the core starts too.
fn rules_lines(app: &App) -> Vec<String> {
    if app.snapshot.rules.is_empty() && app.snapshot.rule_sets.is_empty() {
        return vec!["（尚无激活配置；先启动一次内核）".to_owned()];
    }
    let mut lines = Vec::new();
    if !app.snapshot.rule_sets.is_empty() {
        lines.push("── 规则集 ──".to_owned());
        for set in &app.snapshot.rule_sets {
            let source = if set.url.is_empty() {
                "（本地）".to_owned()
            } else {
                short_label(&set.url, 64)
            };
            lines.push(format!("• {}（{}） ← {}", set.tag, set.kind, source));
        }
        lines.push(String::new());
    }
    lines.push("── 分流规则（自上而下匹配）──".to_owned());
    for (index, rule) in app.snapshot.rules.iter().enumerate() {
        lines.push(format!(
            "{:>2}. {} → {}",
            index + 1,
            rule.matcher,
            if rule.outbound.is_empty() {
                "（动作）"
            } else {
                &rule.outbound
            }
        ));
    }
    lines
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

// ---------------------------------------------------------------- UI ------

fn draw(frame: &mut Frame, app: &mut App) {
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

fn selected_node(app: &App) -> String {
    app.snapshot
        .proxy_groups
        .iter()
        .find(|group| group.name == SELECTOR_TAG)
        .map(|group| group.current.clone())
        .or_else(|| app.snapshot.current_node.clone())
        .unwrap_or_else(|| "等待选择节点".to_owned())
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

/// The end of `text` that fits in `width` display columns.
fn tail_within_width(text: &str, width: usize) -> String {
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

/// Whether a kernel log line contains the active keyword filter.
fn log_query_matches(query: &str, line: &str) -> bool {
    query.is_empty() || line.to_lowercase().contains(&query.to_lowercase())
}

fn draw_dashboard(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
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

fn draw_proxies(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
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

fn draw_connections(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
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

fn draw_logs(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
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

fn draw_settings(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    let items: Vec<ListItem> = app
        .snapshot
        .profiles
        .iter()
        .map(|profile| {
            ListItem::new(format!(
                "{}{}（更新于 {}）",
                if profile.active { "✓ " } else { "  " },
                profile.name,
                age_label(profile.last_updated)
            ))
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(panel("订阅档案 · Enter 激活"))
            .highlight_style(
                Style::default()
                    .fg(Color::Rgb(8, 18, 28))
                    .bg(CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        columns[0],
        &mut app.profiles_list,
    );
    let settings = &app.snapshot.settings;
    let core_path = settings::core_path(&app.dir);
    let info = vec![
        Line::from(format!(
            "内核: {}",
            if app.snapshot.core_installed {
                core_path.display().to_string()
            } else {
                "未安装".into()
            }
        )),
        Line::from(format!(
            "版本: {}   镜像: {}",
            app.snapshot
                .core_version
                .clone()
                .unwrap_or_else(|| "未检测".into()),
            if settings.mirror.is_empty() {
                "直连"
            } else {
                settings.mirror.as_str()
            }
        )),
        Line::from(format!(
            "运行版本: {}   内存: {}",
            app.snapshot
                .core_runtime_version
                .clone()
                .unwrap_or_else(|| "未运行".into()),
            if app.snapshot.core_running {
                human_bytes(app.snapshot.memory_used)
            } else {
                "-".to_owned()
            }
        )),
        Line::from(format!(
            "模式: {}   混合端口: {}   延迟地址: {}",
            app.snapshot.traffic_mode.label(),
            settings.mixed_port,
            settings.test_url
        )),
        Line::from(format!(
            "自动更新: {}   启动内核: {}   自动系统代理: {}",
            if settings.auto_update_minutes == 0 {
                "关".to_owned()
            } else {
                format!("{} 分钟", settings.auto_update_minutes)
            },
            if settings.auto_start { "开" } else { "关" },
            if settings.auto_system_proxy {
                "开"
            } else {
                "关"
            }
        )),
        Line::from(format!(
            "系统代理后端: {}   订阅用量: {}",
            system_proxy::platform_label(),
            usage_label(app.snapshot.subscription_usage.as_ref())
        )),
        Line::from(""),
        Line::from("档案: n 新增 ｜ f 本地文件 ｜ e 改链接 ｜ Delete 删除 ｜ Enter 激活"),
        Line::from("内核: v 改版本 ｜ r 改镜像 ｜ d 下载 ｜ u 更新订阅"),
        Line::from("选项: a 自动更新 ｜ P 混合端口 ｜ U 延迟地址 ｜ g 启动内核 ｜ y 自动代理"),
    ];
    frame.render_widget(Paragraph::new(info).block(panel("运行环境")), columns[1]);
}

/// Copies text to the terminal clipboard via the OSC 52 escape sequence, which
/// works over SSH and needs no platform-specific clipboard dependency.
fn copy_to_clipboard_osc52(text: &str) {
    use base64::Engine;
    use std::io::Write;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    print!("\x1b]52;c;{encoded}\x07");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LogFilter;

    #[test]
    fn tabs_cycle_in_both_directions() {
        assert_eq!(Tab::Dashboard.next(), Tab::Proxies);
        assert_eq!(Tab::Settings.next(), Tab::Dashboard);
        assert_eq!(Tab::Dashboard.previous(), Tab::Settings);
        assert_eq!(Tab::from_index(2), Some(Tab::Connections));
        assert_eq!(Tab::from_index(9), None);
    }

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

    #[tokio::test]
    async fn rules_lines_render_the_engine_snapshot() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        app.snapshot.rule_sets = vec![client_core::state::RuleSetSummary {
            tag: "geoip-cn".to_owned(),
            url: "https://example/srs".to_owned(),
            kind: "remote".to_owned(),
        }];
        app.snapshot.rules = vec![
            client_core::state::RouteRuleSnapshot {
                matcher: "规则集 · geoip-cn".to_owned(),
                outbound: "🚀节点选择".to_owned(),
            },
            client_core::state::RouteRuleSnapshot {
                matcher: "其他未命中流量".to_owned(),
                outbound: "🚀节点选择".to_owned(),
            },
        ];
        let lines = rules_lines(&app);
        assert!(lines.iter().any(|line| line.contains("geoip-cn")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("1. 规则集 · geoip-cn"))
        );
        assert!(lines.iter().any(|line| line.contains("其他未命中流量")));
    }
}
