//! The `sbctl` binary entry point: parse the clap surface and dispatch to the
//! command handlers. Everything that is not the entry point or the dispatch
//! table lives under `cli::`.

mod cli;

use clap::Parser;
use cli::args::{
    CertificateCommand, Cli, CliOverrideTarget, Command, ConfigCommand, CredentialCommand,
    InstallOptions, OverrideCommand, ReleaseCommand, SingBoxCommand, SystemCommand, TrafficCommand,
};
use cli::menu::{confirm_menu_action, menu};
use cli::prompt::{ConsolePrompts, protocol_ports, required_install_value, select_protocols};
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    // Rust ignores SIGPIPE by default, which turns `sbctl ... | head` into a
    // panic on the broken pipe. Restore the platform default so the process
    // ends quietly when the reader goes away.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let root = cli.root.as_deref().unwrap_or_else(|| Path::new("/"));
    let command = match cli.command {
        Some(command) => command,
        None => {
            // No subcommand supplied: open the interactive menu when the user is
            // at a terminal (the `ly` shortcut path). Non-interactive invocations
            // print the help text instead.
            if io::stdin().is_terminal() {
                return menu(root);
            }
            use clap::CommandFactory;
            let mut command = Cli::command();
            let _ = command.print_help();
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Install {
            mode,
            subscription_host,
            proxy_host,
            http_port,
            interface,
            reality_decoy_sni,
            protocol_sni,
            disable_protocol,
            vless_port,
            vmess_port,
            hysteria2_port,
            tuic_port,
            anytls_port,
            sing_box_bin,
            manifest,
            no_start,
        } => install(
            root,
            InstallOptions {
                mode,
                subscription_host,
                proxy_host,
                http_port,
                interface,
                reality_decoy_sni,
                protocol_sni,
                disable_protocol,
                vless_port,
                vmess_port,
                hysteria2_port,
                tuic_port,
                anytls_port,
                sing_box_bin,
                manifest,
                no_start,
            },
        ),
        Command::Menu => menu(root),
        Command::Status { json } => {
            if json {
                print_status_json(root)
            } else {
                print_status(root)
            }
        }
        Command::Traffic { command } => match command {
            None | Some(TrafficCommand::Show) => print_traffic(root),
            Some(TrafficCommand::SetUsed { bytes, rx, tx }) => {
                traffic_set_used(root, bytes, rx, tx)
            }
        },
        Command::Node => print_nodes(root),
        Command::Restart { sing_box_bin } => restart(root, sing_box_bin),
        Command::Uninstall { purge } => uninstall(root, purge),
        Command::Update {
            check,
            manifest,
            sbctl_artifact,
            sing_box_artifact,
        } => update(
            root,
            check,
            manifest.as_deref(),
            sbctl_artifact.as_deref(),
            sing_box_artifact.as_deref(),
        ),
        Command::SingBox { command } => sing_box(root, command),
        Command::Release { command } => release(command),
        Command::Sub { format } => print_subscription_urls(root, format),
        Command::Qr { format, all } => print_subscription_qr(root, format, all),
        Command::Credential { command } => run_credential(root, command),
        Command::Serve { bind, max_requests } => serve_subscription(root, bind, max_requests),
        Command::Certificate { command } => run_certificate(root, command),
        Command::System { command } => run_system(root, command),
        Command::Config { command } => run_config(root, command),
        Command::AccountingReset => run_accounting_reset(root),
        Command::Regenerate { sing_box_bin } => regenerate(root, sing_box_bin),
    }
}

fn sing_box(root: &Path, command: SingBoxCommand) -> ExitCode {
    let result = match command {
        SingBoxCommand::Download { manifest, output } => sbctl::update::read_manifest(&manifest)
            .and_then(|manifest| sbctl::update::download_sing_box(&manifest, &output))
            .map(|_| format!("sing-box downloaded and verified: {}", output.display())),
        SingBoxCommand::Install { manifest, artifact } => sbctl::update::read_manifest(&manifest)
            .and_then(|manifest| sbctl::update::verify_sing_box_artifact(&manifest, &artifact))
            .and_then(|_| {
                sbctl::lifecycle::install_checked_sing_box(root, &artifact)
                    .map_err(sbctl::update::UpdateError::Storage)
            })
            .map(|_| "sing-box installed".to_owned()),
        SingBoxCommand::Update { manifest, artifact } => match manifest {
            Some(path) => {
                // 签名 manifest 流程：URL 与摘要全部固定并校验签名后才会使用。
                sbctl::update::read_manifest(&path)
                    .and_then(|manifest| {
                        let temporary = tempfile::NamedTempFile::new().map_err(|error| {
                            sbctl::update::UpdateError::DownloadFailed(
                                "sing-box",
                                error.to_string(),
                            )
                        })?;
                        // Holds the candidate path without an open write handle:
                        // Linux refuses to execute a file that is still open for
                        // writing (`Text file busy`, ETXTBSY).
                        let mut guard = None;
                        let candidate = match artifact {
                            Some(candidate) => {
                                sbctl::update::verify_sing_box_artifact(&manifest, &candidate)?;
                                candidate
                            }
                            None => {
                                let path = temporary.into_temp_path();
                                let candidate = path.to_path_buf();
                                sbctl::update::download_sing_box(&manifest, &candidate)?;
                                guard = Some(path);
                                candidate
                            }
                        };
                        let result = sbctl::update::apply_sing_box(
                            &sbctl::config::DeploymentStore::new(root),
                            &manifest,
                            &candidate,
                        );
                        drop(guard);
                        result
                    })
                    .map(|rollback| {
                        format!("sing-box updated; rollback point: {}", rollback.display())
                    })
            }
            None => update_sing_box_official(root, artifact.as_deref()),
        },
        SingBoxCommand::Remove => sbctl::lifecycle::remove_managed_sing_box(root)
            .map(|_| "sing-box removed".to_owned())
            .map_err(sbctl::update::UpdateError::Operation),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("sing-box operation failed: {error}");
            ExitCode::from(2)
        }
    }
}

