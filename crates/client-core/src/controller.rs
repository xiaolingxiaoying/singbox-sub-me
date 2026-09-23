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
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use crate::clash_api::{ClashApi, OutboundMode};
use crate::command::SettingsPatch;
use crate::event_code::{EventCode, EventRecord};
use crate::format::now_epoch;
use crate::settings::{self, Profiles, Settings};
use crate::state::{ClientSnapshot, ProfileSummary, ProxyGroupSnapshot};
use crate::system_proxy::{self, TrafficMode};
use crate::{ClientCommand, ClientError, ClientEvent, core, subscription};

/// How often the engine wakes up to poll the core and check timers.
const TICK: Duration = Duration::from_millis(500);
/// How often the engine samples core traffic. Published to the UIs: a window
/// label computed from the render tick instead of this would misreport the
/// span of `traffic_history` by the ratio of the two.
pub const TRAFFIC_EVERY: Duration = Duration::from_secs(1);
const PROXIES_EVERY: Duration = Duration::from_secs(3);
const CONNECTIONS_EVERY: Duration = Duration::from_secs(2);
/// Maximum number of undelivered push events. UIs poll, so this only needs to
/// absorb a short burst for consumers that use `recv()`.
const EVENT_BACKLOG: usize = 32;
/// Automatic crash restarts stop after this many consecutive failures; a
/// persistent cause (bad config, port conflict) needs an administrator.
const MAX_AUTO_RESTARTS: u32 = 5;
const STABLE_RUN: Duration = Duration::from_secs(60);
/// How long [`ClientController::shutdown`] waits for the engine to reap its
/// core child. Stopping is local (kill plus a registry write); sing-box's own
/// graceful window is 10s and we never wait for a signal it cannot receive.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
/// Cap on how much of the core log one poll consumes, so a chatty core that is
/// never restarted cannot grow the read buffer without limit.
const CORE_LOG_READ_LIMIT: u64 = 256 * 1024;

type Completion = Box<dyn FnOnce(&mut Engine) -> Result<()> + Send>;

struct PendingOperation {
    task: tokio::task::JoinHandle<Result<Completion>>,
    label: String,
    restarting: bool,
}

impl Drop for PendingOperation {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Owns the long-lived client state and presents a small command/seam to UIs.
pub struct ClientController {
    command_tx: mpsc::UnboundedSender<ClientCommand>,
    shared: Arc<Mutex<ClientSnapshot>>,
    events: tokio::sync::Mutex<mpsc::Receiver<ClientEvent>>,
    /// Resolves once the engine loop has returned, which is after it reaped its
    /// core child. `Disconnected` means the engine task is gone entirely.
    stopped_rx: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl ClientController {
    /// Starts the engine for one data directory. Must be called inside a Tokio
    /// runtime context because the engine runs on `tokio::spawn`.
    pub fn start(dir: PathBuf) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        // Bounded on purpose: UIs poll `snapshot()`, so an event stream that
        // nobody drains must not grow without limit.
        let (event_tx, event_rx) = mpsc::channel(EVENT_BACKLOG);
        // Seed the pre-publish window with a record, not the default Chinese
        // string: a UI that reads before the engine's first publish must be
        // able to render its own language.
        let mut initial = ClientSnapshot::default();
        initial.push_record(crate::event_code::EventRecord::new(
            crate::event_code::EventCode::ReadyToImportFirstProfile,
            vec![],
        ));
        initial.status = initial.event_records.back().unwrap().render_zh();
        initial.status_level = Some(crate::event_code::EventLevel::Info);
        let shared = Arc::new(Mutex::new(initial));
        let worker = shared.clone();
        // Lets a UI wait for the engine to finish reaping its child instead of
        // racing process exit against it; see [`Self::shutdown`].
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
        tokio::spawn(async move {
            let mut engine = Engine::new(dir, event_tx).await;
            engine.publish(&worker);
            engine.run(command_rx, worker).await;
            let _ = stopped_tx.send(());
        });
        Self {
            command_tx,
            shared,
            events: tokio::sync::Mutex::new(event_rx),
            stopped_rx: std::sync::Mutex::new(stopped_rx),
        }
    }

