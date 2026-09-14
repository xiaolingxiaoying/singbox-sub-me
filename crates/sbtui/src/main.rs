//! sbtui — a terminal UI sing-box proxy client.
//!
//! Tabs (Tab / number keys): dashboard, proxies, connections, logs, settings.
//! Everything runs on the keyboard: start/stop the core, switch nodes, run
//! latency tests, toggle the system proxy or TUN mode, and manage
//! subscription profiles. The control channel is the clash_api endpoint that
//! the server-side full client profile exposes on 127.0.0.1:9090.

mod clash_api;
mod core;
mod settings;
mod subscription;
mod system_proxy;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Tabs,
};
use tokio::process::Child;

use crate::clash_api::{ClashApi, OutboundMode};
use crate::settings::{Profile, Profiles, Settings};
use crate::system_proxy::TrafficMode;

const TAB_TITLES: [&str; 5] = ["🏠 仪表盘", "🧭 代理", "🔗 连接", "📜 日志", "⚙️ 设置"];
const TICK_MS: u64 = 500;
const LOG_LINES: usize = 500;
const SELECTOR_TAG: &str = "🚀节点选择";

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

struct App {
    tab: Tab,
    dir: PathBuf,
    settings: Settings,
    profiles: Profiles,
    core_path: Option<PathBuf>,
    core_version: Option<String>,
    core_child: Option<Child>,
    running: bool,
    mode: TrafficMode,
    api: ClashApi,
    groups: Vec<clash_api::ProxyGroup>,
    selected_group: usize,
    selected_member: usize,
    delays: HashMap<String, u64>,
    mode_outbound: OutboundMode,
    system_proxy_on: bool,
    up: u64,
    down: u64,
    total_up: u64,
    total_down: u64,
    last_totals: Option<(u64, u64, Instant)>,
    connections: clash_api::ConnectionsSnapshot,
    conn_sort: ConnSort,
    subscription_usage: Option<subscription::SubscriptionUserinfo>,
    logs: VecDeque<String>,
    /// Kernel log tail state: the Logs page streams `cache/core.log` from
    /// this byte offset while the core runs.
    core_logs: VecDeque<String>,
    core_log_offset: u64,
    last_auto_update: Option<Instant>,
    /// The Logs tab toggles between the live log tail (default) and a static
    /// rendering of the active configuration's routing rules.
    show_rules: bool,
    rules_lines: Vec<String>,
    input: Option<InputGoal>,
    pending_profile_name: Option<String>,
    /// A profile name awaiting a second Delete press.
    confirm_delete: Option<String>,
    input_text: String,
    status: String,
    profiles_list: ListState,
    group_list: ListState,
    /// Automatic restart state after an unexpected core exit.
    restart_attempts: u32,
    restart_at: Option<Instant>,
    /// Pending confirmations for the mode switch and the exit-keep-proxy prompt.
    confirm_mode: bool,
    confirm_quit: bool,
    /// Logs page: pause the live tail and filter by level.
    log_paused: bool,
    log_filter: LogFilter,
}

impl App {
    fn new(dir: PathBuf, settings: Settings, profiles: Profiles) -> Self {
        let core_path = settings::core_path(&dir);
        let core_path = core_path.is_file().then_some(core_path);
        let core_version = core_path
            .as_deref()
            .and_then(|path| core::detect_version(path).ok());
        Self {
            tab: Tab::Dashboard,
            dir,
            settings,
            profiles,
            core_path,
            core_version,
            core_child: None,
            running: false,
            mode: TrafficMode::SystemProxy,
            api: ClashApi::new(clash_api::DEFAULT_CONTROLLER),
            groups: Vec::new(),
            selected_group: 0,
            selected_member: 0,
            delays: HashMap::new(),
            mode_outbound: OutboundMode::Rule,
            system_proxy_on: false,
            up: 0,
            down: 0,
            total_up: 0,
            total_down: 0,
            last_totals: None,
            connections: Default::default(),
            conn_sort: ConnSort::Download,
            subscription_usage: None,
            logs: VecDeque::with_capacity(LOG_LINES),
            show_rules: false,
            rules_lines: Vec::new(),
            core_logs: VecDeque::with_capacity(LOG_LINES),
            core_log_offset: 0,
            last_auto_update: None,
            input: None,
            pending_profile_name: None,
            confirm_delete: None,
            input_text: String::new(),
            status: "就绪。先在「设置」导入订阅，再按 s 启动内核。".to_owned(),
            profiles_list: ListState::default(),
            group_list: ListState::default(),
            restart_attempts: 0,
            restart_at: None,
            confirm_mode: false,
            confirm_quit: false,
            log_paused: false,
            log_filter: LogFilter::All,
        }
    }

    fn log(&mut self, message: impl Into<String>) {
        self.logs.push_back(message.into());
        while self.logs.len() > LOG_LINES {
            self.logs.pop_front();
        }
    }

