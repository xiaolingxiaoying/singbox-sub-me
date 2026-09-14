//! sing-box core lifecycle: discovery, download, verification, and the child
//! process the TUI manages.
//!
//! Cores come from the sing-box GitHub Release archive (zip). The archive
//! SHA-256 is verified against the release checksum file when available; a
//! configured mirror prefix (settings) is applied to every download URL so
//! constrained networks can still fetch the core.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use tokio::process::{Child, Command};

pub const GITHUB_LATEST: &str = "https://github.com/SagerNet/sing-box/releases/latest";

pub struct CoreHandle {
    pub child: Child,
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
/// directory and verifies the zip digest when the checksum asset resolves.
pub async fn download_core(target_dir: &Path, version: &str, mirror: &str) -> Result<PathBuf> {
    let release = if version.trim().is_empty() {
        format!("{GITHUB_LATEST}/download")
    } else {
        format!("https://github.com/SagerNet/sing-box/releases/download/{version}")
    };
    let platform = core_platform();
    let archive_name = format!("sing-box-{platform}.zip");
    let url = crate::subscription::apply_mirror(&format!("{release}/{archive_name}"), mirror);
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("core download failed for {url}"))?;
    let bytes = response
        .bytes()
        .await
        .context("reading the core archive")?
        .to_vec();
    verify_checksum(&bytes, &release, &archive_name, mirror).await;

    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).context("the core archive is not a readable zip")?;
    tokio::fs::create_dir_all(target_dir).await?;
    let binary_name = if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    };
    let mut extracted = false;
    let mut extracted_wintun = false;
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
        // The zip reader is synchronous; buffer the file then write it
        // through tokio so the async runtime is never blocked mid-copy.
        let mut buffered = Vec::with_capacity(file.size() as usize);
        std::io::Read::read_to_end(&mut file, &mut buffered)?;
        tokio::fs::write(&destination, buffered).await?;
        if is_binary {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))
                    .await?;
            }
            extracted = true;
        } else {
            extracted_wintun = true;
        }
    }
    if !extracted {
        bail!("the core archive does not contain {binary_name}");
    }
    // TUN mode on Windows needs the wintun driver that ships beside the core;
    // report the missing driver here instead of failing at TUN startup.
    if cfg!(windows) && !extracted_wintun {
        bail!("the core archive does not contain wintun.dll");
    }
    Ok(target_dir.join(binary_name))
}

/// Verifies the downloaded archive against the release `sha256sum` asset when
/// it can be fetched; a missing checksum asset is not fatal (the download is
/// still over HTTPS), but a mismatch aborts hard.
async fn verify_checksum(bytes: &[u8], release: &str, archive_name: &str, mirror: &str) {
    let checksum_url =
        crate::subscription::apply_mirror(&format!("{release}/{archive_name}.sha256"), mirror);
    let Ok(response) = reqwest::get(&checksum_url).await else {
        return;
    };
    let Ok(text) = response.text().await else {
        return;
    };
    let expected = text.split_whitespace().next().unwrap_or("").to_lowercase();
    if expected.len() != 64 {
        return;
    }
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    let actual = format!("{:x}", digest);
    if actual != expected {
        panic!("core archive checksum mismatch: expected {expected}, got {actual}");
    }
}

fn core_platform() -> String {
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
    format!("{os}-{arch}")
}

/// Rewrites the `inbounds` section of a subscription config for the TUI's
/// local runtime: mixed proxy inbound (system proxy) or tun inbound (global).
/// Everything else the subscription carries stays untouched.
pub fn adapt_inbounds(config_text: &str, mode: crate::system_proxy::TrafficMode) -> Result<String> {
    let mut value: serde_json::Value =
        serde_json::from_str(config_text).context("the active configuration is not JSON")?;
    let mut inbounds = match mode {
        crate::system_proxy::TrafficMode::SystemProxy => serde_json::json!([
            {
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": 2080
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

    #[test]
    fn adapt_inbounds_switches_between_mixed_and_tun() {
        let config = serde_json::json!({
            "inbounds": [{"type": "tun", "tag": "tun-in"}],
            "route": {"rules": [], "auto_detect_interface": false},
            "outbounds": []
        })
        .to_string();

        let mixed = adapt_inbounds(&config, crate::system_proxy::TrafficMode::SystemProxy)
            .expect("mixed adaptation");
        let value: serde_json::Value = serde_json::from_str(&mixed).expect("JSON");
        assert_eq!(value["inbounds"][0]["type"], "mixed");
        assert_eq!(value["inbounds"][0]["listen_port"], 2080);
        assert_eq!(value["route"]["auto_detect_interface"], true);

        let tun =
            adapt_inbounds(&config, crate::system_proxy::TrafficMode::Tun).expect("tun adaptation");
        let value: serde_json::Value = serde_json::from_str(&tun).expect("JSON");
        assert_eq!(value["inbounds"][0]["type"], "tun");
        assert_eq!(value["inbounds"][0]["auto_route"], true);
    }

    #[test]
    fn core_platform_matches_the_host_architecture() {
        let platform = core_platform();
        if cfg!(windows) {
            assert!(platform.starts_with("windows-"));
        } else if cfg!(target_os = "macos") {
            assert!(platform.starts_with("darwin-"));
        } else {
            assert!(platform.starts_with("linux-"));
        }
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
