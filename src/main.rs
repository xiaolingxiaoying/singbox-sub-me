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
        #[arg(long)]
        interface: Option<String>,
        #[arg(long)]
        reality_decoy_sni: Option<String>,
        /// Explicitly omit a Managed protocol; all five are enabled by default.
        #[arg(long, value_enum)]
        disable_protocol: Vec<CliManagedProtocol>,
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
        /// Create units and configuration without starting services (acceptance fixture use).
        #[arg(long, hide = true)]
        no_start: bool,
    },
    /// Show whether sbctl currently manages a deployment.
    Status,
    /// Reconcile and show VPS traffic for the current accounting period.
    Traffic,
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
}

struct InstallOptions {
    mode: CliSubscriptionMode,
    subscription_host: Option<String>,
    proxy_host: Option<String>,
    interface: Option<String>,
    reality_decoy_sni: Option<String>,
    disable_protocol: Vec<CliManagedProtocol>,
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
        #[arg(long)]
        reality_decoy_sni: Option<String>,
        #[arg(long, default_value_t = 0)]
        monthly_traffic_limit: u64,
        #[arg(long, value_enum, default_value_t = CliAccountingPolicy::NaturalMonth)]
        accounting_policy: CliAccountingPolicy,
        /// Named IANA timezone; defaults to the VPS system timezone when available.
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
            interface,
            reality_decoy_sni,
            disable_protocol,
            sing_box_bin,
            no_start,
        } => install(
            root,
            InstallOptions {
                mode,
                subscription_host,
                proxy_host,
                interface,
                reality_decoy_sni,
                disable_protocol,
                sing_box_bin,
                no_start,
            },
        ),
        Command::Status => print_status(root),
        Command::Traffic => print_traffic(root),
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
        Command::Serve { bind, max_requests } => serve_subscription(root, bind, max_requests),
        Command::Certificate { command } => run_certificate(root, command),
        Command::Config { command } => run_config(root, command),
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
        let config = sbctl::config::DeploymentConfig::new(
            options.mode.into(),
            subscription_host,
            options.proxy_host,
            None,
            interface,
            protocols,
            reality_decoy_sni,
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
        sbctl::lifecycle::install_units(&store, server)?;
        if !options.no_start {
            sbctl::lifecycle::start_services(root)
                .map_err(sbctl::config::ConfigError::StateContent)?;
        }
        Ok::<_, sbctl::config::ConfigError>(config)
    })();
    match result {
        Ok(config) => {
            println!(
                "installation completed\nenabled protocols: {}\nrequired firewall ports (not changed):\n{}",
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
            CertificateCommand::Renew => sbctl::certificate::renew(&config),
        }
        .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))
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
            match sbctl::traffic::reconcile(&sbctl::config::DeploymentStore::new(root), &config) {
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

fn print_traffic(root: &Path) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = match store.load() {
        Ok(config) => sbctl::traffic::reconcile(&store, &config).map_err(|error| error.to_string()),
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

fn run_config(root: &Path, command: ConfigCommand) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    let result = match command {
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
        } => {
            let interface = interface.map(Ok).unwrap_or_else(|| {
                sbctl::traffic::detect_default_route_interface(root).map_err(|_| {
                    sbctl::config::ConfigError::InvalidValue(
                        "could not detect a default-route interface; specify --interface",
                    )
                })
            });
            interface.and_then(|interface| {
                let mut config = sbctl::config::DeploymentConfig::new(
                    mode.into(),
                    subscription_host,
                    proxy_host,
                    http_port,
                    interface,
                    protocols.into_iter().map(Into::into).collect(),
                    reality_decoy_sni,
                )?;
                config.subscription_listen_port = listen_port;
                config.monthly_traffic_limit = monthly_traffic_limit;
                config.accounting_policy = accounting_policy.into();
                config.accounting_timezone =
                    accounting_timezone.unwrap_or_else(|| system_accounting_timezone(root));
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

fn system_accounting_timezone(root: &Path) -> String {
    std::fs::read_to_string(root.join("etc/timezone"))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| value.parse::<chrono_tz::Tz>().is_ok())
        .unwrap_or_else(|| "UTC".to_owned())
}
