use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "sbctl", about = "Manage a private sing-box deployment")]
struct Cli {
    #[arg(long, global = true, hide = true, value_name = "PATH")]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check whether this host is safe for a new sbctl deployment.
    Install,
    /// Show whether sbctl currently manages a deployment.
    Status,
    /// Reconcile and show VPS traffic for the current accounting period.
    Traffic,
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
        Command::Install => match sbctl::preflight::preflight(root) {
            Ok(()) => {
                println!("install preflight passed: host is ready for a new sbctl deployment");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("install preflight failed: {error}");
                ExitCode::from(2)
            }
        },
        Command::Status => print_status(root),
        Command::Traffic => print_traffic(root),
        Command::Sub { format } => print_subscription_urls(root, format.map(Into::into)),
        Command::Serve { bind, max_requests } => serve_subscription(root, bind, max_requests),
        Command::Certificate { command } => run_certificate(root, command),
        Command::Config { command } => run_config(root, command),
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
        let bind = bind.unwrap_or_else(|| {
            format!(
                "{}:{}",
                config.subscription_host,
                config.http_port.unwrap_or_default()
            )
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
                config.monthly_traffic_limit = monthly_traffic_limit;
                config.accounting_policy = accounting_policy.into();
                config.accounting_timezone =
                    accounting_timezone.unwrap_or_else(|| system_accounting_timezone(root));
                config.anchored_reset_at = anchored_reset_at;
                config.validate()?;
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
                    )
                });
                if requires_sing_box_check && sing_box_bin.is_none() {
                    return Err(sbctl::config::ConfigError::InvalidValue(
                        "VMess WebSocket and Hysteria2 require --sing-box-bin for configuration validation",
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
