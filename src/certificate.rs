use std::process::Command;

use thiserror::Error;

use crate::config::{DeploymentConfig, DeploymentStore, SubscriptionMode};

#[derive(Debug, Error)]
pub enum CertificateError {
    #[error("certificates are managed only in direct subscription mode")]
    NotDirect,
    #[error("Certbot must be installed from the Debian or Ubuntu package repository: {0}")]
    Certbot(std::io::Error),
    #[error("Certbot failed: {0}")]
    Failed(String),
}

pub fn obtain(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    email: &str,
) -> Result<(), CertificateError> {
    require_direct(config)?;
    let status = Command::new("certbot")
        .args(["certonly", "--webroot", "--webroot-path"])
        .arg(store.acme_webroot())
        .args([
            "--domain",
            &config.subscription_host,
            "--email",
            email,
            "--agree-tos",
            "--non-interactive",
            "--keep-until-expiring",
        ])
        .output()
        .map_err(CertificateError::Certbot)?;
    certbot_result(status)
}

pub fn renew(config: &DeploymentConfig) -> Result<(), CertificateError> {
    require_direct(config)?;
    let status = Command::new("certbot")
        .args(["renew", "--cert-name", &config.subscription_host])
        .output()
        .map_err(CertificateError::Certbot)?;
    // The daemon reloads certificate files before every TLS handshake. A successful
    // renewal therefore takes effect without signalling or restarting a service.
    certbot_result(status)
}

fn require_direct(config: &DeploymentConfig) -> Result<(), CertificateError> {
    (config.subscription_mode == SubscriptionMode::Direct)
        .then_some(())
        .ok_or(CertificateError::NotDirect)
}

fn certbot_result(output: std::process::Output) -> Result<(), CertificateError> {
    if output.status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(CertificateError::Failed(if diagnostic.is_empty() {
        format!("exited with {}", output.status)
    } else {
        diagnostic
    }))
}