/// Updates the managed sing-box kernel to the latest stable release from the
/// official SagerNet repository (github.com/SagerNet/sing-box), or installs a
/// locally supplied candidate artifact. Both paths run the same configuration
/// check and health-check rollback flow as the signed-manifest update.
fn update_sing_box_official(
    root: &Path,
    artifact: Option<&Path>,
) -> Result<String, sbctl::update::UpdateError> {
    let store = sbctl::config::DeploymentStore::new(root);
    let temporary = tempfile::NamedTempFile::new().map_err(|error| {
        sbctl::update::UpdateError::DownloadFailed("sing-box", error.to_string())
    })?;
    // Keeps the downloaded candidate on disk until the update finishes, while
    // holding no open write handle. On Linux a file that is still open for
    // writing cannot be executed (`Text file busy`, ETXTBSY), and the candidate
    // is executed for the pre-install `sing-box check`.
    let mut candidate_guard: Option<tempfile::TempPath> = None;
    let (candidate, version_note) = match artifact {
        Some(path) => (path.to_path_buf(), "本地 sing-box 候选".to_owned()),
        None => {
            let version = sbctl::update::fetch_latest_official_sing_box_version()?;
            println!("官方最新稳定版：sing-box {version}，开始下载并校验…");
            sbctl::update::download_sing_box_official(&version, temporary.path())?;
            let path = temporary.into_temp_path();
            let candidate = path.to_path_buf();
            candidate_guard = Some(path);
            (candidate, format!("sing-box {version}（官方最新稳定版）"))
        }
    };
    // The candidate is verified again here: it must run and pass a
    // `sing-box check` against the active server configuration before the
    // managed binary is replaced.
    let contents = fs::read(&candidate)?;
    let rollback = sbctl::update::install_candidate_sing_box(&store, &candidate, &contents)?;
    drop(candidate_guard);
    Ok(format!(
        "{version_note} 更新完成，已通过配置检查与服务健康检查；回滚点：{}",
        rollback.display()
    ))
}

