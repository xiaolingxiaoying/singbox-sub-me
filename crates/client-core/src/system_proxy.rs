//! System proxy switching and the traffic-mode model.
//!
//! Windows writes the WinINET user proxy (registry + refresh broadcast);
//! macOS uses `networksetup`; on Linux the desktop environment's proxy is
//! set via gsettings when available, otherwise the TUI prints the env-var
//! commands to apply manually.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How the core receives traffic: a local mixed inbound paired with the OS
/// proxy, or the tun inbound that takes over routing globally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrafficMode {
    #[default]
    SystemProxy,
    Tun,
}

impl TrafficMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::SystemProxy => "系统代理",
            Self::Tun => "TUN",
        }
    }
}

/// The mixed inbound port the system proxy points at.
pub const LOCAL_MIXED_PORT: u16 = 2080;
const PROXY_SERVER: &str = "127.0.0.1";

/// The OS proxy state captured before sbtui first enables its proxy, so
/// `disable` restores whatever the user had instead of always clearing it.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ProxyBackup {
    #[serde(default)]
    windows: Option<WindowsBackup>,
    #[serde(default)]
    macos: Option<MacosBackup>,
    #[serde(default)]
    linux: Option<LinuxBackup>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct WindowsBackup {
    enable: u32,
    server: String,
    overrides: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct MacosBackup {
    service: String,
    web_enabled: bool,
    web_server: String,
    web_port: String,
    secure_enabled: bool,
    secure_server: String,
    secure_port: String,
}

/// The GNOME proxy values captured on Linux; values keep gsettings' own
/// quoting (for example `'manual'`) so they can be written back verbatim.
#[derive(Debug, Default, Serialize, Deserialize)]
struct LinuxBackup {
    mode: String,
    http_host: String,
    http_port: String,
}

/// The backup lives in the calling client's own data directory. Sharing one
/// global path (the old behavior pinned it to the sbtui directory) let the
/// GUI overwrite the TUI's captured state and restore the wrong settings.
fn backup_path(dir: &Path) -> std::path::PathBuf {
    dir.join("cache/system-proxy-backup.json")
}

/// Whether a previous session captured the operating-system proxy state and
/// then died before restoring it. A leftover backup means the live proxy still
/// points at a mixed port that no longer exists, which silently breaks the
/// user's network while a fresh snapshot reports the proxy as off.
pub fn has_residual_backup(dir: &Path) -> bool {
    backup_path(dir).is_file()
}

/// Captures the pre-existing proxy state once (a later enable must not
/// overwrite the user's original settings with the client's own).
fn capture_backup(dir: &Path) {
    let path = backup_path(dir);
    if path.is_file() {
        return;
    }
    #[cfg(windows)]
    let backup = ProxyBackup {
        windows: capture_windows(),
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    let backup = ProxyBackup {
        macos: capture_macos(),
        ..Default::default()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let backup = ProxyBackup {
        linux: capture_linux(),
        ..Default::default()
    };
    #[cfg(not(any(windows, unix)))]
    let backup = ProxyBackup::default();
    if let Ok(text) = serde_json::to_string_pretty(&backup) {
        let _ = std::fs::write(&path, text);
    }
}

/// Restores the captured state and removes the backup. A missing or unreadable
/// backup is not an error (best effort).
fn restore_backup(dir: &Path) -> Result<()> {
    let path = backup_path(dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(backup) = serde_json::from_str::<ProxyBackup>(&text) else {
        return Ok(());
    };
    // Keep the fields referenced on every platform (the apply arms are
    // cfg-gated), so the deserialized value is never "unused".
    let _has_state = backup.windows.is_some() || backup.macos.is_some() || backup.linux.is_some();
    #[cfg(windows)]
    if let Some(window) = backup.windows.as_ref() {
        apply_windows(window)?;
    }
    #[cfg(target_os = "macos")]
    if let Some(macos) = backup.macos.as_ref() {
        apply_macos(macos)?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(linux) = backup.linux.as_ref() {
        apply_linux(linux)?;
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Applies the OS proxy to point at the local mixed inbound. `dir` is the
/// calling client's data directory, which keeps the captured backup scoped
/// to that client (the TUI and the GUI keep separate data directories).
pub fn enable(dir: &Path, port: u16) -> Result<()> {
    capture_backup(dir);
    set_proxy(&format!("{PROXY_SERVER}:{port}"))
}

/// Restores the OS proxy captured by [`enable`]; when no backup exists this is
/// the old behavior of clearing the proxy.
pub fn disable(dir: &Path) -> Result<()> {
    if backup_path(dir).is_file() {
        return restore_backup(dir);
    }
    set_proxy("")
}

#[cfg(windows)]
fn capture_windows() -> Option<WindowsBackup> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_READ,
        )
        .ok()?;
    Some(WindowsBackup {
        enable: settings.get_value("ProxyEnable").unwrap_or(0),
        server: settings.get_value("ProxyServer").unwrap_or_default(),
        overrides: settings.get_value("ProxyOverride").unwrap_or_default(),
    })
}

#[cfg(windows)]
fn apply_windows(backup: &WindowsBackup) -> Result<()> {
    use anyhow::Context;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_SET_VALUE,
        )
        .context("opening the WinINET registry key")?;
    settings
        .set_value("ProxyEnable", &backup.enable)
        .context("restoring ProxyEnable")?;
    if !backup.server.is_empty() {
        settings
            .set_value("ProxyServer", &backup.server)
            .context("restoring ProxyServer")?;
    }
    if !backup.overrides.is_empty() {
        settings
            .set_value("ProxyOverride", &backup.overrides)
            .context("restoring ProxyOverride")?;
    }
    refresh_wininet();
    Ok(())
}

#[cfg(target_os = "macos")]
fn capture_macos() -> Option<MacosBackup> {
    let service = default_network_service().ok()?;
    let (web_enabled, web_server, web_port) = networksetup_get("web", &service);
    let (secure_enabled, secure_server, secure_port) = networksetup_get("secureweb", &service);
    Some(MacosBackup {
        service,
        web_enabled,
        web_server,
        web_port,
        secure_enabled,
        secure_server,
        secure_port,
    })
}

#[cfg(target_os = "macos")]
fn networksetup_get(kind: &str, service: &str) -> (bool, String, String) {
    let flag = format!("-get{kind}proxy");
    let Ok(output) = std::process::Command::new("networksetup")
        .args([flag.as_str(), service])
        .output()
    else {
        return (false, String::new(), String::new());
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    (
        field("Enabled:").eq_ignore_ascii_case("yes"),
        field("Server:"),
        field("Port:"),
    )
}

#[cfg(target_os = "macos")]
fn apply_macos(backup: &MacosBackup) -> Result<()> {
    let set = |flag: &str, host: &str, port: &str| -> Result<()> {
        let status = std::process::Command::new("networksetup")
            .args([flag, &backup.service, host, port])
            .status()?;
        if !status.success() {
            anyhow::bail!("networksetup {flag} exited with {status}");
        }
        Ok(())
    };
    let off = |flag: &str| -> Result<()> {
        let status = std::process::Command::new("networksetup")
            .args([flag, &backup.service, "off"])
            .status()?;
        if !status.success() {
            anyhow::bail!("networksetup {flag} exited with {status}");
        }
        Ok(())
    };
    if backup.web_enabled {
        set("-setwebproxy", &backup.web_server, &backup.web_port)?;
    } else {
        off("-setwebproxystate")?;
    }
    if backup.secure_enabled {
        set(
            "-setsecurewebproxy",
            &backup.secure_server,
            &backup.secure_port,
        )?;
    } else {
        off("-setsecurewebproxystate")?;
    }
    Ok(())
}

#[cfg(windows)]
fn set_proxy(server: &str) -> Result<()> {
    use anyhow::Context;
    use winreg::RegKey;
    use winreg::enums::*;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let settings = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            KEY_SET_VALUE | KEY_READ,
        )
        .context("opening the WinINET registry key")?;
    let enabled = !server.is_empty();
    settings
        .set_value("ProxyEnable", &u32::from(enabled))
        .context("writing ProxyEnable")?;
    if enabled {
        settings
            .set_value("ProxyServer", &server)
            .context("writing ProxyServer")?;
        settings
            .set_value("ProxyOverride", &"<local>")
            .context("writing ProxyOverride")?;
    }
    // Ask WinINET to reload the settings so running browsers pick the change
    // up without a re-login.
    refresh_wininet();
    Ok(())
}

#[cfg(windows)]
fn refresh_wininet() {
    // InternetSetOption is declared manually to avoid a full windows-rs
    // dependency; the two options flush per-user proxy settings. wininet.dll
    // only exports the suffixed W/A variants, hence InternetSetOptionW.
    #[link(name = "wininet")]
    unsafe extern "system" {
        fn InternetSetOptionW(
            hinternet: *mut core::ffi::c_void,
            option: u32,
            buffer: *mut core::ffi::c_void,
            buffer_length: u32,
        ) -> i32;
    }
    const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
    const INTERNET_OPTION_REFRESH: u32 = 37;
    unsafe {
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null_mut(),
            0,
        );
        InternetSetOptionW(
            std::ptr::null_mut(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null_mut(),
            0,
        );
    }
}

#[cfg(target_os = "macos")]
fn set_proxy(server: &str) -> Result<()> {
    let service = default_network_service()?;
    let enabled = !server.is_empty();
    let args = if enabled {
        vec![
            "-setwebproxy".to_owned(),
            service.clone(),
            PROXY_SERVER.to_owned(),
            port_arg(server),
            "-setsecurewebproxy".to_owned(),
            service.clone(),
            PROXY_SERVER.to_owned(),
            port_arg(server),
        ]
    } else {
        vec![
            "-setwebproxystate".to_owned(),
            service.clone(),
            "off".to_owned(),
            "-setsecurewebproxystate".to_owned(),
            service,
            "off".to_owned(),
        ]
    };
    let status = std::process::Command::new("networksetup")
        .args(args)
        .status()?;
    if !status.success() {
        anyhow::bail!("networksetup exited with {status}");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn default_network_service() -> Result<String> {
    let output = std::process::Command::new("networksetup")
        .args(["-listallnetworkservices"])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .skip(1)
        .find(|line| line.contains("Wi-Fi") || line.contains("Ethernet"))
        .unwrap_or("Wi-Fi")
        .trim()
        .to_owned())
}

#[cfg(target_os = "macos")]
fn port_arg(server: &str) -> String {
    server.rsplit(':').next().unwrap_or(server).to_owned()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn set_proxy(server: &str) -> Result<()> {
    let enabled = !server.is_empty();
    let (host, port) = match server.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), port.to_owned()),
        None => (String::new(), String::new()),
    };
    let gsettings_status = |args: &[&str]| -> Result<bool> {
        let status = std::process::Command::new("gsettings").args(args).output();
        match status {
            Ok(output) => Ok(output.status.success()),
            Err(_) => Ok(false),
        }
    };
    if enabled
        && gsettings_status(&["set", "org.gnome.system.proxy", "mode", "'manual'"])?
        && gsettings_status(&[
            "set",
            "org.gnome.system.proxy.http",
            "host",
            &format!("'{host}'"),
        ])?
        && gsettings_status(&["set", "org.gnome.system.proxy.http", "port", &port])?
    {
        return Ok(());
    }
    if !enabled && gsettings_status(&["set", "org.gnome.system.proxy", "mode", "'none'"])? {
        return Ok(());
    }
    anyhow::bail!(
        "此桌面环境未自动应用系统代理；请手动设置环境变量：export http_proxy=http://{server} https_proxy=http://{server} all_proxy=socks5://{server}"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn capture_linux() -> Option<LinuxBackup> {
    let get = |schema: &str, key: &str| -> Option<String> {
        let output = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    Some(LinuxBackup {
        mode: get("org.gnome.system.proxy", "mode")?,
        http_host: get("org.gnome.system.proxy.http", "host")?,
        http_port: get("org.gnome.system.proxy.http", "port")?,
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn apply_linux(backup: &LinuxBackup) -> Result<()> {
    let set = |schema: &str, key: &str, value: &str| -> Result<()> {
        let status = std::process::Command::new("gsettings")
            .args(["set", schema, key, value])
            .status()?;
        if !status.success() {
            anyhow::bail!("gsettings set {schema} {key} exited with {status}");
        }
        Ok(())
    };
    if backup.mode == "'manual'" {
        set("org.gnome.system.proxy.http", "host", &backup.http_host)?;
        set("org.gnome.system.proxy.http", "port", &backup.http_port)?;
        set("org.gnome.system.proxy", "mode", "'manual'")?;
    } else {
        set("org.gnome.system.proxy", "mode", "'none'")?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn platform_label() -> &'static str {
    "Windows 注册表 + WinINET 刷新"
}

#[cfg(target_os = "macos")]
pub fn platform_label() -> &'static str {
    "macOS networksetup"
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn platform_label() -> &'static str {
    "Linux gsettings / 环境变量"
}

/// Whether this process can create the tun inbound. TUN needs administrator
/// (Windows) or root (Unix); the check is advisory — the core still reports
/// the definitive failure when it cannot open the device.
pub fn can_use_tun() -> bool {
    #[cfg(windows)]
    {
        // `net session` only succeeds for elevated processes.
        std::process::Command::new("net")
            .arg("session")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim() == "0")
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_mode_labels_are_distinct() {
        assert_ne!(TrafficMode::SystemProxy.label(), TrafficMode::Tun.label());
    }
}
