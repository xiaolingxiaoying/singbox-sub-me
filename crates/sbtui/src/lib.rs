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
pub use client_core::{clash_api, command, core, settings, state, subscription, system_proxy};

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Sparkline, Table, Tabs,
};

use crate::clash_api::{Connection, SELECTOR_TAG};
use crate::command::SettingsPatch;
use crate::state::{ProxyGroupSnapshot, TrafficPoint};
use crate::subscription::SubscriptionUserinfo;
use crate::system_proxy::TrafficMode;

const TAB_TITLES: [&str; 5] = ["概览", "节点", "连接", "日志", "设置"];
const TICK_MS: u64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Dashboard,
    Proxies,
    Connections,
    Logs,
    Settings,
}

impl Tab {
    fn next(self) -> Self {
        match self {
            Self::Dashboard => Self::Proxies,
            Self::Proxies => Self::Connections,
            Self::Connections => Self::Logs,
            Self::Logs => Self::Settings,
            Self::Settings => Self::Dashboard,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Dashboard => Self::Settings,
            Self::Proxies => Self::Dashboard,
            Self::Connections => Self::Proxies,
            Self::Logs => Self::Connections,
            Self::Settings => Self::Logs,
        }
    }

    fn from_index(index: usize) -> Option<Self> {
        Some(match index {
            0 => Self::Dashboard,
            1 => Self::Proxies,
            2 => Self::Connections,
            3 => Self::Logs,
            4 => Self::Settings,
            _ => return None,
        })
    }

    fn index(self) -> usize {
        match self {
            Self::Dashboard => 0,
            Self::Proxies => 1,
            Self::Connections => 2,
            Self::Logs => 3,
            Self::Settings => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InputGoal {
    ProfileName,
    ProfileUrl,
    ProfileFile,
    ProfileEditUrl,
    CoreVersion,
    Mirror,
    AutoUpdate,
    MixedPort,
    TestUrl,
    /// A substring filter over the connection table.
    ConnFilter,
    /// A substring filter over the kernel log tail.
    LogQuery,
}

/// Connection-table sort order, cycled with `S`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnSort {
    Download,
    Upload,
    Host,
    Target,
}

impl ConnSort {
    fn next(self) -> Self {
        match self {
            Self::Download => Self::Upload,
            Self::Upload => Self::Host,
            Self::Host => Self::Target,
            Self::Target => Self::Download,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Download => "下载",
            Self::Upload => "上传",
            Self::Host => "主机",
            Self::Target => "目标",
        }
    }
}

/// Kernel log level filter, cycled with `l` on the Logs tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogFilter {
    All,
    Info,
    Warn,
    Error,
}

impl LogFilter {
    fn next(self) -> Self {
        match self {
            Self::All => Self::Info,
            Self::Info => Self::Warn,
            Self::Warn => Self::Error,
            Self::Error => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Info => "info+",
            Self::Warn => "warn+",
            Self::Error => "error",
        }
    }

