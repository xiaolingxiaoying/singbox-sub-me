//! sbtui — a terminal UI sing-box proxy client.
//!
//! Tabs (Tab / number keys): dashboard, proxies, connections, logs, settings.
//! Everything runs on the keyboard: start/stop the core, switch nodes, run
//! latency tests, toggle the system proxy or TUN mode, and manage
//! subscription profiles. The control channel is the clash_api endpoint that
//! the server-side full client profile exposes on 127.0.0.1:9090.

pub use client_core::{ClientCommand, ClientController, ClientError, ClientEvent, ClientSnapshot};
/// The shared control plane lives in `client-core` so the desktop client can
/// reuse exactly the same clash_api client, core manager, settings store,
/// subscription handling and OS-proxy integration. Re-exported at the crate
/// root so existing `crate::clash_api::…` paths keep working.
pub use client_core::{clash_api, core, settings, subscription, system_proxy};

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
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
use tokio::process::Child;

use crate::clash_api::{ClashApi, OutboundMode, SELECTOR_TAG};
use crate::settings::{Profile, Profiles, Settings};
use crate::system_proxy::TrafficMode;

const TAB_TITLES: [&str; 5] = ["概览", "节点", "连接", "日志", "设置"];
const TICK_MS: u64 = 500;
const LOG_LINES: usize = 500;
/// How many rate samples the dashboard sparkline keeps.
const TRAFFIC_HISTORY: usize = 300;

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
    /// Highlighted member inside the selected proxy group.
    selected_member: usize,
    /// Highlighted row in the connection table (independent from the proxy
    /// member highlight, which `refresh_connections` used to clobber).
    selected_connection: usize,
    delays: HashMap<String, u64>,
    /// Members whose most recent latency test failed or timed out.
    delay_failed: Vec<String>,
    mode_outbound: OutboundMode,
    system_proxy_on: bool,
    up: u64,
    down: u64,
    total_up: u64,
    total_down: u64,
    /// Rolling `(up, down)` rate history for the dashboard sparkline.
    traffic_history: VecDeque<(u64, u64)>,
    last_totals: Option<(u64, u64, Instant)>,
    connections: clash_api::ConnectionsSnapshot,
    conn_sort: ConnSort,
    /// Case-insensitive substring filter for the connection table.
    conn_filter: String,
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
    /// When the proxy groups were last polled (they change rarely).
    proxies_refresh_at: Option<Instant>,
    /// The group index whose member highlight was last synced to its current
    /// node. While it matches `selected_group`, polls leave the highlight where
    /// the user put it instead of snapping back to the running node.
    proxies_synced_group: Option<usize>,
    /// Pending confirmations for the mode switch and the exit-keep-proxy prompt.
    confirm_mode: bool,
    confirm_quit: bool,
    /// Logs page: pause the live tail and filter by level.
    log_paused: bool,
    log_filter: LogFilter,
    /// Case-insensitive substring filter applied on top of the level filter.
    log_query: String,
    /// A discoverable keyboard reference overlay for first-run users.
    show_help: bool,
}