    /// A cheap, non-blocking copy of the latest renderable state.
    pub fn snapshot(&self) -> ClientSnapshot {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Queues a command; safe to call from any thread and never blocks.
    pub fn send(&self, command: ClientCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .map_err(|_| anyhow::anyhow!("客户端控制器已停止"))
    }

    /// Stops the core and blocks (bounded by [`SHUTDOWN_GRACE`]) until the
    /// engine task has reaped its child. The operating-system proxy is left
    /// exactly as it is: the UI decides keep-or-restore before it calls here.
    ///
    /// UIs must call this on their exit path. The child is otherwise killed by
    /// `kill_on_drop` when the engine loop ends, and a process that exits first
    /// gives that task no chance to run — leaving sing-box holding the mixed
    /// port and, in TUN mode, the routes it took over.
    pub fn shutdown(&self) {
        let _ = self.command_tx.send(ClientCommand::Shutdown);
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        let stopped = self.stopped_rx.lock().ok();
        loop {
            let done = match &stopped {
                Some(receiver) => !matches!(
                    receiver.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Empty)
                ),
                // A poisoned lock means no engine will ever report back.
                None => true,
            };
            if done || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
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
    pending: Option<PendingOperation>,
    healthy_since: Option<Instant>,
    /// Why persisting is refused, if it is. See [`Engine::new`].
    store_unreadable: Option<String>,
}

impl Engine {
    async fn new(dir: PathBuf, event_tx: mpsc::Sender<ClientEvent>) -> Self {
        let settings = Settings::load_or_create(&dir);
        let profiles = Profiles::load_or_create(&dir);
        // A store that cannot be read must not be "repaired" by the next save:
        // the in-memory copy is empty at that point, so writing it back would
        // delete every subscription link the administrator imported. The engine
        // keeps running and refuses to persist until the file is fixed.
        let store_unreadable = [settings.as_ref().err(), profiles.as_ref().err()]
            .into_iter()
            .flatten()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("；");
        let store_unreadable = (!store_unreadable.is_empty()).then_some(store_unreadable);
        let settings = settings.unwrap_or_default();
        let profiles = profiles.unwrap_or_default();
        // A previous instance can die (crash, task manager, logoff) after
        // pointing the operating-system proxy at its own mixed port. Nothing
        // else would ever revisit that setting: the fresh snapshot reports the
        // proxy as off, so the machine keeps a dead proxy and the user loses
        // network access while the client looks idle. Restore before the core
        // (and possibly the proxy) comes back up.
        if system_proxy::has_residual_backup(&dir) {
            let _ = system_proxy::disable(&dir);
        }
        let core_path = settings::core_path(&dir);
        let core_installed = core_path.is_file();
        let core_version = core_installed
            .then(|| core::detect_version(&core_path).ok())
            .flatten();
        let mut snapshot = ClientSnapshot {
            settings: (&settings).into(),
            traffic_mode: settings.traffic_mode,
            core_installed,
            core_version,
            active_profile: profiles
                .active_profile()
                .map(|profile| ProfileSummary::from_profile(profile, true)),
            profiles: profile_summaries(&profiles),
            ..ClientSnapshot::default()
        };
        // The initial status is a record, not a bare string, so an English UI
        // renders it in English. `note_event` still writes `snapshot.status`
        // (Chinese), keeping the string history byte-identical.
        let (status_code, status_args) = match &store_unreadable {
            Some(reason) => (EventCode::StoreUnreadable, vec![reason.clone()]),
            None if core_installed => (EventCode::CoreReadyToStart, vec![]),
            None => (EventCode::CoreNotInstalledHint, vec![]),
        };
        // The active configuration from a previous run is still the rules
        // source until the next start rewrites it.
        if let Ok(text) = std::fs::read_to_string(dir.join("cache/active-config.json")) {
            let (rules, rule_sets) = crate::state::parse_route_rules(&text);
            snapshot.rules = rules;
            snapshot.rule_sets = rule_sets;
            snapshot.inbounds = crate::state::parse_inbounds(&text);
        }
        let mut engine = Self {
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
            pending: None,
            healthy_since: None,
            store_unreadable,
        };
        engine.note_event(status_code, status_args);
        engine
    }

    async fn run(
        &mut self,
        mut command_rx: mpsc::UnboundedReceiver<ClientCommand>,
        shared: Arc<Mutex<ClientSnapshot>>,
    ) {
        if self.settings.auto_start
            && self.snapshot.core_installed
            && self.profiles.active_profile().is_some()
        {
            self.snapshot.busy = Some("自动启动内核".into());
            if let Err(error) = self.start_core().await {
                self.note_event(EventCode::CoreAutoStartFailed, vec![error.to_string()]);
            }
            if self.pending.is_none() {
                self.snapshot.busy = None;
            }
            self.publish(&shared);
        }
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let command = tokio::select! {
                command = command_rx.recv() => command,
                _ = tick.tick() => {
                    self.finish_operation().await;
                    self.publish(&shared);
                    // Telemetry requests are read-only and cancellation-safe.
                    // A stop/exit must never wait for a stalled API request.
                    tokio::select! {
                        command = command_rx.recv() => command,
                        _ = self.poll() => { self.publish(&shared); continue; }
                    }
                }
            };
            let Some(command) = command else {
                break;
            };
            // Exit skips the operation gate entirely: shutting down must never
            // wait for a download it is free to cancel.
            if matches!(command, ClientCommand::Shutdown) {
                break;
            }
            if self.pending.is_some()
                && !matches!(
                    command,
                    ClientCommand::StopCore | ClientCommand::RestartCore
                )
            {
                self.note_event(EventCode::OperationAlreadyRunning, vec![]);
                self.publish(&shared);
                continue;
            }
            let label = command.label();
            self.snapshot.busy = Some(label.clone());
            let _ = self
                .event_tx
                .try_send(ClientEvent::OperationStarted(label.clone()));
            self.publish(&shared);
            if let Err(error) = self.apply(command).await {
                self.operation_error(&label, error);
            } else if self.pending.is_none() {
                let _ = self
                    .event_tx
                    .try_send(ClientEvent::OperationFinished(label));
            }
            if self.pending.is_none() {
                self.snapshot.busy = None;
            }
            self.publish(&shared);
        }
        // Child handles and pending jobs are owned by this engine. Preserve
        // the UI's explicit OS-proxy exit choice, but always reap its child.
        self.cancel_operation().await;
        if let Some(mut child) = self.child.take() {
            let _ = child.child.kill().await;
        }
    }

    /// Persists the profile table, unless the on-disk store was unreadable at
    /// startup — in which case the in-memory table is empty and saving it would
    /// delete the subscription links the failure made invisible.
    fn save_profiles(&self) -> Result<()> {
        self.store_writable()?;
        self.profiles.save(&self.dir)
    }

    fn save_settings(&self) -> Result<()> {
        self.store_writable()?;
        self.settings.save(&self.dir)
    }

    fn store_writable(&self) -> Result<()> {
        match &self.store_unreadable {
            None => Ok(()),
            Some(reason) => anyhow::bail!(
                "本地存储不可写（{reason}）；请先修复数据目录内的配置文件，本次改动未保存"
            ),
        }
    }

    fn operation_error(&mut self, label: &str, error: anyhow::Error) {
        let message = error.to_string();
        self.note(format!("{label} 失败: {message}"));
        let _ = self.event_tx.try_send(ClientEvent::Error(ClientError {
            operation: label.into(),
            message,
        }));
    }

    fn spawn_operation(
        &mut self,
        future: impl std::future::Future<Output = Result<Completion>> + Send + 'static,
    ) {
        let label = self
            .snapshot
            .busy
            .clone()
            .unwrap_or_else(|| "后台操作".into());
        self.snapshot.busy = Some(label.clone());
        self.pending = Some(PendingOperation {
            task: tokio::spawn(future),
            label,
            restarting: self.restart_attempts > 0 && self.snapshot.starting,
        });
    }

    async fn cancel_operation(&mut self) {
        if let Some(mut pending) = self.pending.take() {
            pending.task.abort();
            // If startup completed concurrently, dropping the returned closure
            // also drops its child handle (kill_on_drop).
            let _ = (&mut pending.task).await;
        }
        self.snapshot.starting = false;
        self.snapshot.busy = None;
    }

    async fn finish_operation(&mut self) {
        if !self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.task.is_finished())
        {
            return;
        }
        let mut pending = self.pending.take().unwrap();
        let result = match (&mut pending.task).await {
            Ok(Ok(complete)) => complete(self),
            Ok(Err(error)) => Err(error),
            Err(error) => Err(error.into()),
        };
        self.snapshot.starting = false;
        self.snapshot.busy = None;
        if let Err(error) = result {
            self.operation_error(&pending.label, error);
            if pending.restarting {
                self.schedule_restart();
            }
        } else {
            let _ = self
                .event_tx
                .try_send(ClientEvent::OperationFinished(pending.label.clone()));
        }
    }