    /// Whether a kernel log line passes the filter. Lines are matched on the
    /// uppercase level token sing-box emits (`INFO`/`WARN`/`ERROR`).
    fn matches(self, line: &str) -> bool {
        let upper = line.to_ascii_uppercase();
        match self {
            Self::All => true,
            Self::Info => {
                upper.contains("INFO") || upper.contains("WARN") || upper.contains("ERROR")
            }
            Self::Warn => upper.contains("WARN") || upper.contains("ERROR"),
            Self::Error => upper.contains("ERROR"),
        }
    }
}

/// The terminal client's own state. Everything the engine owns (the core, the
/// settings, the profiles, live traffic) lives in `snapshot`; the fields below
/// are only view state: which row is highlighted, what filter is active, and
/// which overlay is open.
struct App {
    controller: ClientController,
    snapshot: ClientSnapshot,
    dir: PathBuf,
    tab: Tab,
    // Proxies tab.
    selected_group: usize,
    /// Highlighted member inside the selected proxy group.
    selected_member: usize,
    group_list: ListState,
    /// The group index whose member highlight was last synced to its current
    /// node. While it matches `selected_group`, ticks leave the highlight where
    /// the user put it instead of snapping back to the running node.
    proxies_synced_group: Option<usize>,
    // Connections tab.
    /// Highlighted row in the connection table (independent from the proxy
    /// member highlight).
    selected_connection: usize,
    conn_sort: ConnSort,
    /// Case-insensitive substring filter for the connection table.
    conn_filter: String,
    // Logs tab.
    /// Toggle between the live log tail (default) and a rendering of the
    /// active configuration's routing rules.
    show_rules: bool,
    /// Freeze the log tail so the view can be inspected while lines stream.
    paused_logs: Option<Vec<String>>,
    log_filter: LogFilter,
    /// Case-insensitive substring filter applied on top of the level filter.
    log_query: String,
    // Settings tab.
    profiles_list: ListState,
    /// A profile name awaiting a second Delete press.
    confirm_delete: Option<String>,
    // Input overlay.
    input: Option<InputGoal>,
    pending_profile_name: Option<String>,
    input_text: String,
    // Confirmations.
    confirm_mode: bool,
    confirm_quit: bool,
    /// A discoverable keyboard reference overlay for first-run users.
    show_help: bool,
    /// The status line the footer shows. Engine status changes overwrite it
    /// (tracked via `last_engine_status`); UI-level messages (filters,
    /// confirmations) write it directly.
    status: String,
    last_engine_status: String,
}

impl App {
    fn new(controller: ClientController, dir: PathBuf) -> Self {
        let snapshot = controller.snapshot();
        let status = snapshot.status.clone();
        Self {
            controller,
            snapshot,
            dir,
            tab: Tab::Dashboard,
            selected_group: 0,
            selected_member: 0,
            group_list: ListState::default(),
            proxies_synced_group: None,
            selected_connection: 0,
            conn_sort: ConnSort::Download,
            conn_filter: String::new(),
            show_rules: false,
            paused_logs: None,
            log_filter: LogFilter::All,
            log_query: String::new(),
            profiles_list: ListState::default(),
            confirm_delete: None,
            input: None,
            pending_profile_name: None,
            input_text: String::new(),
            confirm_mode: false,
            confirm_quit: false,
            show_help: false,
            status,
            last_engine_status: String::new(),
        }
    }

    /// Pulls the latest engine state. Called on every tick; the UI never
    /// touches the core or the network itself.
    fn refresh_snapshot(&mut self) {
        self.snapshot = self.controller.snapshot();
        if self.snapshot.status != self.last_engine_status {
            self.last_engine_status = self.snapshot.status.clone();
            self.status = self.snapshot.status.clone();
        }
        // Keep the display order stable across the engine's periodic
        // connection refreshes; the engine publishes in core order.
        let sort = self.conn_sort;
        self.snapshot
            .connections
            .connections
            .sort_by(|a, b| conn_sort_key(a, b, sort));
        self.sync_proxy_selection();
    }

    /// Mirrors the engine's proxy groups into the highlight state: snap the
    /// member highlight to the running node the first time a group is shown
    /// (or right after switching groups), then keep the user's selection.
    fn sync_proxy_selection(&mut self) {
        let groups = &self.snapshot.proxy_groups;
        if self.selected_group >= groups.len() {
            self.selected_group = 0;
            self.proxies_synced_group = None;
        }
        if groups.is_empty() {
            self.proxies_synced_group = None;
            self.selected_member = 0;
            return;
        }
        let selected = self.selected_group;
        let synced = self.proxies_synced_group == Some(selected);
        let group = &groups[selected];
        let current_position = group
            .members
            .iter()
            .position(|member| member == &group.current);
        let member_count = group.members.len();
        if !synced {
            self.selected_member = current_position.unwrap_or(0);
            self.proxies_synced_group = Some(selected);
        } else if member_count == 0 {
            self.selected_member = 0;
        } else if self.selected_member >= member_count {
            self.selected_member = member_count - 1;
        }
        self.group_list.select(Some(selected));
    }

    fn selected_group_snapshot(&self) -> Option<&ProxyGroupSnapshot> {
        self.snapshot.proxy_groups.get(self.selected_group)
    }

    /// The reported delay for the node the dashboard highlights, from any
    /// group that carries it.
    fn node_delay(&self, node: &str) -> Option<u64> {
        self.snapshot
            .proxy_groups
            .iter()
            .find(|group| group.members.iter().any(|member| member == node))
            .and_then(|group| group.delays.get(node).copied())
    }

