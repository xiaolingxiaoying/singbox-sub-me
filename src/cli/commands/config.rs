//! The deployment configuration surface: `sbctl config`, its override
//! templates sub-tree, the interactive wizard, the regeneration transaction and
//! the service restart with rollback that both the wizard and credential
//! rotation rely on.

use crate::cli::args::{CliOverrideTarget, ConfigCommand, OverrideCommand};
use crate::cli::prompt::{ConsolePrompts, protocol_ports};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) fn restart(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        let binary = sing_box_bin.unwrap_or_else(|| root.join("usr/local/bin/sing-box"));
        let server =
            std::fs::read_to_string(root.join("var/lib/sbctl/artifacts/sing-box-server.json"))
                .map_err(sbctl::config::ConfigError::Storage)?;
        sbctl::subscription::check_sing_box_config(&binary, &server)
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        sbctl::lifecycle::restart_services(root)
            .map_err(sbctl::config::ConfigError::StateContent)?;
        Ok(config)
    });
    match result {
        Ok(_) => {
            println!("sing-box and sbctl services restarted");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("restart failed: {error}");
            ExitCode::from(2)
        }
    }
}

/// Manages the server-side override templates (`sbctl config override ...`).
/// Overrides merge into the generated client artifacts at regeneration time,
/// so every edit finishes with a regenerate to keep the served files current.
fn run_config_override(root: &Path, command: OverrideCommand) -> ExitCode {
    use sbctl::override_template::{CLASH_OVERRIDE_RELATIVE_PATH, SING_BOX_OVERRIDE_RELATIVE_PATH};
    let overrides_dir = root.join("etc/sbctl/overrides");
    match command {
        OverrideCommand::Show => {
            for (target, relative) in [
                ("sing-box", SING_BOX_OVERRIDE_RELATIVE_PATH),
                ("clash", CLASH_OVERRIDE_RELATIVE_PATH),
            ] {
                let path = root.join(relative);
                let status = if path.is_file() {
                    "已启用"
                } else {
                    "未创建（不影响生成）"
                };
                println!("{target:9} {}  [{status}]", path.display());
            }
            println!(
                "\n合并语义：对象递归合并；数组整体替换；键名为 rules 的数组会前插到生成规则之前。"
            );
            println!(
                "影响工件：sing-box-full.json、sing-box-<版本>.json、clash.yaml、clash-1.18.yaml。"
            );
            ExitCode::SUCCESS
        }
        OverrideCommand::Validate { sing_box_bin } => {
            if let Err(error) = sbctl::override_template::Overrides::load(root) {
                eprintln!("override 校验失败：{error}");
                return ExitCode::from(2);
            }
            let store = sbctl::config::DeploymentStore::new(root);
            let Ok(config) = store.load() else {
                println!("override 模板结构有效（部署尚未初始化，跳过合并后真核 check）。");
                return ExitCode::SUCCESS;
            };
            let artifacts = match sbctl::subscription::generated_artifacts(&config, root) {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    eprintln!("override 合并失败：{error}");
                    return ExitCode::from(2);
                }
            };
            let Some(binary) = resolve_sing_box_bin(root, sing_box_bin) else {
                println!(
                    "override 模板结构有效；未找到 sing-box 内核（用 --sing-box-bin 指定），跳过合并后真核 check。"
                );
                return ExitCode::SUCCESS;
            };
            let name = sbctl::subscription::SubscriptionFormat::SingBoxFull
                .artifact_name()
                .into_owned();
            let Some((_, merged)) = artifacts.iter().find(|(artifact, _)| *artifact == name) else {
                eprintln!("override 校验失败：缺少 sing-box-full 工件");
                return ExitCode::from(2);
            };
            match sbctl::subscription::check_sing_box_config(&binary, merged) {
                Ok(()) => {
                    println!(
                        "override 模板有效；合并后 sing-box 配置已通过真核 check（{}）。",
                        binary.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!(
                        "override 合并后 sing-box check 失败（内核 {}）：{error}\n\
                         提示：合并后的 sing-box-full 工件面向最新稳定版内核；若上面报告未知字段，请先升级服务端内核（sbctl sing-box update）。",
                        binary.display()
                    );
                    ExitCode::from(2)
                }
            }
        }
        OverrideCommand::Edit {
            target,
            sing_box_bin,
        } => {
            let (relative, sample) = match target {
                CliOverrideTarget::SingBox => (
                    SING_BOX_OVERRIDE_RELATIVE_PATH,
                    "{\n  \"log\": {\"level\": \"warn\"},\n  \"route\": {\n    \"rules\": [\n      {\"domain_suffix\": [\"example.com\"], \"outbound\": \"🚀节点选择\"}\n    ]\n  }\n}\n",
                ),
                CliOverrideTarget::Clash => (
                    CLASH_OVERRIDE_RELATIVE_PATH,
                    "# 键名为 rules 的数组会前插到生成规则之前。\nrules:\n  - DOMAIN-SUFFIX,example.com,🌍选择代理节点\n",
                ),
            };
            let path = root.join(relative);
            if !path.is_file() {
                if let Err(error) = fs::create_dir_all(&overrides_dir) {
                    eprintln!("override 编辑失败：{error}");
                    return ExitCode::from(2);
                }
                if let Err(error) = fs::write(&path, sample) {
                    eprintln!("override 编辑失败：{error}");
                    return ExitCode::from(2);
                }
            }
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_owned()
                } else {
                    "vi".to_owned()
                }
            });
            let status = std::process::Command::new(&editor).arg(&path).status();
            match status {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    eprintln!("编辑器 {editor} 退出码 {status}；模板未验证。");
                    return ExitCode::from(2);
                }
                Err(error) => {
                    eprintln!("无法启动编辑器 {editor}：{error}（可设置 EDITOR 环境变量）");
                    return ExitCode::from(2);
                }
            }
            if let Err(error) = sbctl::override_template::Overrides::load(root) {
                eprintln!("override 校验失败：{error}");
                return ExitCode::from(2);
            }
            println!("override 模板有效，正在重新生成订阅工件……");
            regenerate(root, sing_box_bin)
        }
        OverrideCommand::Clear => {
            for relative in [
                SING_BOX_OVERRIDE_RELATIVE_PATH,
                CLASH_OVERRIDE_RELATIVE_PATH,
            ] {
                let path = root.join(relative);
                if path.is_file()
                    && let Err(error) = fs::remove_file(&path)
                {
                    eprintln!("override 清理失败：{error}");
                    return ExitCode::from(2);
                }
            }
            println!("override 模板已删除，正在重新生成订阅工件……");
            regenerate(root, None)
        }
    }
}

