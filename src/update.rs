use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{ConfigError, DeploymentStore};

const MANAGED_PATHS: &[&str] = &[
    "usr/local/bin/sbctl",
    "usr/local/bin/sing-box",
    "etc/sbctl/config.toml",
    "var/lib/sbctl/state.json",
    "etc/sing-box/config.json",
];

#[derive(Debug, Deserialize)]
pub struct ReleaseManifest {
    pub sbctl: ReleaseArtifact,
    pub sing_box: ReleaseArtifact,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseArtifact {
    pub version: String,
    pub sha256: String,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("explicit update requires {0}")]
    MissingArtifactArgument(&'static str),
    #[error("could not read the pinned release manifest: {0}")]
    ManifestRead(#[from] std::io::Error),
    #[error("could not parse the pinned release manifest: {0}")]
    ManifestParse(#[from] serde_json::Error),
    #[error("pinned release manifest has an invalid SHA-256 for {0}")]
    InvalidDigest(&'static str),
    #[error("{0} artifact does not match the pinned release manifest")]
    DigestMismatch(&'static str),
    #[error("pinned release manifest has no download URL for {0}")]
    MissingDownloadUrl(&'static str),
    #[error("download of {0} failed: {1}")]
    DownloadFailed(&'static str, String),
    #[error("sbctl candidate health check failed: {0}")]
    SbctlHealth(String),
    #[error("sing-box candidate configuration check failed: {0}")]
    SingBoxCheck(String),
    #[error("service health check failed: {0}")]
    ServiceHealth(String),
    #[error("rollback failed: {0}")]
    Rollback(String),
    #[error(transparent)]
    Storage(#[from] ConfigError),
}

pub fn read_manifest(path: &Path) -> Result<ReleaseManifest, UpdateError> {
    let manifest: ReleaseManifest = serde_json::from_slice(&fs::read(path)?)?;
    validate_digest("sbctl", &manifest.sbctl.sha256)?;
    validate_digest("sing-box", &manifest.sing_box.sha256)?;
    Ok(manifest)
}

pub fn available_versions(manifest: &ReleaseManifest) -> String {
    format!(
        "sbctl: {} available\nsing-box: {} available",
        manifest.sbctl.version, manifest.sing_box.version
    )
}

pub fn download_sing_box(manifest: &ReleaseManifest, output: &Path) -> Result<(), UpdateError> {
    download_artifact("sing-box", &manifest.sing_box, output)
}

pub fn download_sbctl(manifest: &ReleaseManifest, output: &Path) -> Result<(), UpdateError> {
    download_artifact("sbctl", &manifest.sbctl, output)
}

fn download_artifact(
    name: &'static str,
    artifact: &ReleaseArtifact,
    output: &Path,
) -> Result<(), UpdateError> {
    let url = artifact
        .url
        .as_deref()
        .ok_or(UpdateError::MissingDownloadUrl(name))?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = output.with_extension("download.tmp");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&temporary)
        .arg(url)
        .status()
        .map_err(|error| UpdateError::DownloadFailed(name, error.to_string()))?;
    if !status.success() {
        let _ = fs::remove_file(&temporary);
        return Err(UpdateError::DownloadFailed(
            name,
            format!("curl exited with {status}"),
        ));
    }
    let result = verify_artifact(name, &temporary, &artifact.sha256);
    if result.is_ok() {
        fs::rename(&temporary, output)?;
    } else {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn verify_sing_box_artifact(
    manifest: &ReleaseManifest,
    candidate: &Path,
) -> Result<(), UpdateError> {
    verify_artifact("sing-box", candidate, &manifest.sing_box.sha256)
}

/// Updates only the data-plane binary. The existing generated configuration is
/// checked with the candidate before the managed binary is replaced.
pub fn apply_sing_box(
    store: &DeploymentStore,
    manifest: &ReleaseManifest,
    candidate: &Path,
) -> Result<PathBuf, UpdateError> {
    verify_sing_box_artifact(manifest, candidate)?;
    let _lock = store.acquire_operation_lock()?;
    let server_config = fs::read_to_string(
        store
            .root()
            .join("var/lib/sbctl/artifacts/sing-box-server.json"),
    )
    .map_err(ConfigError::Storage)?;
    crate::subscription::check_sing_box_config(candidate, &server_config)
        .map_err(|error| UpdateError::SingBoxCheck(error.to_string()))?;

    let rollback = rollback_directory(store.root());
    let old_path = store.root().join("usr/local/bin/sing-box");
    let old_contents = fs::read(&old_path).ok();
    if let Some(contents) = &old_contents {
        store.write_relative_locked(
            &format!(
                "var/lib/sbctl/rollback/{}/usr/local/bin/sing-box",
                rollback
                    .file_name()
                    .ok_or_else(|| UpdateError::Rollback("invalid rollback path".to_owned()))?
                    .to_string_lossy()
            ),
            contents,
        )?;
    }
    store.write_relative_locked("usr/local/bin/sing-box", &fs::read(candidate)?)?;
    if let Err(error) = crate::lifecycle::restart_sing_box_service(store.root()) {
        if old_contents.is_some() {
            let backup = rollback.join("usr/local/bin/sing-box");
            store.write_relative_locked("usr/local/bin/sing-box", &fs::read(backup)?)?;
        } else {
            let _ = fs::remove_file(&old_path);
        }
        let _ = crate::lifecycle::restart_sing_box_service(store.root());
        return Err(UpdateError::ServiceHealth(error));
    }
    Ok(rollback)
}

pub fn apply(
    store: &DeploymentStore,
    manifest: &ReleaseManifest,
    sbctl_candidate: &Path,
    sing_box_candidate: &Path,
) -> Result<PathBuf, UpdateError> {
    verify_artifact("sbctl", sbctl_candidate, &manifest.sbctl.sha256)?;
    verify_artifact("sing-box", sing_box_candidate, &manifest.sing_box.sha256)?;
    check_sbctl_candidate(sbctl_candidate)?;

    let _lock = store.acquire_operation_lock()?;
    let config = store.load()?;
    let server_config = fs::read_to_string(
        store
            .root()
            .join("var/lib/sbctl/artifacts/sing-box-server.json"),
    )
    .map_err(ConfigError::Storage)?;
    crate::subscription::check_sing_box_config(sing_box_candidate, &server_config)
        .map_err(|error| UpdateError::SingBoxCheck(error.to_string()))?;

    let rollback = rollback_directory(store.root());
    let backup = backup(store, &rollback)?;
    store.write_relative_locked("usr/local/bin/sbctl", &fs::read(sbctl_candidate)?)?;
    store.write_relative_locked("usr/local/bin/sing-box", &fs::read(sing_box_candidate)?)?;

    if let Err(error) = crate::lifecycle::restart_services(store.root()) {
        restore(store, &backup)
            .map_err(|rollback_error| UpdateError::Rollback(rollback_error.to_string()))?;
        let _ = crate::lifecycle::restart_services(store.root());
        return Err(UpdateError::ServiceHealth(error));
    }
    // Ensure the loaded configuration was valid before acknowledging the update.
    config.validate()?;
    Ok(rollback)
}

struct BackupEntry {
    relative: &'static str,
    contents: Option<Vec<u8>>,
}

fn backup(store: &DeploymentStore, rollback: &Path) -> Result<Vec<BackupEntry>, UpdateError> {
    let mut entries = Vec::new();
    for relative in MANAGED_PATHS {
        let source = store.root().join(relative);
        let contents = match fs::read(&source) {
            Ok(contents) => {
                let backup_path = rollback.join(relative);
                let backup_relative = backup_path
                    .strip_prefix(store.root())
                    .expect("rollback path is inside the managed root")
                    .to_str()
                    .expect("managed rollback path is UTF-8");
                store.write_relative_locked(backup_relative, &contents)?;
                Some(contents)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        entries.push(BackupEntry { relative, contents });
    }
    Ok(entries)
}

fn restore(store: &DeploymentStore, backup: &[BackupEntry]) -> Result<(), ConfigError> {
    for entry in backup {
        let path = store.root().join(entry.relative);
        match &entry.contents {
            Some(contents) => store.write_relative_locked(entry.relative, contents)?,
            None if path.exists() => fs::remove_file(path)?,
            None => {}
        }
    }
    Ok(())
}

fn rollback_directory(root: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    root.join("var/lib/sbctl/rollback")
        .join(timestamp.to_string())
}

fn verify_artifact(name: &'static str, path: &Path, expected: &str) -> Result<(), UpdateError> {
    let contents = fs::read(path)?;
    let actual = format!("{:x}", Sha256::digest(contents));
    (actual == expected)
        .then_some(())
        .ok_or(UpdateError::DigestMismatch(name))
}

fn validate_digest(name: &'static str, digest: &str) -> Result<(), UpdateError> {
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(())
        .ok_or(UpdateError::InvalidDigest(name))
}

fn check_sbctl_candidate(candidate: &Path) -> Result<(), UpdateError> {
    let status = Command::new(candidate)
        .arg("--version")
        .status()
        .map_err(|error| UpdateError::SbctlHealth(error.to_string()))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| UpdateError::SbctlHealth(format!("candidate exited with {status}")))
}