    fn send(&mut self, command: ClientCommand) {
        self.status = format!("{}…", command.label());
        let _ = self.controller.send(command);
    }
}

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
                            handle_key(&mut app, key.code);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            _ = tick.tick() => {
                app.refresh_snapshot();
            }
        }
        terminal.draw(|frame| draw(frame, &mut app))?;
    }
    // Dropping the controller stops the engine, and the engine's `kill_on_drop`
    // child tears the core down with it. The OS proxy, however, is an
    // OS-wide setting: clear it unless the user explicitly chose to keep it.
    app.refresh_snapshot();
    if app.snapshot.system_proxy_enabled && !app.confirm_quit {
        let _ = system_proxy::disable(&app.dir);
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyCode) {
    if app.show_help {
        if matches!(key, KeyCode::Esc | KeyCode::Char('?')) {
            app.show_help = false;
        }
        return;
    }
    if let Some(goal) = app.input.clone() {
        match key {
            KeyCode::Esc => {
                app.input = None;
                app.input_text.clear();
            }
            KeyCode::Enter => commit_input(app, goal),
            KeyCode::Backspace => {
                app.input_text.pop();
            }
            KeyCode::Char(ch) => app.input_text.push(ch),
            _ => {}
        }
        return;
    }
    // The exit-keep-proxy prompt is cancelled by any key other than the
    // handled quit keys (q / Ctrl+C never reach here).
    app.confirm_quit = false;
    if key != KeyCode::Char('m') {
        app.confirm_mode = false;
    }
    match key {
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Tab => app.tab = app.tab.next(),
        KeyCode::BackTab => app.tab = app.tab.previous(),
        KeyCode::Left if app.tab == Tab::Proxies => move_proxy_group(app, -1),
        KeyCode::Right if app.tab == Tab::Proxies => move_proxy_group(app, 1),
        KeyCode::Char(ch @ '1'..='5') => {
            if let Some(tab) = Tab::from_index(ch as usize - '1' as usize) {
                app.tab = tab;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => move_cursor(app, 1),
        KeyCode::Up | KeyCode::Char('k') => move_cursor(app, -1),
        KeyCode::Enter => select_current(app),
        KeyCode::Char('s') => {
            if app.snapshot.core_running {
                app.send(ClientCommand::StopCore);
            } else {
                app.send(ClientCommand::StartCore);
            }
        }
        KeyCode::Char('p') => app.send(ClientCommand::ToggleSystemProxy),
        KeyCode::Char('m') => toggle_mode(app),
        KeyCode::Char('t') if app.tab == Tab::Proxies => test_current_delay(app),
        KeyCode::Char('T') if app.tab == Tab::Proxies => test_group_delays(app),
        KeyCode::Char('u') => app.send(ClientCommand::UpdateSubscription),
        KeyCode::Char('x') if app.tab == Tab::Connections => close_selected_connection(app),
        KeyCode::Char('X') if app.tab == Tab::Connections => close_all_connections(app),
        KeyCode::Char('S') if app.tab == Tab::Connections => {
            app.conn_sort = app.conn_sort.next();
            let sort = app.conn_sort;
            app.snapshot
                .connections
                .connections
                .sort_by(|a, b| conn_sort_key(a, b, sort));
            app.status = format!("连接排序: {}", app.conn_sort.label());
        }
        KeyCode::Char('f') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::ProfileFile);
            app.input_text.clear();
        }
        KeyCode::Char('e') if app.tab == Tab::Settings => {
            if let Some(name) = selected_profile_name(app) {
                app.pending_profile_name = Some(name);
                app.input = Some(InputGoal::ProfileEditUrl);
                app.input_text.clear();
            }
        }
        KeyCode::Delete if app.tab == Tab::Settings => delete_selected_profile(app),
        KeyCode::Char('n') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::ProfileName);
            app.input_text.clear();
        }
        KeyCode::Char('v') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::CoreVersion);
            app.input_text = app.snapshot.settings.core_version.clone();
        }
        KeyCode::Char('r') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::Mirror);
            app.input_text = app.snapshot.settings.mirror.clone();
        }
        KeyCode::Char('d') if app.tab == Tab::Settings => app.send(ClientCommand::DownloadCore),
        KeyCode::Char('r') if app.tab == Tab::Logs => {
            app.show_rules = !app.show_rules;
        }
        KeyCode::Char('o') => {
            let next = app.snapshot.outbound_mode.next();
            app.send(ClientCommand::SetOutboundMode(next));
        }
        KeyCode::Char(' ') if app.tab == Tab::Logs => {
            if app.paused_logs.is_some() {
                app.paused_logs = None;
                app.status = "日志已恢复".to_owned();
            } else {
                app.paused_logs = Some(current_log_lines(app));
                app.status = "日志已暂停（Space 恢复）".to_owned();
            }
        }
        KeyCode::Char('l') if app.tab == Tab::Logs => {
            app.log_filter = app.log_filter.next();
            app.status = format!("日志级别过滤: {}", app.log_filter.label());
        }
        KeyCode::Char('c') if app.tab == Tab::Logs => {
            let line = current_log_lines(app)
                .iter()
                .rev()
                .find(|line| {
                    app.log_filter.matches(line) && log_query_matches(&app.log_query, line)
                })
                .or_else(|| app.snapshot.events.back())
                .cloned();
            match line {
                Some(line) => {
                    copy_to_clipboard_osc52(&line);
                    app.status = "已复制当前行到剪贴板（OSC52）".to_owned();
                }
                None => app.status = "没有可复制的日志行".to_owned(),
            }
        }
        KeyCode::Char('/') if app.tab == Tab::Connections => {
            app.input = Some(InputGoal::ConnFilter);
            app.input_text = app.conn_filter.clone();
            app.status = "输入连接过滤关键字（留空 = 显示全部）".to_owned();
        }
        KeyCode::Char('/') if app.tab == Tab::Logs => {
            app.input = Some(InputGoal::LogQuery);
            app.input_text = app.log_query.clone();
            app.status = "输入日志关键字（留空 = 不过滤）".to_owned();
        }
        KeyCode::Char('a') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::AutoUpdate);
            app.input_text = app.snapshot.settings.auto_update_minutes.to_string();
            app.status = "自动更新间隔（分钟，0 = 关闭）".to_owned();
        }
        KeyCode::Char('P') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::MixedPort);
            app.input_text = app.snapshot.settings.mixed_port.to_string();
            app.status = "本地混合代理端口（1024–65535）".to_owned();
        }
        KeyCode::Char('U') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::TestUrl);
            app.input_text = app.snapshot.settings.test_url.clone();
            app.status = "延迟测试地址".to_owned();
        }
        KeyCode::Char('g') if app.tab == Tab::Settings => {
            let enabled = !app.snapshot.settings.auto_start;
            app.send(ClientCommand::UpdateSettings(SettingsPatch {
                auto_start: Some(enabled),
                ..Default::default()
            }));
            app.status = format!("启动时自动启动内核: {}", if enabled { "开" } else { "关" });
        }
        KeyCode::Char('y') if app.tab == Tab::Settings => {
            let enabled = !app.snapshot.settings.auto_system_proxy;
            app.send(ClientCommand::UpdateSettings(SettingsPatch {
                auto_system_proxy: Some(enabled),
                ..Default::default()
            }));
            app.status = format!(
                "内核就绪后自动开启系统代理: {}",
                if enabled { "开" } else { "关" }
            );
        }
        _ => {}
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

