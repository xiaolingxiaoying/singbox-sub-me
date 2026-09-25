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
    #[error("official sing-box archive does not match GitHub's SHA-256 digest")]
    OfficialChecksumMismatch,
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
            "--proto",
            "=https",
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

/// The official sing-box project the data-plane kernel is downloaded from.
pub const SING_BOX_OFFICIAL_PROJECT_URL: &str = "https://github.com/SagerNet/sing-box";

const SING_BOX_OFFICIAL_RELEASE_API: &str =
    "https://api.github.com/repos/SagerNet/sing-box/releases/latest";

/// The archive asset name published by the official project for one version
/// and architecture, e.g. `sing-box-1.14.1-linux-amd64.tar.gz`.
fn official_archive_asset(version: &str) -> String {
    format!(
        "sing-box-{version}-linux-{}.tar.gz",
        official_release_arch()
    )
}

/// The official release-asset URL for one stable version. The version is
/// validated with the same strict parser the signed manifest uses, so a
/// pre-release tag or a malformed string can never reach a download URL.
pub fn official_sing_box_archive_url(version: &str) -> Result<String, UpdateError> {
    let parsed = crate::release::parse_version(version, "official sing-box version")
        .map_err(|error| UpdateError::Operation(error.to_string()))?;
    let [major, minor, patch] = parsed;
    let version = format!("{major}.{minor}.{patch}");
    Ok(format!(
        "{SING_BOX_OFFICIAL_PROJECT_URL}/releases/download/v{version}/{}",
        official_archive_asset(&version)
    ))
}

/// The host architecture translated to the official release asset naming
/// (`amd64`/`arm64`).
pub fn official_release_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => other,
    }
}

/// Resolves the latest stable sing-box version from the official project.
/// GitHub's `releases/latest` endpoint already excludes drafts and
/// pre-releases, so the returned tag is the latest stable version.
pub fn fetch_latest_official_sing_box_version() -> Result<String, UpdateError> {
    let release = fetch_official_release_json(SING_BOX_OFFICIAL_RELEASE_API)?;
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            UpdateError::DownloadFailed(
                "official sing-box release",
                "GitHub 发布 JSON 缺少 tag_name 字段".to_owned(),
            )
        })?;
    let version = tag.strip_prefix('v').unwrap_or(tag);
    crate::release::parse_version(version, "official sing-box version")
        .map_err(|error| UpdateError::Operation(error.to_string()))?;
    Ok(version.to_owned())
}

fn fetch_official_release_json(url: &str) -> Result<serde_json::Value, UpdateError> {
    let temporary = tempfile::NamedTempFile::new().map_err(|error| {
        UpdateError::DownloadFailed("official sing-box release", error.to_string())
    })?;
    let output = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "60",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "User-Agent: sbctl",
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            "--output",
        ])
        .arg(temporary.path())
        .arg(url)
        .output()
        .map_err(|error| {
            UpdateError::DownloadFailed(
                "official sing-box release",
                format!("{error}（需要 curl 命令与可用的 GitHub 访问）"),
            )
        })?;
    if !output.status.success() {
        return Err(UpdateError::DownloadFailed(
            "official sing-box release",
            curl_diagnostic("official sing-box release", &output),
        ));
    }
    serde_json::from_reader(fs::File::open(temporary.path())?).map_err(|error| {
        UpdateError::DownloadFailed(
            "official sing-box release",
            format!("GitHub 返回的不是有效的发布 JSON：{error}"),
        )
    })
}

fn official_release_api_url(version: &str) -> Result<String, UpdateError> {
    let parsed = crate::release::parse_version(version, "official sing-box version")
        .map_err(|error| UpdateError::Operation(error.to_string()))?;
    let [major, minor, patch] = parsed;
    Ok(format!(
        "https://api.github.com/repos/SagerNet/sing-box/releases/tags/v{major}.{minor}.{patch}"
    ))
}

fn fetch_official_archive_digest(version: &str) -> Result<Option<String>, UpdateError> {
    let url = official_release_api_url(version)?;
    let release = fetch_official_release_json(&url)?;
    parse_official_archive_digest(&release.to_string(), &official_archive_asset(version))
}

