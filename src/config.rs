use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CONFIG_RELATIVE_PATH: &str = "etc/sbctl/config.toml";
const STATE_RELATIVE_PATH: &str = "var/lib/sbctl/state.json";
const ARTIFACTS_RELATIVE_PATH: &str = "var/lib/sbctl/artifacts";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DeploymentConfig {
    pub subscription_mode: SubscriptionMode,
    pub subscription_host: String,
    pub proxy_host: Option<String>,
    pub http_port: Option<u16>,
    pub interface: String,
    pub enabled_protocols: Vec<ManagedProtocol>,
    pub reality_decoy_sni: Option<String>,
    pub subscription_credential: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionMode {
    Direct,
    ExternalProxy,
    IpFallback,
}

impl fmt::Display for SubscriptionMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Direct => "direct",
            Self::ExternalProxy => "external-proxy",
            Self::IpFallback => "ip-fallback",
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedProtocol {
    VlessReality,
    VmessWebsocket,
    Hysteria2,
    Tuic,
    Anytls,
}

impl fmt::Display for ManagedProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::VlessReality => "vless-reality",
            Self::VmessWebsocket => "vmess-websocket",
            Self::Hysteria2 => "hysteria2",
            Self::Tuic => "tuic",
            Self::Anytls => "anytls",
        })
    }
}