    fn publish(&self, shared: &Arc<Mutex<ClientSnapshot>>) {
        // A panic in some other task must not leave the interface frozen on a
        // default snapshot while the core keeps running, so take the guard
        // through the poison instead of skipping the publish.
        let mut guard = shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = self.snapshot.clone();
        drop(guard);
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
        if !self.snapshot.core_running
            && matches!(
                command,
                ClientCommand::SwitchNode { .. }
                    | ClientCommand::TestNode(_)
                    | ClientCommand::TestGroup(_)
                    | ClientCommand::CloseConnection(_)
                    | ClientCommand::CloseAllConnections
            )
        {
            anyhow::bail!("内核未运行；请先启动本客户端的内核");
        }
        match command {
            ClientCommand::StartCore => {
                if !self.snapshot.core_running && !self.snapshot.starting {
                    self.restart_attempts = 0;
                    self.snapshot.restart_attempts = 0;
                }
                self.start_core().await
            }
            ClientCommand::StopCore => {
                self.stop_core().await;
                Ok(())
            }
            ClientCommand::RestartCore => {
                self.stop_core().await;
                self.start_core().await
            }
            ClientCommand::ToggleSystemProxy => self.toggle_system_proxy(),
            ClientCommand::SetTrafficMode { mode, restart } => {
                self.set_traffic_mode(mode, restart).await
            }
            ClientCommand::SetOutboundMode(mode) => self.set_outbound_mode(mode),
            ClientCommand::SwitchProfile(name) => self.switch_profile(name),
            ClientCommand::SwitchNode { group, node } => self.switch_node(group, node),
            ClientCommand::TestNode(node) => self.test_nodes(vec![node]),
            ClientCommand::TestGroup(group) => {
                let group = self
                    .snapshot
                    .proxy_groups
                    .iter()
                    .find(|candidate| candidate.name == group)
                    .context("代理组不存在")?;
                self.test_nodes(group.members.clone())
            }
            ClientCommand::CloseConnection(id) => {
                let api = self.api.clone();
                self.spawn_operation(async move {
                    api.close_connection(&id).await?;
                    Ok(Box::new(|engine: &mut Engine| {
                        engine.note("已关闭连接");
                        Ok(())
                    }) as Completion)
                });
                Ok(())
            }
            ClientCommand::CloseAllConnections => self.close_all_connections(),
            ClientCommand::UpdateSubscription => self.update_subscription().await,
            ClientCommand::ImportSubscription { name, url } => {
                self.import_subscription(name, url).await
            }
            ClientCommand::ImportProfileFile(path) => self.import_profile_file(path).await,
            ClientCommand::SetProfileUrl { name, url } => self.set_profile_url(name, url).await,
            ClientCommand::RemoveProfile(name) => self.remove_profile(&name).await,
            ClientCommand::DownloadCore => self.download_core(),
            ClientCommand::UpdateSettings(patch) => self.update_settings(patch),
            // The core-log reader keeps its file offset, so clearing shows only
            // lines written from now on rather than replaying the tail.
            ClientCommand::ClearLogs => {
                self.snapshot.core_logs.clear();
                self.snapshot.events.clear();
                Ok(())
            }
            ClientCommand::Refresh => {
                self.last_traffic_at = Instant::now() - TRAFFIC_EVERY;
                self.last_proxies_at = Instant::now() - PROXIES_EVERY;
                self.last_connections_at = Instant::now() - CONNECTIONS_EVERY;
                Ok(())
            }
            // The engine loop intercepts this before `apply`, so reaching it
            // means a caller invented an exit path that skips the teardown.
            ClientCommand::Shutdown => {
                anyhow::bail!("Shutdown is handled by the engine loop, not by a command")
            }
        }
    }

    /// Persists and publishes a traffic mode. Split from the restart sequencing
    /// so the rollback path can call it twice without duplicating the writes.
    fn apply_traffic_mode(&mut self, mode: TrafficMode) -> Result<()> {
        self.settings.traffic_mode = mode;
        self.save_settings()?;
        self.snapshot.traffic_mode = mode;
        self.snapshot.settings.traffic_mode = mode;
        Ok(())
    }