/// The input overlay's Enter: applies the committed value, either directly on
/// the UI state (filters) or as a command for the engine (everything that
/// touches the settings, profiles, or the core).
fn commit_input(app: &mut App, goal: InputGoal) {
    let text = app.input_text.trim().to_owned();
    app.input = None;
    app.input_text.clear();
    match goal {
        InputGoal::ProfileName => {
            app.pending_profile_name = Some(text);
            app.input = Some(InputGoal::ProfileUrl);
            app.status = "输入订阅链接（任意 sbctl 订阅后缀都会自动归一化）".to_owned();
        }
        InputGoal::ProfileUrl => {
            let name = app.pending_profile_name.take().unwrap_or_default();
            if name.is_empty() || text.is_empty() {
                app.status = "档案名与链接都不能为空".to_owned();
                return;
            }
            app.send(ClientCommand::ImportSubscription {
                name: Some(name),
                url: text,
            });
        }
        InputGoal::ProfileFile => {
            app.send(ClientCommand::ImportProfileFile(text));
        }
        InputGoal::ProfileEditUrl => {
            let name = app.pending_profile_name.take().unwrap_or_default();
            if text.is_empty() {
                app.status = "已取消编辑".to_owned();
                return;
            }
            app.send(ClientCommand::SetProfileUrl { name, url: text });
        }
        InputGoal::CoreVersion => {
            app.send(ClientCommand::UpdateSettings(SettingsPatch {
                core_version: Some(text.trim_start_matches('v').to_owned()),
                ..Default::default()
            }));
            app.status = "内核版本已保存；按 d 下载".to_owned();
        }
        InputGoal::Mirror => {
            app.send(ClientCommand::UpdateSettings(SettingsPatch {
                mirror: Some(text),
                ..Default::default()
            }));
            app.status = "镜像前缀已保存".to_owned();
        }
        InputGoal::AutoUpdate => {
            let minutes = if text.is_empty() {
                0
            } else {
                match text.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => {
                        app.status = "自动更新间隔必须是整数分钟".to_owned();
                        return;
                    }
                }
            };
            app.send(ClientCommand::UpdateSettings(SettingsPatch {
                auto_update_minutes: Some(minutes),
                ..Default::default()
            }));
            app.status = if minutes == 0 {
                "已关闭订阅自动更新".to_owned()
            } else {
                format!("订阅每 {minutes} 分钟自动更新")
            };
        }
        InputGoal::MixedPort => match text.parse::<u16>() {
            Ok(port) if port >= 1024 => {
                app.send(ClientCommand::UpdateSettings(SettingsPatch {
                    mixed_port: Some(port),
                    ..Default::default()
                }));
                app.status = format!("混合代理端口已设为 {port}（重启内核生效）");
            }
            _ => app.status = "端口必须是 1024–65535 之间的整数".to_owned(),
        },
        InputGoal::TestUrl => {
            if text.is_empty() {
                app.status = "延迟测试地址不能为空".to_owned();
            } else {
                app.send(ClientCommand::UpdateSettings(SettingsPatch {
                    test_url: Some(text),
                    ..Default::default()
                }));
                app.status = "延迟测试地址已保存".to_owned();
            }
        }
        InputGoal::ConnFilter => {
            app.conn_filter = text;
            app.selected_connection = 0;
            app.status = if app.conn_filter.is_empty() {
                "已清除连接过滤".to_owned()
            } else {
                format!("连接过滤: {}", app.conn_filter)
            };
        }
        InputGoal::LogQuery => {
            app.log_query = text;
            app.status = if app.log_query.is_empty() {
                "已清除日志关键字".to_owned()
            } else {
                format!("日志关键字: {}", app.log_query)
            };
        }
    }
}