fn parse_official_archive_digest(
    release_json: &str,
    expected_asset_name: &str,
) -> Result<Option<String>, UpdateError> {
    let release: serde_json::Value = serde_json::from_str(release_json).map_err(|error| {
        UpdateError::DownloadFailed(
            "official sing-box release",
            format!("GitHub 返回的不是有效的发布 JSON：{error}"),
        )
    })?;
    let assets = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            UpdateError::DownloadFailed(
                "official sing-box release",
                "GitHub 发布 JSON 缺少 assets 列表".to_owned(),
            )
        })?;
    let asset = assets
        .iter()
        .find(|asset| {
            asset.get("name").and_then(serde_json::Value::as_str) == Some(expected_asset_name)
        })
        .ok_or_else(|| {
            UpdateError::DownloadFailed(
                "official sing-box release",
                format!("GitHub 发布中找不到预期资产 {expected_asset_name}"),
            )
        })?;
    let Some(digest) = asset.get("digest").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let digest = digest.as_str().ok_or_else(|| {
        UpdateError::DownloadFailed(
            "official sing-box release",
            "GitHub 资产 digest 字段不是字符串".to_owned(),
        )
    })?;
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(UpdateError::DownloadFailed(
            "official sing-box release",
            "GitHub 资产 digest 不是 sha256:<64 位十六进制>".to_owned(),
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdateError::DownloadFailed(
            "official sing-box release",
            "GitHub 资产 digest 不是 sha256:<64 位十六进制>".to_owned(),
        ));
    }
    Ok(Some(hex.to_ascii_lowercase()))
}

fn verify_official_archive_checksum(archive: &Path, expected: &str) -> Result<(), UpdateError> {
    let contents = fs::read(archive)?;
    let actual = format!("{:x}", Sha256::digest(contents));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(UpdateError::OfficialChecksumMismatch)
    }
}

/// Downloads the official sing-box release archive, checks GitHub's asset
/// SHA-256 digest when supplied, extracts the kernel binary, confirms it runs
/// and reports the requested version, and copies it to `output_bin`. The
/// official digest is an integrity check from GitHub, not an independent
/// publisher signature; signed manifests remain the stronger trust path.
pub fn download_sing_box_official(version: &str, output_bin: &Path) -> Result<(), UpdateError> {
    let url = official_sing_box_archive_url(version)?;
    let expected_digest = fetch_official_archive_digest(version)?;
    let archive = tempfile::Builder::new()
        .suffix(".tar.gz")
        .tempfile()
        .map_err(|error| UpdateError::DownloadFailed("sing-box", error.to_string()))?;
    let download = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "600",
            "--output",
        ])
        .arg(archive.path())
        .arg(&url)
        .output()
        .map_err(|error| {
            UpdateError::DownloadFailed(
                "sing-box",
                format!("{error}（需要 curl 命令与可用的 GitHub 访问）"),
            )
        })?;
    if !download.status.success() {
        return Err(UpdateError::DownloadFailed(
            "sing-box",
            curl_diagnostic("sing-box", &download),
        ));
    }
    if let Some(expected_digest) = expected_digest {
        verify_official_archive_checksum(archive.path(), &expected_digest)?;
    } else {
        eprintln!(
            "warning: GitHub 未提供 sing-box {} 资产的 SHA-256 摘要；将继续通过 HTTPS 下载并检查候选内核版本，但归档完整性未校验。需要签名验证时请使用固定版本 release manifest。",
            version
        );
    }
    let extracted = tempfile::tempdir().map_err(|error| {
        UpdateError::DownloadFailed("sing-box", format!("无法创建解压目录：{error}"))
    })?;
    let tar_status = Command::new("tar")
        .arg("-xzf")
        .arg(archive.path())
        .arg("-C")
        .arg(extracted.path())
        .status()
        .map_err(|error| {
            UpdateError::DownloadFailed(
                "sing-box",
                format!("解压官方发布包失败（需要 tar 命令，Debian/Ubuntu 自带）：{error}"),
            )
        })?;
    if !tar_status.success() {
        return Err(UpdateError::DownloadFailed(
            "sing-box",
            format!("tar 解压失败，退出码 {tar_status}；下载的发布包可能不完整"),
        ));
    }
    let candidate = extracted
        .path()
        .join(format!(
            "sing-box-{version}-linux-{}",
            official_release_arch()
        ))
        .join("sing-box");
    let candidate = candidate.as_path();
    confirm_sing_box_candidate(version, candidate)?;
    if let Some(parent) = output_bin.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(candidate, output_bin)?;
    set_executable(output_bin)?;
    Ok(())
}

