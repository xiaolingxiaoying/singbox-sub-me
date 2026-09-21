//! Subscription delivery: `sbctl serve`, `sbctl sub` and `sbctl qr`.

use std::path::Path;
use std::process::ExitCode;

pub(crate) fn serve_subscription(
    root: &Path,
    bind: Option<String>,
    max_requests: Option<usize>,
) -> ExitCode {
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
                // An IPv6 bind address is only parseable bracketed; the stored
                // host stays bare because the generated configs need it that way.
                sbctl::canonical::uri_host(&config.subscription_host),
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

pub(crate) fn print_subscription_urls(
    root: &Path,
    format: Option<sbctl::subscription::SubscriptionFormat>,
) -> ExitCode {
    use sbctl::subscription::SubscriptionRoute;
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        let is_ip_fallback =
            config.subscription_mode == sbctl::config::SubscriptionMode::IpFallback;
        let contents = match format {
            Some(format) => sbctl::subscription::subscription_url(&config, format)
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?,
            None => {
                let mut table = String::from("按客户端选择订阅链接（推荐）：\n\n");
                for row in sbctl::subscription::client_subscription_matrix() {
                    table.push_str(&format!("{}：{}\n", row.client, row.note));
                    for recommended in &row.formats {
                        let url =
                            sbctl::subscription::subscription_url(&config, recommended.format)
                                .map_err(|error| {
                                    sbctl::config::ConfigError::StateContent(error.to_string())
                                })?;
                        table.push_str(&format!("  {url}\n    （{}）\n", recommended.note));
                    }
                    table.push('\n');
                }
                table.push_str("全部订阅格式：\n\n");
                for info in sbctl::subscription::subscription_matrix() {
                    let url = sbctl::subscription::subscription_url(&config, info.format).map_err(
                        |error| sbctl::config::ConfigError::StateContent(error.to_string()),
                    )?;
                    let qr =
                        sbctl::subscription::route_url(&config, SubscriptionRoute::Qr(info.format))
                            .map_err(|error| {
                                sbctl::config::ConfigError::StateContent(error.to_string())
                            })?;
                    table.push_str(&format!(
                        "{label}\n  订阅链接：{url}\n  二维码：{qr}\n  说明：{note}\n\n",
                        label = info.label,
                        url = url,
                        qr = qr,
                        note = info.note
                    ));
                }
                let index = sbctl::subscription::route_url(&config, SubscriptionRoute::Index)
                    .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
                table.push_str(&format!(
                    "订阅总览页（含按客户端速查、全部二维码与导入步骤）：\n  {index}\n"
                ));
                table
            }
        };
        Ok((contents, is_ip_fallback))
    });
    match result {
        Ok((contents, is_ip_fallback)) => {
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

pub(crate) fn print_subscription_qr(
    root: &Path,
    format: Option<sbctl::subscription::SubscriptionFormat>,
    all: bool,
) -> ExitCode {
    use sbctl::subscription::SubscriptionFormat;
    let store = sbctl::config::DeploymentStore::new(root);
    let result = store.load().and_then(|config| {
        let formats: Vec<SubscriptionFormat> = if all {
            sbctl::subscription::subscription_matrix()
                .into_iter()
                .map(|info| info.format)
                .collect()
        } else {
            vec![format.unwrap_or(SubscriptionFormat::SingBoxFull)]
        };
        let mut rendered = Vec::with_capacity(formats.len());
        for format in formats {
            let url = sbctl::subscription::subscription_url(&config, format)
                .map_err(|error| sbctl::config::ConfigError::StateContent(error.to_string()))?;
            let qr =
                sbctl::qr::render_ansi(&url).map_err(sbctl::config::ConfigError::StateContent)?;
            rendered.push((format, url, qr));
        }
        Ok(rendered)
    });
    match result {
        Ok(rendered) => {
            for (format, url, qr) in rendered {
                println!("{} 订阅二维码：", format.display_label());
                println!("{url}");
                println!("{qr}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("subscription qr failed: {error}");
            ExitCode::from(2)
        }
    }
}