fn move_cursor(app: &mut App, delta: isize) {
    match app.tab {
        Tab::Proxies => move_proxy_member(app, delta),
        Tab::Connections => {
            let len = visible_connections(app).len();
            if len > 0 {
                app.selected_connection = ((app.selected_connection as isize + delta)
                    .clamp(0, len as isize - 1)) as usize;
            } else {
                app.selected_connection = 0;
            }
        }
        Tab::Settings if !app.snapshot.profiles.is_empty() => {
            let len = app.snapshot.profiles.len();
            let current = app.profiles_list.selected().unwrap_or(0) as isize;
            let next = (current + delta).clamp(0, len as isize - 1) as usize;
            app.profiles_list.select(Some(next));
        }
        Tab::Settings => {
            app.profiles_list.select(Some(0));
        }
        _ => {}
    }
}

/// Moves the member highlight inside the selected proxy group (`↑↓` / `j k`).
fn move_proxy_member(app: &mut App, delta: isize) {
    let member_count = app
        .snapshot
        .proxy_groups
        .get(app.selected_group)
        .map(|group| group.members.len())
        .unwrap_or(0);
    if member_count == 0 {
        return;
    }
    app.selected_member =
        ((app.selected_member as isize + delta).clamp(0, member_count as isize - 1)) as usize;
}

/// Switches the selected proxy group (`←→`) and snaps the member highlight to
/// that group's current node.
fn move_proxy_group(app: &mut App, delta: isize) {
    if app.snapshot.proxy_groups.is_empty() {
        return;
    }
    let len = app.snapshot.proxy_groups.len() as isize;
    let next = ((app.selected_group as isize + delta).clamp(0, len - 1)) as usize;
    let position = app.snapshot.proxy_groups[next]
        .members
        .iter()
        .position(|member| member == &app.snapshot.proxy_groups[next].current)
        .unwrap_or(0);
    app.selected_group = next;
    app.selected_member = position;
    app.proxies_synced_group = Some(next);
    app.group_list.select(Some(next));
}

fn select_current(app: &mut App) {
    match app.tab {
        Tab::Settings => {
            let name = app
                .profiles_list
                .selected()
                .and_then(|index| app.snapshot.profiles.get(index))
                .map(|profile| profile.name.clone());
            if let Some(name) = name {
                app.send(ClientCommand::SwitchProfile(name));
            }
        }
        Tab::Proxies => {
            if app.snapshot.core_running
                && let Some(group) = app.selected_group_snapshot()
                && let Some(member) = group.members.get(app.selected_member).cloned()
            {
                let group = group.name.clone();
                app.send(ClientCommand::SwitchNode {
                    group,
                    node: member,
                });
            }
        }
        _ => {}
    }
}

