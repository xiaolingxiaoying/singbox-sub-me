use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::mpsc;

use crate::state::ClientSnapshot;
use crate::{ClientCommand, ClientEvent};
use crate::{core, settings, subscription, system_proxy};

/// Owns the long-lived client state and presents a small command/event seam to UIs.
pub struct ClientController {
    command_tx: mpsc::Sender<ClientCommand>,
    event_rx: mpsc::Receiver<ClientEvent>,
}

impl ClientController {
    pub fn start(dir: PathBuf) -> Self {
        let (command_tx, mut command_rx) = mpsc::channel(32);
        let (event_tx, event_rx) = mpsc::channel(128);
        tokio::spawn(async move {
            let mut snapshot = ClientSnapshot::default();
            let mut child = None;
            let _ = event_tx
                .send(ClientEvent::SnapshotChanged(snapshot.clone()))
                .await;
            while let Some(command) = command_rx.recv().await {
                let label = format!("{:?}", command);
                let _ = event_tx
                    .send(ClientEvent::OperationStarted(label.clone()))
                    .await;
                let result = apply_command(&dir, &mut snapshot, &mut child, command).await;
                match result {
                    Ok(()) => {
                        let _ = event_tx
                            .send(ClientEvent::SnapshotChanged(snapshot.clone()))
                            .await;
                        let _ = event_tx.send(ClientEvent::OperationFinished(label)).await;
                    }
                    Err(error) => {
                        let _ = event_tx
                            .send(ClientEvent::Error(crate::ClientError {
                                operation: label,
                                message: error.to_string(),
                            }))
                            .await;
                    }
                }
            }
        });
        Self {
            command_tx,
            event_rx,
        }
    }

    pub async fn send(&self, command: ClientCommand) -> Result<()> {
        self.command_tx
            .send(command)
            .await
            .map_err(|_| anyhow::anyhow!("client controller stopped"))
    }

    pub async fn recv(&mut self) -> Option<ClientEvent> {
        self.event_rx.recv().await
    }
}

async fn apply_command(
    dir: &PathBuf,
    snapshot: &mut ClientSnapshot,
    child: &mut Option<core::CoreHandle>,
    command: ClientCommand,
) -> Result<()> {
    match command {
        ClientCommand::StartCore => {
            let _ = settings::Settings::load_or_create(dir)?;
            let profiles = settings::Profiles::load_or_create(dir)?;
            let profile = profiles
                .active_profile()
                .ok_or_else(|| anyhow::anyhow!("没有激活的订阅档案；先在设置中导入"))?;
            let binary = settings::core_path(dir);
            if !binary.is_file() {
                anyhow::bail!("没有 sing-box 内核；先在设置中下载");
            }
            let cache = settings::profile_cache_path(dir, &profile.name);
            let raw = tokio::fs::read_to_string(&cache).await?;
            let active = dir.join("cache/active-config.json");
            let adapted = core::adapt_inbounds(&raw, snapshot.traffic_mode)?;
            tokio::fs::write(&active, adapted).await?;
            core::check_config(&binary, &active)?;
            *child = Some(core::start(&binary, &active, &dir.join("cache/core.log")).await?);
            snapshot.active_profile = Some(profile.into());
            snapshot.core_running = true;
            snapshot.status = "内核运行中".into();
        }
        ClientCommand::StopCore => {
            if let Some(mut running) = child.take() {
                let _ = running.child.kill().await;
            }
            snapshot.core_running = false;
            snapshot.system_proxy_enabled = false;
            snapshot.status = "内核已停止".into();
        }
        ClientCommand::ToggleSystemProxy => {
            if snapshot.system_proxy_enabled {
                system_proxy::disable()?;
            } else {
                system_proxy::enable(system_proxy::LOCAL_MIXED_PORT)?;
            }
            snapshot.system_proxy_enabled = !snapshot.system_proxy_enabled;
        }
        ClientCommand::SetTrafficMode(mode) => {
            if snapshot.core_running {
                anyhow::bail!("切换流量模式前请先停止内核");
            }
            snapshot.traffic_mode = mode;
        }
        ClientCommand::SetOutboundMode(mode) => {
            if snapshot.core_running {
                let api = crate::clash_api::ClashApi::new(crate::clash_api::DEFAULT_CONTROLLER);
                api.set_mode(mode).await?;
            }
            snapshot.outbound_mode = mode;
        }
        ClientCommand::SwitchProfile(name) => {
            let mut profiles = settings::Profiles::load_or_create(dir)?;
            profiles
                .activate(&name)
                .ok_or_else(|| anyhow::anyhow!("订阅档案不存在: {name}"))?;
            profiles.save(dir)?;
            snapshot.active_profile = Some(crate::state::ProfileSummary {
                name,
                ..Default::default()
            });
        }
        ClientCommand::SwitchNode(node) => {
            let api = crate::clash_api::ClashApi::new(crate::clash_api::DEFAULT_CONTROLLER);
            api.select("🚀节点选择", &node).await?;
            snapshot.current_node = Some(node);
        }
        ClientCommand::UpdateSubscription => {
            let mut profiles = settings::Profiles::load_or_create(dir)?;
            let active = profiles
                .active_profile()
                .ok_or_else(|| anyhow::anyhow!("没有激活的订阅档案"))?
                .clone();
            let fetched = subscription::fetch(&active.url, "").await?;
            let cache = settings::profile_cache_path(dir, &active.name);
            tokio::fs::write(&cache, fetched.body).await?;
            if let Some(profile) = profiles.profiles.iter_mut().find(|p| p.name == active.name) {
                profile.last_updated = now_epoch();
            }
            profiles.save(dir)?;
            snapshot.status = "订阅更新完成".into();
        }
        ClientCommand::TestNode(node) => {
            let api = crate::clash_api::ClashApi::new(crate::clash_api::DEFAULT_CONTROLLER);
            let delay = api.delay(&node).await?;
            snapshot.status = format!("{node} 延迟 {delay} ms");
        }
        ClientCommand::CloseConnection(id) => {
            let api = crate::clash_api::ClashApi::new(crate::clash_api::DEFAULT_CONTROLLER);
            api.close_connection(&id).await?;
        }
        ClientCommand::CloseAllConnections => {
            let api = crate::clash_api::ClashApi::new(crate::clash_api::DEFAULT_CONTROLLER);
            let current = api.connections().await?;
            for connection in current.connections {
                api.close_connection(&connection.id).await?;
            }
        }
    }
    Ok(())
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

    #[tokio::test]
    async fn commands_produce_snapshot_events() {
        let tempdir = tempfile::tempdir().unwrap();
        let mut controller = ClientController::start(tempdir.path().to_path_buf());
        let _ = controller.recv().await;
        controller
            .send(ClientCommand::SetOutboundMode(
                crate::clash_api::OutboundMode::Global,
            ))
            .await
            .unwrap();
        let mut running = false;
        for _ in 0..3 {
            if let Some(ClientEvent::SnapshotChanged(snapshot)) = controller.recv().await {
                running = snapshot.outbound_mode == crate::clash_api::OutboundMode::Global;
                break;
            }
        }
        assert!(running);
    }
}
