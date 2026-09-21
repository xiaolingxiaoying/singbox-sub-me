//! The `sbctl` binary entry point: parse the clap surface and dispatch to the
//! command handlers. Everything that is not the entry point or the dispatch
//! table lives under `cli::`.

mod cli;

use clap::Parser;
use cli::args::{Cli, Command, InstallOptions, TrafficCommand};
use cli::commands::{
    certificate::run_certificate,
    config::{regenerate, restart, run_config},
    install::install,
    serve::{print_subscription_qr, print_subscription_urls, serve_subscription},
    status::{
        print_nodes, print_status, print_status_json, print_traffic, run_accounting_reset,
        traffic_set_used,
    },
    system::{run_credential, run_system},
    update::{release, sing_box, uninstall, update},
};
use cli::menu::menu;
use std::io::{self, IsTerminal};
use std::path::Path;
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
