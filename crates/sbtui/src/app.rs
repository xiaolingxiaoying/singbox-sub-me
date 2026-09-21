//! The terminal client's own state: the enums that name the tabs, filters and
//! overlays, and [`App`], which pairs the engine snapshot with the view state.

use std::path::PathBuf;

use ratatui::widgets::ListState;

use crate::input::conn_sort_key;
use crate::state::{LogLevel, ProxyGroupSnapshot, log_level_shown};
use crate::{ClientCommand, ClientController, ClientSnapshot};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Dashboard,
    Proxies,
    Connections,
    Logs,
    Settings,
}

impl Tab {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Dashboard => Self::Proxies,
            Self::Proxies => Self::Connections,
            Self::Connections => Self::Logs,
            Self::Logs => Self::Settings,
            Self::Settings => Self::Dashboard,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::Dashboard => Self::Settings,
            Self::Proxies => Self::Dashboard,
            Self::Connections => Self::Proxies,
            Self::Logs => Self::Connections,
            Self::Settings => Self::Logs,
        }
    }

    pub(crate) fn from_index(index: usize) -> Option<Self> {
        Some(match index {
            0 => Self::Dashboard,
            1 => Self::Proxies,
            2 => Self::Connections,
            3 => Self::Logs,
            4 => Self::Settings,
            _ => return None,
        })
    }

    pub(crate) fn index(self) -> usize {
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
pub(crate) enum InputGoal {
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
pub(crate) enum ConnSort {
    Download,
    Upload,
    Host,
    Target,
}

impl ConnSort {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Download => Self::Upload,
            Self::Upload => Self::Host,
            Self::Host => Self::Target,
            Self::Target => Self::Download,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Download => "下载",
            Self::Upload => "上传",
            Self::Host => "主机",
            Self::Target => "目标",
        }
    }
}

/// Log level filter, cycled with `l` on the Logs tab. The threshold semantics
/// come from `client_core::state::log_level_shown`, which both clients share so
/// the same line passes or is hidden identically in either UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LogFilter {
    All,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogFilter {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::All => Self::Debug,
            Self::Debug => Self::Info,
            Self::Info => Self::Warn,
            Self::Warn => Self::Error,
            Self::Error => Self::All,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Debug => "debug+",
            Self::Info => "info+",
            Self::Warn => "warn+",
            Self::Error => "error",
        }
    }

    fn minimum(self) -> Option<LogLevel> {
        match self {
            Self::All => None,
            Self::Debug => Some(LogLevel::Debug),
            Self::Info => Some(LogLevel::Info),
            Self::Warn => Some(LogLevel::Warn),
            Self::Error => Some(LogLevel::Error),
        }
    }

    pub(crate) fn matches(self, line: &str) -> bool {
        log_level_shown(self.minimum(), line)
    }
}

/// The terminal client's own state. Everything the engine owns (the core, the
/// settings, the profiles, live traffic) lives in `snapshot`; the fields below
/// are only view state: which row is highlighted, what filter is active, and
/// which overlay is open.
pub(crate) struct App {
    pub(crate) controller: ClientController,
    pub(crate) snapshot: ClientSnapshot,
    pub(crate) dir: PathBuf,
    pub(crate) tab: Tab,
    // Proxies tab.
    pub(crate) selected_group: usize,
    /// Highlighted member inside the selected proxy group.
    pub(crate) selected_member: usize,
    pub(crate) group_list: ListState,
    /// The group index whose member highlight was last synced to its current
    /// node. While it matches `selected_group`, ticks leave the highlight where
    /// the user put it instead of snapping back to the running node.
    pub(crate) proxies_synced_group: Option<usize>,
    // Connections tab.
    /// The highlighted connection, tracked by id: the engine re-sorts the table
    /// on every refresh, so a row index would drift onto a different connection
    /// and `x` would close the wrong one.
    pub(crate) selected_connection_id: Option<String>,
    pub(crate) conn_sort: ConnSort,
    /// Case-insensitive substring filter for the connection table.
    pub(crate) conn_filter: String,
    // Logs tab.
    /// Toggle between the live log tail (default) and a rendering of the
    /// active configuration's routing rules.
    pub(crate) show_rules: bool,
    /// Rows scrolled back from the newest line; 0 follows the tail.
    pub(crate) log_scroll: usize,
    /// Inner text height of the last rendered log panel, used to page.
    pub(crate) log_view_height: u16,
    /// Freeze the log tail so the view can be inspected while lines stream.
    pub(crate) paused_logs: Option<Vec<String>>,
    pub(crate) log_filter: LogFilter,
    /// Case-insensitive substring filter applied on top of the level filter.
    pub(crate) log_query: String,
    // Settings tab.
    pub(crate) profiles_list: ListState,
    /// A profile name awaiting a second Delete press.
    pub(crate) confirm_delete: Option<String>,
    // Input overlay.
    pub(crate) input: Option<InputGoal>,
    pub(crate) pending_profile_name: Option<String>,
    pub(crate) input_text: String,
    // Confirmations.
    pub(crate) confirm_mode: bool,
    pub(crate) confirm_quit: bool,
    /// A discoverable keyboard reference overlay for first-run users.
    pub(crate) show_help: bool,
    /// The status line the footer shows. Engine status changes overwrite it
    /// (tracked via `last_engine_status`); UI-level messages (filters,
    /// confirmations) write it directly.
    pub(crate) status: String,
    pub(crate) last_engine_status: String,
}

impl App {
    pub(crate) fn new(controller: ClientController, dir: PathBuf) -> Self {
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
            selected_connection_id: None,
            conn_sort: ConnSort::Download,
            conn_filter: String::new(),
            show_rules: false,
            log_scroll: 0,
            log_view_height: 0,
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
    pub(crate) fn refresh_snapshot(&mut self) {
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

    pub(crate) fn selected_group_snapshot(&self) -> Option<&ProxyGroupSnapshot> {
        self.snapshot.proxy_groups.get(self.selected_group)
    }

    /// The reported delay for the node the dashboard highlights, from any
    /// group that carries it.
    pub(crate) fn node_delay(&self, node: &str) -> Option<u64> {
        self.snapshot
            .proxy_groups
            .iter()
            .find(|group| group.members.iter().any(|member| member == node))
            .and_then(|group| group.delays.get(node).copied())
    }

    pub(crate) fn send(&mut self, command: ClientCommand) {
        self.status = format!("{}…", command.label());
        let _ = self.controller.send(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_cycle_in_both_directions() {
        assert_eq!(Tab::Dashboard.next(), Tab::Proxies);
        assert_eq!(Tab::Settings.next(), Tab::Dashboard);
        assert_eq!(Tab::Dashboard.previous(), Tab::Settings);
        assert_eq!(Tab::from_index(2), Some(Tab::Connections));
        assert_eq!(Tab::from_index(9), None);
    }
}