    /// Switches the traffic mode, restarting the core when the caller accepts it.
    ///
    /// The mode decides the generated `inbounds`, so a running core cannot pick
    /// it up: previously the change was simply refused, which left the toggle
    /// reading as a preference for the next launch rather than a switch. If the
    /// restarted core will not come up in the new mode — TUN without the
    /// privilege, or a missing `wintun.dll` — the previous mode is restored and
    /// started again, because ADR-0003 promises that a failed change leaves the
    /// deployment working rather than half-applied.
    async fn set_traffic_mode(&mut self, mode: TrafficMode, restart: bool) -> Result<()> {
        if !self.snapshot.core_running {
            self.apply_traffic_mode(mode)?;
            self.note(format!("流量模式: {}", mode.label()));
            return Ok(());
        }
        if !restart {
            anyhow::bail!("内核正在运行；切换流量模式需要重启内核，请先停止内核或改用带重启的切换");
        }
        let previous = self.settings.traffic_mode;
        self.apply_traffic_mode(mode)?;
        self.stop_core().await;
        match self.start_core().await {
            Ok(()) => {
                self.note(format!("流量模式: {}（内核已重启）", mode.label()));
                Ok(())
            }
            Err(error) => {
                eprintln!("切换流量模式失败: {error}");
                self.apply_traffic_mode(previous)?;
                self.stop_core().await;
                let _ = self.start_core().await;
                Err(anyhow::anyhow!("切换流量模式失败，已恢复原模式: {error}"))
            }
        }
    }

    fn set_outbound_mode(&mut self, mode: OutboundMode) -> Result<()> {
        if self.snapshot.core_running {
            let api = self.api.clone();
            self.spawn_operation(async move {
                api.set_mode(mode).await?;
                Ok(Box::new(move |engine: &mut Engine| {
                    engine.snapshot.outbound_mode = mode;
                    engine.note(format!("出站模式: {}", mode.label()));
                    Ok(())
                }) as Completion)
            });
        } else {
            self.snapshot.outbound_mode = mode;
            self.note(format!("出站模式: {}", mode.label()));
        }
        Ok(())
    }

    fn switch_profile(&mut self, name: String) -> Result<()> {
        self.profiles
            .activate(&name)
            .ok_or_else(|| anyhow::anyhow!("订阅档案不存在: {name}"))?;
        self.save_profiles()?;
        self.snapshot.active_profile = self
            .profiles
            .active_profile()
            .map(|profile| ProfileSummary::from_profile(profile, true));
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.note_event(EventCode::ProfileActivated, vec![name.to_string()]);
        Ok(())
    }

    fn switch_node(&mut self, group: String, node: String) -> Result<()> {
        let api = self.api.clone();
        self.spawn_operation(async move {
            api.select(&group, &node).await?;
            Ok(Box::new(move |engine: &mut Engine| {
                for snapshot in &mut engine.snapshot.proxy_groups {
                    if snapshot.name == group {
                        snapshot.current = node.clone();
                    }
                }
                engine.snapshot.current_node = Some(node.clone());
                engine.note(format!("{group} → {node}"));
                Ok(())
            }) as Completion)
        });
        Ok(())
    }

    fn close_all_connections(&mut self) -> Result<()> {
        let api = self.api.clone();
        self.spawn_operation(async move {
            let current = api.connections().await?;
            let mut closed = 0;
            for connection in current.connections {
                if api.close_connection(&connection.id).await.is_ok() {
                    closed += 1;
                }
            }
            Ok(Box::new(move |engine: &mut Engine| {
                engine.note(format!("已关闭 {closed} 条连接"));
                Ok(())
            }) as Completion)
        });
        Ok(())
    }

    fn download_core(&mut self) -> Result<()> {
        if self.snapshot.core_running {
            anyhow::bail!("下载内核前请先停止内核");
        }
        let version = self.settings.core_version.clone();
        let mirror = self.settings.mirror.clone();
        let target = self.dir.join("core");
        self.spawn_operation(async move {
            let download = core::download_core(&target, &version, &mirror).await?;
            Ok(Box::new(move |engine: &mut Engine| {
                engine.snapshot.core_installed = true;
                engine.snapshot.core_version = core::detect_version(&download.path).ok();
                engine.note("内核已安装");
                Ok(())
            }) as Completion)
        });
        Ok(())
    }

    fn update_settings(&mut self, patch: SettingsPatch) -> Result<()> {
        if self.snapshot.core_running
            && (patch.traffic_mode.is_some() || patch.mixed_port.is_some())
        {
            anyhow::bail!("切换流量模式或混合端口前请先停止内核");
        }
        patch.apply(&mut self.settings);
        self.save_settings()?;
        self.snapshot.settings = (&self.settings).into();
        self.snapshot.traffic_mode = self.settings.traffic_mode;
        self.note_event(EventCode::SettingsSaved, vec![]);
        Ok(())
    }

    async fn start_core(&mut self) -> Result<()> {
        if self.snapshot.core_running || self.snapshot.starting {
            return Ok(());
        }
        let mode = self.settings.traffic_mode;
        if mode == TrafficMode::Tun {
            if !system_proxy::can_use_tun() {
                anyhow::bail!("TUN 模式需要管理员/root 权限");
            }
            if cfg!(windows) && !settings::wintun_path(&self.dir).is_file() {
                anyhow::bail!("TUN 模式需要 wintun.dll");
            }
        }
        let profile = self
            .profiles
            .active_profile()
            .context("没有激活的订阅档案；先导入订阅")?
            .clone();
        if !settings::core_path(&self.dir).is_file() {
            anyhow::bail!("没有 sing-box 内核；先下载内核");
        }
        let cache = settings::profile_cache_path(&self.dir, &profile.name);
        let dir = self.dir.clone();
        let port = self.settings.mixed_port;
        self.snapshot.starting = true;
        self.spawn_operation(async move {
            let raw = tokio::fs::read_to_string(cache).await.context(
                "读取订阅缓存失败；请更新或重新导入订阅（旧缓存名称冲突时不会自动迁移）",
            )?;
            let started = core::start_managed(&dir, &raw, mode, port).await?;
            Ok(Box::new(move |engine: &mut Engine| {
                if !started.handle.orphan_guard {
                    engine.note(
                        "警告：无法为内核建立系统级回收保护；若本程序被强制结束，sing-box 可能残留",
                    );
                }
                engine.child = Some(started.handle);
                engine.api = started.api;
                engine.snapshot.core_runtime_version = Some(started.version);
                (engine.snapshot.rules, engine.snapshot.rule_sets) =
                    crate::state::parse_route_rules(&started.config);
                engine.snapshot.inbounds = crate::state::parse_inbounds(&started.config);
                engine.snapshot.core_running = true;
                engine.healthy_since = Some(Instant::now());
                engine.restart_at = None;
                engine.log_offset = 0;
                engine.snapshot.core_logs.clear();
                engine.note("内核已启动");
                if mode == TrafficMode::SystemProxy && engine.settings.auto_system_proxy {
                    match system_proxy::enable(&engine.dir, port) {
                        Ok(()) => {
                            engine.snapshot.system_proxy_enabled = true;
                            engine.note(format!("内核已启动；系统代理 → 127.0.0.1:{port}"));
                        }
                        Err(error) => engine.note(format!("内核已启动；系统代理设置失败: {error}")),
                    }
                }
                Ok(())
            }) as Completion)
        });
        Ok(())
    }