fn test_current_delay(app: &mut App) {
    if !app.snapshot.core_running {
        return;
    }
    let Some(node) = app
        .selected_group_snapshot()
        .and_then(|group| group.members.get(app.selected_member).cloned())
    else {
        return;
    };
    app.send(ClientCommand::TestNode(node));
}

/// Tests every member of the selected group. The engine issues all probes at
/// once and polls them together, so the result returns in roughly one timeout.
fn test_group_delays(app: &mut App) {
    if !app.snapshot.core_running {
        return;
    }
    let Some(group) = app
        .selected_group_snapshot()
        .map(|group| group.name.clone())
    else {
        return;
    };
    app.send(ClientCommand::TestGroup(group));
}

/// Connections after the connection-table filter is applied, in display order.
fn visible_connections(app: &App) -> Vec<&Connection> {
    app.snapshot
        .connections
        .connections
        .iter()
        .filter(|connection| connection.matches(&app.conn_filter))
        .collect()
}

fn close_selected_connection(app: &mut App) {
    if !app.snapshot.core_running {
        return;
    }
    let Some(connection) = visible_connections(app)
        .get(app.selected_connection)
        .map(|connection| (*connection).clone())
    else {
        return;
    };
    app.send(ClientCommand::CloseConnection(connection.id));
}

fn close_all_connections(app: &mut App) {
    if !app.snapshot.core_running {
        return;
    }
    if app.snapshot.connections.connections.is_empty() {
        app.status = "没有可关闭的连接".to_owned();
        return;
    }
    app.send(ClientCommand::CloseAllConnections);
}

/// The comparison behind the connection-table sort, shared by the `S` cycle
/// and the per-tick re-sort that keeps the display order stable.
fn conn_sort_key(a: &Connection, b: &Connection, sort: ConnSort) -> std::cmp::Ordering {
    match sort {
        ConnSort::Download => b.download.cmp(&a.download),
        ConnSort::Upload => b.upload.cmp(&a.upload),
        ConnSort::Host => a
            .metadata
            .destination_host
            .cmp(&b.metadata.destination_host),
        ConnSort::Target => a
            .metadata
            .destination_ip
            .cmp(&b.metadata.destination_ip)
            .then_with(|| {
                a.metadata
                    .destination_port
                    .cmp(&b.metadata.destination_port)
            }),
    }
}

fn selected_profile_name(app: &App) -> Option<String> {
    app.profiles_list
        .selected()
        .and_then(|index| app.snapshot.profiles.get(index))
        .map(|profile| profile.name.clone())
}

/// Deletes the selected profile; the first Delete arms, the second confirms.
/// The engine refuses to delete the active profile while the core runs.
fn delete_selected_profile(app: &mut App) {
    let Some(name) = selected_profile_name(app) else {
        return;
    };
    if app.confirm_delete.as_deref() != Some(name.as_str()) {
        app.confirm_delete = Some(name.clone());
        app.status = format!("再按 Delete 确认删除档案 {name}（不可撤销）");
        return;
    }
    app.confirm_delete = None;
    app.send(ClientCommand::RemoveProfile(name));
}

