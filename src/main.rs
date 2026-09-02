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
        #[arg(long = "interface")]
        interface: String,
        #[arg(long = "protocol", value_enum, required = true)]
        protocols: Vec<CliManagedProtocol>,
        #[arg(long)]
        reality_decoy_sni: Option<String>,
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
        Command::Config { command } => run_config(root, command),
    }
}

fn print_status(root: &Path) -> ExitCode {
    match sbctl::config::DeploymentStore::new(root).load() {
        Ok(config) => {
            println!("{}", config.summary());
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
        } => sbctl::config::DeploymentConfig::new(
            mode.into(),
            subscription_host,
            proxy_host,
            http_port,
            interface,
            protocols.into_iter().map(Into::into).collect(),
            reality_decoy_sni,
        )
        .and_then(|config| store.initialize(&config))
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