    async fn stop_core(&mut self) {
        self.cancel_operation().await;
        self.healthy_since = None;
        self.last_totals = None;
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
        self.note_event(EventCode::CoreStopped, vec![]);
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

    fn test_nodes(&mut self, members: Vec<String>) -> Result<()> {
        let api = self.api.clone();
        let url = self.settings.test_url.clone();
        self.spawn_operation(async move {
            let results = futures_util::future::join_all(members.into_iter().map(|member| {
                let api = api.clone();
                let url = url.clone();
                async move {
                    let result = api.delay_with(&member, &url).await;
                    (member, result)
                }
            }))
            .await;
            Ok(Box::new(move |engine: &mut Engine| {
                for (member, result) in results {
                    match result {
                        Ok(delay) => {
                            engine.delays.insert(member.clone(), delay);
                            engine.failed.retain(|failed| failed != &member);
                        }
                        Err(_) => {
                            engine.delays.remove(&member);
                            if !engine.failed.contains(&member) {
                                engine.failed.push(member);
                            }
                        }
                    }
                }
                engine.apply_delays();
                engine.note("延迟测试完成");
                Ok(())
            }) as Completion)
        });
        Ok(())
    }

    async fn update_subscription(&mut self) -> Result<()> {
        let profile = self
            .profiles
            .active_profile()
            .context("没有激活的订阅档案")?
            .clone();
        if profile.url.is_empty() {
            self.note_event(EventCode::LocalProfileNeedsNoUpdate, vec![]);
            return Ok(());
        }
        let mirror = self.settings.mirror.clone();
        self.spawn_operation(async move {
            let fetched = match subscription::fetch(&profile.url, &mirror).await {
                Ok(fetched) => Ok(fetched),
                Err(error) => match subscription::bare_sing_box_fallback_url(&profile.url) {
                    Some(fallback) if fallback != profile.url => {
                        subscription::fetch(&fallback, &mirror).await
                    }
                    _ => Err(error),
                },
            };
            Ok(Box::new(move |engine: &mut Engine| {
                let cache = settings::profile_cache_path(&engine.dir, &profile.name);
                let fetched = match fetched {
                    Ok(fetched) => fetched,
                    Err(error) if cache.is_file() => {
                        engine.note(format!("订阅更新失败（{error}）；继续使用上次缓存"));
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                let parsed = subscription::parse(&fetched.body)?;
                std::fs::write(cache, &parsed.raw)?;
                for stored in &mut engine.profiles.profiles {
                    if stored.name == profile.name {
                        stored.last_updated = now_epoch();
                    }
                }
                engine.save_profiles()?;
                engine.snapshot.subscription_usage = fetched.userinfo;
                engine.snapshot.profiles = profile_summaries(&engine.profiles);
                engine.snapshot.active_profile = engine
                    .profiles
                    .active_profile()
                    .map(|p| ProfileSummary::from_profile(p, true));
                engine.note(format!("订阅已更新（{} 个节点）", parsed.nodes.len()));
                Ok(())
            }) as Completion)
        });
        Ok(())
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
            self.save_profiles()?;
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
        self.save_profiles()?;
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
        self.save_profiles()?;
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
        self.save_profiles()?;
        self.snapshot.profiles = profile_summaries(&self.profiles);
        self.note_event(EventCode::ProfileUrlUpdated, vec![name.to_string()]);
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
        self.save_profiles()?;
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
        self.note_event(EventCode::ProfileDeleted, vec![name.to_string()]);
        Ok(())
    }

    async fn poll(&mut self) {
        // Tail even while stopped, so a failed startup leaves its sing-box
        // diagnostics visible in the log view.
        self.tail_core_log();
        if self.snapshot.core_running {
            if self.watch_core_exit() {
                self.cancel_operation().await;
                return;
            }
            self.reset_restarts_after_stable_run();
            // A refresh slot is stamped only once its request has run to
            // completion, and the stamp is the completion instant: the engine's
            // inner `select!` drops this future whenever a command arrives, and
            // a stamp taken before the await would burn the slot while nothing
            // was fetched -- freezing traffic, memory and the connection table
            // together even though the core is healthy.
            if Instant::now().duration_since(self.last_connections_at) >= CONNECTIONS_EVERY {
                self.refresh_connections().await;
                self.last_connections_at = Instant::now();
            }
            if Instant::now().duration_since(self.last_traffic_at) >= TRAFFIC_EVERY {
                self.refresh_traffic().await;
                match self.api.memory().await {
                    Ok(memory) => self.snapshot.memory_used = memory,
                    Err(_) => self.snapshot.memory_used = 0,
                }
                self.last_traffic_at = Instant::now();
            }
            if Instant::now().duration_since(self.last_proxies_at) >= PROXIES_EVERY {
                if let Err(error) = self.refresh_proxies().await {
                    self.note_event(EventCode::ProxyGroupRefreshFailed, vec![error.to_string()]);
                }
                self.last_proxies_at = Instant::now();
            }
            let auto_update = self.pending.is_none() && self.auto_update_due();
            if auto_update {
                if let Err(error) = self.update_subscription().await {
                    self.note_event(
                        EventCode::SubscriptionAutoUpdateFailed,
                        vec![error.to_string()],
                    );
                }
                // Paired with `auto_update_due` being side-effect free: this is
                // the longest request in the pass, so it is the one most likely
                // to be dropped mid-flight.
                self.last_auto_update = Some(Instant::now());
            }
        } else {
            // Drop stale runtime state that only makes sense while the core runs.
            if !self.snapshot.proxy_groups.is_empty() {
                self.snapshot.proxy_groups.clear();
            }
            if self.pending.is_none()
                && let Some(at) = self.restart_at
                && Instant::now() >= at
            {
                self.restart_at = None;
                if let Err(error) = self.start_core().await {
                    self.note_event(EventCode::CoreAutoRestartFailed, vec![error.to_string()]);
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
            Err(error) => {
                self.note_event(EventCode::ConnectionRefreshFailed, vec![error.to_string()])
            }
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
        self.snapshot.current_node =
            crate::state::find_selector_group(&snapshots, &self.snapshot.rules)
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
                self.note_event(EventCode::CoreExitedUnexpectedly, vec![status.to_string()]);
                self.schedule_restart();
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.note_event(EventCode::CoreStatusCheckFailed, vec![error.to_string()]);
                false
            }
        }
    }

    fn reset_restarts_after_stable_run(&mut self) {
        if self
            .healthy_since
            .is_some_and(|since| since.elapsed() >= STABLE_RUN)
        {
            self.restart_attempts = 0;
            self.snapshot.restart_attempts = 0;
        }
    }

    fn schedule_restart(&mut self) {
        self.healthy_since = None;
        self.restart_at = None;
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
        if attempt >= MAX_AUTO_RESTARTS {
            // Endless retrying would hide a persistent failure (a stale core
            // still bound to the clash API port, for example). Stop and put
            // the decision back with the administrator.
            self.note(format!(
                "内核连续异常退出 {attempt} 次，已停止自动重启；请排查内核或 \
                 {} 端口占用后手动启动",
                crate::clash_api::DEFAULT_CONTROLLER
            ));
            return;
        }
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
        use std::io::{Read, Seek, SeekFrom};
        let path = self.dir.join("cache/core.log");
        let Ok(mut file) = std::fs::File::open(&path) else {
            return;
        };
        let Ok(meta) = file.metadata() else {
            return;
        };
        let size = meta.len();
        // Each launch truncates the log. A shorter file means the offset points
        // past the end, so start over instead of stalling forever.
        if size < self.log_offset {
            self.log_offset = 0;
        }
        if size == self.log_offset {
            return;
        }
        if file.seek(SeekFrom::Start(self.log_offset)).is_err() {
            self.log_offset = 0;
            return;
        }
        // Read a bounded slice per poll: a core that logs heavily without a
        // restart would otherwise grow this buffer without limit.
        let mut bytes = Vec::new();
        if file
            .take(CORE_LOG_READ_LIMIT)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return;
        }
        // Consume only complete lines, and split on the raw bytes. Decoding
        // first would let an invalid sequence expand to a three-byte replacement
        // character, pushing the recorded offset past the bytes actually read and
        // silently skipping every later line.
        let mut consumed = 0usize;
        for chunk in bytes.split_inclusive(|byte| *byte == b'\n') {
            if chunk.last() != Some(&b'\n') {
                break;
            }
            consumed += chunk.len();
            let line = String::from_utf8_lossy(&chunk[..chunk.len() - 1]);
            let trimmed = line.trim_end_matches('\r');
            if !trimmed.trim().is_empty() {
                self.snapshot.push_log(strip_ansi(trimmed));
            }
        }
        self.log_offset = self.log_offset.saturating_add(consumed as u64);
    }

    fn auto_update_due(&self) -> bool {
        let minutes = self.settings.auto_update_minutes;
        if minutes == 0 || self.profiles.active.is_none() {
            return false;
        }
        // The interval comes straight from a text field, so a huge value must
        // degrade to "effectively never" rather than overflow.
        let interval = Duration::from_secs(minutes.saturating_mul(60));
        match self.last_auto_update {
            Some(last) => last.elapsed() >= interval,
            None => true,
        }
    }

    fn note(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.snapshot.status = message.clone();
        // No code, so no level: the UIs keep their legacy guess for this line.
        self.snapshot.status_level = None;
        self.snapshot.push_event(message);
    }

    /// The structured form of [`Self::note`]: the record carries the code and
    /// its arguments, and the Chinese line is derived from them so today's
    /// string-only consumers keep working while the vocabulary is filled in.
    /// Prefer this for any new engine event — a literal here is what the
    /// English interface cannot render.
    fn note_event(&mut self, code: EventCode, args: Vec<String>) {
        let record = EventRecord::new(code, args);
        self.snapshot.status_level = Some(record.level());
        self.snapshot.status = record.render_zh();
        self.snapshot.push_record(record);
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

/// Removes ANSI escape sequences (sing-box colors its log lines) so the
/// clients' log views show plain text.
fn strip_ansi(line: &str) -> String {
    if !line.contains('\u{1b}') {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // CSI: consume parameters, intermediates and the final byte.
                for follow in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&follow) {
                        break;
                    }
                }
            }
            Some(']') => {
                // OSC: run to the string terminator (BEL or ST).
                let mut previous = ' ';
                for follow in chars.by_ref() {
                    if follow == '\u{7}' || (previous == '\u{1b}' && follow == '\\') {
                        break;
                    }
                    previous = follow;
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clash_api::OutboundMode;
    use crate::settings::Profile;

    async fn test_engine(dir: &std::path::Path) -> Engine {
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        let (events, _) = mpsc::channel(32);
        Engine::new(dir.to_path_buf(), events).await
    }

    const NODE_CONFIG: &str = r#"{"outbounds":[{"type":"shadowsocks","tag":"test","server":"example.test","server_port":443,"method":"aes-128-gcm","password":"fixture"}]}"#;

    #[tokio::test]
    async fn short_lived_successes_exhaust_restarts_and_stable_runs_reset_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        for attempt in 1..=MAX_AUTO_RESTARTS {
            engine.healthy_since = Some(Instant::now());
            engine.reset_restarts_after_stable_run();
            engine.schedule_restart();
            assert_eq!(engine.restart_attempts, attempt);
            assert!(engine.restart_at.is_some());
        }
        engine.healthy_since = Some(Instant::now());
        engine.reset_restarts_after_stable_run();
        engine.schedule_restart();
        assert!(engine.restart_at.is_none());
        engine.healthy_since = Some(Instant::now() - STABLE_RUN);
        engine.reset_restarts_after_stable_run();
        assert_eq!(engine.restart_attempts, 0);
    }

    /// A clash_api stub whose `/connections` reply is deliberately slow, so a
    /// test can drop a `poll()` while the request is in flight.
    async fn slow_connections_api(count: usize) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut request = [0_u8; 1024];
                    let read = stream.read(&mut request).await.unwrap_or(0);
                    let first_line = String::from_utf8_lossy(&request[..read])
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    let path = first_line.split_whitespace().nth(1).unwrap_or_default();
                    let body = if path.starts_with("/connections") {
                        tokio::time::sleep(Duration::from_millis(400)).await;
                        serde_json::json!({
                            "uploadTotal": 1_000 * count as u64,
                            "downloadTotal": 2_000 * count as u64,
                            "connections": (0..count)
                                .map(|index| serde_json::json!({
                                    "id": index.to_string(),
                                    "upload": index,
                                    "download": index,
                                    "start": "2026-09-23T00:00:00Z",
                                    "rule": "MATCH",
                                    "chains": ["🚀节点选择"],
                                    "metadata": {
                                        "destinationIP": "1.1.1.1",
                                        "process": "fixture",
                                    },
                                }))
                                .collect::<Vec<_>>(),
                        })
                        .to_string()
                    } else {
                        "{}".to_owned()
                    };
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: \
                         {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        base
    }

    /// `poll()` is cancelled whenever a command wins the engine's inner
    /// `select!`, so any refresh slot it consumed without finishing the
    /// request must remain open. Otherwise traffic, memory and the connection
    /// table freeze together while the core is demonstrably fine.
    /// A core that comes up and dies again immediately must not buy itself a
    /// fresh allowance of restarts. Only a run long enough to count as healthy
    /// clears the counter; otherwise a crash loop whose rounds each last under
    /// `STABLE_RUN` would retry forever and hide a persistent cause such as a
    /// stale core still bound to the clash_api port.
    /// While the vocabulary is being filled in, the event history exists twice:
    /// strings for today's consumers, records for the language-aware ones. If
    /// they drift, the English and Chinese interfaces show different histories,
    /// so the pairing is pinned here along with the exact Chinese text each
    /// converted call site used to build by hand.
    #[tokio::test]
    async fn a_recorded_event_also_lands_in_the_string_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        // `test_engine` already carries the engine's own initial-status record,
        // so pin the growth relative to that baseline rather than assuming an
        // empty history; the pairing and exact strings are still asserted.
        let events_before = engine.snapshot.events.len();
        let records_before = engine.snapshot.event_records.len();
        engine.note_event(EventCode::CoreStatusCheckFailed, vec!["boom".into()]);
        assert_eq!(engine.snapshot.events.len(), events_before + 1);
        assert_eq!(engine.snapshot.event_records.len(), records_before + 1);
        assert_eq!(
            engine.snapshot.events.back().map(String::as_str),
            Some("检测内核状态失败: boom"),
            "the rendered Chinese line must be the string the call site replaced"
        );
        assert_eq!(engine.snapshot.status, "检测内核状态失败: boom");
        assert_eq!(
            engine.snapshot.event_records.back().unwrap().render_en(),
            "Could not read the core's status: boom"
        );
    }

    /// The toggle must be a switch, not a preference for the next launch: with
    /// the core running, a caller that has not accepted a restart is refused
    /// rather than silently deferred, and nothing is persisted.
    #[tokio::test]
    async fn a_mode_switch_while_the_core_runs_is_refused_without_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        engine.snapshot.core_running = true;
        assert_eq!(engine.settings.traffic_mode, TrafficMode::SystemProxy);
        let error = engine
            .apply(ClientCommand::SetTrafficMode {
                mode: TrafficMode::Tun,
                restart: false,
            })
            .await
            .expect_err("a running core must refuse a switch that has not accepted a restart");
        // Asserting only `is_err` is not enough: attempting the switch and having
        // the restarted core fail produces an error too, and the rollback makes
        // the settings assertions pass. The refusal has to name the way out.
        let message = error.to_string();
        assert!(
            message.contains("重启内核"),
            "the refusal must name the action required, got: {message}"
        );
        assert_eq!(
            engine.settings.traffic_mode,
            TrafficMode::SystemProxy,
            "a refused switch must not persist the mode"
        );
        assert_eq!(engine.snapshot.traffic_mode, TrafficMode::SystemProxy);
    }

    #[tokio::test]
    async fn a_mode_switch_with_the_core_stopped_applies_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        engine
            .apply(ClientCommand::SetTrafficMode {
                mode: TrafficMode::Tun,
                restart: true,
            })
            .await
            .expect("a stopped core applies the mode directly");
        assert_eq!(engine.settings.traffic_mode, TrafficMode::Tun);
        assert_eq!(engine.snapshot.traffic_mode, TrafficMode::Tun);
        assert_eq!(engine.snapshot.settings.traffic_mode, TrafficMode::Tun);
        // The persisted store, not just memory: the mode has to survive the
        // client exiting without a cooperative stop.
        let reloaded = settings::Settings::load_or_create(&engine.dir).expect("settings reload");
        assert_eq!(reloaded.traffic_mode, TrafficMode::Tun);
    }

    #[tokio::test]
    async fn a_brief_success_does_not_reset_the_restart_allowance() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        engine.restart_attempts = 3;
        engine.healthy_since = Some(Instant::now());
        engine.reset_restarts_after_stable_run();
        assert_eq!(
            engine.restart_attempts, 3,
            "a sub-STABLE_RUN success must not clear the counter"
        );
        engine.schedule_restart();
        assert_eq!(
            engine.restart_attempts, 4,
            "the next crash continues the same allowance rather than restarting at 1"
        );
    }

