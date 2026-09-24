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
use sha2::{Digest, Sha256};

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
            let profiles: Self =
                toml::from_str(&text).map_err(|e| anyhow::anyhow!("parsing profiles.toml: {e}"))?;
            profiles.migrate_caches(dir)?;
            return Ok(profiles);
        }
        let profiles = Self::default();
        profiles.save(dir)?;
        profiles.migrate_caches(dir)?;
        Ok(profiles)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let text = toml::to_string_pretty(self)?;
        fs::write(dir.join("profiles.toml"), text)?;
        Ok(())
    }

    /// Copy only unambiguous legacy caches. Never guess which profile owned a
    /// colliding file; retain originals so the user can recover/re-import them.
    fn migrate_caches(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir.join("cache/profiles"))?;
        for profile in &self.profiles {
            let old = legacy_profile_cache_path(dir, &profile.name);
            let new = profile_cache_path(dir, &profile.name);
            let key = old.to_string_lossy().to_lowercase();
            let owners = self
                .profiles
                .iter()
                .filter(|other| {
                    legacy_profile_cache_path(dir, &other.name)
                        .to_string_lossy()
                        .to_lowercase()
                        == key
                })
                .count();
            if owners == 1
                && !profile.name.eq_ignore_ascii_case("active-config")
                && !new.exists()
                && old.is_file()
            {
                fs::copy(old, new)?;
            }
        }
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
    // Full digest of the exact name also distinguishes case-only names on
    // case-insensitive filesystems. Profiles cannot currently be renamed.
    let id = format!("{:x}", Sha256::digest(name.as_bytes()));
    dir.join("cache/profiles").join(format!("{id}.json"))
}

/// The user's override file for one profile, or where it would live.
///
/// Deliberately outside `cache/`: the subscription cache is rewritten by every
/// update and cleared by profile maintenance, while an override is the user's
/// own work and must survive both. The digest is the same one
/// [`profile_cache_path`] uses, so a profile's two files always agree, and the
/// name stays filesystem-safe for a profile called `a/b:c`.
pub fn override_path(dir: &Path, name: &str) -> PathBuf {
    let id = format!("{:x}", Sha256::digest(name.as_bytes()));
    dir.join("overrides").join(format!("{id}.json"))
}

fn legacy_profile_cache_path(dir: &Path, name: &str) -> PathBuf {
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
/// separate profile/core copies; runtime API authentication additionally
/// isolates their child processes.
pub fn data_dir_for(app: &str) -> Result<PathBuf> {
    let base = dirs::config_dir().context("cannot resolve the user config directory")?;
    let dir = base.join(app);
    fs::create_dir_all(dir.join("cache/profiles")).context("creating the client data directory")?;
    fs::create_dir_all(dir.join("core")).context("creating the client core directory")?;
    Ok(dir)
}

/// Held for the lifetime of the process; dropping it releases the lock.
pub struct InstanceLock {
    _file: fs::File,
}

/// Takes the single-instance lock for one data directory, or `None` when
/// another instance already holds it. Both clients fight over global
/// resources (the mixed port, the operating-system proxy, the active runtime
/// configuration), so a second instance must not start. The lock is released by
/// the operating system when the holder dies, so a crash cannot wedge a client
/// out of its own directory.
pub fn acquire_instance_lock(dir: &Path) -> Result<Option<InstanceLock>> {
    use fs2::FileExt;
    let file = fs::OpenOptions::new()
        .create(true)
        // The lock is the whole content; never truncate a file another
        // instance may be holding.
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("client.lock"))
        .context("opening the instance lock file")?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(InstanceLock { _file: file })),
        Err(error) if lock_is_held(&error) => Ok(None),
        Err(error) => Err(anyhow::Error::new(error).context("locking the instance")),
    }
}

/// A contended advisory lock surfaces as `WouldBlock` on Unix, but fs2 hands
/// back the raw `ERROR_LOCK_VIOLATION` on Windows without normalizing it.
fn lock_is_held(error: &std::io::Error) -> bool {
    const ERROR_LOCK_VIOLATION: i32 = 33;
    error.kind() == std::io::ErrorKind::WouldBlock
        || error.raw_os_error() == Some(ERROR_LOCK_VIOLATION)
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
    fn a_second_instance_of_one_data_directory_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire_instance_lock(dir.path())
            .expect("first lock")
            .expect("the directory starts unlocked");
        assert!(
            acquire_instance_lock(dir.path())
                .expect("second attempt")
                .is_none(),
            "two instances must not share the mixed port, the OS proxy and the runtime config"
        );
        drop(first);
        assert!(
            acquire_instance_lock(dir.path())
                .expect("attempt after release")
                .is_some(),
            "the lock must die with its holder, not outlive it"
        );
    }

    #[test]
    fn profile_cache_names_are_filesystem_safe() {
        let path = profile_cache_path(Path::new("/x"), "a/b:c");
        assert_eq!(path.parent().unwrap(), Path::new("/x/cache/profiles"));
        assert_ne!(path, profile_cache_path(Path::new("/x"), "a_b_c"));
        assert_ne!(
            profile_cache_path(Path::new("/x"), "A"),
            profile_cache_path(Path::new("/x"), "a")
        );
        assert_ne!(
            profile_cache_path(Path::new("/x"), "active-config"),
            Path::new("/x/cache/active-config.json")
        );
    }

    /// The override file is addressed by the same identity as the cache, so
    /// "this profile's override" can never mean a different profile's file, and
    /// it lives outside the cache the subscription updates rewrite.
    #[test]
    fn override_paths_share_the_profile_identity_but_not_the_cache_directory() {
        let path = override_path(Path::new("/x"), "a/b:c");
        assert_eq!(path.parent().unwrap(), Path::new("/x/overrides"));
        let digest = path
            .file_name()
            .expect("a hashed file name")
            .to_string_lossy()
            .strip_suffix(".json")
            .expect("a JSON override file")
            .to_owned();
        assert_eq!(
            profile_cache_path(Path::new("/x"), "a/b:c")
                .file_name()
                .expect("a hashed file name")
                .to_string_lossy()
                .strip_suffix(".json")
                .expect("a cached profile"),
            digest,
            "the same profile name must hash to the same identity in both places"
        );
        assert_ne!(
            path,
            override_path(Path::new("/x"), "A/B:c"),
            "case-only differences distinguish profiles on case-insensitive filesystems"
        );
    }

    #[test]
    fn migration_preserves_unique_caches_but_does_not_guess_collisions() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("cache")).unwrap();
        let profiles = Profiles {
            active: None,
            profiles: ["a b", "a_b", "unique", "active-config"]
                .into_iter()
                .map(|name| Profile {
                    name: name.into(),
                    url: String::new(),
                    source: String::new(),
                    last_updated: 0,
                })
                .collect(),
        };
        profiles.save(dir.path()).unwrap();
        for name in ["a_b", "unique", "active-config"] {
            fs::write(legacy_profile_cache_path(dir.path(), name), "old").unwrap();
        }
        Profiles::load_or_create(dir.path()).unwrap();
        assert_eq!(
            fs::read_to_string(profile_cache_path(dir.path(), "unique")).unwrap(),
            "old"
        );
        for name in ["a b", "a_b", "active-config"] {
            assert!(!profile_cache_path(dir.path(), name).exists());
        }
        assert!(legacy_profile_cache_path(dir.path(), "a_b").exists());
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sbtui-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
