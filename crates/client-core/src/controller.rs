//! The shared client engine.
//!
//! [`ClientController::start`] spawns one long-lived task that owns the sing-box
//! core, the clash_api control channel and the persisted settings. UIs read
//! [`ClientController::snapshot`] (cheap, non-blocking) and send
//! [`ClientCommand`]s; they never touch the core directly. This is what keeps
//! the terminal and desktop clients behaving identically.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use crate::clash_api::ClashApi;
use crate::settings::{self, Profiles, Settings};
use crate::state::{ClientSnapshot, ProfileSummary, ProxyGroupSnapshot};
use crate::system_proxy::{self, TrafficMode};
use crate::{ClientCommand, ClientError, ClientEvent, core, subscription};

/// How often the engine wakes up to poll the core and check timers.
const TICK: Duration = Duration::from_millis(500);
const TRAFFIC_EVERY: Duration = Duration::from_secs(1);
const PROXIES_EVERY: Duration = Duration::from_secs(3);
const CONNECTIONS_EVERY: Duration = Duration::from_secs(2);
/// Maximum number of undelivered push events. UIs poll, so this only needs to
/// absorb a short burst for consumers that use `recv()`.
const EVENT_BACKLOG: usize = 32;

/// Owns the long-lived client state and presents a small command/seam to UIs.
pub struct ClientController {
    command_tx: mpsc::UnboundedSender<ClientCommand>,
    shared: Arc<Mutex<ClientSnapshot>>,
    events: tokio::sync::Mutex<mpsc::Receiver<ClientEvent>>,
}

impl ClientController {
    /// Starts the engine for one data directory. Must be called inside a Tokio
    /// runtime context because the engine runs on `tokio::spawn`.
    pub fn start(dir: PathBuf) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        // Bounded on purpose: UIs poll `snapshot()`, so an event stream that
        // nobody drains must not grow without limit.
        let (event_tx, event_rx) = mpsc::channel(EVENT_BACKLOG);
        let shared = Arc::new(Mutex::new(ClientSnapshot::default()));
        let worker = shared.clone();
        tokio::spawn(async move {
            let mut engine = Engine::new(dir, event_tx).await;
            engine.publish(&worker);
            engine.run(command_rx, worker).await;
        });
        Self {
            command_tx,
            shared,
            events: tokio::sync::Mutex::new(event_rx),
        }
    }

    /// A cheap, non-blocking copy of the latest renderable state.
    pub fn snapshot(&self) -> ClientSnapshot {
        self.shared
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    /// Queues a command; safe to call from any thread and never blocks.
    pub fn send(&self, command: ClientCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| anyhow::anyhow!("客户端控制器已停止"))
    }

    /// Receives the next push event. UIs that poll [`Self::snapshot`] can
    /// ignore this; it exists for consumers that prefer an event stream.
    pub async fn recv(&mut self) -> Option<ClientEvent> {
        self.events.lock().await.recv().await
    }
}

struct Engine {
    dir: PathBuf,
    settings: Settings,
    profiles: Profiles,
    api: ClashApi,
    child: Option<core::CoreHandle>,
    snapshot: ClientSnapshot,
    /// Delays measured by this client's own latency tests; they override the
    /// core-reported history in the UI display.
    delays: HashMap<String, u64>,
    /// Last known delays the core itself reports through `/proxies` node
    /// history, so the node list shows latencies before any manual test.
    reported_delays: HashMap<String, u64>,
    failed: Vec<String>,
    last_traffic_at: Instant,
    last_proxies_at: Instant,
    last_connections_at: Instant,
    last_totals: Option<(u64, u64, Instant)>,
    log_offset: u64,
    last_auto_update: Option<Instant>,
    restart_at: Option<Instant>,
    restart_attempts: u32,
    event_tx: mpsc::Sender<ClientEvent>,
}

