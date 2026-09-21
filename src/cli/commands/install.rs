//! `sbctl install`: the install transaction, the post-install checklist it
//! prints, and the firewall commands that checklist offers to copy.

use crate::cli::args::InstallOptions;
use crate::cli::prompt::{protocol_ports, required_install_value, select_protocols};
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;

pub(crate) fn install(root: &Path, options: InstallOptions) -> ExitCode {
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