/// The mode switch (`m`): a two-step confirmation, then the engine persists
/// the traffic mode. TUN additionally needs admin/root and wintun.dll; the
/// engine enforces both at start time and reports a precise error.
fn toggle_mode(app: &mut App) {
    let target = match app.snapshot.traffic_mode {
        TrafficMode::SystemProxy => TrafficMode::Tun,
        TrafficMode::Tun => TrafficMode::SystemProxy,
    };
    if !app.confirm_mode {
        app.confirm_mode = true;
        app.status = format!(
            "再按 m 确认切换到 {}（下次启动内核时生效；TUN 需要管理员/root 权限）",
            target.label()
        );
        return;
    }
    app.confirm_mode = false;
    app.send(ClientCommand::SetTrafficMode(target));
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

const PANEL: Color = Color::Rgb(11, 28, 42);
const EDGE: Color = Color::Rgb(37, 92, 116);
const TEXT: Color = Color::Rgb(217, 235, 242);
const MUTED: Color = Color::Rgb(123, 157, 172);
const CYAN: Color = Color::Rgb(42, 213, 235);
const MINT: Color = Color::Rgb(101, 235, 157);
const AMBER: Color = Color::Rgb(255, 190, 81);
const DANGER: Color = Color::Rgb(255, 104, 97);

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
        Tab::Logs => "Space 暂停  ·  l 级别  ·  / 关键字  ·  c 复制  ·  r 分流规则",
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

fn panel<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
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

fn selected_node(app: &App) -> String {
    app.snapshot
        .proxy_groups
        .iter()
        .find(|group| group.name == SELECTOR_TAG)
        .map(|group| group.current.clone())
        .or_else(|| app.snapshot.current_node.clone())
        .unwrap_or_else(|| "等待选择节点".to_owned())
}

fn short_label(value: &str, max: usize) -> String {
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

fn meter(value: u64, width: usize) -> String {
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

fn status_color(status: &str) -> Color {
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
    let body = vec![
        Line::from(input_label(goal)).style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from(app.input_text.clone()).style(Style::default().fg(TEXT)),
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
}

fn draw_help_overlay(frame: &mut Frame, app: &App) {
    let page_keys = match app.tab {
        Tab::Dashboard => "s 启动/停止 · u 更新订阅 · p 开/关系统代理 · m 切换模式",
        Tab::Proxies => "↑↓ 选节点 · ←→ 切组 · Enter 切换 · t 测当前 · T 测全组",
        Tab::Connections => "↑↓ 选择 · x 关闭连接 · X 关闭全部 · S 切换排序 · / 关键字过滤",
        Tab::Logs => "Space 暂停 · l 切换级别 · / 关键字过滤 · c 复制 · r 查看规则",
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

fn delay_color(delay: Option<u64>) -> Color {
    match delay {
        Some(d) if d < 200 => MINT,
        Some(d) if d < 500 => AMBER,
        Some(_) => DANGER,
        None => MUTED,
    }
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
            usage_label(app.snapshot.subscription_usage),
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
        history.len() as u64 * TICK_MS / 1000
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
            Cell::from(if index == app.selected_connection {
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

fn draw_logs(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
    if app.show_rules {
        let lines: Vec<Line> = rules_lines(app).into_iter().map(Line::from).collect();
        frame.render_widget(
            Paragraph::new(lines).block(panel("分流规则 · r 返回日志")),
            area,
        );
        return;
    }
    let lines: Vec<Line> = log_page_lines(app).into_iter().map(Line::from).collect();
    let title = format!(
        "日志 [{}]{}（Space 暂停，l 级别，/ 关键字，c 复制，r 规则）",
        app.log_filter.label(),
        if app.paused_logs.is_some() {
            " · 已暂停"
        } else {
            ""
        }
    );
    let title = if app.log_query.is_empty() {
        title
    } else {
        format!("{title} · 关键字「{}」", app.log_query)
    };
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
            usage_label(app.snapshot.subscription_usage)
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

fn human_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A one-line rendering of the subscription's traffic metadata.
fn usage_label(usage: Option<SubscriptionUserinfo>) -> String {
    let Some(usage) = usage else {
        return "未知（更新订阅后显示）".to_owned();
    };
    let mut text = match usage.remaining() {
        Some(remaining) => format!(
            "已用 {} / {}（剩余 {}）",
            human_bytes(usage.used()),
            human_bytes(usage.total),
            human_bytes(remaining)
        ),
        None => format!("已用 {}（未设配额）", human_bytes(usage.used())),
    };
    if let Some(expire) = usage.expire {
        let now = now_epoch();
        if expire > now {
            text.push_str(&format!(" · {} 天后重置", (expire - now) / 86_400));
        } else {
            text.push_str(" · 已到期");
        }
    }
    text
}

fn age_label(epoch_seconds: u64) -> String {
    if epoch_seconds == 0 {
        return "从未".to_owned();
    }
    let age = now_epoch().saturating_sub(epoch_seconds);
    if age < 3600 {
        format!("{} 分钟前", age / 60)
    } else if age < 86_400 {
        format!("{} 小时前", age / 3600)
    } else {
        format!("{} 天前", age / 86_400)
    }
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_progresses_through_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn tabs_cycle_in_both_directions() {
        assert_eq!(Tab::Dashboard.next(), Tab::Proxies);
        assert_eq!(Tab::Settings.next(), Tab::Dashboard);
        assert_eq!(Tab::Dashboard.previous(), Tab::Settings);
        assert_eq!(Tab::from_index(2), Some(Tab::Connections));
        assert_eq!(Tab::from_index(9), None);
    }

    #[test]
    fn age_label_distinguishes_never_from_recent() {
        assert_eq!(age_label(0), "从未");
        let now = now_epoch();
        assert_eq!(age_label(now - 90), "1 分钟前");
    }

    #[test]
    fn meter_keeps_the_requested_visual_width() {
        assert_eq!(meter(0, 12).chars().count(), 12);
        assert_eq!(meter(1024 * 1024, 12).chars().count(), 12);
    }

    #[test]
    fn log_query_matches_are_case_insensitive() {
        assert!(log_query_matches("DNS", "[123] inbound/dns: lookup"));
        assert!(!log_query_matches("DNS", "[123] outbound/tcp: connect"));
        assert!(log_query_matches("", "anything"));
    }

    #[test]
    fn usage_label_reports_the_reset_window() {
        let usage = subscription::SubscriptionUserinfo {
            upload: 0,
            download: 0,
            total: 1024,
            expire: Some(now_epoch() + 3 * 86_400),
        };
        let label = usage_label(Some(usage));
        assert!(label.contains("3 天后重置"), "unexpected label: {label}");
        let expired = subscription::SubscriptionUserinfo {
            expire: Some(now_epoch().saturating_sub(10)),
            ..usage
        };
        assert!(usage_label(Some(expired)).contains("已到期"));
    }

    fn connection(host: &str, rule: &str) -> Connection {
        Connection {
            id: host.to_owned(),
            metadata: clash_api::ConnectionMetadata {
                destination_host: host.to_owned(),
                ..Default::default()
            },
            rule: rule.to_owned(),
            ..Default::default()
        }
    }

    fn group(name: &str, current: &str, members: &[&str]) -> ProxyGroupSnapshot {
        ProxyGroupSnapshot {
            name: name.to_owned(),
            kind: "Selector".to_owned(),
            current: current.to_owned(),
            members: members.iter().map(|member| (*member).to_owned()).collect(),
            delays: Default::default(),
            failed: Vec::new(),
        }
    }

    #[tokio::test]
    async fn view_state_separates_from_engine_state() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        app.snapshot.proxy_groups = vec![group("g", "b", &["a", "b", "c"])];
        app.selected_group = 0;
        app.selected_member = 1;

        move_proxy_member(&mut app, 1);
        assert_eq!(app.selected_member, 2);
        move_proxy_member(&mut app, 1);
        assert_eq!(app.selected_member, 2, "clamps at the last member");
        move_proxy_member(&mut app, -1);
        assert_eq!(app.selected_member, 1);

        app.selected_connection = 0;
        assert_eq!(app.selected_member, 1, "connection state is separate");
    }

    #[tokio::test]
    async fn proxy_group_switch_snaps_member_to_the_current_node() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        app.snapshot.proxy_groups = vec![
            group("one", "a", &["a", "b"]),
            group("two", "y", &["x", "y", "z"]),
        ];
        app.selected_group = 0;
        app.selected_member = 1;

        move_proxy_group(&mut app, 1);
        assert_eq!(app.selected_group, 1);
        assert_eq!(app.selected_member, 1, "snaps to the group's current node");
    }

    #[tokio::test]
    async fn connection_filter_keeps_only_matching_rows() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        app.snapshot.connections.connections = vec![
            connection("api.github.com", "Proxy"),
            connection("cdn.example.net", "DIRECT"),
        ];
        app.conn_filter = "github".to_owned();
        let visible = visible_connections(&app);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].metadata.destination_host, "api.github.com");

        app.conn_filter = "direct".to_owned();
        assert_eq!(visible_connections(&app).len(), 1);

        app.conn_filter.clear();
        assert_eq!(visible_connections(&app).len(), 2);
    }

    #[tokio::test]
    async fn connection_sort_orders_by_download_then_target() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        let mut heavy = connection("heavy.example.net", "Proxy");
        heavy.download = 500;
        let mut light = connection("light.example.net", "Proxy");
        light.download = 100;
        app.snapshot.connections.connections = vec![light, heavy];
        app.conn_sort = ConnSort::Download;
        app.snapshot
            .connections
            .connections
            .sort_by(|a, b| conn_sort_key(a, b, app.conn_sort));
        assert_eq!(
            app.snapshot.connections.connections[0]
                .metadata
                .destination_host,
            "heavy.example.net"
        );
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