    /// Appends the kernel's log-file growth to the Logs page. Reads from the
    /// last consumed byte offset so each tick only picks up new lines; the
    /// file is small (core stderr) so a synchronous read at tick cadence is
    /// fine.
    fn tail_core_log(&mut self) {
        if self.log_paused {
            return;
        }
        let path = self.dir.join("cache/core.log");
        let Ok(meta) = std::fs::metadata(&path) else {
            return;
        };
        let size = meta.len();
        if size <= self.core_log_offset {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let start = usize::try_from(self.core_log_offset.min(text.len() as u64)).unwrap_or(0);
        for line in text[start..].lines() {
            if !line.is_empty() {
                self.core_logs.push_back(line.to_owned());
            }
        }
        while self.core_logs.len() > LOG_LINES {
            self.core_logs.pop_front();
        }
        self.core_log_offset = size;
    }

    /// Whether the configured auto-update interval has elapsed since the last
    /// subscription refresh (auto-update only runs while the core is active).
    fn auto_update_due(&mut self) -> bool {
        let minutes = self.settings.auto_update_minutes;
        if minutes == 0 || self.profiles.active.is_none() {
            return false;
        }
        let interval = Duration::from_secs(minutes * 60);
        let due = match self.last_auto_update {
            Some(last) => last.elapsed() >= interval,
            None => true,
        };
        if due {
            self.last_auto_update = Some(Instant::now());
        }
        due
    }

    /// Whether the core process exited on its own (not a user stop). On a
    /// crash the restart is scheduled with exponential backoff.
    fn watch_core_exit(&mut self) -> bool {
        let Some(child) = self.core_child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                self.core_child = None;
                self.log(format!("内核意外退出（{status}），准备自动重启"));
                self.schedule_restart();
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.log(format!("检测内核状态失败: {error}"));
                false
            }
        }
    }

    /// Clears the live state and schedules a backed-off restart. The system
    /// proxy is turned off because it would otherwise point at a dead core.
    fn schedule_restart(&mut self) {
        self.running = false;
        self.groups.clear();
        self.connections = Default::default();
        if self.system_proxy_on {
            let _ = system_proxy::disable();
            self.system_proxy_on = false;
        }
        let attempt = self.restart_attempts;
        self.restart_attempts = attempt.saturating_add(1);
        let delay = core::restart_backoff(attempt);
        self.restart_at = Some(Instant::now() + delay);
        self.status = format!(
            "内核崩溃；{} 秒后自动重启（第 {} 次）",
            delay.as_secs(),
            attempt + 1
        );
    }

    async fn refresh_proxies(&mut self) {
        if !self.running {
            self.groups.clear();
            return;
        }
        match self.api.proxies().await {
            Ok((groups, _nodes)) => {
                if let Some(position) = groups
                    .iter()
                    .find(|g| g.name == SELECTOR_TAG)
                    .and_then(|selector| selector.all.iter().position(|m| m == &selector.now))
                {
                    self.selected_member = position;
                }
                self.groups = groups;
                self.mode_outbound = self.api.mode().await.unwrap_or(OutboundMode::Rule);
            }
            Err(error) => self.log(format!("刷新代理组失败: {error}")),
        }
    }

    async fn refresh_connections(&mut self) {
        if !self.running {
            self.connections = Default::default();
            return;
        }
        // Prefer the streaming /traffic endpoint for the live rate; fall back
        // to the connection-total delta when the endpoint is unavailable.
        let traffic_ok = match self.api.traffic().await {
            Ok(sample) => {
                self.up = sample.up;
                self.down = sample.down;
                true
            }
            Err(_) => false,
        };
        match self.api.connections().await {
            Ok(snapshot) => {
                let now = Instant::now();
                let totals = (snapshot.upload_total, snapshot.download_total);
                if let Some((last_up, last_down, last_time)) = self.last_totals {
                    let elapsed = now.duration_since(last_time).as_secs_f64();
                    if !traffic_ok && elapsed > 0.05 {
                        self.up = (totals.0.saturating_sub(last_up) as f64 / elapsed) as u64;
                        self.down = (totals.1.saturating_sub(last_down) as f64 / elapsed) as u64;
                    }
                }
                self.last_totals = Some((totals.0, totals.1, now));
                self.total_up = totals.0;
                self.total_down = totals.1;
                self.connections = snapshot;
                sort_connections(self);
            }
            Err(error) => self.log(format!("刷新连接失败: {error}")),
        }
    }
}

#[derive(Parser)]
#[command(name = "sbtui", version, about = "终端 sing-box 代理客户端")]
struct Cli {
    /// Print the resolved data directory and exit (for debugging).
    #[arg(long)]
    print_dir: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = settings::data_dir()?;
    if cli.print_dir {
        println!("{}", dir.display());
        return Ok(());
    }
    let settings = Settings::load_or_create(&dir)?;
    let profiles = Profiles::load_or_create(&dir)?;

    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal, dir, settings, profiles).await;
    ratatui::restore();
    result
}

