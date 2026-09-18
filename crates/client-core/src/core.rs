//! sing-box core lifecycle: discovery, download, verification, and the child
//! process the TUI manages.
//!
//! Cores come from the sing-box GitHub Release: a versioned archive
//! (`...-windows-<arch>.zip` on Windows, `...-linux-<arch>.tar.gz` /
//! `...-darwin-<arch>.tar.gz` elsewhere) fetched directly or through a
//! configured mirror prefix (settings).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};

pub struct CoreHandle {
    pub child: Child,
}

/// The result of a core download: where the binary was installed and the
/// SHA-256 of the release archive it came from.
pub struct CoreDownload {
    pub path: PathBuf,
    pub sha256: String,
}

/// The core's reported version (`sing-box version`), or an error when the
/// binary is missing or unreadable.
pub fn detect_version(core: &Path) -> Result<String> {
    let output = std::process::Command::new(core)
        .arg("version")
        .output()
        .context("running `sing-box version` failed")?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().next().unwrap_or("").trim().to_owned())
}

/// Validates a configuration with `sing-box check`.
pub fn check_config(core: &Path, config: &Path) -> Result<()> {
    let output = std::process::Command::new(core)
        .args(["check", "-c"])
        .arg(config)
        .output()
        .context("running `sing-box check` failed")?;
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "sing-box check rejected the configuration: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Starts the core with the activated configuration, logging to a file the
/// TUI can tail.
pub async fn start(core: &Path, config: &Path, log_path: &Path) -> Result<CoreHandle> {
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let log_file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .await
        .context("opening the core log file")?;
    let child = Command::new(core)
        .args(["run", "-c"])
        .arg(config)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file.into_std().await))
        .kill_on_drop(true)
        .spawn()
        .context("spawning the sing-box core")?;
    Ok(CoreHandle { child })
}

/// Exponential backoff for automatic core restarts after a crash: 2s, 4s,
/// 8s, 16s, then capped at 30s so a persistently failing core never spins.
pub fn restart_backoff(attempt: u32) -> std::time::Duration {
    let seconds = 2u64.saturating_pow((attempt + 1).min(5)).min(30);
    std::time::Duration::from_secs(seconds)
}

/// Downloads the sing-box release for the current platform into the data
/// directory. The release is resolved through the GitHub API (or the pinned
/// version) and the versioned asset is fetched: `...-windows-<arch>.zip` on
/// Windows, `...-linux-<arch>.tar.gz` / `...-darwin-<arch>.tar.gz` elsewhere.
pub async fn download_core(target_dir: &Path, version: &str, mirror: &str) -> Result<CoreDownload> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let tag = resolve_release_tag(&client, version).await?;
    let release = tag.trim_start_matches('v').to_owned();
    let (os, arch) = core_platform();
    let (archive_name, is_zip) = release_asset_name(&release, os, arch);
    let url = crate::subscription::apply_mirror(
        &format!("https://github.com/SagerNet/sing-box/releases/download/{tag}/{archive_name}"),
        mirror,
    );
    let bytes = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("core download failed for {url}"))?
        .bytes()
        .await
        .context("reading the core archive")?
        .to_vec();
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    tokio::fs::create_dir_all(target_dir).await?;
    verify_or_record_hash(target_dir, &tag, &sha256)?;
    let binary_name = core_binary_name();
    if is_zip {
        extract_zip_core(&bytes, target_dir, binary_name)?;
    } else {
        extract_tar_gz_core(&bytes, target_dir, binary_name)?;
    }
    Ok(CoreDownload {
        path: target_dir.join(binary_name),
        sha256,
    })
}