/// Confirms the candidate kernel runs and identifies itself as `version`, so a
/// truncated or wrong-architecture archive is caught before installation.
fn confirm_sing_box_candidate(version: &str, candidate: &Path) -> Result<(), UpdateError> {
    let output = Command::new(candidate)
        .arg("version")
        .output()
        .map_err(|error| {
            UpdateError::SbctlHealth(format!(
                "下载的 sing-box 无法运行（架构不匹配或文件损坏）：{error}"
            ))
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.contains(version) {
        return Err(UpdateError::SbctlHealth(format!(
            "下载的 sing-box 自检失败：期望版本 {version}，实际输出 {}",
            stdout.trim()
        )));
    }
    Ok(())
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
            "--proto",
            download_protocols(url),
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

/// Manifest-supplied artifact URLs must be fetched over HTTPS. Loopback hosts
/// are the one exception: that traffic never leaves the machine and the
/// artifact's SHA-256 is pinned by the signed manifest, so a plaintext
/// download there cannot be swapped undetected. This is what lets the
/// integration tests drive the real download path against a local server.
fn download_protocols(url: &str) -> &'static str {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "=https";
    };
    if scheme != "http" {
        return "=https";
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Strip userinfo, then IPv6 brackets, then the port, before comparing.
    let mut host = authority.rsplit('@').next().unwrap_or_default();
    host = match host.strip_prefix('[') {
        Some(value) => value.split(']').next().unwrap_or(value),
        None => match host.rsplit_once(':') {
            // Only strip a real `host:port` tail; anything else is not a
            // host this check understands and stays HTTPS-only.
            Some((value, port))
                if !value.contains(':')
                    && !port.is_empty()
                    && port.bytes().all(|b| b.is_ascii_digit()) =>
            {
                value
            }
            _ => host,
        },
    };
    match host.to_ascii_lowercase().as_str() {
        "127.0.0.1" | "::1" | "localhost" => "=https,http",
        _ => "=https",
    }
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

/// Updates only the data-plane binary from a signed manifest. The existing
/// generated configuration is checked with the candidate before the managed
/// binary is replaced.
pub fn apply_sing_box(
    store: &DeploymentStore,
    manifest: &ReleaseManifest,
    candidate: &Path,
) -> Result<PathBuf, UpdateError> {
    verify_trusted(manifest)?;
    let verified = read_verified_artifact("sing-box", candidate, &manifest.sing_box.sha256)?;
    install_candidate_sing_box(store, candidate, &verified)
}

/// Installs an already-trusted candidate sing-box binary with the full
/// check → backup → replace → restart → rollback flow. Used by both the
/// signed-manifest update and the official-release update.
pub fn install_candidate_sing_box(
    store: &DeploymentStore,
    candidate: &Path,
    verified_contents: &[u8],
) -> Result<PathBuf, UpdateError> {
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

    let rollback = rollback_directory(store.root())?;
    let backup = backup(store, &rollback, &config)?;
    write_managed_binary(store, "usr/local/bin/sing-box", verified_contents)?;
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
    // Read each candidate exactly once and verify the in-memory bytes, so the
    // installed binary is the very buffer that was hashed — no window for the
    // file to be swapped between verification and installation.
    let sbctl_contents = read_verified_artifact("sbctl", sbctl_candidate, &manifest.sbctl.sha256)?;
    let sing_box_contents =
        read_verified_artifact("sing-box", sing_box_candidate, &manifest.sing_box.sha256)?;
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

    let rollback = rollback_directory(store.root())?;
    let backup = backup(store, &rollback, &config)?;
    write_managed_binary(store, "usr/local/bin/sbctl", &sbctl_contents)?;
    if let Err(error) = write_managed_binary(store, "usr/local/bin/sing-box", &sing_box_contents) {
        // The services were not restarted yet, so restoring the backup puts
        // the host back on the previous consistent pair of binaries.
        if let Err(rollback_error) = restore(store, &backup) {
            eprintln!(
                "warning: the automatic rollback failed ({}); restore the rollback point \
                 {} manually before retrying",
                rollback_error,
                rollback.display()
            );
        }
        return Err(error);
    }

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
                store.write_root_only_locked(backup_relative, &contents)?;
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

/// Rollback points live outside `var/lib/sbctl` on purpose: that whole tree is
/// chowned to the unprivileged sbctl account by the daemon-storage preparation,
/// and a rollback copy holds the data plane's credentials and the TLS private
/// key. `var/backups/sbctl` is root-only and is what an uninstall already uses
/// for the same reason. Public because failure messages point administrators at
/// it, and the black-box suite asserts against it rather than a copy.
pub const ROLLBACK_ROOT: &str = "var/backups/sbctl/rollback";

/// Each generation copies every managed path, including two multi-megabyte
/// binaries. Without a ceiling an update loop fills the disk, and every atomic
/// write and accounting save then fails behind it.
const ROLLBACK_GENERATIONS_KEPT: usize = 3;

fn rollback_directory(root: &Path) -> Result<PathBuf, UpdateError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let destination = root.join(ROLLBACK_ROOT).join(timestamp.to_string());
    fs::create_dir_all(&destination)?;
    crate::lifecycle::set_private_directory_permissions(&destination)
        .map_err(UpdateError::Operation)?;
    prune_rollback_history(root)?;
    Ok(destination)
}

/// Removes rollback points older than the kept window. Generation names are
/// nanoseconds since the epoch, so the name sorts as the time.
fn prune_rollback_history(root: &Path) -> Result<(), UpdateError> {
    let Ok(entries) = fs::read_dir(root.join(ROLLBACK_ROOT)) else {
        return Ok(());
    };
    let mut generations: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .collect();
    generations.sort_by_key(|entry| entry.file_name());
    let removable = generations.len().saturating_sub(ROLLBACK_GENERATIONS_KEPT);
    for stale in generations.iter().take(removable) {
        if let Err(error) = fs::remove_dir_all(stale.path()) {
            eprintln!(
                "warning: could not remove the old rollback point {}: {error}",
                stale.path().display()
            );
        }
    }
    Ok(())
}

fn verify_artifact(name: &'static str, path: &Path, expected: &str) -> Result<(), UpdateError> {
    let contents = fs::read(path)?;
    verify_bytes(name, &contents, expected)
}

/// Reads a candidate artifact once and verifies the in-memory bytes against
/// the pinned digest, returning them so the installation writes exactly the
/// verified buffer. The comparison is case-insensitive because the manifest
/// schema accepts uppercase hex digests.
fn read_verified_artifact(
    name: &'static str,
    path: &Path,
    expected: &str,
) -> Result<Vec<u8>, UpdateError> {
    let contents = fs::read(path)?;
    verify_bytes(name, &contents, expected)?;
    Ok(contents)
}

fn verify_bytes(name: &'static str, contents: &[u8], expected: &str) -> Result<(), UpdateError> {
    let actual = format!("{:x}", Sha256::digest(contents));
    (actual.eq_ignore_ascii_case(expected))
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

    #[test]
    fn official_release_asset_digest_selects_the_exact_archive_name() {
        let expected = format!("sha256:{:x}", Sha256::digest(b"official archive"));
        let release = serde_json::json!({
            "assets": [
                {"name": "other.tar.gz", "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"},
                {"name": "sing-box-1.14.1-linux-amd64.tar.gz", "digest": expected},
            ]
        });

        assert_eq!(
            parse_official_archive_digest(
                &release.to_string(),
                "sing-box-1.14.1-linux-amd64.tar.gz"
            )
            .expect("the exact official archive digest is parsed"),
            Some(format!("{:x}", Sha256::digest(b"official archive")))
        );
    }

    #[test]
    fn official_release_asset_digest_is_optional_but_malformed_values_fail_closed() {
        let missing = serde_json::json!({
            "assets": [{"name": "sing-box-1.14.1-linux-amd64.tar.gz"}]
        });
        assert_eq!(
            parse_official_archive_digest(
                &missing.to_string(),
                "sing-box-1.14.1-linux-amd64.tar.gz"
            )
            .expect("older GitHub API response may omit an asset digest"),
            None
        );

        for digest in ["sha512:abcd", "sha256:xyz", "sha256:abcd"] {
            let malformed = serde_json::json!({
                "assets": [{
                    "name": "sing-box-1.14.1-linux-amd64.tar.gz",
                    "digest": digest,
                }]
            });
            assert!(
                parse_official_archive_digest(
                    &malformed.to_string(),
                    "sing-box-1.14.1-linux-amd64.tar.gz"
                )
                .is_err(),
                "malformed digest must fail closed: {digest}"
            );
        }
    }

    #[test]
    fn official_archive_checksum_rejects_modified_downloads() {
        let archive = tempfile::NamedTempFile::new().expect("archive fixture is created");
        fs::write(archive.path(), b"modified archive").expect("archive fixture is written");

        let result = verify_official_archive_checksum(
            archive.path(),
            &format!("{:x}", Sha256::digest(b"official archive")),
        );

        assert!(result.is_err(), "a digest mismatch must reject the archive");
    }

    #[test]
    fn official_archive_checksum_accepts_the_matching_download() {
        let archive = tempfile::NamedTempFile::new().expect("archive fixture is created");
        fs::write(archive.path(), b"official archive").expect("archive fixture is written");

        verify_official_archive_checksum(
            archive.path(),
            &format!("{:x}", Sha256::digest(b"official archive")),
        )
        .expect("the expected official archive is accepted");
    }

    #[test]
    fn official_release_asset_digest_handles_null_and_missing_assets_explicitly() {
        let null_digest = serde_json::json!({
            "assets": [{
                "name": "sing-box-1.14.1-linux-amd64.tar.gz",
                "digest": null,
            }]
        });
        assert_eq!(
            parse_official_archive_digest(
                &null_digest.to_string(),
                "sing-box-1.14.1-linux-amd64.tar.gz"
            )
            .expect("null digest is treated as unavailable"),
            None
        );

        let wrong_asset = serde_json::json!({
            "assets": [{"name": "sing-box-1.14.1-linux-arm64.tar.gz", "digest": null}]
        });
        assert!(
            parse_official_archive_digest(
                &wrong_asset.to_string(),
                "sing-box-1.14.1-linux-amd64.tar.gz"
            )
            .is_err(),
            "a release that omits the expected architecture asset must fail"
        );
    }

    #[test]
    fn rollback_history_keeps_only_the_newest_generations() {
        let root = tempfile::tempdir().expect("fixture root");
        let base = root.path().join(ROLLBACK_ROOT);
        for generation in 1..=6 {
            fs::create_dir_all(base.join(format!("{generation:020}"))).expect("fixture generation");
        }

        prune_rollback_history(root.path()).expect("pruning a fixture tree");

        let kept: Vec<String> = fs::read_dir(&base)
            .expect("rollback root")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            kept.len(),
            ROLLBACK_GENERATIONS_KEPT,
            "an unbounded history fills the disk behind every later write"
        );
        assert!(kept.contains(&"00000000000000000006".to_owned()));
        assert!(!kept.contains(&"00000000000000000001".to_owned()));
    }

    #[test]
    fn rollback_history_of_an_unupdated_host_is_not_an_error() {
        let root = tempfile::tempdir().expect("fixture root");
        prune_rollback_history(root.path()).expect("a missing rollback root is fine");
    }

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

    #[test]
    fn download_protocols_allow_plain_http_only_on_loopback() {
        assert_eq!(
            download_protocols("https://example.test/sing-box"),
            "=https"
        );
        assert_eq!(download_protocols("http://example.test/sing-box"), "=https");
        assert_eq!(download_protocols("http://10.1.2.3/sing-box"), "=https");
        assert_eq!(download_protocols("http://192.168.0.9/x"), "=https");
        assert_eq!(
            download_protocols("http://[::ffff:127.0.0.1]/sing-box"),
            "=https",
            "IPv4-mapped addresses are not loopback literals"
        );
        assert_eq!(
            download_protocols("http://127.0.0.1:8080/sing-box"),
            "=https,http"
        );
        assert_eq!(
            download_protocols("http://localhost/sing-box"),
            "=https,http"
        );
        assert_eq!(
            download_protocols("http://[::1]:8080/sing-box"),
            "=https,http"
        );
        // A https URL must never be downgraded, and relative/odd input is
        // rejected conservatively.
        assert_eq!(download_protocols("https://localhost/sing-box"), "=https");
        assert_eq!(download_protocols("ftp://localhost/sing-box"), "=https");
        assert_eq!(download_protocols("localhost/sing-box"), "=https");
    }
}