async fn run_app(
    terminal: &mut ratatui::DefaultTerminal,
    dir: PathBuf,
    settings: Settings,
    profiles: Profiles,
) -> Result<()> {
    let mut app = App::new(dir, settings, profiles);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));

    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { break };
                match event {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        let quit_requested = (key.code == KeyCode::Char('q')
                            && key.modifiers.is_empty())
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL));
                        if quit_requested {
                            if app.system_proxy_on && !app.confirm_quit {
                                app.confirm_quit = true;
                                app.status =
                                    "系统代理仍开启：再按 q 退出并保留代理设置，或按 p 关闭后退出"
                                        .to_owned();
                            } else {
                                break;
                            }
                        } else {
                            handle_key(&mut app, key.code).await?;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            _ = tick.tick() => {
                if app.running {
                    if app.watch_core_exit() {
                        // A crash schedules the backed-off restart itself.
                    } else {
                        app.refresh_connections().await;
                        if app.groups.is_empty() {
                            app.refresh_proxies().await;
                        }
                        app.tail_core_log();
                        if app.auto_update_due()
                            && let Err(error) = update_subscription(&mut app).await
                        {
                            app.log(format!("自动更新订阅失败: {error}"));
                        }
                    }
                } else if app.restart_at.is_some_and(|at| Instant::now() >= at) {
                    app.restart_at = None;
                    if let Err(error) = start_core(&mut app).await {
                        app.log(format!("自动重启失败: {error}"));
                        app.schedule_restart();
                    }
                }
            }
        }
        terminal.draw(|frame| draw(frame, &mut app))?;
    }
    // Stop the core. The OS proxy is cleared only when the user did not ask to
    // keep it in the exit confirmation.
    if let Some(mut child) = app.core_child.take() {
        let _ = child.kill().await;
    }
    if app.system_proxy_on && !app.confirm_quit {
        let _ = system_proxy::disable();
    }
    Ok(())
}

