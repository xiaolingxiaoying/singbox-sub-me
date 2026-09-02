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
    /// Create, inspect, and validate the persistent deployment configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

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
    },
    /// Display the persisted deployment summary without exposing credentials.
    Show,
    /// Parse and validate the persisted deployment configuration.
    Validate,
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
        Command::Config { command } => run_config(root, command),
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
                store.initialize(&config)
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