impl Engine {
    async fn new(dir: PathBuf, event_tx: mpsc::Sender<ClientEvent>) -> Self {
        let settings = Settings::load_or_create(&dir).unwrap_or_default();
        let profiles = Profiles::load_or_create(&dir).unwrap_or_default();
        let core_path = settings::core_path(&dir);
        let core_installed = core_path.is_file();
        let core_version = core_installed
            .then(|| core::detect_version(&core_path).ok())
            .flatten();
        let mut snapshot = ClientSnapshot::default();
        snapshot.settings = (&settings).into();
        snapshot.traffic_mode = settings.traffic_mode;
        snapshot.core_installed = core_installed;
        snapshot.core_version = core_version;
        snapshot.active_profile = profiles
            .active_profile()
            .map(|profile| ProfileSummary::from_profile(profile, true));
        snapshot.profiles = profile_summaries(&profiles);
        snapshot.status = if snapshot.core_installed {
            "就绪。按“启动内核”开始。".to_owned()
        } else {
            "就绪。先下载 sing-box 内核，再导入订阅。".to_owned()
        };
        // The active configuration from a previous run is still the rules
        // source until the next start rewrites it.
        if let Ok(text) = std::fs::read_to_string(dir.join("cache/active-config.json")) {
            let (rules, rule_sets) = crate::state::parse_route_rules(&text);
            snapshot.rules = rules;
            snapshot.rule_sets = rule_sets;
        }
        Self {
            dir,
            settings,
            profiles,
            api: ClashApi::new(crate::clash_api::DEFAULT_CONTROLLER),
            child: None,
            snapshot,
            delays: HashMap::new(),
            reported_delays: HashMap::new(),
            failed: Vec::new(),
            last_traffic_at: Instant::now() - TRAFFIC_EVERY,
            last_proxies_at: Instant::now() - PROXIES_EVERY,
            last_connections_at: Instant::now() - CONNECTIONS_EVERY,
            last_totals: None,
            log_offset: 0,
            last_auto_update: None,
            restart_at: None,
            restart_attempts: 0,
            event_tx,
        }
    }