async fn start_core(app: &mut App) -> Result<()> {
    if app.mode == TrafficMode::Tun {
        if !system_proxy::can_use_tun() {
            anyhow::bail!(
                "TUN 模式需要管理员/root 权限；请以管理员身份运行终端，或按 m 切回系统代理模式"
            );
        }
        if cfg!(windows) && !settings::wintun_path(&app.dir).is_file() {
            anyhow::bail!(
                "TUN 模式需要 wintun.dll：请在「设置」按 d 重新下载内核，或手动放入 {}",
                settings::wintun_path(&app.dir).display()
            );
        }
    }
    let profile = app
        .profiles
        .active_profile()
        .context("没有激活的订阅档案；先在「设置」导入")?
        .clone();
    let core = app
        .core_path
        .clone()
        .context("没有 sing-box 内核；在「设置」按 d 下载")?;
    let cache = settings::profile_cache_path(&app.dir, &profile.name);
    let raw = tokio::fs::read_to_string(&cache)
        .await
        .context("读取订阅缓存失败；先按 u 更新订阅")?;
    let adapted = core::adapt_inbounds(&raw, app.mode)?;
    let active = app.dir.join("cache/active-config.json");
    tokio::fs::write(&active, adapted).await?;
    core::check_config(&core, &active)?;
    let handle = core::start(&core, &active, &app.dir.join("cache/core.log")).await?;
    app.core_child = Some(handle.child);

    for _ in 0..20 {
        if app.api.alive().await {
            app.running = true;
            app.restart_attempts = 0;
            app.restart_at = None;
            if app.mode == TrafficMode::SystemProxy {
                match system_proxy::enable(system_proxy::LOCAL_MIXED_PORT) {
                    Ok(()) => {
                        app.system_proxy_on = true;
                        app.status = format!(
                            "内核已启动；系统代理 → 127.0.0.1:{}",
                            system_proxy::LOCAL_MIXED_PORT
                        );
                    }
                    Err(error) => app.status = format!("内核已启动；系统代理设置失败: {error}"),
                }
            } else {
                app.status = "内核已启动（TUN 模式）。".to_owned();
            }
            let status = app.status.clone();
            app.log(status);
            app.refresh_proxies().await;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if let Some(mut child) = app.core_child.take() {
        let _ = child.kill().await;
    }
    app.log("内核已启动但 clash_api 未就绪；确认订阅配置包含 clash_api。");
    app.status = "内核未就绪（clash_api 未响应）".to_owned();
    Ok(())
}

async fn stop_core(app: &mut App) -> Result<()> {
    app.running = false;
    app.restart_at = None;
    app.restart_attempts = 0;
    if app.system_proxy_on {
        let _ = system_proxy::disable();
        app.system_proxy_on = false;
    }
    if let Some(mut child) = app.core_child.take() {
        let _ = child.kill().await;
    }
    app.groups.clear();
    app.connections = Default::default();
    app.status = "内核已停止。".to_owned();
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

async fn update_subscription(app: &mut App) -> Result<()> {
    let (name, url, mirror) = {
        let profile = app
            .profiles
            .active_profile()
            .context("没有激活的订阅档案")?;
        (
            profile.name.clone(),
            profile.url.clone(),
            app.settings.mirror.clone(),
        )
    };
    app.status = format!("正在更新订阅 {name}…");
    let fetched = match subscription::fetch(&url, &mirror).await {
        Ok(fetched) => fetched,
        Err(error) => {
            // Offline fallback: keep serving the previous cache rather than
            // failing the update and stranding the user with nothing.
            let cache = settings::profile_cache_path(&app.dir, &name);
            if tokio::fs::try_exists(&cache).await.unwrap_or(false) {
                app.status = format!("订阅更新失败（{error}）；继续使用上次缓存");
                let status = app.status.clone();
                app.log(status);
                return Ok(());
            }
            return Err(error);
        }
    };
    app.subscription_usage = fetched.userinfo;
    let snapshot = subscription::parse(&fetched.body)?;
    let cache = settings::profile_cache_path(&app.dir, &name);
    tokio::fs::write(&cache, &snapshot.raw).await?;
    for profile in &mut app.profiles.profiles {
        if profile.name == name {
            profile.last_updated = now_epoch();
        }
    }
    app.profiles.save(&app.dir)?;
    app.status = format!("订阅已更新（{} 个节点）", snapshot.nodes.len());
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

async fn download_core(app: &mut App) -> Result<()> {
    app.status = "正在下载 sing-box 内核…".to_owned();
    let target = app.dir.join("core");
    let version = app.settings.core_version.clone();
    let mirror = app.settings.mirror.clone();
    let path = core::download_core(&target, &version, &mirror).await?;
    app.core_path = Some(path.clone());
    app.core_version = core::detect_version(&path).ok();
    app.status = format!(
        "内核已安装: {}",
        app.core_version.clone().unwrap_or_default()
    );
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

async fn handle_key(app: &mut App, key: KeyCode) -> Result<()> {
    if let Some(goal) = app.input.clone() {
        match key {
            KeyCode::Esc => {
                app.input = None;
                app.input_text.clear();
            }
            KeyCode::Enter => commit_input(app, goal).await?,
            KeyCode::Backspace => {
                app.input_text.pop();
            }
            KeyCode::Char(ch) => app.input_text.push(ch),
            _ => {}
        }
        return Ok(());
    }
    // The exit-keep-proxy prompt is cancelled by any key other than the
    // handled quit keys (q / Ctrl+C never reach here).
    app.confirm_quit = false;
    if key != KeyCode::Char('m') {
        app.confirm_mode = false;
    }
    match key {
        KeyCode::Tab => app.tab = app.tab.next(),
        KeyCode::BackTab => app.tab = app.tab.previous(),
        KeyCode::Char(ch @ '1'..='5') => {
            if let Some(tab) = Tab::from_index(ch as usize - '1' as usize) {
                app.tab = tab;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => move_cursor(app, 1),
        KeyCode::Up | KeyCode::Char('k') => move_cursor(app, -1),
        KeyCode::Enter => select_current(app).await?,
        KeyCode::Char('s') => {
            if app.running {
                stop_core(app).await?;
            } else if let Err(error) = start_core(app).await {
                app.status = format!("启动失败: {error}");
                let status = app.status.clone();
                app.log(status);
            }
        }
        KeyCode::Char('p') => toggle_system_proxy(app)?,
        KeyCode::Char('m') => toggle_mode(app),
        KeyCode::Char('t') => test_current_delay(app).await,
        KeyCode::Char('T') => test_group_delays(app).await,
        KeyCode::Char('u') => {
            if let Err(error) = update_subscription(app).await {
                app.status = format!("订阅更新失败: {error}");
                let status = app.status.clone();
                app.log(status);
            }
        }
        KeyCode::Char('x') => close_selected_connection(app).await,
        KeyCode::Char('X') => close_all_connections(app).await,
        KeyCode::Char('S') => {
            app.conn_sort = app.conn_sort.next();
            sort_connections(app);
            app.status = format!("连接排序: {}", app.conn_sort.label());
        }
        KeyCode::Char('f') => {
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
        KeyCode::Delete if app.tab == Tab::Settings => delete_selected_profile(app)?,
        KeyCode::Char('n') => {
            app.input = Some(InputGoal::ProfileName);
            app.input_text.clear();
        }
        KeyCode::Char('v') => {
            app.input = Some(InputGoal::CoreVersion);
            app.input_text.clear();
        }
        KeyCode::Char('r') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::Mirror);
            app.input_text.clear();
        }
        KeyCode::Char('d') => {
            if let Err(error) = download_core(app).await {
                app.status = format!("内核下载失败: {error}");
                let status = app.status.clone();
                app.log(status);
            }
        }
        KeyCode::Char('r') if app.tab == Tab::Logs => {
            app.show_rules = !app.show_rules;
            if app.show_rules {
                load_rules(app);
            }
        }
        KeyCode::Char('o') if app.running => match app.api.set_mode(app.mode_outbound.next()).await
        {
            Ok(()) => {
                app.mode_outbound = app.mode_outbound.next();
                app.status = format!("出站模式: {}", app.mode_outbound.label());
            }
            Err(error) => app.status = format!("切换出站模式失败: {error}"),
        },
        KeyCode::Char(' ') if app.tab == Tab::Logs => {
            app.log_paused = !app.log_paused;
            app.status = if app.log_paused {
                "日志已暂停（Space 恢复）".to_owned()
            } else {
                "日志已恢复".to_owned()
            };
        }
        KeyCode::Char('l') if app.tab == Tab::Logs => {
            app.log_filter = app.log_filter.next();
            app.status = format!("日志级别过滤: {}", app.log_filter.label());
        }
        KeyCode::Char('c') if app.tab == Tab::Logs => {
            let line = app
                .core_logs
                .iter()
                .rev()
                .find(|line| app.log_filter.matches(line))
                .or_else(|| app.logs.back())
                .cloned();
            match line {
                Some(line) => {
                    copy_to_clipboard_osc52(&line);
                    app.status = "已复制当前行到剪贴板（OSC52）".to_owned();
                }
                None => app.status = "没有可复制的日志行".to_owned(),
            }
        }
        _ => {}
    }
    Ok(())
}

/// Loads a human-readable rendering of the active configuration's routing
/// rules and rule-set sources into `rules_lines`.
fn load_rules(app: &mut App) {
    let active = app.dir.join("cache/active-config.json");
    let Ok(text) = std::fs::read_to_string(&active) else {
        app.rules_lines = vec!["（尚无激活配置；先启动一次内核）".to_owned()];
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        app.rules_lines = vec!["（激活配置不是有效的 JSON）".to_owned()];
        return;
    };
    let mut lines = Vec::new();
    if let Some(rule_sets) = value
        .get("route")
        .and_then(|r| r.get("rule_set"))
        .and_then(|r| r.as_array())
    {
        lines.push("── 远程规则集 ──".to_owned());
        for set in rule_sets {
            let tag = set.get("tag").and_then(|v| v.as_str()).unwrap_or("?");
            let url = set.get("url").and_then(|v| v.as_str()).unwrap_or("");
            lines.push(format!("• {tag} ← {url}"));
        }
        lines.push(String::new());
    }
    lines.push("── 分流规则（自上而下匹配）──".to_owned());
    if let Some(rules) = value
        .get("route")
        .and_then(|r| r.get("rules"))
        .and_then(|r| r.as_array())
    {
        for (index, rule) in rules.iter().enumerate() {
            let target = rule
                .get("outbound")
                .or_else(|| rule.get("action"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let matcher =
                if let Some(suffixes) = rule.get("domain_suffix").and_then(|v| v.as_array()) {
                    let list: Vec<String> = suffixes
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    format!("域名后缀 {}", list.join(", "))
                } else if let Some(sets) = rule.get("rule_set").and_then(|v| v.as_array()) {
                    let list: Vec<String> = sets
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    format!("规则集 {}", list.join(", "))
                } else if rule.get("ip_is_private").is_some() {
                    "私有地址".to_owned()
                } else if let Some(protocol) = rule.get("protocol").and_then(|v| v.as_str()) {
                    format!("协议 {protocol}")
                } else {
                    "其他".to_owned()
                };
            let target = if target.is_empty() {
                "（动作）"
            } else {
                target
            };
            lines.push(format!("{:>2}. {} → {}", index + 1, matcher, target));
        }
    }
    if let Some(final_outbound) = value
        .get("route")
        .and_then(|r| r.get("final"))
        .and_then(|v| v.as_str())
    {
        lines.push(format!("兜底（final）→ {final_outbound}"));
    }
    app.rules_lines = lines;
}

fn move_cursor(app: &mut App, delta: isize) {
    match app.tab {
        Tab::Proxies if !app.groups.is_empty() => {
            let len = app.groups.len();
            app.selected_group =
                ((app.selected_group as isize + delta).clamp(0, len as isize - 1)) as usize;
            app.selected_member = 0;
        }
        Tab::Proxies => {}
        Tab::Connections if !app.connections.connections.is_empty() => {
            let len = app.connections.connections.len();
            app.selected_member =
                ((app.selected_member as isize + delta).clamp(0, len as isize - 1)) as usize;
        }
        Tab::Connections => {}
        Tab::Settings if !app.profiles.profiles.is_empty() => {
            let len = app.profiles.profiles.len();
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

async fn select_current(app: &mut App) -> Result<()> {
    match app.tab {
        Tab::Settings => {
            let name = app
                .profiles_list
                .selected()
                .and_then(|index| app.profiles.profiles.get(index))
                .map(|profile| profile.name.clone());
            if let Some(name) = name {
                app.profiles.activate(&name);
                app.profiles.save(&app.dir)?;
                app.status = format!("已激活档案 {name}");
            }
        }
        Tab::Proxies => {
            if app.running
                && let Some(group) = app.groups.get(app.selected_group).cloned()
                && let Some(member) = group.all.get(app.selected_member).cloned()
            {
                app.api.select(&group.name, &member).await?;
                app.status = format!("{} → {}", group.name, member);
                let status = app.status.clone();
                app.log(status);
            }
        }
        _ => {}
    }
    Ok(())
}

async fn test_current_delay(app: &mut App) {
    if !app.running {
        return;
    }
    let Some(group) = app.groups.get(app.selected_group).cloned() else {
        return;
    };
    let Some(node) = group.all.get(app.selected_member).cloned() else {
        return;
    };
    let delay = app.api.delay(&node).await.ok();
    if let Some(delay) = delay {
        app.delays.insert(node, delay);
    }
}

async fn test_group_delays(app: &mut App) {
    if !app.running {
        return;
    }
    let Some(group) = app.groups.get(app.selected_group).cloned() else {
        return;
    };
    app.status = format!("正在测试 {} 组延迟…", group.name);
    for member in &group.all {
        if let Ok(delay) = app.api.delay(member).await {
            app.delays.insert(member.clone(), delay);
        }
    }
    app.status = "延迟测试完成".to_owned();
}

async fn close_selected_connection(app: &mut App) {
    if !app.running {
        return;
    }
    let Some(connection) = app
        .connections
        .connections
        .get(app.selected_member)
        .cloned()
    else {
        return;
    };
    if let Err(error) = app.api.close_connection(&connection.id).await {
        app.status = format!("关闭连接失败: {error}");
    }
}

/// Closes every active connection (bound to `X`).
async fn close_all_connections(app: &mut App) {
    if !app.running {
        return;
    }
    let ids: Vec<String> = app
        .connections
        .connections
        .iter()
        .map(|connection| connection.id.clone())
        .collect();
    if ids.is_empty() {
        app.status = "没有可关闭的连接".to_owned();
        return;
    }
    let mut closed = 0;
    for id in &ids {
        if app.api.close_connection(id).await.is_ok() {
            closed += 1;
        }
    }
    app.status = format!("已关闭 {closed}/{} 条连接", ids.len());
    let status = app.status.clone();
    app.log(status);
}

/// Re-orders the connection table in place for the active [`ConnSort`].
fn sort_connections(app: &mut App) {
    let sort = app.conn_sort;
    app.connections.connections.sort_by(|a, b| match sort {
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
    });
}

fn selected_profile_name(app: &App) -> Option<String> {
    app.profiles_list
        .selected()
        .and_then(|index| app.profiles.profiles.get(index))
        .map(|profile| profile.name.clone())
}

/// Deletes the selected profile; the first Delete arms, the second confirms.
fn delete_selected_profile(app: &mut App) -> Result<()> {
    let Some(name) = selected_profile_name(app) else {
        return Ok(());
    };
    if app.confirm_delete.as_deref() != Some(name.as_str()) {
        app.confirm_delete = Some(name.clone());
        app.status = format!("再按 Delete 确认删除档案 {name}（不可撤销）");
        return Ok(());
    }
    app.profiles.profiles.retain(|profile| profile.name != name);
    if app.profiles.active.as_deref() == Some(name.as_str()) {
        app.profiles.active = None;
    }
    app.confirm_delete = None;
    app.profiles.save(&app.dir)?;
    app.status = format!("已删除档案 {name}");
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

fn toggle_system_proxy(app: &mut App) -> Result<()> {
    if app.system_proxy_on {
        system_proxy::disable()?;
        app.system_proxy_on = false;
        app.status = "系统代理已关闭".to_owned();
    } else {
        system_proxy::enable(system_proxy::LOCAL_MIXED_PORT)?;
        app.system_proxy_on = true;
        app.status = format!(
            "系统代理已开启 → 127.0.0.1:{}",
            system_proxy::LOCAL_MIXED_PORT
        );
    }
    Ok(())
}

fn toggle_mode(app: &mut App) {
    if !app.confirm_mode {
        app.confirm_mode = true;
        let target = match app.mode {
            TrafficMode::SystemProxy => TrafficMode::Tun,
            TrafficMode::Tun => TrafficMode::SystemProxy,
        };
        app.status = format!(
            "再按 m 确认切换到 {}（下次启动内核时生效；TUN 需要管理员/root 权限）",
            target.label()
        );
        return;
    }
    app.confirm_mode = false;
    app.mode = match app.mode {
        TrafficMode::SystemProxy => TrafficMode::Tun,
        TrafficMode::Tun => TrafficMode::SystemProxy,
    };
    let privilege_note = if app.mode == TrafficMode::Tun && !system_proxy::can_use_tun() {
        "；⚠ 当前终端没有管理员/root 权限，TUN 启动会被拒绝"
    } else if app.mode == TrafficMode::Tun
        && cfg!(windows)
        && !settings::wintun_path(&app.dir).is_file()
    {
        "；⚠ 缺少 wintun.dll，请先在设置页按 d 重新下载内核"
    } else {
        ""
    };
    app.status = format!(
        "出站方式将切换为 {}（下次启动内核时生效{}）",
        app.mode.label(),
        privilege_note
    );
}

async fn commit_input(app: &mut App, goal: InputGoal) -> Result<()> {
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
                return Ok(());
            }
            let normalized = subscription::normalize_url(&text);
            app.profiles.profiles.push(Profile {
                name,
                url: normalized,
                source: text,
                last_updated: 0,
            });
            let latest = app.profiles.profiles.len() - 1;
            app.profiles.active = Some(app.profiles.profiles[latest].name.clone());
            app.profiles.save(&app.dir)?;
            app.status = format!("档案已添加并激活：{}", app.profiles.profiles[latest].name);
            let status = app.status.clone();
            app.log(status);
        }
        InputGoal::ProfileFile => {
            let path = PathBuf::from(&text);
            if !path.is_file() {
                app.status = format!("本地文件不存在或不可读：{}", path.display());
                return Ok(());
            }
            let body = std::fs::read_to_string(&path).context("读取本地订阅文件失败")?;
            let snapshot = subscription::parse(&body)?;
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("本地订阅")
                .to_owned();
            let cache = settings::profile_cache_path(&app.dir, &name);
            tokio::fs::write(&cache, &snapshot.raw).await?;
            app.profiles.profiles.push(Profile {
                name: name.clone(),
                url: String::new(),
                source: format!("file:{}", path.display()),
                last_updated: now_epoch(),
            });
            app.profiles.active = Some(name.clone());
            app.profiles.save(&app.dir)?;
            app.status = format!(
                "已从文件导入并激活：{name}（{} 个节点）",
                snapshot.nodes.len()
            );
            let status = app.status.clone();
            app.log(status);
        }
        InputGoal::ProfileEditUrl => {
            let name = app.pending_profile_name.take().unwrap_or_default();
            if text.is_empty() {
                app.status = "订阅链接不能为空".to_owned();
                return Ok(());
            }
            let normalized = subscription::normalize_url(&text);
            let Some(profile) = app
                .profiles
                .profiles
                .iter_mut()
                .find(|profile| profile.name == name)
            else {
                app.status = format!("找不到档案 {name}");
                return Ok(());
            };
            profile.url = normalized;
            profile.source = text;
            app.profiles.save(&app.dir)?;
            app.status = format!("已更新档案 {name} 的订阅链接");
            let status = app.status.clone();
            app.log(status);
        }
        InputGoal::CoreVersion => {
            app.settings.core_version = text;
            app.settings.save(&app.dir)?;
            app.status = "内核版本已保存；按 d 下载".to_owned();
        }
        InputGoal::Mirror => {
            app.settings.mirror = text;
            app.settings.save(&app.dir)?;
            app.status = "镜像前缀已保存".to_owned();
        }
    }
    Ok(())
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------- UI ------

fn draw(frame: &mut Frame, app: &mut App) {
    let outer = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());
    let titles: Vec<Line> = TAB_TITLES.iter().map(|t| Line::from(*t)).collect();
    frame.render_widget(
        Tabs::new(titles)
            .select(app.tab.index())
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White)),
        outer[0],
    );
    match app.tab {
        Tab::Dashboard => draw_dashboard(frame, outer[1], app),
        Tab::Proxies => draw_proxies(frame, outer[1], app),
        Tab::Connections => draw_connections(frame, outer[1], app),
        Tab::Logs => draw_logs(frame, outer[1], app),
        Tab::Settings => draw_settings(frame, outer[1], app),
    }
    let status = if let Some(goal) = &app.input {
        format!(
            "输入 {}: {}（Enter 确认 / Esc 取消）",
            input_label(goal),
            app.input_text
        )
    } else {
        format!("{} ｜ Tab 切换页 ｜ q 退出", app.status)
    };
    frame.render_widget(Paragraph::new(status), outer[2]);
}

fn input_label(goal: &InputGoal) -> &'static str {
    match goal {
        InputGoal::ProfileName => "档案名",
        InputGoal::ProfileUrl => "订阅链接",
        InputGoal::ProfileFile => "本地订阅文件路径",
        InputGoal::ProfileEditUrl => "新的订阅链接（留空取消）",
        InputGoal::CoreVersion => "内核版本（留空 = 最新）",
        InputGoal::Mirror => "镜像前缀（留空 = 直连）",
    }
}

fn delay_color(delay: Option<u64>) -> Color {
    match delay {
        Some(d) if d < 200 => Color::Green,
        Some(d) if d < 500 => Color::Yellow,
        Some(_) => Color::Red,
        None => Color::Gray,
    }
}

fn draw_dashboard(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
    let current = app
        .groups
        .iter()
        .find(|g| g.name == SELECTOR_TAG)
        .map(|g| g.now.clone())
        .unwrap_or_else(|| "（未选择）".to_owned());
    let lines = vec![
        Line::from(format!(
            "内核: {}",
            if app.running {
                "运行中"
            } else {
                "已停止"
            }
        )),
        Line::from(format!(
            "出站方式: {} ｜ 出站模式: {}",
            app.mode.label(),
            app.mode_outbound.label()
        )),
        Line::from(format!("当前节点: {current}")),
        Line::from(format!(
            "系统代理: {}",
            if app.system_proxy_on { "开" } else { "关" }
        )),
        Line::from(format!(
            "速率: ↑ {}/s  ↓ {}/s",
            human_bytes(app.up),
            human_bytes(app.down)
        )),
        Line::from(format!(
            "累计: ↑ {}  ↓ {}",
            human_bytes(app.total_up),
            human_bytes(app.total_down)
        )),
        Line::from(format!("订阅流量: {}", usage_label(app.subscription_usage))),
        Line::from(format!(
            "内核版本: {}",
            app.core_version.clone().unwrap_or_else(|| "未检测".into())
        )),
        Line::from(""),
        Line::from(
            "快捷键: s 启动/停止 ｜ p 系统代理 ｜ m 切换 系统代理/TUN ｜ o 出站模式 ｜ u 更新订阅",
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("仪表盘")),
        area,
    );
}

fn draw_proxies(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(area);
    let group_items: Vec<ListItem> = app
        .groups
        .iter()
        .map(|g| ListItem::new(format!("{} ({})", g.name, g.kind)))
        .collect();
    frame.render_stateful_widget(
        List::new(group_items)
            .block(Block::default().borders(Borders::ALL).title("代理组"))
            .highlight_style(Style::default().bg(Color::Blue)),
        columns[0],
        &mut app.group_list,
    );
    if let Some(group) = app.groups.get(app.selected_group).cloned() {
        let member_items: Vec<ListItem> = group
            .all
            .iter()
            .map(|member| {
                let delay = app.delays.get(member).copied();
                let marker = if member == &group.now { "✓ " } else { "  " };
                let delay_text = delay
                    .map(|d| format!("{d}ms"))
                    .unwrap_or_else(|| "-".to_owned());
                ListItem::new(Span::styled(
                    format!("{marker}{member}  [{delay_text}]"),
                    Style::default().fg(delay_color(delay)),
                ))
            })
            .collect();
        let mut member_state = ListState::default().with_selected(Some(app.selected_member));
        frame.render_stateful_widget(
            List::new(member_items)
                .block(Block::default().borders(Borders::ALL).title(format!(
                    "{} 节点（Enter 切换，t 测当前，T 测全组）",
                    group.name
                )))
                .highlight_style(Style::default().bg(Color::Blue)),
            columns[1],
            &mut member_state,
        );
    } else {
        frame.render_widget(Paragraph::new("（无代理组；启动内核后显示）"), columns[1]);
    }
}

fn draw_connections(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
    let rows = app
        .connections
        .connections
        .iter()
        .enumerate()
        .map(|(index, connection)| {
            Row::new(vec![
                Cell::from(if index == app.selected_member {
                    "▸"
                } else {
                    ""
                }),
                Cell::from(format!(
                    "{}:{}",
                    connection.metadata.destination_ip, connection.metadata.destination_port
                )),
                Cell::from(connection.metadata.destination_host.clone()),
                Cell::from(connection.metadata.network.clone()),
                Cell::from(human_bytes(connection.upload)),
                Cell::from(human_bytes(connection.download)),
            ])
        });
    let table = Table::new(
        rows,
        [
            Constraint::Length(2),
            Constraint::Length(24),
            Constraint::Min(20),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(10),
        ],
    )
    .header(
        Row::new(vec!["", "目标", "主机", "网络", "上传", "下载"]).style(Style::default().bold()),
    )
    .block(Block::default().borders(Borders::ALL).title(format!(
        "连接（x 关闭选中，X 关闭全部，S 排序: {}）",
        app.conn_sort.label()
    )));
    frame.render_widget(table, area);
}

fn draw_logs(frame: &mut Frame, area: ratatui::prelude::Rect, app: &App) {
    if app.show_rules {
        let lines: Vec<Line> = app
            .rules_lines
            .iter()
            .map(|line| Line::from(line.clone()))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("分流规则（r 返回日志）"),
            ),
            area,
        );
        return;
    }
    let filtered: Vec<Line> = app
        .core_logs
        .iter()
        .filter(|line| app.log_filter.matches(line))
        .map(|line| Line::from(line.clone()))
        .collect();
    let lines = if filtered.is_empty() {
        app.logs
            .iter()
            .map(|line| Line::from(line.clone()))
            .collect()
    } else {
        filtered
    };
    let title = format!(
        "日志 [{}]{}（Space 暂停，l 级别，c 复制，r 规则）",
        app.log_filter.label(),
        if app.log_paused { " · 已暂停" } else { "" }
    );
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_settings(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    let items: Vec<ListItem> = app
        .profiles
        .profiles
        .iter()
        .map(|profile| {
            let active = app.profiles.active.as_deref() == Some(profile.name.as_str());
            ListItem::new(format!(
                "{}{}（更新于 {}）",
                if active { "✓ " } else { "  " },
                profile.name,
                age_label(profile.last_updated)
            ))
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("订阅档案（Enter 激活）"),
            )
            .highlight_style(Style::default().bg(Color::Blue)),
        columns[0],
        &mut app.profiles_list,
    );
    let info = vec![
        Line::from(format!(
            "内核: {}",
            app.core_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "未安装".into())
        )),
        Line::from(format!(
            "版本: {}",
            app.core_version.clone().unwrap_or_else(|| "未检测".into())
        )),
        Line::from(format!(
            "镜像: {}",
            if app.settings.mirror.is_empty() {
                "直连"
            } else {
                app.settings.mirror.as_str()
            }
        )),
        Line::from(format!(
            "自动更新: {} 分钟",
            app.settings.auto_update_minutes
        )),
        Line::from(format!("系统代理后端: {}", system_proxy::platform_label())),
        Line::from(""),
        Line::from(
            "快捷键: n 新增档案 ｜ f 本地文件 ｜ e 改链接 ｜ Delete 删除档案 ｜ u 更新订阅 ｜ v 改内核版本 ｜ r 改镜像 ｜ d 下载内核",
        ),
    ];
    frame.render_widget(
        Paragraph::new(info).block(Block::default().borders(Borders::ALL).title("设置")),
        columns[1],
    );
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
fn usage_label(usage: Option<subscription::SubscriptionUserinfo>) -> String {
    let Some(usage) = usage else {
        return "未知（更新订阅后显示）".to_owned();
    };
    match usage.remaining() {
        Some(remaining) => format!(
            "已用 {} / {}（剩余 {}）",
            human_bytes(usage.used()),
            human_bytes(usage.total),
            human_bytes(remaining)
        ),
        None => format!("已用 {}（未设配额）", human_bytes(usage.used())),
    }
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
}
