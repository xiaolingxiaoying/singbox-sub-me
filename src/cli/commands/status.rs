//! Reporting and accounting: `sbctl status` (human and JSON), `sbctl node`,
//! `sbctl traffic`, and the periodic accounting reset the timer runs.

use std::path::Path;
use std::process::ExitCode;

pub(crate) fn format_local_time(instant: chrono::DateTime<chrono::Utc>, timezone: &str) -> String {
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

pub(crate) fn print_nodes(root: &Path, uri: bool) -> ExitCode {
    match sbctl::config::DeploymentStore::new(root).load() {
        Ok(config) => {
            println!("{}", sbctl::lifecycle::enabled_nodes(&config));
            if uri {
                let links = sbctl::lifecycle::node_share_links(&config);
                if !links.is_empty() {
                    println!("\n原生分享链接（含节点凭据，仅输出到本终端）:");
                    print!("{links}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("node failed: {error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn days_until(timestamp: i64) -> Option<i64> {
    let now = chrono::Utc::now().timestamp();
    Some((timestamp - now).div_euclid(86_400))
}

pub(crate) fn print_status(root: &Path) -> ExitCode {
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
            // Advisory only: an installed kernel newer than the version table is
            // a gap in this tool's registry, not a fault in the deployment, so
            // it is printed here and never changes the exit status.
            if let Some(warning) = sbctl::subscription::kernel_band_warning(
                super::config::resolve_sing_box_bin(root, None).as_deref(),
            ) {
                println!("\n{warning}");
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

pub(crate) fn print_status_json(root: &Path) -> ExitCode {
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
            let kernel_warning = sbctl::subscription::kernel_band_warning(
                super::config::resolve_sing_box_bin(root, None).as_deref(),
            );
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
                "kernel_version_warning": kernel_warning,
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

pub(crate) fn print_traffic(root: &Path) -> ExitCode {
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

pub(crate) fn traffic_set_used(
    root: &Path,
    bytes: Option<u64>,
    rx: Option<u64>,
    tx: Option<u64>,
) -> ExitCode {
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

pub(crate) fn run_accounting_reset(root: &Path) -> ExitCode {
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