    async fn run(
        &mut self,
        mut command_rx: mpsc::UnboundedReceiver<ClientCommand>,
        shared: Arc<Mutex<ClientSnapshot>>,
    ) {
        // Mirror the TUI's `auto_start` behavior: bring the core up on
        // launch so the GUI toggle does something instead of only persisting.
        if self.settings.auto_start
            && self.snapshot.core_installed
            && self.profiles.active_profile().is_some()
        {
            self.snapshot.busy = Some("自动启动内核".to_owned());
            let _ = self
                .event_tx
                .try_send(ClientEvent::OperationStarted("自动启动内核".into()));
            if let Err(error) = self.start_core().await {
                let message = error.to_string();
                self.snapshot.status = format!("自动启动失败: {message}");
                self.snapshot.push_event(self.snapshot.status.clone());
                let _ = self.event_tx.try_send(ClientEvent::Error(ClientError {
                    operation: "自动启动内核".into(),
                    message,
                }));
            }
            self.snapshot.busy = None;
            self.publish(&shared);
        }
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let command = tokio::select! {
                command = command_rx.recv() => command,
                _ = tick.tick() => {
                    self.poll().await;
                    self.publish(&shared);
                    continue;
                }
            };
            // Every sender was dropped: the UI is gone, so stop the engine.
            let Some(command) = command else {
                break;
            };
            let label = command.label();
            self.snapshot.busy = Some(label.clone());
            let _ = self
                .event_tx
                .try_send(ClientEvent::OperationStarted(label.clone()));
            match self.apply(command).await {
                Ok(()) => {
                    let _ = self
                        .event_tx
                        .try_send(ClientEvent::OperationFinished(label));
                }
                Err(error) => {
                    let message = error.to_string();
                    self.snapshot.status = format!("{label} 失败: {message}");
                    self.snapshot.push_event(self.snapshot.status.clone());
                    let _ = self.event_tx.try_send(ClientEvent::Error(ClientError {
                        operation: label,
                        message,
                    }));
                }
            }
            self.snapshot.busy = None;
            self.publish(&shared);
        }
    }

    fn publish(&self, shared: &Arc<Mutex<ClientSnapshot>>) {
        if let Ok(mut guard) = shared.lock() {
            *guard = self.snapshot.clone();
        }
        // Skip the snapshot clone entirely when no consumer is draining the
        // event stream (the common case: UIs poll `snapshot()` instead).
        if self.event_tx.capacity() > 0 {
            let _ = self
                .event_tx
                .try_send(ClientEvent::SnapshotChanged(Box::new(
                    self.snapshot.clone(),
                )));
        }
    }

    async fn apply(&mut self, command: ClientCommand) -> Result<()> {
        match command {
            ClientCommand::StartCore => self.start_core().await,
            ClientCommand::StopCore => {
                self.stop_core().await;
                Ok(())
            }
            ClientCommand::RestartCore => {
                self.stop_core().await;
                self.start_core().await
            }
            ClientCommand::ToggleSystemProxy => self.toggle_system_proxy(),
            ClientCommand::SetTrafficMode(mode) => {
                if self.snapshot.core_running {
                    anyhow::bail!("切换流量模式前请先停止内核");
                }
                self.settings.traffic_mode = mode;
                self.settings.save(&self.dir)?;
                self.snapshot.traffic_mode = mode;
                self.snapshot.settings.traffic_mode = mode;
                self.note(format!("流量模式: {}", mode.label()));
                Ok(())
            }
            ClientCommand::SetOutboundMode(mode) => {
                if self.snapshot.core_running {
                    self.api.set_mode(mode).await?;
                }
                self.snapshot.outbound_mode = mode;
                self.note(format!("出站模式: {}", mode.label()));
                Ok(())
            }
            ClientCommand::SwitchProfile(name) => {
                self.profiles
                    .activate(&name)
                    .ok_or_else(|| anyhow::anyhow!("订阅档案不存在: {name}"))?;
                self.profiles.save(&self.dir)?;
                self.snapshot.active_profile = self
                    .profiles
                    .active_profile()
                    .map(|profile| ProfileSummary::from_profile(profile, true));
                self.snapshot.profiles = profile_summaries(&self.profiles);
                self.note(format!("已激活档案 {name}"));
                Ok(())
            }
            ClientCommand::SwitchNode { group, node } => {
                self.api.select(&group, &node).await?;
                for snapshot in &mut self.snapshot.proxy_groups {
                    if snapshot.name == group {
                        snapshot.current = node.clone();
                    }
                }
                self.snapshot.current_node = Some(node.clone());
                self.note(format!("{group} → {node}"));
                Ok(())
            }
            ClientCommand::TestNode(node) => {
                let url = self.settings.test_url.clone();
                let delay = self.api.delay_with(&node, &url).await?;
                self.delays.insert(node.clone(), delay);
                self.failed.retain(|failed| failed != &node);
                self.apply_delays();
                self.note(format!("{node} 延迟 {delay} ms"));
                Ok(())
            }
            ClientCommand::TestGroup(group) => self.test_group(&group).await,
            ClientCommand::CloseConnection(id) => {
                self.api.close_connection(&id).await?;
                self.note("已关闭连接");
                Ok(())
            }
            ClientCommand::CloseAllConnections => {
                let current = self.api.connections().await?;
                let mut closed = 0;
                for connection in &current.connections {
                    if self.api.close_connection(&connection.id).await.is_ok() {
                        closed += 1;
                    }
                }
                self.note(format!("已关闭 {closed} 条连接"));
                Ok(())
            }
            ClientCommand::UpdateSubscription => self.update_subscription().await,
            ClientCommand::ImportSubscription { name, url } => {
                self.import_subscription(name, url).await
            }
            ClientCommand::ImportProfileFile(path) => self.import_profile_file(path).await,
            ClientCommand::SetProfileUrl { name, url } => self.set_profile_url(name, url).await,
            ClientCommand::RemoveProfile(name) => self.remove_profile(&name).await,
            ClientCommand::DownloadCore => {
                self.snapshot.status = "正在下载 sing-box 内核…".to_owned();
                let version = self.settings.core_version.clone();
                let mirror = self.settings.mirror.clone();
                let download =
                    core::download_core(&self.dir.join("core"), &version, &mirror).await?;
                self.snapshot.core_installed = true;
                self.snapshot.core_version = core::detect_version(&download.path).ok();
                self.note(format!(
                    "内核已安装: {}（SHA-256 {}…）",
                    self.snapshot.core_version.clone().unwrap_or_default(),
                    &download.sha256[..download.sha256.len().min(16)]
                ));
                Ok(())
            }
            ClientCommand::UpdateSettings(patch) => {
                if self.snapshot.core_running
                    && (patch.traffic_mode.is_some() || patch.mixed_port.is_some())
                {
                    anyhow::bail!("切换流量模式或混合端口前请先停止内核");
                }
                patch.apply(&mut self.settings);
                self.settings.save(&self.dir)?;
                self.snapshot.settings = (&self.settings).into();
                self.snapshot.traffic_mode = self.settings.traffic_mode;
                self.note("设置已保存");
                Ok(())
            }
            ClientCommand::Refresh => {
                self.poll().await;
                Ok(())
            }
        }
    }

    async fn start_core(&mut self) -> Result<()> {
        if self.snapshot.core_running || self.snapshot.starting {
            return Ok(());
        }
        self.snapshot.starting = true;
        let result = self.start_core_inner().await;
        self.snapshot.starting = false;
        result
    }

    async fn start_core_inner(&mut self) -> Result<()> {
        let mode = self.settings.traffic_mode;
        if mode == TrafficMode::Tun {
            if !system_proxy::can_use_tun() {
                anyhow::bail!(
                    "TUN 模式需要管理员/root 权限；请以管理员身份运行，或切回系统代理模式"
                );
            }
            if cfg!(windows) && !settings::wintun_path(&self.dir).is_file() {
                anyhow::bail!(
                    "TUN 模式需要 wintun.dll：请将该文件放入 {}",
                    settings::wintun_path(&self.dir).display()
                );
            }
        }
        let profile = self
            .profiles
            .active_profile()
            .context("没有激活的订阅档案；先导入订阅")?
            .clone();
        let core_path = settings::core_path(&self.dir);
        if !core_path.is_file() {
            anyhow::bail!("没有 sing-box 内核；先下载内核");
        }
        let cache = settings::profile_cache_path(&self.dir, &profile.name);
        let raw = tokio::fs::read_to_string(&cache)
            .await
            .context("读取订阅缓存失败；先更新订阅")?;
        let adapted = core::adapt_inbounds(&raw, mode, self.settings.mixed_port)?;
        let active = self.dir.join("cache/active-config.json");
        // The rewritten configuration is the rules view's source of truth.
        let (rules, rule_sets) = crate::state::parse_route_rules(&adapted);
        self.snapshot.rules = rules;
        self.snapshot.rule_sets = rule_sets;
        tokio::fs::write(&active, adapted.as_bytes()).await?;
        core::check_config(&core_path, &active)?;
        let handle = core::start(&core_path, &active, &self.dir.join("cache/core.log")).await?;
        self.child = Some(handle);
        self.log_offset = 0;
        self.snapshot.core_logs.clear();

        for _ in 0..20 {
            if self.api.alive().await {
                self.snapshot.core_running = true;
                self.restart_attempts = 0;
                self.restart_at = None;
                self.snapshot.restart_attempts = 0;
                self.snapshot.core_runtime_version = self.api.version().await.ok();
                let mut status = "内核已启动".to_owned();
                if mode == TrafficMode::SystemProxy && self.settings.auto_system_proxy {
                    match system_proxy::enable(&self.dir, self.settings.mixed_port) {
                        Ok(()) => {
                            self.snapshot.system_proxy_enabled = true;
                            status = format!(
                                "内核已启动；系统代理 → 127.0.0.1:{}",
                                self.settings.mixed_port
                            );
                        }
                        Err(error) => {
                            status = format!("内核已启动；系统代理设置失败: {error}");
                        }
                    }
                } else if mode == TrafficMode::Tun {
                    status = "内核已启动（TUN 模式）".to_owned();
                }
                self.snapshot.status = status.clone();
                self.snapshot.push_event(status);
                self.refresh_proxies().await?;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.child.kill().await;
        }
        anyhow::bail!("内核已启动但 clash_api 未响应；确认订阅配置包含 clash_api")
    }

    async fn stop_core(&mut self) {
        self.snapshot.core_running = false;
        self.snapshot.starting = false;
        self.restart_at = None;
        self.restart_attempts = 0;
        self.snapshot.restart_attempts = 0;
        if self.snapshot.system_proxy_enabled {
            let _ = system_proxy::disable(&self.dir);
            self.snapshot.system_proxy_enabled = false;
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.child.kill().await;
        }
        self.snapshot.core_runtime_version = None;
        self.snapshot.memory_used = 0;
        self.snapshot.traffic_history.clear();
        self.snapshot.proxy_groups.clear();
        self.snapshot.connections = Default::default();
        self.snapshot.active_connections = 0;
        self.snapshot.current_node = None;
        self.note("内核已停止");
    }

    fn toggle_system_proxy(&mut self) -> Result<()> {
        if self.snapshot.system_proxy_enabled {
            system_proxy::disable(&self.dir)?;
            self.snapshot.system_proxy_enabled = false;
            self.note("系统代理已关闭");
        } else {
            if self.snapshot.traffic_mode == TrafficMode::Tun {
                anyhow::bail!("当前是 TUN 模式，系统代理不适用；切回系统代理模式后再开启");
            }
            if !self.snapshot.core_running {
                anyhow::bail!("内核未运行；先启动内核，再开启系统代理");
            }
            system_proxy::enable(&self.dir, self.settings.mixed_port)?;
            self.snapshot.system_proxy_enabled = true;
            self.note(format!(
                "系统代理已开启 → 127.0.0.1:{}",
                self.settings.mixed_port
            ));
        }
        Ok(())
    }

    async fn test_group(&mut self, group: &str) -> Result<()> {
        let Some(snapshot) = self
            .snapshot
            .proxy_groups
            .iter()
            .find(|candidate| candidate.name == group)
            .cloned()
        else {
            anyhow::bail!("代理组不存在: {group}");
        };
        self.note(format!("正在测试 {} 组延迟…", group));
        let url = self.settings.test_url.clone();
        let api = &self.api;
        let results = futures_util::future::join_all(snapshot.members.iter().map(|member| {
            let url = url.clone();
            async move { (member.clone(), api.delay_with(member, &url).await) }
        }))
        .await;
        for (member, result) in results {
            match result {
                Ok(delay) => {
                    self.delays.insert(member.clone(), delay);
                    self.failed.retain(|failed| failed != &member);
                }
                Err(_) => {
                    self.delays.remove(&member);
                    if !self.failed.contains(&member) {
                        self.failed.push(member.clone());
                    }
                }
            }
        }
        self.apply_delays();
        self.note(format!("{} 组延迟测试完成", group));
        Ok(())
    }

    async fn update_subscription(&mut self) -> Result<()> {
        let profile = self
            .profiles
            .active_profile()
            .context("没有激活的订阅档案")?
            .clone();
        self.snapshot.status = format!("正在更新订阅 {}…", profile.name);
        let fetched = match self.fetch_with_compat(&profile.url).await {
            Ok(fetched) => fetched,
            Err(error) => {
                let cache = settings::profile_cache_path(&self.dir, &profile.name);
                if tokio::fs::try_exists(&cache).await.unwrap_or(false) {
                    self.note(format!("订阅更新失败（{error}）；继续使用上次缓存"));
                    return Ok(());
                }
                return Err(error);
            }
        };
        self.snapshot.subscription_usage = fetched.userinfo;
        let parsed = subscription::parse(&fetched.body)?;
        let cache = settings::profile_cache_path(&self.dir, &profile.name);
        tokio::fs::write(&cache, &parsed.raw).await?;
        for stored in &mut self.profiles.profiles {
            if stored.name == profile.name {
                stored.last_updated = now_epoch();
            }
        }
        self.profiles.save(&self.dir)?;
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.snapshot.active_profile = self
            .profiles
            .active_profile()
            .map(|profile| ProfileSummary::from_profile(profile, true));
        self.note(format!("订阅已更新（{} 个节点）", parsed.nodes.len()));
        Ok(())
    }

    /// Fetches the profile URL, retrying the old bare `sing-box.json`
    /// endpoint when the normalized full-profile link fails on an older
    /// sbctl server. `parse` wraps that bare node list into a runnable
    /// client configuration, so the caller needs no special casing.
    async fn fetch_with_compat(&self, url: &str) -> Result<subscription::Fetched> {
        match subscription::fetch(url, &self.settings.mirror).await {
            Ok(fetched) => Ok(fetched),
            Err(error) => match subscription::bare_sing_box_fallback_url(url) {
                Some(fallback) => subscription::fetch(&fallback, &self.settings.mirror).await,
                None => Err(error),
            },
        }
    }

    async fn import_subscription(&mut self, name: Option<String>, url: String) -> Result<()> {
        let source = url.trim().to_owned();
        if !(source.starts_with("https://") || source.starts_with("http://")) {
            anyhow::bail!("订阅地址必须是有效的 HTTP/HTTPS 链接");
        }
        // Store the canonical full-profile link the same way the TUI does, so
        // a pasted clash.yaml / qr / index suffix fetches a sing-box client
        // profile instead of a body the engine cannot parse.
        let url = subscription::normalize_url(&source);

        if let Some(existing) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.url == url)
            .map(|profile| profile.name.clone())
        {
            self.profiles.active = Some(existing.clone());
            self.profiles.save(&self.dir)?;
            self.snapshot.active_profile = self
                .profiles
                .active_profile()
                .map(|profile| ProfileSummary::from_profile(profile, true));
            self.snapshot.profiles = profile_summaries(&self.profiles);
            self.note(format!("订阅已存在，已切换到 {existing}"));
            return self.update_subscription().await;
        }

        let name = match name {
            Some(name) if !name.trim().is_empty() => unique_profile_name(
                name.trim(),
                self.profiles.profiles.iter().map(|p| p.name.as_str()),
            ),
            _ => {
                let mut index = self.profiles.profiles.len() + 1;
                loop {
                    let candidate = format!("订阅 {index}");
                    if !self
                        .profiles
                        .profiles
                        .iter()
                        .any(|profile| profile.name == candidate)
                    {
                        break candidate;
                    }
                    index += 1;
                }
            }
        };
        self.profiles.profiles.push(crate::settings::Profile {
            name: name.clone(),
            url,
            source,
            last_updated: 0,
        });
        self.profiles.active = Some(name.clone());
        self.profiles.save(&self.dir)?;
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.snapshot.active_profile = self
            .profiles
            .active_profile()
            .map(|profile| ProfileSummary::from_profile(profile, true));
        self.note(format!("已导入 {name}，正在拉取订阅"));
        self.update_subscription().await
    }

    /// Imports a local sing-box JSON configuration as a profile without a
    /// subscription URL. The parsed body is cached immediately, so the
    /// profile is startable without network access.
    async fn import_profile_file(&mut self, path_text: String) -> Result<()> {
        let path = PathBuf::from(path_text.trim());
        if !path.is_file() {
            anyhow::bail!("本地文件不存在或不可读：{}", path.display());
        }
        let body = tokio::fs::read_to_string(&path)
            .await
            .context("读取本地订阅文件失败")?;
        let parsed = subscription::parse(&body)?;
        let name = unique_profile_name(
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("本地订阅"),
            self.profiles.profiles.iter().map(|p| p.name.as_str()),
        );
        let cache = settings::profile_cache_path(&self.dir, &name);
        tokio::fs::write(&cache, &parsed.raw).await?;
        self.profiles.profiles.push(crate::settings::Profile {
            name: name.clone(),
            url: String::new(),
            source: format!("file:{}", path.display()),
            last_updated: now_epoch(),
        });
        self.profiles.active = Some(name.clone());
        self.profiles.save(&self.dir)?;
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.snapshot.active_profile = self
            .profiles
            .active_profile()
            .map(|profile| ProfileSummary::from_profile(profile, true));
        self.note(format!(
            "已从文件导入并激活：{name}（{} 个节点）",
            parsed.nodes.len()
        ));
        Ok(())
    }

    /// Replaces one profile's subscription link with a renormalized one.
    async fn set_profile_url(&mut self, name: String, url: String) -> Result<()> {
        let source = url.trim().to_owned();
        if !(source.starts_with("https://") || source.starts_with("http://")) {
            anyhow::bail!("订阅地址必须是有效的 HTTP/HTTPS 链接");
        }
        let normalized = subscription::normalize_url(&source);
        let Some(profile) = self
            .profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.name == name)
        else {
            anyhow::bail!("订阅档案不存在: {name}");
        };
        profile.url = normalized;
        profile.source = source;
        self.profiles.save(&self.dir)?;
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.note(format!("已更新档案 {name} 的订阅链接"));
        Ok(())
    }

    async fn remove_profile(&mut self, name: &str) -> Result<()> {
        if self.snapshot.core_running && self.profiles.active.as_deref() == Some(name) {
            anyhow::bail!("删除当前订阅前请先停止内核");
        }
        let before = self.profiles.profiles.len();
        self.profiles
            .profiles
            .retain(|profile| profile.name != name);
        if self.profiles.profiles.len() == before {
            anyhow::bail!("订阅档案不存在: {name}");
        }
        if self.profiles.active.as_deref() == Some(name) {
            self.profiles.active = self
                .profiles
                .profiles
                .first()
                .map(|profile| profile.name.clone());
        }
        self.profiles.save(&self.dir)?;
        let cache = settings::profile_cache_path(&self.dir, name);
        if tokio::fs::try_exists(&cache).await.unwrap_or(false) {
            tokio::fs::remove_file(cache).await?;
        }
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.snapshot.active_profile = self
            .profiles
            .active_profile()
            .map(|profile| ProfileSummary::from_profile(profile, true));
        self.snapshot.subscription_usage = None;
        self.note(format!("已删除订阅 {name}"));
        Ok(())
    }

    async fn poll(&mut self) {
        if self.snapshot.core_running {
            if self.watch_core_exit() {
                return;
            }
            self.tail_core_log();
            let now = Instant::now();
            if now.duration_since(self.last_connections_at) >= CONNECTIONS_EVERY {
                self.last_connections_at = now;
                self.refresh_connections().await;
            }
            if now.duration_since(self.last_traffic_at) >= TRAFFIC_EVERY {
                self.last_traffic_at = now;
                self.refresh_traffic().await;
                match self.api.memory().await {
                    Ok(memory) => self.snapshot.memory_used = memory,
                    Err(_) => self.snapshot.memory_used = 0,
                }
            }
            if now.duration_since(self.last_proxies_at) >= PROXIES_EVERY {
                self.last_proxies_at = now;
                if let Err(error) = self.refresh_proxies().await {
                    self.note(format!("刷新代理组失败: {error}"));
                }
            }
            if self.auto_update_due()
                && let Err(error) = self.update_subscription().await
            {
                self.note(format!("自动更新订阅失败: {error}"));
            }
        } else {
            // Drop stale runtime state that only makes sense while the core runs.
            if !self.snapshot.proxy_groups.is_empty() {
                self.snapshot.proxy_groups.clear();
            }
            if let Some(at) = self.restart_at
                && Instant::now() >= at
            {
                self.restart_at = None;
                if let Err(error) = self.start_core().await {
                    self.note(format!("自动重启失败: {error}"));
                    self.schedule_restart();
                }
            }
        }
    }

    async fn refresh_traffic(&mut self) {
        match self.api.traffic().await {
            Ok(sample) => self.snapshot.push_traffic(sample.up, sample.down),
            Err(_) => {
                // Fall back to the connection-total delta when the streaming
                // endpoint is unavailable.
                if let Some((last_up, last_down, last_time)) = self.last_totals {
                    let elapsed = last_time.elapsed().as_secs_f64();
                    if elapsed > 0.05 {
                        let up = (self.snapshot.total_upload.saturating_sub(last_up) as f64
                            / elapsed) as u64;
                        let down = (self.snapshot.total_download.saturating_sub(last_down) as f64
                            / elapsed) as u64;
                        self.snapshot.push_traffic(up, down);
                    }
                }
            }
        }
    }

    async fn refresh_connections(&mut self) {
        match self.api.connections().await {
            Ok(snapshot) => {
                self.snapshot.total_upload = snapshot.upload_total;
                self.snapshot.total_download = snapshot.download_total;
                self.snapshot.active_connections = snapshot.connections.len();
                self.last_totals = Some((
                    snapshot.upload_total,
                    snapshot.download_total,
                    Instant::now(),
                ));
                self.snapshot.connections = snapshot;
            }
            Err(error) => self.note(format!("刷新连接失败: {error}")),
        }
    }

    async fn refresh_proxies(&mut self) -> Result<()> {
        let (groups, nodes) = self.api.proxies().await?;
        // The core reports each node's most recent latency result through its
        // `/proxies` history; surfacing it means the node list shows usable
        // delays before the user runs any manual test.
        self.reported_delays = nodes
            .iter()
            .filter_map(|node| {
                node.history
                    .last()
                    .filter(|entry| entry.delay > 0)
                    .map(|entry| (node.name.clone(), entry.delay))
            })
            .collect();
        let merged = self.merged_delays();
        let mut snapshots: Vec<ProxyGroupSnapshot> =
            groups.into_iter().map(ProxyGroupSnapshot::from).collect();
        for group in &mut snapshots {
            group.delays = merged.clone();
            group.failed = self.failed.clone();
        }
        self.snapshot.current_node = snapshots
            .iter()
            .find(|group| group.name == crate::clash_api::SELECTOR_TAG)
            .map(|group| group.current.clone())
            .or_else(|| self.snapshot.current_node.clone());
        self.snapshot.proxy_groups = snapshots;
        self.snapshot.outbound_mode = self.api.mode().await.unwrap_or(self.snapshot.outbound_mode);
        Ok(())
    }

    /// Node delays as displayed: core-reported history as the baseline with
    /// this client's own latency tests taking precedence.
    fn merged_delays(&self) -> HashMap<String, u64> {
        let mut merged = self.reported_delays.clone();
        merged.extend(self.delays.clone());
        merged
    }

    fn apply_delays(&mut self) {
        let merged = self.merged_delays();
        for group in &mut self.snapshot.proxy_groups {
            group.delays = merged.clone();
            group.failed = self.failed.clone();
        }
    }

    /// Detects an unexpected core exit and schedules a backed-off restart.
    fn watch_core_exit(&mut self) -> bool {
        let Some(handle) = self.child.as_mut() else {
            return false;
        };
        match handle.child.try_wait() {
            Ok(Some(status)) => {
                self.child = None;
                self.note(format!("内核意外退出（{status}），准备自动重启"));
                self.schedule_restart();
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.note(format!("检测内核状态失败: {error}"));
                false
            }
        }
    }

    fn schedule_restart(&mut self) {
        self.snapshot.core_running = false;
        self.snapshot.core_runtime_version = None;
        self.snapshot.memory_used = 0;
        self.snapshot.proxy_groups.clear();
        self.snapshot.connections = Default::default();
        self.snapshot.active_connections = 0;
        if self.snapshot.system_proxy_enabled {
            let _ = system_proxy::disable(&self.dir);
            self.snapshot.system_proxy_enabled = false;
        }
        let attempt = self.restart_attempts;
        self.restart_attempts = attempt.saturating_add(1);
        self.snapshot.restart_attempts = self.restart_attempts;
        let delay = core::restart_backoff(attempt);
        self.restart_at = Some(Instant::now() + delay);
        self.note(format!(
            "内核崩溃；{} 秒后自动重启（第 {} 次）",
            delay.as_secs(),
            attempt + 1
        ));
    }

    fn tail_core_log(&mut self) {
        let path = self.dir.join("cache/core.log");
        let Ok(meta) = std::fs::metadata(&path) else {
            return;
        };
        let size = meta.len();
        if size <= self.log_offset {
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let start = usize::try_from(self.log_offset.min(text.len() as u64)).unwrap_or(0);
        for line in text[start..].lines() {
            if !line.is_empty() {
                self.snapshot.push_log(line);
            }
        }
        self.log_offset = size;
    }

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

    fn note(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.snapshot.status = message.clone();
        self.snapshot.push_event(message);
    }
}