impl App {
    fn new(dir: PathBuf, settings: Settings, profiles: Profiles) -> Self {
        let core_path = settings::core_path(&dir);
        let core_path = core_path.is_file().then_some(core_path);
        let core_version = core_path
            .as_deref()
            .and_then(|path| core::detect_version(path).ok());
        let settings_mode = settings.traffic_mode;
        Self {
            tab: Tab::Dashboard,
            dir,
            settings,
            profiles,
            core_path,
            core_version,
            core_child: None,
            running: false,
            mode: settings_mode,
            api: ClashApi::new(clash_api::DEFAULT_CONTROLLER),
            groups: Vec::new(),
            selected_group: 0,
            selected_member: 0,
            selected_connection: 0,
            delays: HashMap::new(),
            delay_failed: Vec::new(),
            mode_outbound: OutboundMode::Rule,
            system_proxy_on: false,
            up: 0,
            down: 0,
            total_up: 0,
            total_down: 0,
            traffic_history: VecDeque::new(),
            last_totals: None,
            connections: Default::default(),
            conn_sort: ConnSort::Download,
            conn_filter: String::new(),
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
            proxies_refresh_at: None,
            proxies_synced_group: None,
            confirm_mode: false,
            confirm_quit: false,
            log_paused: false,
            log_filter: LogFilter::All,
            log_query: String::new(),
            show_help: false,
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
        if minutes == 0 {
            return false;
        }
        // Local-file profiles have no URL to refresh; skip them silently.
        match self.profiles.active_profile() {
            Some(profile) if !profile.url.trim().is_empty() => {}
            _ => return false,
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
        self.traffic_history.clear();
        self.proxies_refresh_at = None;
        self.proxies_synced_group = None;
        if self.system_proxy_on {
            let _ = system_proxy::disable(&self.dir);
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
            self.proxies_synced_group = None;
            return;
        }
        match self.api.proxies().await {
            Ok((groups, _nodes)) => {
                self.groups = groups;
                if self.selected_group >= self.groups.len() {
                    self.selected_group = 0;
                    self.proxies_synced_group = None;
                }
                if self.groups.is_empty() {
                    self.proxies_synced_group = None;
                    self.selected_member = 0;
                } else {
                    let selected_group = self.selected_group;
                    let synced = self.proxies_synced_group == Some(selected_group);
                    let (current_position, member_count) = {
                        let group = &self.groups[selected_group];
                        (
                            group.all.iter().position(|member| member == &group.now),
                            group.all.len(),
                        )
                    };
                    // Snap the highlight to the running node only the first time
                    // a group is shown (or right after the user switches groups).
                    // Afterwards keep the user's selection, otherwise every poll
                    // would steal the highlight back to the current node.
                    if !synced {
                        self.selected_member = current_position.unwrap_or(0);
                        self.proxies_synced_group = Some(selected_group);
                    } else if member_count == 0 {
                        self.selected_member = 0;
                    } else if self.selected_member >= member_count {
                        self.selected_member = member_count - 1;
                    }
                    // Keep the stateful list widget's own selection aligned with
                    // `selected_group`, otherwise the highlighted row is wrong.
                    self.group_list.select(Some(selected_group));
                }
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
                self.push_traffic_sample();
                sort_connections(self);
                if self.selected_connection >= self.connections.connections.len() {
                    self.selected_connection = 0;
                }
            }
            Err(error) => self.log(format!("刷新连接失败: {error}")),
        }
    }

    /// Appends the current rates to the dashboard history window.
    fn push_traffic_sample(&mut self) {
        self.traffic_history.push_back((self.up, self.down));
        while self.traffic_history.len() > TRAFFIC_HISTORY {
            self.traffic_history.pop_front();
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

pub async fn run() -> Result<()> {
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
    if app.settings.auto_start && app.core_path.is_some() && app.profiles.active.is_some() {
        app.status = "启动时自动启动内核…".to_owned();
        if let Err(error) = start_core(&mut app).await {
            app.status = format!("自动启动失败: {error}");
            let status = app.status.clone();
            app.log(status);
        }
    }
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
                        if app
                            .proxies_refresh_at
                            .is_none_or(|at| at.elapsed() >= Duration::from_secs(3))
                        {
                            app.proxies_refresh_at = Some(Instant::now());
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
        let _ = system_proxy::disable(&app.dir);
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
                "TUN 模式需要 wintun.dll：请将该文件放入 {}",
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
    let adapted = core::adapt_inbounds(&raw, app.mode, app.settings.mixed_port)?;
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
            if app.mode == TrafficMode::SystemProxy && app.settings.auto_system_proxy {
                match system_proxy::enable(&app.dir, app.settings.mixed_port) {
                    Ok(()) => {
                        app.system_proxy_on = true;
                        app.status = format!(
                            "内核已启动；系统代理 → 127.0.0.1:{}",
                            app.settings.mixed_port
                        );
                    }
                    Err(error) => app.status = format!("内核已启动；系统代理设置失败: {error}"),
                }
            } else if app.mode == TrafficMode::Tun {
                app.status = "内核已启动（TUN 模式）。".to_owned();
            } else {
                app.status = "内核已启动；系统代理未自动开启（按 p 开启）".to_owned();
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
        let _ = system_proxy::disable(&app.dir);
        app.system_proxy_on = false;
    }
    if let Some(mut child) = app.core_child.take() {
        let _ = child.kill().await;
    }
    app.groups.clear();
    app.connections = Default::default();
    app.traffic_history.clear();
    app.proxies_refresh_at = None;
    app.proxies_synced_group = None;
    app.status = "内核已停止。".to_owned();
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

async fn update_subscription(app: &mut App) -> Result<()> {
    let (name, url, source, mirror) = {
        let profile = app
            .profiles
            .active_profile()
            .context("没有激活的订阅档案")?;
        (
            profile.name.clone(),
            profile.url.clone(),
            profile.source.clone(),
            app.settings.mirror.clone(),
        )
    };
    if url.trim().is_empty() {
        anyhow::bail!("档案 {name} 是本地文件导入，没有订阅链接；按 e 设置链接或按 f 重新导入");
    }
    app.status = format!("正在更新订阅 {name}…");
    let (fetched, used_bare_compatibility) =
        match fetch_subscription_with_compatibility(&url, &source, &mirror).await {
            Ok(result) => result,
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
    let snapshot = subscription::parse(&fetched.body)?;
    app.subscription_usage = fetched.userinfo;
    let cache = settings::profile_cache_path(&app.dir, &name);
    tokio::fs::write(&cache, &snapshot.raw).await?;
    for profile in &mut app.profiles.profiles {
        if profile.name == name {
            profile.last_updated = now_epoch();
        }
    }
    app.profiles.save(&app.dir)?;
    app.status = if used_bare_compatibility {
        format!(
            "订阅已更新（{} 个节点；已兼容旧版裸节点端点）",
            snapshot.nodes.len()
        )
    } else {
        format!("订阅已更新（{} 个节点）", snapshot.nodes.len())
    };
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

/// Fetch a full client profile first. Old sbctl deployments may expose only
/// the original `sing-box.json` node list; when that exact source is known,
/// retry it and let `subscription::parse` construct the required local
/// runtime wrapper.
async fn fetch_subscription_with_compatibility(
    url: &str,
    source: &str,
    mirror: &str,
) -> Result<(subscription::Fetched, bool)> {
    match subscription::fetch(url, mirror).await {
        Ok(fetched) => Ok((fetched, false)),
        Err(primary_error) => {
            let Some(fallback_url) = subscription::bare_sing_box_fallback_url(source) else {
                return Err(primary_error);
            };
            if fallback_url == url {
                return Err(primary_error);
            }
            match subscription::fetch(&fallback_url, mirror).await {
                Ok(fetched) => Ok((fetched, true)),
                Err(fallback_error) => {
                    Err(fallback_error.context(format!("完整配置端点也失败: {primary_error:#}")))
                }
            }
        }
    }
}

async fn download_core(app: &mut App) -> Result<()> {
    app.status = "正在下载 sing-box 内核…".to_owned();
    let target = app.dir.join("core");
    let version = app.settings.core_version.clone();
    let mirror = app.settings.mirror.clone();
    let download = core::download_core(&target, &version, &mirror).await?;
    app.core_path = Some(download.path.clone());
    app.core_version = core::detect_version(&download.path).ok();
    app.status = format!(
        "内核已安装: {}（SHA-256 {}…）",
        app.core_version.clone().unwrap_or_default(),
        &download.sha256[..download.sha256.len().min(16)]
    );
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

async fn handle_key(app: &mut App, key: KeyCode) -> Result<()> {
    if app.show_help {
        if matches!(key, KeyCode::Esc | KeyCode::Char('?')) {
            app.show_help = false;
        }
        return Ok(());
    }
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
        KeyCode::Char('t') if app.tab == Tab::Proxies => test_current_delay(app).await,
        KeyCode::Char('T') if app.tab == Tab::Proxies => test_group_delays(app).await,
        KeyCode::Char('u') => {
            if let Err(error) = update_subscription(app).await {
                app.status = format!("订阅更新失败: {error}");
                let status = app.status.clone();
                app.log(status);
            }
        }
        KeyCode::Char('x') if app.tab == Tab::Connections => close_selected_connection(app).await,
        KeyCode::Char('X') if app.tab == Tab::Connections => close_all_connections(app).await,
        KeyCode::Char('S') if app.tab == Tab::Connections => {
            app.conn_sort = app.conn_sort.next();
            sort_connections(app);
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
        KeyCode::Delete if app.tab == Tab::Settings => delete_selected_profile(app)?,
        KeyCode::Char('n') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::ProfileName);
            app.input_text.clear();
        }
        KeyCode::Char('v') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::CoreVersion);
            app.input_text.clear();
        }
        KeyCode::Char('r') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::Mirror);
            app.input_text.clear();
        }
        KeyCode::Char('d') if app.tab == Tab::Settings => {
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
                .find(|line| app.log_filter.matches(line) && log_query_matches(app, line))
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
            app.input_text = app.settings.auto_update_minutes.to_string();
            app.status = "自动更新间隔（分钟，0 = 关闭）".to_owned();
        }
        KeyCode::Char('P') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::MixedPort);
            app.input_text = app.settings.mixed_port.to_string();
            app.status = "本地混合代理端口（1024–65535）".to_owned();
        }
        KeyCode::Char('U') if app.tab == Tab::Settings => {
            app.input = Some(InputGoal::TestUrl);
            app.input_text = app.settings.test_url.clone();
            app.status = "延迟测试地址".to_owned();
        }
        KeyCode::Char('g') if app.tab == Tab::Settings => {
            app.settings.auto_start = !app.settings.auto_start;
            let _ = app.settings.save(&app.dir);
            app.status = format!(
                "启动时自动启动内核: {}",
                if app.settings.auto_start {
                    "开"
                } else {
                    "关"
                }
            );
        }
        KeyCode::Char('y') if app.tab == Tab::Settings => {
            app.settings.auto_system_proxy = !app.settings.auto_system_proxy;
            let _ = app.settings.save(&app.dir);
            app.status = format!(
                "内核就绪后自动开启系统代理: {}",
                if app.settings.auto_system_proxy {
                    "开"
                } else {
                    "关"
                }
            );
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

/// Moves the member highlight inside the selected proxy group (`↑↓` / `j k`).
fn move_proxy_member(app: &mut App, delta: isize) {
    let member_count = app
        .groups
        .get(app.selected_group)
        .map(|group| group.all.len())
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
    if app.groups.is_empty() {
        return;
    }
    let len = app.groups.len() as isize;
    let next = ((app.selected_group as isize + delta).clamp(0, len - 1)) as usize;
    let position = app.groups[next]
        .all
        .iter()
        .position(|member| member == &app.groups[next].now)
        .unwrap_or(0);
    app.selected_group = next;
    app.selected_member = position;
    app.proxies_synced_group = Some(next);
    app.group_list.select(Some(next));
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
    match app.api.delay_with(&node, &app.settings.test_url).await {
        Ok(delay) => {
            app.delays.insert(node.clone(), delay);
            app.delay_failed.retain(|failed| failed != &node);
            app.status = format!("{node} 延迟 {delay} ms");
        }
        Err(error) => {
            app.delays.remove(&node);
            if !app.delay_failed.contains(&node) {
                app.delay_failed.push(node.clone());
            }
            app.status = format!("{node} 延迟测试失败: {error}");
        }
    }
}

/// Tests every member of the selected group concurrently. Serial testing made a
/// large group take `members × timeout`; issuing all probes at once and polling
/// them together returns in roughly one timeout.
async fn test_group_delays(app: &mut App) {
    if !app.running {
        return;
    }
    let Some(group) = app.groups.get(app.selected_group).cloned() else {
        return;
    };
    if group.all.is_empty() {
        return;
    }
    app.status = format!("正在并发测试 {} 组 {} 个节点…", group.name, group.all.len());
    let test_url = app.settings.test_url.clone();
    let api = app.api.clone();
    let results = futures_util::future::join_all(group.all.iter().cloned().map(|member| {
        let api = api.clone();
        let test_url = test_url.clone();
        async move { (member.clone(), api.delay_with(&member, &test_url).await) }
    }))
    .await;
    let mut ok = 0;
    for (member, result) in results {
        match result {
            Ok(delay) => {
                app.delays.insert(member.clone(), delay);
                app.delay_failed.retain(|failed| failed != &member);
                ok += 1;
            }
            Err(_) => {
                app.delays.remove(&member);
                if !app.delay_failed.contains(&member) {
                    app.delay_failed.push(member.clone());
                }
            }
        }
    }
    app.status = format!(
        "{} 组延迟测试完成：{ok}/{} 可用",
        group.name,
        group.all.len()
    );
}

/// Connections after the connection-table filter is applied, in display order.
fn visible_connections(app: &App) -> Vec<&clash_api::Connection> {
    app.connections
        .connections
        .iter()
        .filter(|connection| connection.matches(&app.conn_filter))
        .collect()
}

async fn close_selected_connection(app: &mut App) {
    if !app.running {
        return;
    }
    let Some(connection) = visible_connections(app)
        .get(app.selected_connection)
        .map(|connection| (*connection).clone())
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
    let was_active = app.profiles.active.as_deref() == Some(name.as_str());
    if was_active && app.running {
        app.confirm_delete = None;
        app.status = "内核运行中，先按 s 停止内核再删除当前档案".to_owned();
        return Ok(());
    }
    app.profiles.profiles.retain(|profile| profile.name != name);
    if was_active {
        app.profiles.active = None;
        app.subscription_usage = None;
    }
    app.confirm_delete = None;
    app.profiles.save(&app.dir)?;
    let _ = std::fs::remove_file(settings::profile_cache_path(&app.dir, &name));
    app.status = format!("已删除档案 {name}");
    let status = app.status.clone();
    app.log(status);
    Ok(())
}

fn toggle_system_proxy(app: &mut App) -> Result<()> {
    if app.system_proxy_on {
        system_proxy::disable(&app.dir)?;
        app.system_proxy_on = false;
        app.status = "系统代理已关闭".to_owned();
        return Ok(());
    }
    if app.mode == TrafficMode::Tun {
        app.status = "当前是 TUN 模式，系统代理不适用；按 m 切回系统代理模式".to_owned();
        return Ok(());
    }
    if !app.running {
        app.status = "内核未运行；先按 s 启动内核，再开启系统代理".to_owned();
        return Ok(());
    }
    system_proxy::enable(&app.dir, app.settings.mixed_port)?;
    app.system_proxy_on = true;
    app.status = format!("系统代理已开启 → 127.0.0.1:{}", app.settings.mixed_port);
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
    app.settings.traffic_mode = app.mode;
    let _ = app.settings.save(&app.dir);
    let privilege_note = if app.mode == TrafficMode::Tun && !system_proxy::can_use_tun() {
        "；⚠ 当前终端没有管理员/root 权限，TUN 启动会被拒绝"
    } else if app.mode == TrafficMode::Tun
        && cfg!(windows)
        && !settings::wintun_path(&app.dir).is_file()
    {
        "；⚠ 缺少 wintun.dll，请将该文件放入客户端 core 目录"
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
                app.status = "已取消编辑".to_owned();
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
        InputGoal::AutoUpdate => {
            let minutes = if text.is_empty() {
                0
            } else {
                match text.parse::<u64>() {
                    Ok(value) => value,
                    Err(_) => {
                        app.status = "自动更新间隔必须是整数分钟".to_owned();
                        return Ok(());
                    }
                }
            };
            app.settings.auto_update_minutes = minutes;
            app.settings.save(&app.dir)?;
            app.status = if minutes == 0 {
                "已关闭订阅自动更新".to_owned()
            } else {
                format!("订阅每 {minutes} 分钟自动更新")
            };
        }
        InputGoal::MixedPort => match text.parse::<u16>() {
            Ok(port) if port >= 1024 => {
                app.settings.mixed_port = port;
                app.settings.save(&app.dir)?;
                app.status = format!("混合代理端口已设为 {port}（重启内核生效）");
            }
            _ => app.status = "端口必须是 1024–65535 之间的整数".to_owned(),
        },
        InputGoal::TestUrl => {
            if text.is_empty() {
                app.status = "延迟测试地址不能为空".to_owned();
            } else {
                app.settings.test_url = text;
                app.settings.save(&app.dir)?;
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
    let state = if app.running {
        "● 运行中"
    } else {
        "○ 已停止"
    };
    let state_color = if app.running { MINT } else { MUTED };
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
            format!("↓ {}/s", human_bytes(app.down)),
            Style::default().fg(CYAN),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!("↑ {}/s", human_bytes(app.up)),
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
    app.groups
        .iter()
        .find(|group| group.name == SELECTOR_TAG)
        .map(|group| group.now.clone())
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
        let target = match app.mode {
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
fn log_query_matches(app: &App, line: &str) -> bool {
    app.log_query.is_empty() || line.to_lowercase().contains(&app.log_query.to_lowercase())
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
    let state_color = if app.running { MINT } else { MUTED };
    let health = vec![
        Line::from(Span::styled(
            if app.running {
                "● 在线"
            } else {
                "○ 离线"
            },
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("内核  ", Style::default().fg(MUTED)),
            Span::styled(
                if app.running {
                    "已启动"
                } else {
                    "未启动"
                },
                Style::default().fg(state_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("代理  ", Style::default().fg(MUTED)),
            Span::styled(
                if app.system_proxy_on {
                    format!("已接管 127.0.0.1:{}", app.settings.mixed_port)
                } else {
                    "未接管系统网络".to_owned()
                },
                Style::default().fg(if app.system_proxy_on { MINT } else { MUTED }),
            ),
        ]),
        Line::from(vec![
            Span::styled("模式  ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{} · {}", app.mode.label(), app.mode_outbound.label()),
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
                app.delays
                    .get(&current)
                    .map(|d| format!("{d} ms"))
                    .unwrap_or_else(|| "待测".into())
            ),
            Style::default().fg(delay_color(app.delays.get(&current).copied())),
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
                if app.system_proxy_on {
                    "已启用"
                } else {
                    "待启用"
                }
            ),
            Style::default().fg(if app.system_proxy_on { MINT } else { MUTED }),
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
                app.connections.connections.len()
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
        .logs
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
                app.mode_outbound.label(),
                app.connections.connections.len()
            ),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled("订阅状态", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            usage_label(app.subscription_usage),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "内核 {}",
                app.core_version.clone().unwrap_or_else(|| "未安装".into())
            ),
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
    let peak = app
        .traffic_history
        .iter()
        .map(|(up, down)| (*up).max(*down))
        .max()
        .unwrap_or(0)
        .max(1);
    let block = panel(format!(
        "实时流量 · 近 {} 秒",
        app.traffic_history.len() as u64 * TICK_MS / 1000
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 6 {
        frame.render_widget(
            Paragraph::new(format!(
                "↓ {}/s   ↑ {}/s",
                human_bytes(app.down),
                human_bytes(app.up)
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
                format!("{}/s", human_bytes(app.down)),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[0],
    );
    frame.render_widget(
        Sparkline::default()
            .data(app.traffic_history.iter().map(|(_, down)| *down))
            .max(peak)
            .style(Style::default().fg(CYAN)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑ ", Style::default().fg(MINT)),
            Span::styled(
                format!("{}/s", human_bytes(app.up)),
                Style::default().fg(MINT).add_modifier(Modifier::BOLD),
            ),
        ])),
        rows[2],
    );
    frame.render_widget(
        Sparkline::default()
            .data(app.traffic_history.iter().map(|(up, _)| *up))
            .max(peak)
            .style(Style::default().fg(MINT)),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(
                "累计 ↓ {}   ↑ {}   峰值 {}/s",
                human_bytes(app.total_down),
                human_bytes(app.total_up),
                human_bytes(peak)
            ),
            Style::default().fg(MUTED),
        )),
        rows[4],
    );
}

fn draw_dashboard_compact(frame: &mut Frame, area: Rect, app: &App) {
    let current = selected_node(app);
    let state = if app.running {
        "运行中"
    } else {
        "已停止"
    };
    let proxy = if app.system_proxy_on {
        "系统代理：已启用"
    } else {
        "系统代理：未启用"
    };
    let delay = app
        .delays
        .get(&current)
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "待测".to_owned());
    let body = vec![
        Line::from(vec![
            Span::styled("状态  ", Style::default().fg(MUTED)),
            Span::styled(
                state,
                Style::default().fg(if app.running { MINT } else { MUTED }),
            ),
            Span::styled("  ·  ", Style::default().fg(EDGE)),
            Span::styled(
                proxy,
                Style::default().fg(if app.system_proxy_on { MINT } else { MUTED }),
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
                app.mode_outbound.label(),
                delay,
                app.connections.connections.len()
            ),
            Style::default().fg(TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("↓ ", Style::default().fg(CYAN)),
            Span::styled(
                format!("{}/s", human_bytes(app.down)),
                Style::default().fg(CYAN),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(meter(app.down, 12), Style::default().fg(CYAN)),
            Span::styled("     ↑ ", Style::default().fg(MINT)),
            Span::styled(
                format!("{}/s", human_bytes(app.up)),
                Style::default().fg(MINT),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(meter(app.up, 12), Style::default().fg(MINT)),
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
        .groups
        .iter()
        .map(|g| ListItem::new(format!("{} ({})", g.name, g.kind)))
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
    if let Some(group) = app.groups.get(app.selected_group).cloned() {
        let member_items: Vec<ListItem> = group
            .all
            .iter()
            .map(|member| {
                let delay = app.delays.get(member).copied();
                let failed = app.delay_failed.contains(member);
                let marker = if member == &group.now { "● " } else { "○ " };
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
        if visible.is_empty() && !app.connections.connections.is_empty() {
            " · 无匹配"
        } else {
            ""
        }
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
            Paragraph::new(lines).block(panel("分流规则 · r 返回日志")),
            area,
        );
        return;
    }
    let filtered: Vec<Line> = app
        .core_logs
        .iter()
        .filter(|line| app.log_filter.matches(line) && log_query_matches(app, line))
        .map(|line| Line::from(line.clone()))
        .collect();
    let lines = if filtered.is_empty() {
        app.logs
            .iter()
            .filter(|line| log_query_matches(app, line))
            .map(|line| Line::from(line.clone()))
            .collect()
    } else {
        filtered
    };
    let title = format!(
        "日志 [{}]{}（Space 暂停，l 级别，/ 关键字，c 复制，r 规则）",
        app.log_filter.label(),
        if app.log_paused { " · 已暂停" } else { "" }
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
    let info = vec![
        Line::from(format!(
            "内核: {}",
            app.core_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "未安装".into())
        )),
        Line::from(format!(
            "版本: {}   镜像: {}",
            app.core_version.clone().unwrap_or_else(|| "未检测".into()),
            if app.settings.mirror.is_empty() {
                "直连"
            } else {
                app.settings.mirror.as_str()
            }
        )),
        Line::from(format!(
            "模式: {}   混合端口: {}   延迟地址: {}",
            app.mode.label(),
            app.settings.mixed_port,
            app.settings.test_url
        )),
        Line::from(format!(
            "自动更新: {}   启动内核: {}   自动系统代理: {}",
            if app.settings.auto_update_minutes == 0 {
                "关".to_owned()
            } else {
                format!("{} 分钟", app.settings.auto_update_minutes)
            },
            if app.settings.auto_start {
                "开"
            } else {
                "关"
            },
            if app.settings.auto_system_proxy {
                "开"
            } else {
                "关"
            }
        )),
        Line::from(format!(
            "系统代理后端: {}   订阅用量: {}",
            system_proxy::platform_label(),
            usage_label(app.subscription_usage)
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
fn usage_label(usage: Option<subscription::SubscriptionUserinfo>) -> String {
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

    fn test_app() -> App {
        App::new(
            PathBuf::from("/nonexistent-sbtui-test"),
            Settings::default(),
            Profiles::default(),
        )
    }

    fn connection(host: &str, rule: &str) -> clash_api::Connection {
        clash_api::Connection {
            id: host.to_owned(),
            metadata: clash_api::ConnectionMetadata {
                destination_host: host.to_owned(),
                ..Default::default()
            },
            rule: rule.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn connection_filter_keeps_only_matching_rows() {
        let mut app = test_app();
        app.connections.connections = vec![
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

    #[test]
    fn log_query_matches_are_case_insensitive() {
        let mut app = test_app();
        app.log_query = "DNS".to_owned();
        assert!(log_query_matches(&app, "[123] inbound/dns: lookup"));
        assert!(!log_query_matches(&app, "[123] outbound/tcp: connect"));
        app.log_query.clear();
        assert!(log_query_matches(&app, "anything"));
    }

    #[test]
    fn traffic_history_is_bounded_and_tracks_the_peak() {
        let mut app = test_app();
        for index in 0..(TRAFFIC_HISTORY + 25) {
            app.down = index as u64;
            app.up = 0;
            app.push_traffic_sample();
        }
        assert_eq!(app.traffic_history.len(), TRAFFIC_HISTORY);
        let peak = app
            .traffic_history
            .iter()
            .map(|(up, down)| (*up).max(*down))
            .max()
            .unwrap();
        assert_eq!(peak, (TRAFFIC_HISTORY + 24) as u64);
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

    fn selector(name: &str, now: &str, members: &[&str]) -> clash_api::ProxyGroup {
        clash_api::ProxyGroup {
            name: name.to_owned(),
            kind: "Selector".to_owned(),
            now: now.to_owned(),
            all: members.iter().map(|member| (*member).to_owned()).collect(),
            history: Vec::new(),
        }
    }

    #[test]
    fn proxy_member_selection_moves_and_stays_independent_from_connections() {
        let mut app = test_app();
        app.groups = vec![selector("g", "b", &["a", "b", "c"])];
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

    #[test]
    fn proxy_group_switch_snaps_member_to_the_current_node() {
        let mut app = test_app();
        app.groups = vec![
            selector("one", "a", &["a", "b"]),
            selector("two", "y", &["x", "y", "z"]),
        ];
        app.selected_group = 0;
        app.selected_member = 1;

        move_proxy_group(&mut app, 1);
        assert_eq!(app.selected_group, 1);
        assert_eq!(app.selected_member, 1, "snaps to the group's current node");
    }
}