/// Trust-on-first-use integrity check over the downloaded archive. sing-box
/// publishes no official checksum file, so the first download of a release
/// tag can only be recorded; every later download of the same tag must
/// reproduce the recorded hash, which turns a mirror silently swapping the
/// archive into a loud failure instead of an unnoticed binary replacement.
/// The ledger (`verified-hashes.json`) stays human-readable so the recorded
/// hash can be compared against a copy of the release the user trusts.
fn verify_or_record_hash(target_dir: &Path, tag: &str, sha256: &str) -> Result<()> {
    let ledger_path = target_dir.join("verified-hashes.json");
    let mut ledger: BTreeMap<String, String> = std::fs::read_to_string(&ledger_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    match ledger.get(tag) {
        Some(recorded) if recorded != sha256 => bail!(
            "下载的 {tag} 内核 SHA-256 与首次记录不一致（{sha256} ≠ {recorded}）；\
             镜像返回的文件可能已被篡改，请更换镜像，或核对发布页后删除 {} 重试",
            ledger_path.display()
        ),
        Some(_) => Ok(()),
        None => {
            ledger.insert(tag.to_owned(), sha256.to_owned());
            std::fs::write(&ledger_path, serde_json::to_string_pretty(&ledger)?)?;
            Ok(())
        }
    }
}

/// The sing-box release tag to use: the pinned version normalized to `vX.Y.Z`,
/// or the latest release's tag fetched from the GitHub API.
async fn resolve_release_tag(client: &reqwest::Client, version: &str) -> Result<String> {
    let version = version.trim();
    if !version.is_empty() {
        return Ok(if version.starts_with('v') {
            version.to_owned()
        } else {
            format!("v{version}")
        });
    }
    let value: serde_json::Value = client
        .get("https://api.github.com/repos/SagerNet/sing-box/releases/latest")
        .header("User-Agent", "sbtui")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("requesting the latest sing-box release")?
        .error_for_status()
        .context("the latest sing-box release request failed")?
        .json()
        .await
        .context("the sing-box release response is not JSON")?;
    value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(str::to_owned)
        .context("the latest sing-box release has no tag_name")
}

fn core_binary_name() -> &'static str {
    if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

/// The sing-box release asset filename for a platform. Windows ships a
/// versioned `.zip`; Linux and macOS ship a versioned `.tar.gz`. The second
/// value reports whether the archive is a zip.
fn release_asset_name(release: &str, os: &str, arch: &str) -> (String, bool) {
    if os == "windows" {
        (format!("sing-box-{release}-windows-{arch}.zip"), true)
    } else {
        (format!("sing-box-{release}-{os}-{arch}.tar.gz"), false)
    }
}

fn extract_zip_core(bytes: &[u8], target_dir: &Path, binary_name: &str) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).context("the core archive is not a readable zip")?;
    let mut extracted = false;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_owned();
        let file_name = Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let is_binary = file_name == binary_name;
        let is_wintun = cfg!(windows) && file_name.eq_ignore_ascii_case("wintun.dll");
        if !is_binary && !is_wintun {
            continue;
        }
        let destination = target_dir.join(file_name);
        let mut buffered = Vec::with_capacity(file.size() as usize);
        std::io::Read::read_to_end(&mut file, &mut buffered)?;
        std::fs::write(&destination, buffered)?;
        if is_binary {
            mark_executable(&destination);
            extracted = true;
        }
    }
    if !extracted {
        bail!("the core archive does not contain {binary_name}");
    }
    Ok(())
}

fn extract_tar_gz_core(bytes: &[u8], target_dir: &Path, binary_name: &str) -> Result<()> {
    // The whole extraction happens synchronously and the archive is dropped
    // before the caller awaits again: `tar::Archive`/`Entry` are not `Send`,
    // and holding them across an await would make the engine future non-Send.
    let extracted = {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        let mut found: Option<(PathBuf, Vec<u8>)> = None;
        for entry in archive
            .entries()
            .context("the core archive is not a readable tar.gz")?
        {
            let mut entry = entry?;
            let path = entry
                .path()
                .context("the core archive has an invalid path")?
                .into_owned();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name != binary_name {
                continue;
            }
            let mut buffered = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut buffered)
                .context("reading the core binary")?;
            found = Some((target_dir.join(binary_name), buffered));
            break;
        }
        found
    };
    let Some((destination, buffered)) = extracted else {
        bail!("the core archive does not contain {binary_name}");
    };
    std::fs::write(&destination, buffered)?;
    mark_executable(&destination);
    Ok(())
}

fn mark_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn core_platform() -> (&'static str, &'static str) {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    (os, arch)
}

