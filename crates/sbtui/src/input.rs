//! Everything the keyboard can change: the key map, the input overlay's commit
//! step, and the cursor/highlight moves they call. This module draws nothing; it
//! edits [`App`] and sends [`ClientCommand`]s to the engine.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ClientCommand;
use crate::app::{App, ConnSort, InputGoal, Tab};
use crate::clash_api::Connection;
use crate::command::SettingsPatch;
use crate::system_proxy::TrafficMode;
use crate::view::logs::{current_log_lines, log_query_matches};
use crate::view::settings::copy_to_clipboard_osc52;

pub(crate) fn handle_key(app: &mut App, event: KeyEvent) {
    // A terminal reports the same `code` for Ctrl+S and for S, so without this
    // guard a stray Ctrl+S silently stops the core, and Ctrl+U used to inject a
    // control character into the search box. Ctrl+C is consumed by the event
    // loop before it gets here.
    if event
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return;
    }
    let key = event.code;
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
        KeyCode::PageDown => page_logs(app, 1),
        KeyCode::PageUp => page_logs(app, -1),
        KeyCode::End => app.log_scroll = 0,
        KeyCode::Enter => select_current(app),
        KeyCode::Char('s') => {
            if app.snapshot.core_running || app.snapshot.starting || app.snapshot.busy.is_some() {
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
        KeyCode::Char('r') if app.tab == Tab::Logs => app.tab = Tab::Rules,
        KeyCode::Char('r') if app.tab == Tab::Rules => app.tab = Tab::Logs,
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
            app.selected_connection_id = None;
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
            let rows = visible_connections(app);
            if rows.is_empty() {
                app.selected_connection_id = None;
                return;
            }
            let current = selected_connection_index(app) as isize;
            let next = (current + delta).clamp(0, rows.len() as isize - 1) as usize;
            app.selected_connection_id = Some(rows[next].id.clone());
        }
        Tab::Logs => {
            // Down means towards the newest line, which is a smaller offset.
            app.log_scroll = if delta > 0 {
                app.log_scroll.saturating_sub(delta as usize)
            } else {
                app.log_scroll.saturating_add((-delta) as usize)
            };
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

/// Pages the Logs panel by one screenful. Other tabs ignore the keys so they
/// stay free for a future binding.
fn page_logs(app: &mut App, direction: isize) {
    if app.tab != Tab::Logs {
        return;
    }
    let page = app.log_view_height.max(1) as isize;
    move_cursor(app, direction * page);
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
            let selected = app.snapshot.core_running.then(|| {
                app.selected_group_snapshot().map(|group| {
                    (
                        group.name.clone(),
                        group.is_auto(),
                        group.members.get(app.selected_member).cloned(),
                    )
                })
            });
            if let Some((group, automatic, Some(node))) = selected.flatten() {
                if automatic {
                    // A urltest group selects its own node; sending SwitchNode
                    // to it always fails, and the desktop client refuses too.
                    app.status = format!("「{group}」是自动选择组，不支持手动切换（按 t 可测速）");
                } else {
                    app.send(ClientCommand::SwitchNode { group, node });
                }
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
pub(crate) fn visible_connections(app: &App) -> Vec<&Connection> {
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
        .get(selected_connection_index(app))
        .map(|connection| (*connection).clone())
    else {
        return;
    };
    app.send(ClientCommand::CloseConnection(connection.id));
}

/// The row the connection highlight currently sits on. A stale id (the
/// connection closed, or the filter changed) falls back to the first row.
pub(crate) fn selected_connection_index(app: &App) -> usize {
    let rows = visible_connections(app);
    app.selected_connection_id
        .as_ref()
        .and_then(|id| rows.iter().position(|row| &row.id == id))
        .unwrap_or(0)
        .min(rows.len().saturating_sub(1))
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
pub(crate) fn conn_sort_key(a: &Connection, b: &Connection, sort: ConnSort) -> std::cmp::Ordering {
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
            "再按 m 确认切换到 {}（将重启内核；TUN 需要管理员/root 权限）",
            target.label()
        );
        return;
    }
    app.confirm_mode = false;
    app.send(ClientCommand::SetTrafficMode {
        mode: target,
        restart: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientController;
    use crate::clash_api;
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

    fn group(name: &str, current: &str, members: &[&str]) -> crate::state::ProxyGroupSnapshot {
        crate::state::ProxyGroupSnapshot {
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

        app.selected_connection_id = None;
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
    async fn the_connection_highlight_follows_the_connection_across_reordering() {
        let tempdir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(tempdir.path().to_path_buf()),
            tempdir.path().to_path_buf(),
        );
        app.tab = Tab::Connections;
        app.snapshot.connections.connections = vec![
            connection("api.github.com", "Proxy"),
            connection("cdn.example.net", "DIRECT"),
        ];

        move_cursor(&mut app, 1);
        assert_eq!(selected_connection_index(&app), 1);

        // The engine re-sorts the table on its next publish.
        app.snapshot.connections.connections.reverse();
        assert_eq!(
            selected_connection_index(&app),
            0,
            "the highlight stays on the same connection, not its old row"
        );
        assert_eq!(
            visible_connections(&app)[selected_connection_index(&app)].id,
            "cdn.example.net"
        );

        app.selected_connection_id = Some("closed".to_owned());
        assert_eq!(
            selected_connection_index(&app),
            0,
            "a vanished connection falls back to the first row"
        );
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
}