fn release(command: ReleaseCommand) -> ExitCode {
    let result: Result<String, String> = match command {
        ReleaseCommand::Sign {
            manifest,
            private_key,
            output,
        } => sbctl::release::sign_manifest_file(&manifest, &private_key, &output)
            .map(|_| format!("signed release manifest written to {}", output.display()))
            .map_err(|error| error.to_string()),
        ReleaseCommand::Verify { manifest } => sbctl::release::verify_manifest(&manifest)
            .map(|_| "release manifest verified against the built-in public key".to_owned())
            .map_err(|error| error.to_string()),
        ReleaseCommand::Keygen { output } => (|| {
            let (public, secret) = sbctl::release::generate_keypair();
            let secret_path = output.join("sbctl-release-secret.hex");
            sbctl::release::write_secret_file(&secret_path, &secret)
                .map_err(|error| error.to_string())?;
            let hex =
                |bytes: &[u8; 32]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            Ok(format!(
                "secret seed written to {} (0600)\npublic key hex: {}\npublic key PEM:\n{}",
                secret_path.display(),
                hex(&public),
                sbctl::release::public_key_pem(&public)
            ))
        })(),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("release operation failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn uninstall(root: &Path, purge: bool) -> ExitCode {
    match sbctl::lifecycle::uninstall(root, purge) {
        Ok(Some(backup)) => {
            println!(
                "sbctl services and binaries removed; backup preserved at {}",
                backup.display()
            );
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!("sbctl services and binaries removed; persistent sbctl data purged");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("uninstall failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn update(
    root: &Path,
    check: bool,
    manifest_path: Option<&Path>,
    sbctl_artifact: Option<&Path>,
    sing_box_artifact: Option<&Path>,
) -> ExitCode {
    let result = update_impl(
        root,
        check,
        manifest_path,
        sbctl_artifact,
        sing_box_artifact,
    );
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("update failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn update_impl(
    root: &Path,
    check: bool,
    manifest_path: Option<&Path>,
    sbctl_artifact: Option<&Path>,
    sing_box_artifact: Option<&Path>,
) -> Result<String, sbctl::update::UpdateError> {
    let manifest = match manifest_path {
        Some(path) => sbctl::update::read_manifest(path)?,
        None => sbctl::update::fetch_latest_manifest()?,
    };
    if check {
        return Ok(format!(
            "update check completed without downloading or changing the host\n{}",
            sbctl::update::available_versions(&manifest)
        ));
    }
    let sbctl_download = tempfile::NamedTempFile::new()
        .map_err(|error| sbctl::update::UpdateError::DownloadFailed("sbctl", error.to_string()))?;
    let sing_box_download = tempfile::NamedTempFile::new().map_err(|error| {
        sbctl::update::UpdateError::DownloadFailed("sing-box", error.to_string())
    })?;
    // Keep both candidates on disk without open write handles: the update runs
    // them for the pre-install checks, and Linux refuses to execute a file that
    // is still open for writing (`Text file busy`, ETXTBSY).
    let mut sbctl_guard = None;
    let mut sing_box_guard = None;
    let sbctl_artifact = match sbctl_artifact {
        Some(path) => path.to_path_buf(),
        None => {
            let path = sbctl_download.into_temp_path();
            let artifact = path.to_path_buf();
            sbctl::update::download_sbctl(&manifest, &artifact)?;
            sbctl_guard = Some(path);
            artifact
        }
    };
    let sing_box_artifact = match sing_box_artifact {
        Some(path) => path.to_path_buf(),
        None => {
            let path = sing_box_download.into_temp_path();
            let artifact = path.to_path_buf();
            sbctl::update::download_sing_box(&manifest, &artifact)?;
            sing_box_guard = Some(path);
            artifact
        }
    };
    let rollback = sbctl::update::apply(
        &sbctl::config::DeploymentStore::new(root),
        &manifest,
        &sbctl_artifact,
        &sing_box_artifact,
    )?;
    drop(sbctl_guard);
    drop(sing_box_guard);
    Ok(format!(
        "update completed after verified validation and service health checks\nrollback point: {}",
        rollback.display()
    ))
}

fn install(root: &Path, options: InstallOptions) -> ExitCode {
    if options.subscription_host.is_none()
        && options.interface.is_none()
        && options.reality_decoy_sni.is_none()
        && options.sing_box_bin.is_none()
        && options.manifest.is_none()
        && !io::stdin().is_terminal()
    {
        return match sbctl::preflight::preflight(root) {
            Ok(()) => {
                println!("install preflight passed: host is ready for interactive installation");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("install preflight failed: {error}");
                ExitCode::from(2)
            }
        };
    }
    let mut installation_started = false;
    // Snapshot before anything is written: a failed install must not delete
    // persistent state it did not create.
    let state_before_install = sbctl::lifecycle::preexisting_state(root);
    let result = (|| {
        sbctl::preflight::preflight(root)
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        let subscription_host =
            required_install_value(options.subscription_host, "Subscription host")?;
        let interface = options.interface.map(Ok).unwrap_or_else(|| {
            sbctl::traffic::detect_default_route_interface(root).map_err(|error| {
                sbctl::config::ConfigError::StateContent(format!(
                    "could not detect a default-route interface ({error}); specify --interface"
                ))
            })
        })?;
        let protocols = select_protocols(&options.disable_protocol)?;
        let needs_reality_sni = protocols.contains(&sbctl::config::ManagedProtocol::VlessReality);
        let reality_decoy_sni = if needs_reality_sni {
            Some(required_install_value(
                options.reality_decoy_sni,
                "Reality decoy SNI",
            )?)
        } else {
            None
        };
        let mut config = sbctl::config::DeploymentConfig::new_with_ports(
            options.mode.into(),
            subscription_host,
            options.proxy_host,
            options.http_port,
            interface,
            protocols,
            reality_decoy_sni,
            protocol_ports(
                options.vless_port,
                options.vmess_port,
                options.hysteria2_port,
                options.tuic_port,
                options.anytls_port,
            ),
        )?;
        config.protocol_sni = options.protocol_sni;
        config.validate()?;
        if options.sing_box_bin.is_some() && options.manifest.is_some() {
            return Err(sbctl::config::ConfigError::InvalidValue(
                "installation accepts either --sing-box-bin or a signed --manifest, not both",
            ));
        }
        let sing_box_bin = match options.sing_box_bin {
            Some(path) => path,
            None => match options.manifest {
                Some(manifest_path) => {
                    let manifest =
                        sbctl::update::read_manifest(&manifest_path).map_err(|error| {
                            sbctl::config::ConfigError::StateContent(error.to_string())
                        })?;
                    let download = tempfile::NamedTempFile::new().map_err(|error| {
                        sbctl::config::ConfigError::StateContent(error.to_string())
                    })?;
                    sbctl::update::download_sing_box(&manifest, download.path()).map_err(
                        |error| sbctl::config::ConfigError::StateContent(error.to_string()),
                    )?;
                    download
                        .keep()
                        .map_err(|error| {
                            sbctl::config::ConfigError::StateContent(error.to_string())
                        })?
                        .1
                }
                None => {
                    // 默认：直接从官方 SagerNet 仓库安装最新稳定版 sing-box 内核。
                    let version = sbctl::update::fetch_latest_official_sing_box_version().map_err(
                        |error| sbctl::config::ConfigError::StateContent(error.to_string()),
                    )?;
                    println!("从官方仓库下载 sing-box 最新稳定版 {version} …");
                    let download = tempfile::NamedTempFile::new().map_err(|error| {
                        sbctl::config::ConfigError::StateContent(error.to_string())
                    })?;
                    sbctl::update::download_sing_box_official(&version, download.path()).map_err(
                        |error| sbctl::config::ConfigError::StateContent(error.to_string()),
                    )?;
                    download
                        .keep()
                        .map_err(|error| {
                            sbctl::config::ConfigError::StateContent(error.to_string())
                        })?
                        .1
                }
            },
        };
        let artifacts = sbctl::subscription::generated_artifacts(&config, root)
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        let server = artifacts
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents)
            .expect("generated server config");
        sbctl::subscription::check_sing_box_config(&sing_box_bin, server)
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        installation_started = true;
        sbctl::lifecycle::install_checked_sing_box(root, &sing_box_bin)?;
        let references = artifacts
            .iter()
            .map(|(name, contents)| (name.clone(), contents.as_bytes()))
            .collect::<Vec<_>>();
        let store = sbctl::config::DeploymentStore::new(root);
        store.initialize_with_artifacts(&config, &references)?;
        let direct = config.subscription_mode == sbctl::config::SubscriptionMode::Direct;
        if !options.no_start {
            sbctl::lifecycle::prepare_daemon_prerequisites(root, direct)
                .map_err(sbctl::config::ConfigError::StateContent)?;
            if direct {
                sbctl::certificate::pin_if_present(&store, &config)
                    .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
            }
            sbctl::traffic::reset(&store, &config)
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        }
        sbctl::lifecycle::install_units(&store, server, direct)?;
        if !options.no_start {
            sbctl::lifecycle::start_services(root, direct)
                .map_err(sbctl::config::ConfigError::StateContent)?;
            sbctl::lifecycle::check_service_health(root, direct)
                .map_err(sbctl::config::ConfigError::StateContent)?;
            // The ownership marker is the commit point of the complete
            // transaction. A `--no-start` fixture install defers startup and
            // the health check, so it never claims ownership.
            sbctl::lifecycle::write_ownership_marker(&store)?;
        }
        Ok::<_, sbctl::config::ConfigError>(config)
    })();
    match result {
        Ok(config) => {
            print_post_install_checklist(&config);
            ExitCode::SUCCESS
        }
        Err(error) => {
            if installation_started {
                sbctl::lifecycle::rollback_fresh_installation(root, state_before_install);
            }
            eprintln!("installation failed: {error}");
            ExitCode::from(2)
        }
    }
}

/// The step-by-step checklist printed after a successful install. sbctl never
/// touches the firewall or DNS, so these are the steps the administrator must
/// not skip; every line is a command that can be copied verbatim.
fn print_post_install_checklist(config: &sbctl::config::DeploymentConfig) {
    use sbctl::config::SubscriptionMode;
    println!("安装完成");
    println!(
        "启用协议: {}",
        config
            .enabled_protocols
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!("后续必做清单（按顺序执行）:");
    match config.subscription_mode {
        SubscriptionMode::Direct => {
            println!(
                "  1. 确认域名解析: dig +short {} 应返回本机公网 IP",
                config.subscription_host
            );
            println!("  2. 放行防火墙端口:");
            println!("     sudo ufw allow 80/tcp && sudo ufw allow 443/tcp");
            for port in firewall_port_commands(config) {
                println!("     {port}");
            }
            println!("  3. 签发证书（替换为你的邮箱，仅用于 ACME 到期通知）:");
            println!("     sbctl certificate obtain --email admin@example.com");
            println!("  4. 检查证书与服务: sbctl certificate status && sbctl status");
        }
        SubscriptionMode::ExternalProxy => {
            println!(
                "  1. 在 Nginx/Caddy 中把 /sub/ 反代到 127.0.0.1:{}（HTTPS 与证书由反代负责）",
                config.subscription_listen_port.unwrap_or(2080)
            );
            println!("  2. 放行协议防火墙端口:");
            for port in firewall_port_commands(config) {
                println!("     {port}");
            }
            println!("  3. 自检订阅（替换为你的域名与订阅凭据）:");
            println!("     curl -fsS https://<域名>/sub/<凭据>/uri >/dev/null && echo OK");
        }
        SubscriptionMode::IpFallback => {
            println!(
                "  注意: IP fallback 订阅走明文 HTTP（端口 {}），安全性较低",
                config.http_port.unwrap_or(2080)
            );
            println!("  1. 放行订阅端口:");
            println!(
                "     sudo ufw allow {}/tcp",
                config.http_port.unwrap_or(2080)
            );
            println!("  2. 放行协议防火墙端口:");
            for port in firewall_port_commands(config) {
                println!("     {port}");
            }
            println!("  3. 自检订阅（替换为订阅凭据）:");
            println!(
                "     curl -fsS http://{}:{}/sub/<凭据>/uri >/dev/null && echo OK",
                config.subscription_host,
                config.http_port.unwrap_or(2080)
            );
        }
    }
    println!("  5. 查看订阅链接与二维码: sbctl sub   单条二维码: sbctl qr");
    println!("  6. 订阅总览页（手机扫码导入）: sbctl sub 输出中的 index 链接");
    println!("再次进入管理菜单: sbctl menu");
}

/// Copy-paste-ready `ufw allow` commands for every enabled protocol port.
fn firewall_port_commands(config: &sbctl::config::DeploymentConfig) -> Vec<String> {
    use sbctl::config::ManagedProtocol;
    let mut commands = Vec::new();
    for protocol in &config.enabled_protocols {
        let transport = match protocol {
            ManagedProtocol::VlessReality
            | ManagedProtocol::VmessWebsocket
            | ManagedProtocol::Anytls => "tcp",
            ManagedProtocol::Hysteria2 | ManagedProtocol::Tuic => "udp",
        };
        if let Some(port) = config.protocol_listener_port(protocol) {
            commands.push(format!("sudo ufw allow {port}/{transport}  # {protocol}"));
        }
    }
    commands
}

fn format_local_time(instant: chrono::DateTime<chrono::Utc>, timezone: &str) -> String {
    timezone
        .parse::<chrono_tz::Tz>()
        .map(|timezone| {
            instant
                .with_timezone(&timezone)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| instant.to_rfc3339())
}

fn system_info() -> (String, String, String, String) {
    let os = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| {
            contents
                .lines()
                .find(|line| line.starts_with("PRETTY_NAME="))
                .map(|line| line["PRETTY_NAME=".len()..].trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let kernel =
        fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_else(|_| "unknown".to_owned());
    let bbr = fs::read_to_string("/proc/sys/net/ipv4/tcp_congestion_control")
        .unwrap_or_else(|_| "unknown".to_owned());
    let cpu = std::env::consts::ARCH.to_owned();
    (os, kernel.trim().to_owned(), cpu, bbr.trim().to_owned())
}

fn print_nodes(root: &Path) -> ExitCode {
    match sbctl::config::DeploymentStore::new(root).load() {
        Ok(config) => {
            println!("{}", sbctl::lifecycle::enabled_nodes(&config));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("node failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn restart(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
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

/// Resolves the ACME registration email for `certificate obtain`. The no-email
/// path is only returned after an explicit interactive confirmation.
fn resolve_obtain_email(email: Option<&str>, no_email: bool) -> Result<Option<String>, String> {
    if no_email {
        if confirm_menu_action("确认不使用邮箱注册证书（--register-unsafely-without-email）？")
        {
            return Ok(None);
        }
        return Err("已取消：未确认免邮箱注册。".to_owned());
    }
    let Some(email) = email else {
        return Err("请提供 --email <邮箱>，或使用 --no-email 跳过（需二次确认）。".to_owned());
    };
    let email = email.trim().to_owned();
    if sbctl::certificate::acme_email_is_valid(&email) {
        Ok(Some(email))
    } else {
        Err(
            "邮箱格式无效（示例 admin@example.com）；该邮箱仅用于证书到期通知。确实不需要时请用 --no-email 并二次确认。"
                .to_owned(),
        )
    }
}

fn run_certificate(root: &Path, command: CertificateCommand) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    if let CertificateCommand::Status = command {
        return match store.load() {
            Ok(config) => {
                print_certificate_status(root, &store, &config);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("certificate operation failed: {error}");
                ExitCode::from(2)
            }
        };
    }
    // The no-email ACME path is explicit and confirmed before touching config.
    let obtain_email = if let CertificateCommand::Obtain { email, no_email } = &command {
        match resolve_obtain_email(email.as_deref(), *no_email) {
            Ok(email) => email,
            Err(message) => {
                eprintln!("certificate operation failed: {message}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    let result = store.load().and_then(|config| {
        match &command {
            CertificateCommand::Obtain { .. } => {
                sbctl::certificate::obtain(&store, &config, obtain_email.as_deref())
            }
            CertificateCommand::Renew => sbctl::certificate::renew(&store, &config),
            CertificateCommand::Verify => sbctl::certificate::deploy_hook(&store, &config),
            // Handled above with its own report; the compiler cannot see that
            // this closure is only reached for the remaining commands.
            CertificateCommand::Status => unreachable!("handled before the operation match"),
        }
        .map(|validated| {
            println!(
                "certificate for {} is valid until {}",
                validated.host,
                chrono::DateTime::from_timestamp(validated.not_after, 0)
                    .map(|when| when.to_rfc3339())
                    .unwrap_or_else(|| "unknown".to_owned())
            );
            println!("fingerprint: {}", validated.fingerprint);
        })
        .map_err(|error| {
            sbctl::config::ConfigError::StateContent(sbctl::subscription::redact_secret(
                &error.to_string(),
                &config.subscription_credential,
            ))
        })
    });
    match result {
        Ok(()) => {
            println!("certificate operation completed");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("certificate operation failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn run_system(root: &Path, command: SystemCommand) -> ExitCode {
    let runtime = sbctl::runtime::Runtime::live(root);
    let result: Result<(), String> = match command {
        SystemCommand::Bbr => sbctl::system::enable_bbr(&runtime).map(|status| {
            println!(
                "BBR enabled: tcp_congestion_control={}, default_qdisc={}",
                status.congestion_control, status.qdisc
            );
        }),
        SystemCommand::Status => sbctl::system::read_current(&runtime).map(|status| {
            println!("tcp_congestion_control={}", status.congestion_control);
            println!("default_qdisc={}", status.qdisc);
        }),
    }
    .map_err(|error| error.to_string());
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("system operation failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn serve_subscription(root: &Path, bind: Option<String>, max_requests: Option<usize>) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        let bind = bind.unwrap_or_else(|| match config.subscription_mode {
            sbctl::config::SubscriptionMode::ExternalProxy => format!(
                "127.0.0.1:{}",
                config
                    .subscription_listen_port
                    .expect("validated external reverse-proxy listener port")
            ),
            sbctl::config::SubscriptionMode::IpFallback => format!(
                "{}:{}",
                // An IPv6 bind address is only parseable bracketed; the stored
                // host stays bare because the generated configs need it that way.
                sbctl::canonical::uri_host(&config.subscription_host),
                config.http_port.expect("validated IP fallback port")
            ),
            sbctl::config::SubscriptionMode::Direct => "0.0.0.0:0".to_owned(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        runtime
            .block_on(sbctl::subscription::serve(
                &store,
                &config,
                &bind,
                max_requests,
            ))
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("subscription service failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn print_subscription_urls(
    root: &Path,
    format: Option<sbctl::subscription::SubscriptionFormat>,
) -> ExitCode {
    use sbctl::subscription::SubscriptionRoute;
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        let is_ip_fallback =
            config.subscription_mode == sbctl::config::SubscriptionMode::IpFallback;
        let contents = match format {
            Some(format) => sbctl::subscription::subscription_url(&config, format)
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?,
            None => {
                let mut table = String::from("按客户端选择订阅链接（推荐）：\n\n");
                for row in sbctl::subscription::client_subscription_matrix() {
                    table.push_str(&format!("{}：{}\n", row.client, row.note));
                    for recommended in &row.formats {
                        let url =
                            sbctl::subscription::subscription_url(&config, recommended.format)
                                .map_err(|error| {
                                    sbctl::config::ConfigError::StateContent(error.to_string())
                                })?;
                        table.push_str(&format!("  {url}\n    （{}）\n", recommended.note));
                    }
                    table.push('\n');
                }
                table.push_str("全部订阅格式：\n\n");
                for info in sbctl::subscription::subscription_matrix() {
                    let url = sbctl::subscription::subscription_url(&config, info.format).map_err(
                        |error| sbctl::config::ConfigError::StateContent(error.to_string()),
                    )?;
                    let qr =
                        sbctl::subscription::route_url(&config, SubscriptionRoute::Qr(info.format))
                            .map_err(|error| {
                                sbctl::config::ConfigError::StateContent(error.to_string())
                            })?;
                    table.push_str(&format!(
                        "{label}\n  订阅链接：{url}\n  二维码：{qr}\n  说明：{note}\n\n",
                        label = info.label,
                        url = url,
                        qr = qr,
                        note = info.note
                    ));
                }
                let index = sbctl::subscription::route_url(&config, SubscriptionRoute::Index)
                    .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                table.push_str(&format!(
                    "订阅总览页（含按客户端速查、全部二维码与导入步骤）：\n  {index}\n"
                ));
                table
            }
        };
        Ok((contents, is_ip_fallback))
    });
    match result {
        Ok((contents, is_ip_fallback)) => {
            if is_ip_fallback {
                eprintln!(
                    "warning: IP fallback subscription uses unencrypted HTTP and is lower security"
                );
            }
            print!("{contents}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("subscription failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn print_subscription_qr(
    root: &Path,
    format: Option<sbctl::subscription::SubscriptionFormat>,
    all: bool,
) -> ExitCode {
    use sbctl::subscription::SubscriptionFormat;
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        let formats: Vec<SubscriptionFormat> = if all {
            sbctl::subscription::subscription_matrix()
                .into_iter()
                .map(|info| info.format)
                .collect()
        } else {
            vec![format.unwrap_or(SubscriptionFormat::SingBoxFull)]
        };
        let mut rendered = Vec::with_capacity(formats.len());
        for format in formats {
            let url = sbctl::subscription::subscription_url(&config, format)
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
            let qr =
                sbctl::qr::render_ansi(&url).map_err(sbctl::config::ConfigError::StateContent)?;
            rendered.push((format, url, qr));
        }
        Ok(rendered)
    });
    match result {
        Ok(rendered) => {
            for (format, url, qr) in rendered {
                println!("{} 订阅二维码：", format.display_label());
                println!("{url}");
                println!("{qr}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("subscription qr failed: {error}");
            ExitCode::from(2)
        }
    }
}

/// Prints the human-readable certificate report for `sbctl certificate status`:
/// validity window with days remaining, SAN coverage, deploy-hook presence,
/// and the fix commands when something is off.
fn print_certificate_status(
    root: &Path,
    store: &sbctl::config::DeploymentStore,
    config: &sbctl::config::DeploymentConfig,
) {
    use sbctl::config::SubscriptionMode;
    if config.subscription_mode != SubscriptionMode::Direct {
        println!(
            "当前订阅模式为 {}，证书由反向代理或自签机制负责，sbctl 不管理证书。",
            config.subscription_mode
        );
        return;
    }
    let status = sbctl::certificate::status(store, config);
    println!("Direct 订阅证书状态（{host}）:", host = status.host);
    match status.state {
        "ok" => {
            let not_before = status
                .not_before
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|when| when.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_owned());
            let not_after = status
                .not_after
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|when| when.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_owned());
            let days = status.not_after.and_then(days_until);
            println!("  状态: 有效");
            println!("  生效自: {not_before}");
            println!("  有效期至: {not_after}");
            if let Some(days) = days {
                println!("  剩余天数: {days} 天");
                if days < 14 {
                    println!(
                        "  提醒: 证书即将到期；确认 certbot.timer 在运行（systemctl status certbot.timer）"
                    );
                }
            }
            if !status.san.is_empty() {
                println!("  SAN: {}", status.san.join(", "));
            }
            if let Some(fingerprint) = &status.fingerprint {
                println!("  SHA-256 指纹: {fingerprint}");
            }
        }
        _ => {
            println!("  状态: 异常");
            if let Some(error) = &status.error {
                println!("  原因: {error}");
            }
            println!(
                "  修复: sbctl certificate renew（续期）或 sbctl certificate obtain --email <邮箱>（首次签发）"
            );
        }
    }
    let hook = root.join(sbctl::lifecycle::CERTBOT_DEPLOY_HOOK_RELATIVE_PATH);
    if hook.is_file() {
        println!("  Certbot deploy hook: 已安装（续期后自动校验并固定证书）");
    } else {
        println!(
            "  Certbot deploy hook: 未找到（{}）；续期后需要手动执行 sbctl certificate verify",
            hook.display()
        );
    }
}

fn days_until(timestamp: i64) -> Option<i64> {
    let now = chrono::Utc::now().timestamp();
    Some((timestamp - now).div_euclid(86_400))
}

fn print_status(root: &Path) -> ExitCode {
    match sbctl::config::DeploymentStore::new(root).load() {
        Ok(config) => {
            println!("{}", config.summary());
            println!("\n{}", sbctl::lifecycle::service_status(root));
            if config.subscription_mode == sbctl::config::SubscriptionMode::Direct {
                let status =
                    sbctl::certificate::status(&sbctl::config::DeploymentStore::new(root), &config);
                if status.state == "ok"
                    && let Some(not_after) = status.not_after
                    && let Some(days) = days_until(not_after)
                {
                    let hint = if days < 14 {
                        "（即将到期，请检查 certbot.timer）"
                    } else {
                        ""
                    };
                    println!("\n证书剩余有效期: {days} 天{hint}");
                } else if let Some(error) = &status.error {
                    println!("\n证书状态: 异常（{error}）；运行 sbctl certificate status 查看详情");
                }
            }
            match sbctl::traffic::report(&sbctl::config::DeploymentStore::new(root), &config) {
                Ok(report) => println!(
                    "\n{}\n下一次刷新（VPS: {}）: {}\n下一次刷新（客户端: {}）: {}",
                    report.summary(),
                    config.accounting_timezone,
                    format_local_time(report.next_reset, &config.accounting_timezone),
                    config.client_display_timezone,
                    format_local_time(report.next_reset, &config.client_display_timezone)
                ),
                Err(error) => println!("\nVPS traffic: unavailable ({error})"),
            }
            ExitCode::SUCCESS
        }
        Err(sbctl::config::ConfigError::Missing) => {
            println!("sbctl status: unmanaged (not installed)");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("status failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn print_status_json(root: &Path) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    match store.load() {
        Ok(config) => {
            let traffic = sbctl::traffic::report(&store, &config)
                .map(|report| {
                    serde_json::json!({
                        "interface": report.interface,
                        "received": report.received,
                        "transmitted": report.transmitted,
                        "total_adjustment": report.total_adjustment,
                        "total": report.total(),
                        "monthly_traffic_limit": report.monthly_traffic_limit,
                        "accounting_period": report.accounting_period,
                        "next_reset": report.next_reset.to_rfc3339(),
                        "next_reset_vps_refresh": format_local_time(
                            report.next_reset,
                            &config.accounting_timezone
                        ),
                        "next_reset_client_display": format_local_time(
                            report.next_reset,
                            &config.client_display_timezone
                        ),
                    })
                })
                .unwrap_or_else(|error| serde_json::json!({ "error": error.to_string() }));
            let services = sbctl::lifecycle::service_status_entries(root)
                .into_iter()
                .map(|(unit, state)| (unit.to_owned(), state))
                .collect::<std::collections::BTreeMap<_, _>>();
            let certificate = (config.subscription_mode == sbctl::config::SubscriptionMode::Direct)
                .then(|| sbctl::certificate::status(&store, &config));
            let status = serde_json::json!({
                "configured": true,
                "mode": config.subscription_mode.to_string(),
                "subscription_host": config.subscription_host,
                "proxy_host": config.proxy_host.as_deref().unwrap_or(&config.subscription_host),
                "interface": config.interface,
                "monthly_traffic_limit": config.monthly_traffic_limit,
                "accounting_policy": config.accounting_policy.to_string(),
                "accounting_timezone": config.accounting_timezone,
                "client_display_timezone": config.client_display_timezone,
                "enabled_protocols": config
                    .enabled_protocols
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "services": services,
                "traffic": traffic,
                "certificate": certificate,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&status).expect("status JSON serializes")
            );
            ExitCode::SUCCESS
        }
        Err(sbctl::config::ConfigError::Missing) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "configured": false }))
                    .expect("status JSON serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("status failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn print_traffic(root: &Path) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = match store.load() {
        Ok(config) => match sbctl::traffic::report(&store, &config) {
            Ok(report) => {
                println!(
                    "{}\n下一次刷新（VPS: {}）: {}\n下一次刷新（客户端: {}）: {}",
                    report.summary(),
                    config.accounting_timezone,
                    format_local_time(report.next_reset, &config.accounting_timezone),
                    config.client_display_timezone,
                    format_local_time(report.next_reset, &config.client_display_timezone)
                );
                return ExitCode::SUCCESS;
            }
            Err(error) => Err(error.to_string()),
        },
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("traffic failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn traffic_set_used(root: &Path, bytes: Option<u64>, rx: Option<u64>, tx: Option<u64>) -> ExitCode {
    let target = if let Some(bytes) = bytes {
        sbctl::traffic::CorrectionTarget::Total(bytes)
    } else {
        sbctl::traffic::CorrectionTarget::Directions {
            rx: rx.expect("validated: --rx requires --tx"),
            tx: tx.expect("validated: --tx requires --rx"),
        }
    };
    let store = sbctl::config::DeploymentStore::new(root);
    let result = match store.load() {
        Ok(config) => {
            sbctl::traffic::set_used(&store, &config, target).map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("traffic correction failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn run_accounting_reset(root: &Path) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = match store.load() {
        Ok(config) => sbctl::traffic::reset(&store, &config).map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(report) => {
            println!(
                "accounting period: {}; received: {} bytes; transmitted: {} bytes",
                report.accounting_period, report.received, report.transmitted
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("accounting reset failed: {error}");
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
fn resolve_sing_box_bin(root: &Path, explicit: Option<PathBuf>) -> Option<PathBuf> {
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

fn regenerate(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
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

fn run_config(root: &Path, command: ConfigCommand) -> ExitCode {
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

fn run_config_wizard(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
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
fn commit_config_change(
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
                let artifacts = sbctl::subscription::generated_artifacts(new, root)
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
                sbctl::lifecycle::prepare_daemon_storage(
                    root,
                    new.subscription_mode == sbctl::config::SubscriptionMode::Direct,
                )
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
fn restart_services_with_rollback(
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

fn run_credential(root: &Path, command: CredentialCommand) -> ExitCode {
    match command {
        CredentialCommand::Rotate => rotate_subscription_credential(root),
    }
}

fn rotate_subscription_credential(root: &Path) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|mut config| {
        let previous = config.subscription_credential.clone();
        config.subscription_credential = sbctl::config::generate_subscription_credential()?;
        store.replace(&config)?;
        restart_services_with_rollback(root, || {
            config.subscription_credential = previous;
            if let Err(error) = store.replace(&config) {
                eprintln!(
                    "warning: restoring the previous subscription credential failed ({error}); \
                     the rotated credential may still be active"
                );
            }
        })?;
        Ok(config)
    });
    match result {
        Ok(_) => {
            println!(
                "subscription credential rotated; all previous subscription URLs are now invalid"
            );
            println!("run 'sbctl sub' to display the new subscription URLs");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("credential rotation failed: {error}");
            ExitCode::from(2)
        }
    }
}