/// Resolves the sing-box binary for an override validation: an explicit path,
/// the managed installation path, or a `sing-box` available on `PATH`.
pub(crate) fn resolve_sing_box_bin(root: &Path, explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(binary) = explicit {
        return Some(binary);
    }
    let managed = root.join("usr/local/bin/sing-box");
    if managed.is_file() {
        return Some(managed);
    }
    let on_path = PathBuf::from(if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    });
    std::process::Command::new(&on_path)
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .map(|_| on_path)
}

pub(crate) fn regenerate(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    // Regenerate always re-syncs the active sing-box configuration on a live
    // host (root "/"), even for an installation that never wrote the ownership
    // marker (a `--no-start` install). Fixture roots are left untouched.
    let update_active_config =
        root == std::path::Path::new("/") || root.join("var/lib/sbctl/ownership").is_file();
    let result = store.load().and_then(|config| {
        let binary = sing_box_bin.unwrap_or_else(|| root.join("usr/local/bin/sing-box"));
        let direct = config.subscription_mode == sbctl::config::SubscriptionMode::Direct;
        sbctl::subscription::regenerate(
            &store,
            &config,
            Some(binary.as_path()),
            update_active_config,
        )
        .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        // Always restore daemon-storage permissions after a live regeneration so
        // rewritten artifacts and the active sing-box configuration stay readable
        // by their service accounts, even for an installation that never wrote the
        // ownership marker (a `--no-start` install) (issue #5). `prepare_daemon_storage`
        // is a no-op for non-live helper roots.
        sbctl::lifecycle::prepare_daemon_storage(root, direct)
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        Ok(())
    });
    match result {
        Ok(()) => {
            println!("canonical protocol artifacts regenerated and validated");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("regenerate failed: {error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run_config(root: &Path, command: ConfigCommand) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = match command {
        ConfigCommand::Wizard { sing_box_bin } => return run_config_wizard(root, sing_box_bin),
        ConfigCommand::Override { command } => return run_config_override(root, command),
        ConfigCommand::Init {
            mode,
            subscription_host,
            proxy_host,
            http_port,
            listen_port,
            interface,
            protocols,
            reality_decoy_sni,
            protocol_sni,
            monthly_traffic_limit,
            accounting_policy,
            accounting_timezone,
            client_display_timezone,
            anchored_reset_at,
            sing_box_bin,
            vless_port,
            vmess_port,
            hysteria2_port,
            tuic_port,
            anytls_port,
        } => {
            let interface = interface.map(Ok).unwrap_or_else(|| {
                sbctl::traffic::detect_default_route_interface(root).map_err(|error| {
                    sbctl::config::ConfigError::StateContent(format!(
                        "could not detect a default-route interface ({error}); specify --interface"
                    ))
                })
            });
            interface.and_then(|interface| {
                let mut config = sbctl::config::DeploymentConfig::new_with_ports(
                    mode.into(),
                    subscription_host,
                    proxy_host,
                    http_port,
                    interface,
                    protocols.into_iter().map(Into::into).collect(),
                    reality_decoy_sni,
                    protocol_ports(vless_port, vmess_port, hysteria2_port, tuic_port, anytls_port),
                )?;
                config.protocol_sni = protocol_sni;
                config.monthly_traffic_limit = monthly_traffic_limit;
                config.accounting_policy = accounting_policy.into();
                if let Some(timezone) = accounting_timezone {
                    config.accounting_timezone = timezone;
                }
                if let Some(timezone) = client_display_timezone {
                    config.client_display_timezone = timezone;
                }
                config.anchored_reset_at = anchored_reset_at;
                // A missing --listen-port keeps the 2080 loopback default that a
                // fresh external-proxy deployment already carries; an explicit
                // value (or an explicit non-external-proxy mode) decides below.
                if listen_port.is_some() {
                    config.subscription_listen_port = listen_port;
                }
                config.validate()?;
                if let Some(port) = config.subscription_listen_port {
                    sbctl::subscription::ensure_external_proxy_listener_available(port)
                        .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                }
                let generated_artifacts = if config
                    .enabled_protocols
                    .iter()
                    .any(sbctl::config::ManagedProtocol::has_generated_subscription_artifacts)
                {
                    sbctl::subscription::generated_artifacts(&config, root).map_err(|error| {
                        sbctl::config::ConfigError::StateContent(error.to_string())
                    })?
                } else {
                    Vec::new()
                };
                let artifact_references = generated_artifacts
                    .iter()
                    .map(|(name, contents)| (name.clone(), contents.as_bytes()))
                    .collect::<Vec<_>>();
                let requires_sing_box_check = config.enabled_protocols.iter().any(|protocol| {
                    matches!(
                        protocol,
                        sbctl::config::ManagedProtocol::VmessWebsocket
                            | sbctl::config::ManagedProtocol::Hysteria2
                            | sbctl::config::ManagedProtocol::Tuic
                            | sbctl::config::ManagedProtocol::Anytls
                    )
                });
                if requires_sing_box_check && sing_box_bin.is_none() {
                    return Err(sbctl::config::ConfigError::InvalidValue(
                        "certificate-based Managed protocols require --sing-box-bin for configuration validation",
                    ));
                }
                if let Some(sing_box_bin) = sing_box_bin {
                    let server_config = generated_artifacts
                        .iter()
                        .find(|(name, _)| *name == "sing-box-server.json")
                        .map(|(_, contents)| contents)
                        .ok_or(sbctl::config::ConfigError::InvalidValue(
                            "no generated sing-box server configuration is available to check",
                        ))?;
                    sbctl::subscription::check_sing_box_config(&sing_box_bin, server_config)
                        .map_err(|error| {
                            sbctl::config::ConfigError::StateContent(error.to_string())
                        })?;
                }
                store.initialize_with_artifacts(&config, &artifact_references)
            })
        }
        .map(|_| "deployment configuration initialized".to_owned()),
        ConfigCommand::SwitchMode { mode, listen_port } => store.load().and_then(|mut config| {
            config.subscription_mode = mode.into();
            config.subscription_listen_port = listen_port;
            if config.subscription_mode != sbctl::config::SubscriptionMode::IpFallback {
                config.http_port = None;
            }
            config.validate()?;
            if let Some(port) = config.subscription_listen_port {
                sbctl::subscription::ensure_external_proxy_listener_available(port)
                    .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
            }
            store.replace(&config)
        })
        .map(|_| "subscription mode changed".to_owned()),
        ConfigCommand::Show => store.load().map(|config| config.summary()),
        ConfigCommand::Validate => store
            .load()
            .map(|_| "deployment configuration is valid".to_owned()),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("configuration failed: {error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run_config_wizard(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let existing = match store.load() {
        Ok(config) => Some(config),
        Err(sbctl::config::ConfigError::Missing) => None,
        Err(error) => {
            eprintln!("configuration wizard failed: {error}");
            return ExitCode::from(2);
        }
    };
    let default_interface = if existing.is_none() {
        sbctl::traffic::detect_default_route_interface(root).ok()
    } else {
        None
    };
    let mut prompts = ConsolePrompts;
    let outcome = match sbctl::wizard::run(existing.as_ref(), default_interface, &mut prompts) {
        Ok(outcome) => outcome,
        Err(error) => {
            eprintln!("configuration wizard failed: {error}");
            return ExitCode::from(2);
        }
    };
    match outcome {
        sbctl::wizard::WizardOutcome::Cancelled => {
            println!("configuration wizard cancelled; the existing deployment is unchanged");
            ExitCode::SUCCESS
        }
        sbctl::wizard::WizardOutcome::Unchanged => {
            println!("deployment configuration is unchanged");
            ExitCode::SUCCESS
        }
        sbctl::wizard::WizardOutcome::Changed(config) => {
            commit_config_change(root, &store, &config, sing_box_bin)
        }
    }
}

/// Commits a confirmed wizard configuration through the artifact/check/health
/// transaction. A fresh deployment initializes artifacts and configuration;
/// an existing deployment atomically replaces the changed files, restarts the
/// managed services, and re-establishes accounting state when the schedule or
/// interface changed. Any failure restores the previous known-good deployment.
pub(crate) fn commit_config_change(
    root: &Path,
    store: &sbctl::config::DeploymentStore,
    new: &sbctl::config::DeploymentConfig,
    sing_box_bin: Option<PathBuf>,
) -> ExitCode {
    let existing = match store.load() {
        Ok(config) => Some(config),
        Err(sbctl::config::ConfigError::Missing) => None,
        Err(error) => {
            eprintln!("configuration wizard failed: {error}");
            return ExitCode::from(2);
        }
    };
    let result = (|| -> Result<(), sbctl::config::ConfigError> {
        if !sbctl::traffic::interface_exists(root, &new.interface) {
            return Err(sbctl::config::ConfigError::InvalidValue(
                "the selected traffic interface does not exist on this host",
            ));
        }
        let binary = sing_box_bin.unwrap_or_else(|| root.join("usr/local/bin/sing-box"));
        match existing {
            None => {
                let artifacts = sbctl::subscription::generated_artifacts_for_kernel(
                    new,
                    root,
                    Some(binary.as_path()),
                )
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                let server = artifacts
                    .iter()
                    .find(|(name, _)| *name == "sing-box-server.json")
                    .map(|(_, contents)| contents)
                    .ok_or(sbctl::config::ConfigError::InvalidValue(
                        "no generated sing-box server configuration is available to check",
                    ))?;
                if !binary.is_file() {
                    return Err(sbctl::config::ConfigError::InvalidValue(
                        "a new deployment requires --sing-box-bin for configuration validation",
                    ));
                }
                sbctl::subscription::check_sing_box_config(&binary, server)
                    .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                let references = artifacts
                    .iter()
                    .map(|(name, contents)| (name.clone(), contents.as_bytes()))
                    .collect::<Vec<_>>();
                store.initialize_with_artifacts(new, &references)?;
                Ok(())
            }
            Some(prior) => {
                let snapshot = sbctl::subscription::apply_config_transaction(
                    store,
                    new,
                    binary.is_file().then_some(binary.as_path()),
                )
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                // Re-applying the daemon storage permissions after a transactional
                // configuration change keeps the rewritten active sing-box
                // configuration readable by the sing-box service account. Without
                // this, the rewritten /etc/sing-box/config.json stays 0600
                // root:root and the service cannot start (deployment issue #5).
                let direct = new.subscription_mode == sbctl::config::SubscriptionMode::Direct;
                if root == Path::new("/") {
                    sbctl::lifecycle::prepare_daemon_prerequisites(root, direct)
                        .map_err(sbctl::config::ConfigError::StateContent)?;
                }
                sbctl::lifecycle::prepare_daemon_storage(root, direct)
                    .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                restart_services_with_rollback(root, || {
                    let _ = sbctl::subscription::restore_config_transaction(store, &snapshot);
                })?;
                if accounting_schedule_changed(&prior, new)
                    && let Err(error) = sbctl::traffic::reset(store, new)
                {
                    eprintln!(
                        "warning: could not establish the new accounting state now ({error}); the next accounting reset timer run will establish it"
                    );
                }
                Ok(())
            }
        }
    })();
    match result {
        Ok(()) => {
            println!("deployment configuration committed\n{}", new.summary());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("configuration wizard failed: {error}");
            ExitCode::from(2)
        }
    }
}

/// A policy, timezone, first reset instant, or interface change alters the
/// accounting cycle, so the wizard establishes a new accounting state instead
/// of carrying the previous period's accumulated traffic forward.
fn accounting_schedule_changed(
    prior: &sbctl::config::DeploymentConfig,
    new: &sbctl::config::DeploymentConfig,
) -> bool {
    prior.accounting_policy != new.accounting_policy
        || prior.accounting_timezone != new.accounting_timezone
        || prior.anchored_reset_at != new.anchored_reset_at
        || prior.interface != new.interface
}

/// Restarts the managed services after a configuration commit. If the health
/// check fails, the rollback closure restores the previous known-good files,
/// the services are restarted again, and the failure is reported.
pub(crate) fn restart_services_with_rollback(
    root: &Path,
    rollback: impl FnOnce(),
) -> Result<(), sbctl::config::ConfigError> {
    if let Err(error) = sbctl::lifecycle::restart_services(root) {
        rollback();
        if let Err(rollback_error) = sbctl::lifecycle::restart_services(root) {
            eprintln!(
                "warning: the rollback restart failed too ({rollback_error}); inspect \
                 `systemctl status sing-box.service sbctl.service` before retrying"
            );
        }
        return Err(sbctl::config::ConfigError::StateContent(error));
    }
    Ok(())
}