impl DeploymentConfig {
    pub fn new(
        subscription_mode: SubscriptionMode,
        subscription_host: String,
        proxy_host: Option<String>,
        http_port: Option<u16>,
        interface: String,
        enabled_protocols: Vec<ManagedProtocol>,
        reality_decoy_sni: Option<String>,
    ) -> Result<Self, ConfigError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| ConfigError::Randomness(error.to_string()))?;
        let subscription_credential =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let config = Self {
            subscription_mode,
            subscription_host,
            proxy_host,
            http_port,
            interface,
            enabled_protocols,
            reality_decoy_sni,
            subscription_credential,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_host("subscription host", &self.subscription_host)?;
        if let Some(proxy_host) = &self.proxy_host {
            validate_host("proxy host", proxy_host)?;
        }
        if self.interface.is_empty()
            || self.interface.len() > 15
            || self.interface.bytes().any(|byte| {
                !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.')
            })
        {
            return Err(ConfigError::InvalidValue(
                "interface must be a Linux interface name",
            ));
        }
        if self.enabled_protocols.is_empty() {
            return Err(ConfigError::InvalidValue(
                "at least one Managed protocol must be enabled",
            ));
        }
        for (index, protocol) in self.enabled_protocols.iter().enumerate() {
            if self.enabled_protocols[..index].contains(protocol) {
                return Err(ConfigError::InvalidValue(
                    "enabled protocols must not contain duplicates",
                ));
            }
        }
        if self
            .enabled_protocols
            .contains(&ManagedProtocol::VlessReality)
            && self.reality_decoy_sni.as_deref().is_none_or(str::is_empty)
        {
            return Err(ConfigError::InvalidValue(
                "VLESS Reality requires a Reality decoy SNI",
            ));
        }
        if let Some(sni) = &self.reality_decoy_sni {
            validate_hostname("Reality decoy SNI", sni)?;
        }
        if self.subscription_credential.len() < 43
            || self
                .subscription_credential
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
        {
            return Err(ConfigError::InvalidValue(
                "subscription credential must be a URL-safe 256-bit secret",
            ));
        }
        match self.subscription_mode {
            SubscriptionMode::IpFallback => {
                if self.subscription_host.parse::<IpAddr>().is_err() {
                    return Err(ConfigError::InvalidValue(
                        "IP fallback subscription requires an IP address as the subscription host",
                    ));
                }
                let Some(port) = self.http_port else {
                    return Err(ConfigError::InvalidValue(
                        "IP fallback subscription requires an HTTP port",
                    ));
                };
                if port <= 1024 {
                    return Err(ConfigError::InvalidValue(
                        "IP fallback HTTP port must be higher than 1024",
                    ));
                }
            }
            SubscriptionMode::Direct | SubscriptionMode::ExternalProxy => {
                if self.subscription_host.parse::<IpAddr>().is_ok() {
                    return Err(ConfigError::InvalidValue(
                        "domain subscription modes require a hostname",
                    ));
                }
                if self.http_port.is_some() {
                    return Err(ConfigError::InvalidValue(
                        "only IP fallback subscription configures an HTTP port",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let protocols = self
            .enabled_protocols
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let mut lines = vec![
            "sbctl status: configured".to_owned(),
            format!("mode: {}", self.subscription_mode),
            format!("subscription host: {}", self.subscription_host),
            format!(
                "proxy host: {}",
                self.proxy_host
                    .as_deref()
                    .unwrap_or(&self.subscription_host)
            ),
            format!("interface: {}", self.interface),
            format!("enabled protocols: {protocols}"),
            "subscription credential: [redacted]".to_owned(),
        ];
        if let Some(port) = self.http_port {
            lines.push(format!("HTTP port: {port}"));
        }
        if let Some(sni) = &self.reality_decoy_sni {
            lines.push(format!("Reality decoy SNI: {sni}"));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid deployment configuration: {0}")]
    InvalidValue(&'static str),
    #[error("deployment configuration is not initialized")]
    Missing,
    #[error("deployment configuration already exists; refusing to overwrite it")]
    AlreadyExists,
    #[error("could not parse deployment configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("could not serialize deployment configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("could not obtain secure randomness: {0}")]
    Randomness(String),
    #[error("configuration storage failed: {0}")]
    Storage(#[from] io::Error),
}

pub struct DeploymentStore {
    root: PathBuf,
}

impl DeploymentStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn initialize(&self, config: &DeploymentConfig) -> Result<(), ConfigError> {
        config.validate()?;
        let path = self.root.join(CONFIG_RELATIVE_PATH);
        let _lock = self.operation_lock()?;
        if path.exists() {
            return Err(ConfigError::AlreadyExists);
        }
        let contents = toml::to_string_pretty(config)?;
        Ok(atomic_write(&path, contents.as_bytes())?)
    }

    pub fn load(&self) -> Result<DeploymentConfig, ConfigError> {
        let path = self.root.join(CONFIG_RELATIVE_PATH);
        let contents = fs::read_to_string(path).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => ConfigError::Missing,
            _ => ConfigError::Storage(error),
        })?;
        let config = toml::from_str::<DeploymentConfig>(&contents)?;
        config.validate()?;
        Ok(config)
    }

    pub fn write_state(&self, contents: &[u8]) -> Result<(), ConfigError> {
        self.atomic_write_managed(&self.root.join(STATE_RELATIVE_PATH), contents)
    }

    pub fn write_artifact(&self, name: &str, contents: &[u8]) -> Result<(), ConfigError> {
        if name.is_empty() || Path::new(name).components().count() != 1 {
            return Err(ConfigError::InvalidValue(
                "artifact name must be a single file name",
            ));
        }
        self.atomic_write_managed(
            &self.root.join(ARTIFACTS_RELATIVE_PATH).join(name),
            contents,
        )
    }

    fn atomic_write_managed(&self, path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
        let _lock = self.operation_lock()?;
        Ok(atomic_write(path, contents)?)
    }

    fn operation_lock(&self) -> Result<OperationLock, ConfigError> {
        Ok(OperationLock::acquire(&self.root.join("var/lib/sbctl"))?)
    }
}

struct OperationLock(File);

impl OperationLock {
    fn acquire(directory: &Path) -> io::Result<Self> {
        create_private_directory(directory)?;
        let lock = private_open(&directory.join(".operation.lock"), false)?;
        lock.lock_exclusive()?;
        Ok(Self(lock))
    }
}

impl Drop for OperationLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().expect("managed path has parent");
    create_private_directory(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("artifact"),
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = private_open(&temporary, true)?;
    let result = (|| {
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        // The rename is the linearization point: readers now see the complete new
        // version. A failed directory sync can affect crash durability but cannot
        // be reported as a failed commit without falsely claiming the old version
        // remains active.
        let _ = sync_directory(parent);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn private_open(path: &Path, create_new: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn create_private_directory(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn sync_directory(_directory: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(_directory)?.sync_all()?;
    Ok(())
}

fn validate_host(label: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    validate_hostname(label, value)
}

fn validate_hostname(label: &'static str, value: &str) -> Result<(), ConfigError> {
    let valid = !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label_part| {
            !label_part.is_empty()
                && label_part.len() <= 63
                && !label_part.starts_with('-')
                && !label_part.ends_with('-')
                && label_part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        });
    valid
        .then_some(())
        .ok_or(ConfigError::InvalidValue(match label {
            "subscription host" => "subscription host must be a valid hostname or IP address",
            "proxy host" => "proxy host must be a valid hostname or IP address",
            _ => "Reality decoy SNI must be a valid hostname",
        }))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::DeploymentStore;

    #[test]
    fn state_replacement_exposes_only_the_complete_new_artifact() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());

        store
            .write_state(b"first complete state")
            .expect("first state is committed");
        store
            .write_state(b"second complete state")
            .expect("second state atomically replaces the first");

        assert_eq!(
            fs::read(fixture.path().join("var/lib/sbctl/state.json"))
                .expect("the complete state file is readable"),
            b"second complete state"
        );
    }

    #[test]
    fn concurrent_reads_observe_only_complete_state_versions() {
        let fixture = TempDir::new().expect("temporary root is created");
        let root = fixture.path().to_path_buf();
        let store = DeploymentStore::new(&root);
        let first = vec![b'a'; 16 * 1024];
        let second = vec![b'b'; 16 * 1024];
        store
            .write_state(&first)
            .expect("initial state is committed");

        let start = Arc::new(Barrier::new(2));
        let writer_start = Arc::clone(&start);
        let writer_root = root.clone();
        let writer_first = first.clone();
        let writer_second = second.clone();
        let writer = thread::spawn(move || {
            let writer_store = DeploymentStore::new(writer_root);
            writer_start.wait();
            for index in 0..50 {
                let contents = if index % 2 == 0 {
                    &writer_first
                } else {
                    &writer_second
                };
                writer_store
                    .write_state(contents)
                    .expect("each complete state version is committed");
            }
        });

        start.wait();
        let state_path = root.join("var/lib/sbctl/state.json");
        for _ in 0..200 {
            let observed = fs::read(&state_path).expect("a complete state remains readable");
            assert!(
                observed == first || observed == second,
                "reader observed a partial or unexpected state"
            );
        }
        writer.join().expect("writer completes");
    }

    #[test]
    fn concurrent_reads_observe_only_complete_artifact_versions() {
        let fixture = TempDir::new().expect("temporary root is created");
        let root = fixture.path().to_path_buf();
        let store = DeploymentStore::new(&root);
        let first = vec![b'j'; 16 * 1024];
        let second = vec![b'k'; 16 * 1024];
        store
            .write_artifact("subscription.cache", &first)
            .expect("initial artifact is committed");

        let start = Arc::new(Barrier::new(2));
        let writer_start = Arc::clone(&start);
        let writer_root = root.clone();
        let writer_first = first.clone();
        let writer_second = second.clone();
        let writer = thread::spawn(move || {
            let writer_store = DeploymentStore::new(writer_root);
            writer_start.wait();
            for index in 0..50 {
                let contents = if index % 2 == 0 {
                    &writer_first
                } else {
                    &writer_second
                };
                writer_store
                    .write_artifact("subscription.cache", contents)
                    .expect("each complete artifact version is committed");
            }
        });

        start.wait();
        let artifact_path = root.join("var/lib/sbctl/artifacts/subscription.cache");
        for _ in 0..200 {
            let observed = fs::read(&artifact_path).expect("a complete artifact remains readable");
            assert!(
                observed == first || observed == second,
                "reader observed a partial or unexpected artifact"
            );
        }
        writer.join().expect("writer completes");
    }

    #[cfg(unix)]
    #[test]
    fn persistent_files_are_owner_readable_only() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        store
            .write_state(b"restricted state")
            .expect("state is committed");

        let permissions = fs::metadata(fixture.path().join("var/lib/sbctl/state.json"))
            .expect("state exists")
            .permissions()
            .mode();
        assert_eq!(permissions & 0o777, 0o600);
    }
}
