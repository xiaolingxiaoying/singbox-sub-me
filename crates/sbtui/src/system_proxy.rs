//! System proxy switching and the traffic-mode model.
//!
//! Windows writes the WinINET user proxy (registry + refresh broadcast);
//! macOS uses `networksetup`; on Linux the desktop environment's proxy is
//! set via gsettings when available, otherwise the TUI prints the env-var
//! commands to apply manually.

use anyhow::{Context, Result};

/// How the core receives traffic: a local mixed inbound paired with the OS
/// proxy, or the tun inbound that takes over routing globally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficMode {
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

/// Applies the OS proxy to point at the local mixed inbound.
pub fn enable(port: u16) -> Result<()> {
    set_proxy(&format!("{PROXY_SERVER}:{port}"))
}

/// Clears the OS proxy.
pub fn disable() -> Result<()> {
    set_proxy("")
}

#[cfg(windows)]
fn set_proxy(server: &str) -> Result<()> {
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
