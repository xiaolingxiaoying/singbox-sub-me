use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::{ConfigError, DeploymentConfig, DeploymentStore};
use crate::release::{ReleaseArtifact, ReleaseError, ReleaseManifest, verify_trusted};

const MANAGED_PATHS: &[&str] = &[
    "usr/local/bin/sbctl",
    "usr/local/bin/sing-box",
    "etc/sbctl/config.toml",
    "var/lib/sbctl/state.json",
    "etc/sing-box/config.json",
    "var/lib/sbctl/artifacts/sing-box-server.json",
    "var/lib/sbctl/artifacts/subscription-sing-box.json",
    "var/lib/sbctl/artifacts/subscription-clash.yaml",
    "var/lib/sbctl/artifacts/subscription-uri.txt",
    "etc/systemd/system/sing-box.service",
    "etc/systemd/system/sbctl.service",
    "etc/systemd/system/sbctl-http.socket",
    "etc/systemd/system/sbctl-accounting-reset.service",
    "etc/systemd/system/sbctl-accounting-reset.timer",
    "etc/letsencrypt/renewal-hooks/deploy/sbctl-certificate-deploy-hook",
];

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("explicit update requires {0}")]
    MissingArtifactArgument(&'static str),
    #[error("{0} artifact does not match the pinned release manifest")]
    DigestMismatch(&'static str),
    #[error("pinned release manifest has no download URL for {0}")]
    MissingDownloadUrl(&'static str),
    #[error("download of {0} failed: {1}")]
    DownloadFailed(&'static str, String),
    #[error("sing-box operation failed: {0}")]
    Operation(String),
    #[error("sbctl candidate health check failed: {0}")]
    SbctlHealth(String),
    #[error("sing-box candidate configuration check failed: {0}")]
    SingBoxCheck(String),
    #[error("service health check failed: {0}")]
    ServiceHealth(String),
    #[error("rollback failed: {0}")]
    Rollback(String),
    #[error(transparent)]
    Release(#[from] ReleaseError),
    #[error("release artifact storage failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Storage(#[from] ConfigError),
}

/// Reads and verifies a pinned release manifest. The Ed25519 signature is
/// verified before any URL or digest in the manifest is trusted, so a failed
/// signature never leads to a download or replacement.
pub fn read_manifest(path: &Path) -> Result<ReleaseManifest, UpdateError> {
    Ok(crate::release::verify_manifest(path)?)
}

/// The release manifest URL for a given host architecture. The `latest`
/// redirect is acceptable here because the fetched manifest pins fixed,
/// versioned artifact URLs and is signature-verified before any URL is trusted.
pub fn manifest_url_for_arch(arch: &str) -> String {
    format!(
        "https://github.com/xiaolingxiaoying/singbox-sub-me/releases/latest/download/manifest-{arch}.json"
    )
}

/// Host architecture translated to the release manifest's `amd64`/`arm64` keys.
pub fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "amd64",
    }
}

/// Downloads and signature-verifies the latest pinned release manifest for this
/// host. The Ed25519 signature is checked before any artifact URL is trusted,
/// so a failed signature never leads to a download or replacement.
pub fn fetch_latest_manifest() -> Result<ReleaseManifest, UpdateError> {
    let temporary = tempfile::NamedTempFile::new()?;
    let url = manifest_url_for_arch(host_arch());
    let output = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "--output",
        ])
        .arg(temporary.path())
        .arg(&url)
        .output()
        .map_err(|error| UpdateError::DownloadFailed("manifest", error.to_string()))?;
    if !output.status.success() {
        return Err(UpdateError::DownloadFailed(
            "manifest",
            curl_diagnostic("manifest", &output),
        ));
    }
    Ok(crate::release::verify_manifest(temporary.path())?)
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
    let download = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "15",
            "--max-time",
            "600",
            "--output",
        ])
        .arg(&temporary)
        .arg(url)
        .output()
        .map_err(|error| UpdateError::DownloadFailed(name, error.to_string()))?;
    if !download.status.success() {
        let _ = fs::remove_file(&temporary);
        return Err(UpdateError::DownloadFailed(
            name,
            curl_diagnostic(name, &download),
        ));
    }
    let result = verify_artifact(name, &temporary, &artifact.sha256);
    if result.is_ok() {
        fs::rename(&temporary, output)?;
        // Downloaded artifacts are executables; make them runnable so the
        // candidate health/config checks can invoke them.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(output, fs::Permissions::from_mode(0o755))?;
        }
    } else {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Builds the download-failure diagnostic: curl's stderr when it has content,
/// otherwise the plain exit status.
fn curl_diagnostic(name: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("curl exited with {}", output.status)
    } else {
        format!("curl failed for {name}: {stderr}")
    }
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
    verify_trusted(manifest)?;
    verify_sing_box_artifact(manifest, candidate)?;
    let _lock = store.acquire_operation_lock()?;
    let config = store.load()?;
    let server_config = fs::read_to_string(
        store
            .root()
            .join("var/lib/sbctl/artifacts/sing-box-server.json"),
    )
    .map_err(ConfigError::Storage)?;
    crate::subscription::check_sing_box_config(candidate, &server_config)
        .map_err(|error| UpdateError::SingBoxCheck(error.to_string()))?;

    let rollback = rollback_directory(store.root());
    let backup = backup(store, &rollback, &config)?;
    write_managed_binary(store, "usr/local/bin/sing-box", &fs::read(candidate)?)?;
    if let Err(error) = crate::lifecycle::restart_sing_box_service(store.root()) {
        restore(store, &backup)
            .map_err(|rollback_error| UpdateError::Rollback(rollback_error.to_string()))?;
        if let Err(rollback_error) = crate::lifecycle::restart_sing_box_service(store.root()) {
            eprintln!(
                "warning: the rollback restart failed too ({}); inspect \
                 `systemctl status sing-box.service` before retrying",
                rollback_error
            );
        }
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
    verify_trusted(manifest)?;
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
    let backup = backup(store, &rollback, &config)?;
    write_managed_binary(store, "usr/local/bin/sbctl", &fs::read(sbctl_candidate)?)?;
    write_managed_binary(
        store,
        "usr/local/bin/sing-box",
        &fs::read(sing_box_candidate)?,
    )?;

    if let Err(error) = crate::lifecycle::restart_services(store.root()) {
        restore(store, &backup)
            .map_err(|rollback_error| UpdateError::Rollback(rollback_error.to_string()))?;
        if let Err(rollback_error) = crate::lifecycle::restart_services(store.root()) {
            eprintln!(
                "warning: the rollback restart failed too ({}); inspect \
                 `systemctl status sing-box.service sbctl.service` before retrying",
                rollback_error
            );
        }
        return Err(UpdateError::ServiceHealth(error));
    }
    // Ensure the loaded configuration was valid before acknowledging the update.
    config.validate()?;
    Ok(rollback)
}

struct BackupEntry {
    relative: String,
    contents: Option<Vec<u8>>,
}

/// Every managed path an update transaction might touch, plus the pinned
/// certificate copy for Direct subscription mode, so a rollback point restores
/// binaries, configuration, artifacts, units, certificate references, and
/// accounting state together.
fn rollback_paths(config: &DeploymentConfig) -> Vec<String> {
    let mut paths = MANAGED_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    if config.subscription_mode == crate::config::SubscriptionMode::Direct {
        for name in ["fullchain.pem", "privkey.pem"] {
            paths.push(format!(
                "var/lib/sbctl/certificates/{}/{}",
                config.subscription_host, name
            ));
        }
    }
    paths
}

fn backup(
    store: &DeploymentStore,
    rollback: &Path,
    config: &DeploymentConfig,
) -> Result<Vec<BackupEntry>, UpdateError> {
    let mut entries = Vec::new();
    for relative in rollback_paths(config) {
        let source = store.root().join(&relative);
        let contents = match fs::read(&source) {
            Ok(contents) => {
                let backup_path = rollback.join(&relative);
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
        let path = store.root().join(&entry.relative);
        match &entry.contents {
            Some(contents) => {
                store.write_relative_locked(&entry.relative, contents)?;
                if entry.relative.starts_with("usr/local/bin/") {
                    set_executable(&path)?;
                }
            }
            None if path.exists() => fs::remove_file(path)?,
            None => {}
        }
    }
    Ok(())
}

/// Replaces a managed service binary atomically and keeps it executable.
/// `write_relative_locked` alone creates the new file 0600, so the executable
/// bit is restored explicitly here; on the live host the ownership enforcement
/// additionally re-asserts root:root 0755.
fn write_managed_binary(
    store: &DeploymentStore,
    relative: &str,
    contents: &[u8],
) -> Result<(), UpdateError> {
    store.write_relative_locked(relative, contents)?;
    set_executable(&store.root().join(relative))?;
    Ok(())
}

fn set_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn managed_binary_writes_and_restores_keep_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = tempfile::TempDir::new().expect("temporary root is created");
        let store = crate::config::DeploymentStore::new(fixture.path());
        let contents = b"updated sing-box";

        write_managed_binary(&store, "usr/local/bin/sing-box", contents)
            .expect("the binary is written");

        let path = fixture.path().join("usr/local/bin/sing-box");
        assert_eq!(fs::read(&path).expect("binary readable"), contents);
        let mode = fs::metadata(&path)
            .expect("binary metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o755,
            "an updated binary must stay root-executable, not 0600"
        );

        let backup = vec![BackupEntry {
            relative: "usr/local/bin/sing-box".to_owned(),
            contents: Some(b"known-good sing-box".to_vec()),
        }];
        restore(&store, &backup).expect("the rollback restore succeeds");
        let mode = fs::metadata(&path)
            .expect("binary metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o755,
            "a restored binary must stay executable"
        );
    }

    #[test]
    fn manifest_url_for_arch_uses_arch_key() {
        let url = manifest_url_for_arch("arm64");
        assert!(
            url.ends_with("manifest-arm64.json"),
            "unexpected url: {url}"
        );
        assert!(
            url.contains("releases/latest/download"),
            "unexpected url: {url}"
        );
    }

    #[test]
    fn host_arch_maps_to_supported_key() {
        let arch = host_arch();
        assert!(matches!(arch, "amd64" | "arm64"), "unexpected arch: {arch}");
    }
}