fn profile_summaries(profiles: &Profiles) -> Vec<ProfileSummary> {
    profiles
        .profiles
        .iter()
        .map(|profile| {
            ProfileSummary::from_profile(profile, profiles.active.as_deref() == Some(&profile.name))
        })
        .collect()
}

/// Returns `base`, or `base 2` / `base 3` … until no existing name collides,
/// so a user-typed or filename-derived profile label never clobbers another.
fn unique_profile_name<'a>(base: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let existing: Vec<String> = existing.map(str::to_owned).collect();
    if !existing.iter().any(|name| name == base) {
        return base.to_owned();
    }
    for index in 2.. {
        let candidate = format!("{base} {index}");
        if !existing.iter().any(|name| name == &candidate) {
            return candidate;
        }
    }
    unreachable!("an index always frees the name")
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clash_api::OutboundMode;
    use crate::settings::Profile;

    #[tokio::test]
    async fn commands_produce_snapshot_events() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut controller = ClientController::start(tempdir.path().to_path_buf());
        controller
            .send(ClientCommand::SetOutboundMode(OutboundMode::Global))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut running = false;
        while Instant::now() < deadline {
            if controller.snapshot().outbound_mode == OutboundMode::Global {
                running = true;
                break;
            }
            let _ = controller.recv().await;
        }
        assert!(running);
    }

    #[test]
    fn profile_summaries_mark_the_active_profile() {
        let profiles = Profiles {
            active: Some("b".to_owned()),
            profiles: vec![
                Profile {
                    name: "a".to_owned(),
                    url: String::new(),
                    source: String::new(),
                    last_updated: 0,
                },
                Profile {
                    name: "b".to_owned(),
                    url: String::new(),
                    source: String::new(),
                    last_updated: 7,
                },
            ],
        };
        let summaries = profile_summaries(&profiles);
        assert!(!summaries[0].active);
        assert!(summaries[1].active);
    }
}