    #[tokio::test]
    async fn a_cancelled_poll_does_not_consume_the_connections_slot() {
        let dir = tempfile::tempdir().unwrap();
        let base = slow_connections_api(3).await;
        let mut engine = test_engine(dir.path()).await;
        engine.api = ClashApi::new(&base);
        engine.snapshot.core_running = true;
        engine.last_connections_at = Instant::now() - CONNECTIONS_EVERY;

        {
            let poll = engine.poll();
            tokio::pin!(poll);
            assert!(
                tokio::time::timeout(Duration::from_millis(80), poll.as_mut())
                    .await
                    .is_err(),
                "the stub should still be in flight when the future is dropped"
            );
        }
        assert_eq!(engine.snapshot.active_connections, 0);

        engine.poll().await;
        assert_eq!(
            engine.snapshot.active_connections, 3,
            "a cancelled poll must not spend the slot without fetching \
             (active_connections: {})",
            engine.snapshot.active_connections
        );
    }

    /// Guard for the fix above, not a red-first repro: deferring the stamp must
    /// not turn an unreachable core into a retry loop every tick. A request that
    /// fails on its own still consumes its slot.
    #[tokio::test]
    async fn a_failed_refresh_still_consumes_its_slot() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        let addr = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap()
        };
        engine.api = ClashApi::new(&format!("http://{addr}"));
        engine.snapshot.core_running = true;
        let seeded = Instant::now() - CONNECTIONS_EVERY;
        engine.last_connections_at = seeded;
        engine.poll().await;
        assert!(
            engine.last_connections_at > seeded,
            "a refused request must still consume the slot, or every tick \
             re-requests a dead core"
        );
    }

    #[tokio::test]
    async fn stopped_clients_refuse_to_control_an_unowned_api() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        assert!(
            engine
                .apply(ClientCommand::CloseAllConnections)
                .await
                .is_err()
        );
        assert!(engine.pending.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_waits_for_the_engine_to_acknowledge_instead_of_timing_out() {
        let dir = tempfile::tempdir().unwrap();
        let controller = ClientController::start(dir.path().to_path_buf());
        let began = Instant::now();
        controller.shutdown();
        assert!(
            began.elapsed() < SHUTDOWN_GRACE / 2,
            "an exit that the engine acknowledged should not spend the grace window"
        );
        // The loop is gone, so a late command must be refused rather than
        // silently queued behind a task that no longer runs.
        assert!(controller.send(ClientCommand::Refresh).is_err());
    }

    #[tokio::test]
    async fn an_unreadable_store_is_reported_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cache")).unwrap();
        std::fs::write(
            dir.path().join("profiles.toml"),
            "not a valid profile table [[",
        )
        .unwrap();
        let engine = test_engine(dir.path()).await;
        assert!(
            engine.snapshot.status.contains("本地存储无法读取"),
            "the failure has to be visible: {}",
            engine.snapshot.status
        );
        assert!(
            engine.save_profiles().is_err(),
            "an empty in-memory table must never replace an unreadable store"
        );
        assert!(engine.save_settings().is_err());
        assert!(
            std::fs::read_to_string(dir.path().join("profiles.toml"))
                .unwrap()
                .contains("not a valid profile table"),
            "the refused save still wrote to disk"
        );
    }

    #[tokio::test]
    async fn deleting_one_profile_preserves_the_other_profiles_cache() {
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        for name in ["a b", "a_b"] {
            let path = dir.path().join(format!("{name}.json"));
            std::fs::write(&path, NODE_CONFIG).unwrap();
            engine
                .import_profile_file(path.to_string_lossy().into())
                .await
                .unwrap();
        }
        engine.remove_profile("a b").await.unwrap();
        assert!(settings::profile_cache_path(dir.path(), "a_b").is_file());
    }

    #[tokio::test]
    async fn importing_a_legacy_link_retries_the_bare_endpoint() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for suffix in ["sing-box-full.json", "sing-box.json"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(String::from_utf8_lossy(&buffer[..count]).contains(suffix));
                let (status, body) = if suffix == "sing-box-full.json" {
                    ("404 Not Found", "")
                } else {
                    ("200 OK", NODE_CONFIG)
                };
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let mut engine = test_engine(dir.path()).await;
        let result = engine
            .import_subscription(
                Some("legacy".into()),
                format!("http://{address}/sub/fixture/sing-box.json"),
            )
            .await;
        assert!(result.is_ok(), "{result:?}");
        tokio::time::timeout(Duration::from_secs(5), async {
            while engine.pending.is_some() {
                engine.finish_operation().await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        server.abort();
        assert!(
            settings::profile_cache_path(dir.path(), "legacy").is_file(),
            "{}",
            engine.snapshot.status
        );
    }

    #[tokio::test]
    async fn a_pending_import_publishes_busy_and_can_be_stopped() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cache")).unwrap();
        let controller = ClientController::start(dir.path().to_path_buf());
        controller
            .send(ClientCommand::ImportSubscription {
                name: None,
                url: format!("http://{address}/config.json"),
            })
            .unwrap();
        let (_socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        assert!(controller.snapshot().busy.is_some());
        controller.send(ClientCommand::StopCore).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if controller.snapshot().busy.is_none()
                    && controller.snapshot().status.contains("内核已停止")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("StopCore must cancel pending network work");
    }

    #[test]
    fn strip_ansi_removes_color_sequences_but_keeps_text() {
        assert_eq!(
            strip_ansi("\u{1b}[36mINFO\u{1b}[0m inbound started"),
            "INFO inbound started"
        );
        assert_eq!(strip_ansi("plain line"), "plain line");
        assert_eq!(
            strip_ansi("\u{1b}[38;5;49m\u{1b}[1mcolored\u{1b}[0m tail"),
            "colored tail"
        );
    }

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
