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
mod view;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::app::App;
use crate::format::{age_label, human_bytes, usage_label};
use crate::input::handle_key;
use crate::style::{CYAN, panel, short_label};
use crate::view::draw;

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

/// Whether a kernel log line contains the active keyword filter.
fn log_query_matches(query: &str, line: &str) -> bool {
    query.is_empty() || line.to_lowercase().contains(&query.to_lowercase())
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
    use crate::app::{LogFilter, Tab};

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
