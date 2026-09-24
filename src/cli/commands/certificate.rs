//! Certificate operations: `sbctl certificate obtain|renew|verify|status`,
//! including the ACME registration email confirmation and the human-readable
//! certificate report.

use crate::cli::args::CertificateCommand;
use crate::cli::commands::status::days_until;
use crate::cli::menu::confirm_menu_action;
use std::path::Path;
use std::process::ExitCode;

/// Resolves the ACME registration email for `certificate obtain`. The no-email
/// path is only returned after an explicit interactive confirmation.
fn resolve_obtain_email(email: Option<&str>, no_email: bool) -> Result<Option<String>, String> {
    if no_email {
        if confirm_menu_action("确认不使用邮箱注册证书（--register-unsafely-without-email）？")
        {
            return Ok(None);
        }
        return Err("已取消：未确认免邮箱注册。".to_owned());
    }
    let Some(email) = email else {
        return Err("请提供 --email <邮箱>，或使用 --no-email 跳过（需二次确认）。".to_owned());
    };
    let email = email.trim().to_owned();
    if sbctl::certificate::acme_email_is_valid(&email) {
        Ok(Some(email))
    } else {
        Err(
            "邮箱格式无效（示例 admin@example.com）；该邮箱仅用于证书到期通知。确实不需要时请用 --no-email 并二次确认。"
                .to_owned(),
        )
    }
}

pub(crate) fn run_certificate(root: &Path, command: CertificateCommand) -> ExitCode {
    let store = sbctl::config::DeploymentStore::new(root);
    if let CertificateCommand::Status = command {
        return match store.load() {
            Ok(config) => {
                print_certificate_status(root, &store, &config);
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("certificate operation failed: {error}");
                ExitCode::from(2)
            }
        };
    }
    // The no-email ACME path is explicit and confirmed before touching config.
    let obtain_email = if let CertificateCommand::Obtain { email, no_email } = &command {
        match resolve_obtain_email(email.as_deref(), *no_email) {
            Ok(email) => email,
            Err(message) => {
                eprintln!("certificate operation failed: {message}");
                return ExitCode::from(2);
            }
        }
    } else {
        None
    };
    let result = store.load().and_then(|config| {
        match &command {
            CertificateCommand::Obtain { .. } => {
                if config.subscription_mode == sbctl::config::SubscriptionMode::Direct {
                    sbctl::certificate::require_certbot(root).map_err(|error| {
                        sbctl::config::ConfigError::StateContent(error.to_string())
                    })?;
                }
                sbctl::certificate::obtain(&store, &config, obtain_email.as_deref())
            }
            CertificateCommand::Renew => sbctl::certificate::renew(&store, &config),
            CertificateCommand::Verify => sbctl::certificate::deploy_hook(&store, &config),
            // Handled above with its own report; the compiler cannot see that
            // this closure is only reached for the remaining commands.
            CertificateCommand::Status => unreachable!("handled before the operation match"),
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
            sbctl::config::ConfigError::StateContent(sbctl::subscription::redact_secret(
                &error.to_string(),
                &config.subscription_credential,
            ))
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

/// Prints the human-readable certificate report for `sbctl certificate status`:
/// validity window with days remaining, SAN coverage, deploy-hook presence,
/// and the fix commands when something is off.
fn print_certificate_status(
    root: &Path,
    store: &sbctl::config::DeploymentStore,
    config: &sbctl::config::DeploymentConfig,
) {
    use sbctl::config::SubscriptionMode;
    if config.subscription_mode != SubscriptionMode::Direct {
        println!(
            "当前订阅模式为 {}，证书由反向代理或自签机制负责，sbctl 不管理证书。",
            config.subscription_mode
        );
        return;
    }
    let status = sbctl::certificate::status(store, config);
    println!("Direct 订阅证书状态（{host}）:", host = status.host);
    match status.state {
        "ok" => {
            let not_before = status
                .not_before
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|when| when.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_owned());
            let not_after = status
                .not_after
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|when| when.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_owned());
            let days = status.not_after.and_then(days_until);
            println!("  状态: 有效");
            println!("  生效自: {not_before}");
            println!("  有效期至: {not_after}");
            if let Some(days) = days {
                println!("  剩余天数: {days} 天");
                if days < 14 {
                    println!(
                        "  提醒: 证书即将到期；确认 certbot.timer 在运行（systemctl status certbot.timer）"
                    );
                }
            }
            if !status.san.is_empty() {
                println!("  SAN: {}", status.san.join(", "));
            }
            if let Some(fingerprint) = &status.fingerprint {
                println!("  SHA-256 指纹: {fingerprint}");
            }
        }
        _ => {
            println!("  状态: 异常");
            if let Some(error) = &status.error {
                println!("  原因: {error}");
            }
            println!(
                "  修复: sbctl certificate renew（续期）或 sbctl certificate obtain --email <邮箱>（首次签发）"
            );
        }
    }
    let hook = root.join(sbctl::lifecycle::CERTBOT_DEPLOY_HOOK_RELATIVE_PATH);
    if hook.is_file() {
        println!("  Certbot deploy hook: 已安装（续期后自动校验并固定证书）");
    } else {
        println!(
            "  Certbot deploy hook: 未找到（{}）；续期后需要手动执行 sbctl certificate verify",
            hook.display()
        );
    }
}
