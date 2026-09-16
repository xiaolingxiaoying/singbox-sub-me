//! Persisted settings and subscription profiles.
//!
//! Everything lives under one data directory (`%APPDATA%\sbtui` on Windows,
//! `~/.config/sbtui` elsewhere): `settings.toml` for application preferences
//! and `profiles.toml` for subscription archives. Missing files are created
//! with defaults on first run.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    /// Download mirror prefix for GitHub releases and subscriptions; empty
    /// means direct connectivity.
    #[serde(default)]
    pub mirror: String,
    /// Pin the sing-box core to this version ("v1.14.0"); empty tracks latest.
    #[serde(default)]
    pub core_version: String,
    /// Subscription auto-update interval in minutes; 0 disables it.
    #[serde(default)]
    pub auto_update_minutes: u64,
    /// The local mixed inbound port the system proxy points at.
    #[serde(default = "default_mixed_port")]
    pub mixed_port: u16,
    /// The latency-test probe URL used by clash_api delay requests.
    #[serde(default = "default_test_url")]
    pub test_url: String,
    /// Start the core automatically when the client launches.
    #[serde(default)]
    pub auto_start: bool,
    /// Enable the system proxy automatically once the core is healthy.
    #[serde(default = "default_true")]
    pub auto_system_proxy: bool,
    /// The traffic mode the next core start uses.
    #[serde(default)]
    pub traffic_mode: crate::system_proxy::TrafficMode,
}

fn default_mixed_port() -> u16 {
    crate::system_proxy::LOCAL_MIXED_PORT
}

fn default_test_url() -> String {
    crate::clash_api::DEFAULT_TEST_URL.to_owned()
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mirror: String::new(),
            core_version: String::new(),
            auto_update_minutes: 0,
            mixed_port: default_mixed_port(),
            test_url: default_test_url(),
            auto_start: false,
            auto_system_proxy: true,
            traffic_mode: crate::system_proxy::TrafficMode::SystemProxy,
        }
    }
}

impl Settings {
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let path = dir.join("settings.toml");
        if path.is_file() {
            let text = fs::read_to_string(&path).context("reading settings.toml")?;
            return toml::from_str(&text)
                .map_err(|e| anyhow::anyhow!("parsing settings.toml: {e}"));
        }
        let settings = Self::default();
        settings.save(dir)?;
        Ok(settings)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self)?;
        fs::write(dir.join("settings.toml"), text)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    pub name: String,
    /// The normalized sing-box-full subscription URL (or a plain JSON URL).
    pub url: String,
    /// Human-readable note of the original pasted link, if different.
    #[serde(default)]
    pub source: String,
    /// Epoch seconds of the last successful update; 0 = never.
    #[serde(default)]
    pub last_updated: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Profiles {
    pub active: Option<String>,
    #[serde(default = "Vec::new")]
    pub profiles: Vec<Profile>,
}

impl Profiles {
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let path = dir.join("profiles.toml");
        if path.is_file() {
            let text = fs::read_to_string(&path).context("reading profiles.toml")?;
            return toml::from_str(&text)
                .map_err(|e| anyhow::anyhow!("parsing profiles.toml: {e}"));
        }
        let profiles = Self::default();
        profiles.save(dir)?;
        Ok(profiles)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self)?;
        fs::write(dir.join("profiles.toml"), text)?;
        Ok(())
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        let active = self.active.as_deref()?;
        self.profiles.iter().find(|p| p.name == active)
    }

    /// Activates a profile by name and returns the cached config path.
    pub fn activate(&mut self, name: &str) -> Option<()> {
        self.profiles.iter().find(|p| p.name == name)?;
        self.active = Some(name.to_owned());
        Some(())
    }
}

/// The per-profile cached subscription body inside the data directory.
pub fn profile_cache_path(dir: &Path, name: &str) -> PathBuf {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("cache/{safe}.json"))
}

/// The sing-box core binary path inside the data directory.
pub fn core_path(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("core/sing-box.exe")
    } else {
        dir.join("core/sing-box")
    }
}

/// The wintun driver beside the core on Windows; TUN mode needs it present.
pub fn wintun_path(dir: &Path) -> PathBuf {
    dir.join("core/wintun.dll")
}

pub fn data_dir() -> Result<PathBuf> {
    data_dir_for("sbtui")
}

/// The data directory for one client application name. The TUI and GUI keep
/// separate profile/core copies so two clients on the same machine cannot
/// fight over the same sing-box process while still sharing the layout.
pub fn data_dir_for(app: &str) -> Result<PathBuf> {
    let base = dirs::config_dir().context("cannot resolve the user config directory")?;
    let dir = base.join(app);
    fs::create_dir_all(dir.join("cache")).context("creating the client data directory")?;
    fs::create_dir_all(dir.join("core")).context("creating the client core directory")?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_defaults() {
        let dir = tempfile_dir();
        let mut settings = Settings::load_or_create(&dir).expect("settings created");
        assert_eq!(settings.auto_update_minutes, 0);
        settings.auto_update_minutes = 60;
        settings.save(&dir).expect("settings saved");
        let loaded = Settings::load_or_create(&dir).expect("settings loaded");
        assert_eq!(loaded.auto_update_minutes, 60);
    }

    #[test]
    fn profile_cache_names_are_filesystem_safe() {
        let path = profile_cache_path(Path::new("/x"), "a/b:c");
        assert!(path.to_string_lossy().ends_with("a_b_c.json"));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sbtui-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
