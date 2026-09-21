//! Host-level commands that are not about the sing-box deployment: the BBR
//! tuning helpers and the subscription credential rotation, plus the host
//! facts the menu header prints.

use crate::cli::args::{CredentialCommand, SystemCommand};
use crate::cli::commands::config::restart_services_with_rollback;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

pub(crate) fn system_info() -> (String, String, String, String) {
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

pub(crate) fn run_system(root: &Path, command: SystemCommand) -> ExitCode {
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

pub(crate) fn run_credential(root: &Path, command: CredentialCommand) -> ExitCode {
    match command {
        CredentialCommand::Rotate => rotate_subscription_credential(root),
    }
}

pub(crate) fn rotate_subscription_credential(root: &Path) -> ExitCode {
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
