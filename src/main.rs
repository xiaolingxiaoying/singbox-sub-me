use clap::{Parser, Subcommand, ValueEnum};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "sbctl",
    version,
    about = "Manage a private sing-box deployment"
)]
struct Cli {
    #[arg(long, global = true, hide = true, value_name = "PATH")]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Interactively install a fresh sbctl deployment.
    Install {
        #[arg(long, value_enum, default_value_t = CliSubscriptionMode::Direct)]
        mode: CliSubscriptionMode,
        #[arg(long)]
        subscription_host: Option<String>,
        #[arg(long)]
        proxy_host: Option<String>,
        /// Public HTTP port used only by IP fallback subscription mode.
        #[arg(long)]
        http_port: Option<u16>,
        #[arg(long)]
        interface: Option<String>,
        #[arg(long)]
        reality_decoy_sni: Option<String>,
        /// Explicitly omit a Managed protocol; all five are enabled by default.
        #[arg(long, value_enum)]
        disable_protocol: Vec<CliManagedProtocol>,
        /// Optional listener ports for the five Managed protocols.
        #[arg(long)]
        vless_port: Option<u16>,
        #[arg(long)]
        vmess_port: Option<u16>,
        #[arg(long)]
        hysteria2_port: Option<u16>,
        #[arg(long)]
        tuic_port: Option<u16>,
        #[arg(long)]
        anytls_port: Option<u16>,
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
        /// Create units and configuration without starting services (acceptance fixture use).
        #[arg(long, hide = true)]
        no_start: bool,
    },
    /// Open the interactive management menu for an installed deployment.
    #[command(alias = "m")]
    Menu,
    /// Show whether sbctl currently manages a deployment.
    Status {
        /// Emit a machine-readable JSON status report.
        #[arg(long)]
        json: bool,
    },
    /// Reconcile and show VPS traffic, or apply an explicit traffic correction.
    Traffic {
        #[command(subcommand)]
        command: Option<TrafficCommand>,
    },
    /// List the generated Managed protocol listeners without exposing credentials.
    Node,
    /// Validate the active sing-box configuration and restart both managed services.
    Restart {
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Remove the sbctl-managed services and binaries; preserve data unless --purge is supplied.
    Uninstall {
        /// Also remove persistent data explicitly owned by sbctl.
        #[arg(long)]
        purge: bool,
    },
    /// Check or install explicitly selected, hash-verified release artifacts.
    Update {
        /// Display versions from the pinned manifest without downloading or changing the host.
        #[arg(long)]
        check: bool,
        /// Fixed release manifest containing the allowed artifact hashes.
        #[arg(long, value_name = "PATH")]
        manifest: PathBuf,
        /// Optional local sbctl candidate artifact; otherwise download from the manifest.
        #[arg(long, value_name = "PATH")]
        sbctl_artifact: Option<PathBuf>,
        /// Optional local sing-box candidate artifact; otherwise download from the manifest.
        #[arg(long, value_name = "PATH")]
        sing_box_artifact: Option<PathBuf>,
    },
    /// Manage the sing-box data-plane binary independently from sbctl.
    #[command(name = "sing-box")]
    SingBox {
        #[command(subcommand)]
        command: SingBoxCommand,
    },
    /// Retrieve a generated subscription representation using its path credential.
    Sub {
        #[arg(long, value_enum)]
        format: Option<CliSubscriptionFormat>,
    },
    /// Rotate the Subscription credential so previous subscription URLs stop working.
    Credential {
        #[command(subcommand)]
        command: CredentialCommand,
    },
    /// Run the subscription service.
    Serve {
        /// Socket address; defaults to the configured public IP and HTTP port.
        #[arg(long)]
        bind: Option<String>,
        /// Stop after this many requests (useful for supervised health checks).
        #[arg(long, hide = true)]
        max_requests: Option<usize>,
    },
    /// Obtain or renew a Direct subscription mode certificate with Certbot.
    Certificate {
        #[command(subcommand)]
        command: CertificateCommand,
    },
    /// Create, inspect, and validate the persistent deployment configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Run the periodic accounting reset task (managed by the systemd timer).
    #[command(name = "accounting-reset", hide = true)]
    AccountingReset,
    /// Validate and atomically replace the canonical protocol artifacts and
    /// the active sing-box configuration from the persisted deployment.
    #[command(name = "regenerate", hide = true)]
    Regenerate {
        /// sing-box binary used to validate the regenerated server configuration.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
}

struct InstallOptions {
    mode: CliSubscriptionMode,
    subscription_host: Option<String>,
    proxy_host: Option<String>,
    http_port: Option<u16>,
    interface: Option<String>,
    reality_decoy_sni: Option<String>,
    disable_protocol: Vec<CliManagedProtocol>,
    vless_port: Option<u16>,
    vmess_port: Option<u16>,
    hysteria2_port: Option<u16>,
    tuic_port: Option<u16>,
    anytls_port: Option<u16>,
    sing_box_bin: Option<PathBuf>,
    no_start: bool,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Create the initial deployment configuration without overwriting one.
    Init {
        #[arg(long, value_enum)]
        mode: CliSubscriptionMode,
        #[arg(long)]
        subscription_host: String,
        #[arg(long)]
        proxy_host: Option<String>,
        #[arg(long)]
        http_port: Option<u16>,
        /// Loopback HTTP port used by an external reverse proxy.
        #[arg(long)]
        listen_port: Option<u16>,
        /// Linux network interface; defaults to the detected default-route interface.
        #[arg(long = "interface")]
        interface: Option<String>,
        #[arg(long = "protocol", value_enum, required = true)]
        protocols: Vec<CliManagedProtocol>,
        /// Optional listener ports for the five Managed protocols.
        #[arg(long)]
        vless_port: Option<u16>,
        #[arg(long)]
        vmess_port: Option<u16>,
        #[arg(long)]
        hysteria2_port: Option<u16>,
        #[arg(long)]
        tuic_port: Option<u16>,
        #[arg(long)]
        anytls_port: Option<u16>,
        #[arg(long)]
        reality_decoy_sni: Option<String>,
        #[arg(long, default_value_t = 0)]
        monthly_traffic_limit: u64,
        #[arg(long, value_enum, default_value_t = CliAccountingPolicy::NaturalMonth)]
        accounting_policy: CliAccountingPolicy,
        /// Named IANA timezone; defaults to UTC.
        #[arg(long)]
        accounting_timezone: Option<String>,
        /// Required for anchored-month: YYYY-MM-DDTHH:MM in the accounting timezone.
        #[arg(long)]
        anchored_reset_at: Option<String>,
        /// sing-box binary used to validate VMess WebSocket or Hysteria2 server configuration.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Change only the subscription delivery mode while retaining generated nodes and credentials.
    SwitchMode {
        #[arg(long, value_enum)]
        mode: CliSubscriptionMode,
        /// Required when switching to external-proxy mode.
        #[arg(long)]
        listen_port: Option<u16>,
    },
    /// Display the persisted deployment summary without exposing credentials.
    Show,
    /// Parse and validate the persisted deployment configuration.
    Validate,
    /// Open the interactive configuration wizard for a new or existing deployment.
    Wizard {
        /// sing-box binary used to validate regenerated protocol configuration.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum CredentialCommand {
    /// Generate a fresh Subscription credential; the previous URLs stop working immediately.
    Rotate,
}

#[derive(Debug, Subcommand)]
enum TrafficCommand {
    /// Show the current accounting period's VPS traffic.
    Show,
    /// Set the reported VPS traffic for the current accounting period.
    #[command(group(
        clap::ArgGroup::new("correction")
            .required(true)
            .multiple(true)
            .args(["bytes", "rx", "tx"])
    ))]
    SetUsed {
        /// Target reported total VPS traffic in bytes; only increases the total.
        #[arg(long, value_name = "TOTAL", conflicts_with_all = ["rx", "tx"])]
        bytes: Option<u64>,
        /// Target reported received bytes; requires --tx.
        #[arg(long, value_name = "BYTES", requires = "tx", conflicts_with = "bytes")]
        rx: Option<u64>,
        /// Target reported transmitted bytes; requires --rx.
        #[arg(long, value_name = "BYTES", requires = "rx", conflicts_with = "bytes")]
        tx: Option<u64>,
    },
}

#[derive(Debug, Subcommand)]
enum CertificateCommand {
    /// Obtain a certificate using Certbot's webroot authenticator.
    Obtain {
        #[arg(long)]
        email: String,
    },
    /// Renew certificates and safely reload the sbctl service if they changed.
    Renew,
    /// Validate the certificate and re-pin it for the service accounts. This is
    /// the Certbot deploy hook and the recommended post-renewal check.
    Verify,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliSubscriptionMode {
    Direct,
    ExternalProxy,
    IpFallback,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliManagedProtocol {
    VlessReality,
    VmessWebsocket,
    Hysteria2,
    Tuic,
    Anytls,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliAccountingPolicy {
    NaturalMonth,
    AnchoredMonth,
}

#[derive(Clone, Debug, ValueEnum)]
enum CliSubscriptionFormat {
    SingBox,
    Clash,
    Uri,
}

#[derive(Debug, Subcommand)]
enum SingBoxCommand {
    /// Download the pinned sing-box artifact and verify its digest.
    Download {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Install a verified sing-box artifact at /usr/local/bin/sing-box.
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact: PathBuf,
    },
    /// Download (when needed), verify, and replace the managed sing-box binary.
    Update {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact: Option<PathBuf>,
    },
    /// Remove only the sbctl-owned sing-box binary and service.
    Remove,
}

impl From<CliSubscriptionFormat> for sbctl::subscription::SubscriptionFormat {
    fn from(format: CliSubscriptionFormat) -> Self {
        match format {
            CliSubscriptionFormat::SingBox => Self::SingBox,
            CliSubscriptionFormat::Clash => Self::Clash,
            CliSubscriptionFormat::Uri => Self::Uri,
        }
    }
}

impl From<CliSubscriptionMode> for sbctl::config::SubscriptionMode {
    fn from(mode: CliSubscriptionMode) -> Self {
        match mode {
            CliSubscriptionMode::Direct => Self::Direct,
            CliSubscriptionMode::ExternalProxy => Self::ExternalProxy,
            CliSubscriptionMode::IpFallback => Self::IpFallback,
        }
    }
}

impl From<CliManagedProtocol> for sbctl::config::ManagedProtocol {
    fn from(protocol: CliManagedProtocol) -> Self {
        match protocol {
            CliManagedProtocol::VlessReality => Self::VlessReality,
            CliManagedProtocol::VmessWebsocket => Self::VmessWebsocket,
            CliManagedProtocol::Hysteria2 => Self::Hysteria2,
            CliManagedProtocol::Tuic => Self::Tuic,
            CliManagedProtocol::Anytls => Self::Anytls,
        }
    }
}

impl From<CliAccountingPolicy> for sbctl::config::AccountingPolicy {
    fn from(policy: CliAccountingPolicy) -> Self {
        match policy {
            CliAccountingPolicy::NaturalMonth => Self::NaturalMonth,
            CliAccountingPolicy::AnchoredMonth => Self::AnchoredMonth,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let root = cli.root.as_deref().unwrap_or_else(|| Path::new("/"));
    match cli.command {
        Command::Install {
            mode,
            subscription_host,
            proxy_host,
            http_port,
            interface,
            reality_decoy_sni,
            disable_protocol,
            vless_port,
            vmess_port,
            hysteria2_port,
            tuic_port,
            anytls_port,
            sing_box_bin,
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
                disable_protocol,
                vless_port,
                vmess_port,
                hysteria2_port,
                tuic_port,
                anytls_port,
                sing_box_bin,
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
            &manifest,
            sbctl_artifact.as_deref(),
            sing_box_artifact.as_deref(),
        ),
        Command::SingBox { command } => sing_box(root, command),
        Command::Sub { format } => print_subscription_urls(root, format.map(Into::into)),
        Command::Credential { command } => run_credential(root, command),
        Command::Serve { bind, max_requests } => serve_subscription(root, bind, max_requests),
        Command::Certificate { command } => run_certificate(root, command),
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
        SingBoxCommand::Update { manifest, artifact } => {
            let result = sbctl::update::read_manifest(&manifest).and_then(|manifest| {
                let temporary = tempfile::NamedTempFile::new().map_err(|error| {
                    sbctl::update::UpdateError::DownloadFailed("sing-box", error.to_string())
                })?;
                let candidate = match artifact {
                    Some(candidate) => {
                        sbctl::update::verify_sing_box_artifact(&manifest, &candidate)?;
                        candidate
                    }
                    None => {
                        let candidate = temporary.path().to_path_buf();
                        sbctl::update::download_sing_box(&manifest, &candidate)?;
                        candidate
                    }
                };
                sbctl::update::apply_sing_box(
                    &sbctl::config::DeploymentStore::new(root),
                    &manifest,
                    &candidate,
                )
            });
            result
                .map(|rollback| format!("sing-box updated; rollback point: {}", rollback.display()))
        }
        SingBoxCommand::Remove => sbctl::lifecycle::remove_managed_sing_box(root)
            .map(|_| "sing-box removed".to_owned())
            .map_err(|error| sbctl::update::UpdateError::DownloadFailed("sing-box", error)),
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
    manifest_path: &Path,
    sbctl_artifact: Option<&Path>,
    sing_box_artifact: Option<&Path>,
) -> ExitCode {
    let result = sbctl::update::read_manifest(manifest_path).and_then(|manifest| {
        if check {
            return Ok(format!(
                "update check completed without downloading or changing the host\n{}",
                sbctl::update::available_versions(&manifest)
            ));
        }
        let sbctl_download = tempfile::NamedTempFile::new().map_err(|error| {
            sbctl::update::UpdateError::DownloadFailed("sbctl", error.to_string())
        })?;
        let sing_box_download = tempfile::NamedTempFile::new().map_err(|error| {
            sbctl::update::UpdateError::DownloadFailed("sing-box", error.to_string())
        })?;
        let sbctl_artifact = match sbctl_artifact {
            Some(path) => path.to_path_buf(),
            None => {
                sbctl::update::download_sbctl(&manifest, sbctl_download.path())?;
                sbctl_download.path().to_path_buf()
            }
        };
        let sing_box_artifact = match sing_box_artifact {
            Some(path) => path.to_path_buf(),
            None => {
                sbctl::update::download_sing_box(&manifest, sing_box_download.path())?;
                sing_box_download.path().to_path_buf()
            }
        };
        let rollback = sbctl::update::apply(
            &sbctl::config::DeploymentStore::new(root),
            &manifest,
            &sbctl_artifact,
            &sing_box_artifact,
        )?;
        Ok(format!(
            "update completed after verified validation and service health checks\nrollback point: {}",
            rollback.display()
        ))
    });
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

fn install(root: &Path, options: InstallOptions) -> ExitCode {
    if options.subscription_host.is_none()
        && options.interface.is_none()
        && options.reality_decoy_sni.is_none()
        && options.sing_box_bin.is_none()
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
    let result = (|| {
        sbctl::preflight::preflight(root)
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
        let subscription_host =
            required_install_value(options.subscription_host, "Subscription host")?;
        let interface = options.interface.map(Ok).unwrap_or_else(|| {
            sbctl::traffic::detect_default_route_interface(root).map_err(|_| {
                sbctl::config::ConfigError::InvalidValue(
                    "could not detect a default-route interface; specify --interface",
                )
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
        let config = sbctl::config::DeploymentConfig::new_with_ports(
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
        config.validate()?;
        let sing_box_bin = options
            .sing_box_bin
            .ok_or(sbctl::config::ConfigError::InvalidValue(
                "installation requires --sing-box-bin after verified sing-box retrieval",
            ))?;
        let artifacts = sbctl::subscription::generated_artifacts(&config)
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
            .map(|(name, contents)| (*name, contents.as_bytes()))
            .collect::<Vec<_>>();
        let store = sbctl::config::DeploymentStore::new(root);
        store.initialize_with_artifacts(&config, &references)?;
        let direct = config.subscription_mode == sbctl::config::SubscriptionMode::Direct;
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
            println!(
                "installation completed\nenabled protocols: {}\nrequired firewall ports (not changed):\n{}\n\n再次进入管理菜单: sbctl menu",
                config
                    .enabled_protocols
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                sbctl::lifecycle::required_firewall_ports(&config).join("\n")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            if installation_started {
                sbctl::lifecycle::rollback_fresh_installation(root);
            }
            eprintln!("installation failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn menu(root: &Path) -> ExitCode {
    if !io::stdin().is_terminal() {
        eprintln!("menu requires an interactive terminal");
        return ExitCode::from(2);
    }

    loop {
        println!(
            "\nsbctl 管理菜单\n1) 查看部署状态\n2) 查看 VPS 流量\n3) 查看节点端口\n4) 显示订阅地址\n5) 校验配置并重启服务\n6) 修改部署配置（向导）\n7) 轮换 Subscription credential\n8) 卸载 sbctl（保留备份和配置）\n0) 退出"
        );
        print!("请选择 [0]: ");
        if let Err(error) = io::stdout().flush() {
            eprintln!("menu failed: {error}");
            return ExitCode::from(2);
        }
        let mut choice = String::new();
        if let Err(error) = io::stdin().read_line(&mut choice) {
            eprintln!("menu failed: {error}");
            return ExitCode::from(2);
        }

        match choice.trim() {
            "" | "0" => return ExitCode::SUCCESS,
            "1" => {
                print_status(root);
            }
            "2" => {
                print_traffic(root);
            }
            "3" => {
                print_nodes(root);
            }
            "4" => {
                print_subscription_urls(root, None);
            }
            "5" => {
                if confirm_menu_action("确认校验配置并重启服务") {
                    restart(root, None);
                }
            }
            "6" => {
                run_config_wizard(root, None);
            }
            "7" => {
                if confirm_menu_action("确认轮换 Subscription credential（旧订阅 URL 将立即失效）") {
                    rotate_subscription_credential(root);
                }
            }
            "8" => {
                if confirm_menu_action("确认卸载 sbctl 服务和二进制（保留备份和配置）")
                {
                    return uninstall(root, false);
                }
            }
            _ => eprintln!("无效选择，请输入 0 到 8。"),
        }
    }
}

fn confirm_menu_action(prompt: &str) -> bool {
    print!("{prompt} [y/N]: ");
    if io::stdout().flush().is_err() {
        return false;
    }
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn select_protocols(
    disabled: &[CliManagedProtocol],
) -> Result<Vec<sbctl::config::ManagedProtocol>, sbctl::config::ConfigError> {
    let defaults = [
        CliManagedProtocol::VlessReality,
        CliManagedProtocol::VmessWebsocket,
        CliManagedProtocol::Hysteria2,
        CliManagedProtocol::Tuic,
        CliManagedProtocol::Anytls,
    ];
    let mut selected = Vec::new();
    for protocol in defaults {
        let disabled_by_flag = disabled
            .iter()
            .any(|disabled| std::mem::discriminant(disabled) == std::mem::discriminant(&protocol));
        if !disabled_by_flag && (!io::stdin().is_terminal() || confirm_protocol(&protocol)?) {
            selected.push(protocol.into());
        }
    }
    Ok(selected)
}

fn protocol_ports(
    vless_reality: Option<u16>,
    vmess_websocket: Option<u16>,
    hysteria2: Option<u16>,
    tuic: Option<u16>,
    anytls: Option<u16>,
) -> sbctl::config::ProtocolPorts {
    sbctl::config::ProtocolPorts {
        vless_reality,
        vmess_websocket,
        hysteria2,
        tuic,
        anytls,
    }
}

fn confirm_protocol(protocol: &CliManagedProtocol) -> Result<bool, sbctl::config::ConfigError> {
    let name = match protocol {
        CliManagedProtocol::VlessReality => "vless-reality",
        CliManagedProtocol::VmessWebsocket => "vmess-websocket",
        CliManagedProtocol::Hysteria2 => "hysteria2",
        CliManagedProtocol::Tuic => "tuic",
        CliManagedProtocol::Anytls => "anytls",
    };
    print!("Enable {name} [Y/n]: ");
    io::stdout()
        .flush()
        .map_err(sbctl::config::ConfigError::Storage)?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(sbctl::config::ConfigError::Storage)?;
    Ok(!matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "n" | "no"
    ))
}

fn required_install_value(
    value: Option<String>,
    label: &str,
) -> Result<String, sbctl::config::ConfigError> {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        return Ok(value);
    }
    print!("{label}: ");
    io::stdout()
        .flush()
        .map_err(sbctl::config::ConfigError::Storage)?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(sbctl::config::ConfigError::Storage)?;
    let value = value.trim().to_owned();
    (!value.is_empty())
        .then_some(value)
        .ok_or(sbctl::config::ConfigError::InvalidValue(
            "interactive installation requires a value",
        ))
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

fn run_certificate(root: &Path, command: CertificateCommand) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        match command {
            CertificateCommand::Obtain { email } => {
                sbctl::certificate::obtain(&store, &config, &email)
            }
            CertificateCommand::Renew => sbctl::certificate::renew(&store, &config),
            CertificateCommand::Verify => sbctl::certificate::deploy_hook(&store, &config),
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
            sbctl::config::ConfigError::StateContent(
                sbctl::subscription::redact_secret(&error.to_string(), &config.subscription_credential),
            )
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
                config.subscription_host,
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
    let store = sbctl::config::DeploymentStore::new(root);
    let is_ip_fallback = store.load().is_ok_and(|config| {
        config.subscription_mode == sbctl::config::SubscriptionMode::IpFallback
    });
    let result = store.load().and_then(|config| {
        let formats = match format {
            Some(format) => vec![format],
            None => [
                sbctl::subscription::SubscriptionFormat::SingBox,
                sbctl::subscription::SubscriptionFormat::Clash,
                sbctl::subscription::SubscriptionFormat::Uri,
            ]
            .into_iter()
            .collect(),
        };
        formats
            .into_iter()
            .map(|format| sbctl::subscription::subscription_url(&config, format))
            .collect::<Result<Vec<_>, _>>()
            .map(|urls| urls.join("\n"))
            .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))
    });
    match result {
        Ok(contents) => {
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

fn print_status(root: &Path) -> ExitCode {
    match sbctl::config::DeploymentStore::new(root).load() {
        Ok(config) => {
            println!("{}", config.summary());
            println!("\n{}", sbctl::lifecycle::service_status(root));
            match sbctl::traffic::report(&sbctl::config::DeploymentStore::new(root), &config) {
                Ok(report) => println!("\n{}", report.summary()),
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
                    })
                })
                .unwrap_or_else(|error| serde_json::json!({ "error": error.to_string() }));
            let services = sbctl::lifecycle::service_status_entries(root)
                .into_iter()
                .map(|(unit, state)| (unit.to_owned(), state))
                .collect::<std::collections::BTreeMap<_, _>>();
            let certificate = (config.subscription_mode
                == sbctl::config::SubscriptionMode::Direct)
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
        Ok(config) => sbctl::traffic::report(&store, &config).map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(report) => {
            println!("{}", report.summary());
            ExitCode::SUCCESS
        }
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

fn regenerate(root: &Path, sing_box_bin: Option<PathBuf>) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let update_active_config = root.join("var/lib/sbctl/ownership").is_file();
    let result = store.load().and_then(|config| {
        let binary = sing_box_bin.unwrap_or_else(|| root.join("usr/local/bin/sing-box"));
        sbctl::subscription::regenerate(
            &store,
            &config,
            Some(binary.as_path()),
            update_active_config,
        )
        .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))
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
        ConfigCommand::Init {
            mode,
            subscription_host,
            proxy_host,
            http_port,
            listen_port,
            interface,
            protocols,
            reality_decoy_sni,
            monthly_traffic_limit,
            accounting_policy,
            accounting_timezone,
            anchored_reset_at,
            sing_box_bin,
            vless_port,
            vmess_port,
            hysteria2_port,
            tuic_port,
            anytls_port,
        } => {
            let interface = interface.map(Ok).unwrap_or_else(|| {
                sbctl::traffic::detect_default_route_interface(root).map_err(|_| {
                    sbctl::config::ConfigError::InvalidValue(
                        "could not detect a default-route interface; specify --interface",
                    )
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
                config.subscription_listen_port = listen_port;
                config.monthly_traffic_limit = monthly_traffic_limit;
                config.accounting_policy = accounting_policy.into();
                if let Some(timezone) = accounting_timezone {
                    config.accounting_timezone = timezone;
                }
                config.anchored_reset_at = anchored_reset_at;
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
                    sbctl::subscription::generated_artifacts(&config).map_err(|error| {
                        sbctl::config::ConfigError::StateContent(error.to_string())
                    })?
                } else {
                    Vec::new()
                };
                let artifact_references = generated_artifacts
                    .iter()
                    .map(|(name, contents)| (*name, contents.as_bytes()))
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

struct ConsolePrompts;

impl sbctl::wizard::Prompts for ConsolePrompts {
    fn ask(&mut self, label: &str, default: Option<&str>) -> io::Result<String> {
        print!("{label}");
        if let Some(default) = default {
            print!(" [{}]", default);
        }
        print!(": ");
        io::stdout().flush()?;
        let mut answer = String::new();
        let read = io::stdin().read_line(&mut answer)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "wizard input ended before the prompt was answered",
            ));
        }
        Ok(answer.trim().to_owned())
    }

    fn report(&mut self, message: &str) {
        println!("{message}");
    }

    fn confirm(&mut self, question: &str, default: bool) -> io::Result<bool> {
        loop {
            print!("{question} [{}]: ", if default { "Y/n" } else { "y/N" });
            io::stdout().flush()?;
            let mut answer = String::new();
            let read = io::stdin().read_line(&mut answer)?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "wizard input ended before the confirmation was answered",
                ));
            }
            match answer.trim().to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => eprintln!("请输入 y 或 n"),
            }
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
                let artifacts = sbctl::subscription::generated_artifacts(new)
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
                    .map(|(name, contents)| (*name, contents.as_bytes()))
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
        let _ = sbctl::lifecycle::restart_services(root);
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
            let _ = store.replace(&config);
        })?;
        Ok(config)
    });
    match result {
        Ok(_) => {
            println!("subscription credential rotated; all previous subscription URLs are now invalid");
            println!("run 'sbctl sub' to display the new subscription URLs");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("credential rotation failed: {error}");
            ExitCode::from(2)
        }
    }
}