/// Rewrites the `inbounds` section of a subscription config for the TUI's
/// local runtime: mixed proxy inbound (system proxy) or tun inbound (global).
/// Everything else the subscription carries stays untouched.
pub fn adapt_inbounds(
    config_text: &str,
    mode: crate::system_proxy::TrafficMode,
    mixed_port: u16,
) -> Result<String> {
    let mut value: serde_json::Value =
        serde_json::from_str(config_text).context("the active configuration is not JSON")?;
    let mut inbounds = match mode {
        crate::system_proxy::TrafficMode::SystemProxy => serde_json::json!([
            {
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": mixed_port
            }
        ]),
        crate::system_proxy::TrafficMode::Tun => serde_json::json!([
            {
                "type": "tun",
                "tag": "tun-in",
                "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
                "mtu": 9000,
                "auto_route": true,
                "strict_route": true,
                "stack": "mixed"
            }
        ]),
    };
    if let Some(route) = value.get_mut("route").and_then(|r| r.as_object_mut()) {
        // A local runtime always needs the outbound interface autodetected;
        // tun additionally relies on it to avoid routing loops.
        route.insert(
            "auto_detect_interface".to_owned(),
            serde_json::Value::Bool(true),
        );
    }
    value["inbounds"] = std::mem::take(&mut inbounds);
    Ok(serde_json::to_string_pretty(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_zip_without_wintun_still_extracts_the_core() {
        use std::io::Write;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        archive
            .start_file("sing-box.exe", zip::write::SimpleFileOptions::default())
            .expect("start core entry");
        archive.write_all(b"test core").expect("write core entry");
        let bytes = archive.finish().expect("finish archive").into_inner();
        let target_dir = tempfile::tempdir().expect("temporary core directory");

        extract_zip_core(&bytes, target_dir.path(), "sing-box.exe")
            .expect("a system-proxy core download must not require wintun.dll");

        assert!(target_dir.path().join("sing-box.exe").is_file());
        assert!(!target_dir.path().join("wintun.dll").exists());
    }

    #[test]
    fn adapt_inbounds_switches_between_mixed_and_tun() {
        let config = serde_json::json!({
            "inbounds": [{"type": "tun", "tag": "tun-in"}],
            "route": {"rules": [], "auto_detect_interface": false},
            "outbounds": []
        })
        .to_string();

        let mixed = adapt_inbounds(&config, crate::system_proxy::TrafficMode::SystemProxy, 2080)
            .expect("mixed adaptation");
        let value: serde_json::Value = serde_json::from_str(&mixed).expect("JSON");
        assert_eq!(value["inbounds"][0]["type"], "mixed");
        assert_eq!(value["inbounds"][0]["listen_port"], 2080);
        assert_eq!(value["route"]["auto_detect_interface"], true);

        let tun = adapt_inbounds(&config, crate::system_proxy::TrafficMode::Tun, 1080)
            .expect("tun adaptation");
        let value: serde_json::Value = serde_json::from_str(&tun).expect("JSON");
        assert_eq!(value["inbounds"][0]["type"], "tun");
        assert_eq!(value["inbounds"][0]["auto_route"], true);
    }

    #[test]
    fn core_platform_matches_the_host_architecture() {
        let (os, arch) = core_platform();
        if cfg!(windows) {
            assert_eq!(os, "windows");
        } else if cfg!(target_os = "macos") {
            assert_eq!(os, "darwin");
        } else {
            assert_eq!(os, "linux");
        }
        assert!(matches!(arch, "amd64" | "arm64"), "unexpected arch {arch}");
    }

    #[test]
    fn release_asset_names_match_the_sing_box_release_assets() {
        assert_eq!(
            release_asset_name("1.14.0", "linux", "amd64"),
            ("sing-box-1.14.0-linux-amd64.tar.gz".to_owned(), false)
        );
        assert_eq!(
            release_asset_name("1.14.0", "darwin", "arm64"),
            ("sing-box-1.14.0-darwin-arm64.tar.gz".to_owned(), false)
        );
        assert_eq!(
            release_asset_name("1.14.0", "windows", "amd64"),
            ("sing-box-1.14.0-windows-amd64.zip".to_owned(), true)
        );
    }

    #[test]
    fn restart_backoff_grows_then_caps() {
        use std::time::Duration;
        assert_eq!(restart_backoff(0), Duration::from_secs(2));
        assert_eq!(restart_backoff(1), Duration::from_secs(4));
        assert_eq!(restart_backoff(2), Duration::from_secs(8));
        assert_eq!(restart_backoff(3), Duration::from_secs(16));
        assert_eq!(restart_backoff(4), Duration::from_secs(30));
        assert_eq!(restart_backoff(10), Duration::from_secs(30));
    }
}
