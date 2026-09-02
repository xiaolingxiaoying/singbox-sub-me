use clap::{Parser, Subcommand};
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
        Command::Status => {
            println!("sbctl status: unmanaged (not installed)");
            ExitCode::SUCCESS
        }
    }
}
