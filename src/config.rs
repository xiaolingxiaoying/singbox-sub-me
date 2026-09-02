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
use x25519_dalek::{X25519_BASEPOINT_BYTES, x25519};

const CONFIG_RELATIVE_PATH: &str = "etc/sbctl/config.toml";
const STATE_RELATIVE_PATH: &str = "var/lib/sbctl/state.json";
const ARTIFACTS_RELATIVE_PATH: &str = "var/lib/sbctl/artifacts";
const ACME_WEBROOT_RELATIVE_PATH: &str = "var/lib/sbctl/acme-webroot";
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
    #[serde(default)]
    pub monthly_traffic_limit: u64,
    #[serde(default)]
    pub accounting_policy: AccountingPolicy,
    #[serde(default = "default_accounting_timezone")]
    pub accounting_timezone: String,
    #[serde(default)]
    pub anchored_reset_at: Option<String>,
    #[serde(default)]
    pub vless_reality: Option<VlessRealityCredentials>,
    #[serde(default)]
    pub vmess_websocket: Option<VmessWebsocketCredentials>,
    #[serde(default)]
    pub hysteria2: Option<Hysteria2Credentials>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct VlessRealityCredentials {
    pub listen_port: u16,
    pub uuid: String,
    pub private_key: String,
    pub public_key: String,
    pub short_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct VmessWebsocketCredentials {
    pub listen_port: u16,
    pub uuid: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Hysteria2Credentials {
    pub listen_port: u16,
    pub password: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccountingPolicy {
    #[default]
    NaturalMonth,
    AnchoredMonth,
}

impl fmt::Display for AccountingPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NaturalMonth => "natural-month",
            Self::AnchoredMonth => "anchored-month",
        })
    }
}

fn default_accounting_timezone() -> String {
    "UTC".to_owned()
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

impl ManagedProtocol {
    pub fn has_generated_subscription_artifacts(&self) -> bool {
        matches!(
            self,
            Self::VlessReality | Self::VmessWebsocket | Self::Hysteria2
        )
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
        let mut allocated_ports = Vec::new();
        let vless_reality = enabled_protocols
            .contains(&ManagedProtocol::VlessReality)
            .then(|| generate_vless_reality_credentials(&mut allocated_ports))
            .transpose()?;
        let vmess_websocket = enabled_protocols
            .contains(&ManagedProtocol::VmessWebsocket)
            .then(|| generate_vmess_websocket_credentials(&mut allocated_ports))
            .transpose()?;
        let hysteria2 = enabled_protocols
            .contains(&ManagedProtocol::Hysteria2)
            .then(|| generate_hysteria2_credentials(&mut allocated_ports))
            .transpose()?;
        let config = Self {
            subscription_mode,
            subscription_host,
            proxy_host,
            http_port,
            interface,
            enabled_protocols,
            reality_decoy_sni,
            subscription_credential,
            monthly_traffic_limit: 0,
            accounting_policy: AccountingPolicy::NaturalMonth,
            accounting_timezone: default_accounting_timezone(),
            anchored_reset_at: None,
            vless_reality,
            vmess_websocket,
            hysteria2,
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
        validate_enabled_credentials(
            &self.enabled_protocols,
            ManagedProtocol::VmessWebsocket,
            self.vmess_websocket.is_some(),
            "VMess WebSocket requires generated node credentials",
        )?;
        validate_enabled_credentials(
            &self.enabled_protocols,
            ManagedProtocol::Hysteria2,
            self.hysteria2.is_some(),
            "Hysteria2 requires generated node credentials",
        )?;
        if self
            .enabled_protocols
            .contains(&ManagedProtocol::VlessReality)
            && self.vless_reality.is_none()
        {
            return Err(ConfigError::InvalidValue(
                "VLESS Reality requires generated node credentials",
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
        if self.accounting_timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(ConfigError::InvalidValue(
                "accounting timezone must be a named IANA timezone",
            ));
        }
        match self.accounting_policy {
            AccountingPolicy::NaturalMonth if self.anchored_reset_at.is_some() => {
                return Err(ConfigError::InvalidValue(
                    "Natural-month reset must not configure an anchored reset time",
                ));
            }
            AccountingPolicy::AnchoredMonth => {
                let Some(reset_at) = &self.anchored_reset_at else {
                    return Err(ConfigError::InvalidValue(
                        "Anchored-month reset requires an anchored reset date and time",
                    ));
                };
                if chrono::NaiveDateTime::parse_from_str(reset_at, "%Y-%m-%dT%H:%M").is_err() {
                    return Err(ConfigError::InvalidValue(
                        "anchored reset time must use YYYY-MM-DDTHH:MM",
                    ));
                }
            }
            _ => {}
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
                if self.enabled_protocols.iter().any(|protocol| {
                    matches!(
                        protocol,
                        ManagedProtocol::VmessWebsocket | ManagedProtocol::Hysteria2
                    )
                }) {
                    return Err(ConfigError::InvalidValue(
                        "VMess WebSocket and Hysteria2 require a domain subscription mode",
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
            format!(
                "monthly traffic limit: {} bytes",
                self.monthly_traffic_limit
            ),
            format!("accounting policy: {}", self.accounting_policy),
            format!("accounting timezone: {}", self.accounting_timezone),
            format!("enabled protocols: {protocols}"),
            "subscription credential: [redacted]".to_owned(),
        ];
        if let Some(port) = self.http_port {
            lines.push(format!("HTTP port: {port}"));
        }
        if let Some(sni) = &self.reality_decoy_sni {
            lines.push(format!("Reality decoy SNI: {sni}"));
        }
        if let Some(reset_at) = &self.anchored_reset_at {
            lines.push(format!("anchored reset: {reset_at}"));
        }
        lines.join("\n")
    }
}

fn validate_enabled_credentials(
    enabled_protocols: &[ManagedProtocol],
    protocol: ManagedProtocol,
    has_credentials: bool,
    message: &'static str,
) -> Result<(), ConfigError> {
    if enabled_protocols.contains(&protocol) && !has_credentials {
        return Err(ConfigError::InvalidValue(message));
    }
    Ok(())
}

fn generate_vless_reality_credentials(
    allocated_ports: &mut Vec<u16>,
) -> Result<VlessRealityCredentials, ConfigError> {
    let mut private = [0_u8; 32];
    let mut short_id = [0_u8; 8];
    getrandom::fill(&mut private).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    getrandom::fill(&mut short_id).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    let public = x25519(private, X25519_BASEPOINT_BYTES);
    let listen_port = allocate_port(allocated_ports)?;
    Ok(VlessRealityCredentials {
        listen_port,
        uuid: generate_uuid()?,
        private_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(private),
        public_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public),
        short_id: short_id.iter().map(|byte| format!("{byte:02x}")).collect(),
    })
}

fn generate_vmess_websocket_credentials(
    allocated_ports: &mut Vec<u16>,
) -> Result<VmessWebsocketCredentials, ConfigError> {
    let mut path = [0_u8; 16];
    getrandom::fill(&mut path).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(VmessWebsocketCredentials {
        listen_port: allocate_port(allocated_ports)?,
        uuid: generate_uuid()?,
        path: format!(
            "/{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path)
        ),
    })
}

fn generate_hysteria2_credentials(
    allocated_ports: &mut Vec<u16>,
) -> Result<Hysteria2Credentials, ConfigError> {
    let mut password = [0_u8; 32];
    getrandom::fill(&mut password).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(Hysteria2Credentials {
        listen_port: allocate_udp_port(allocated_ports)?,
        password: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(password),
    })
}

fn generate_uuid() -> Result<String, ConfigError> {
    let mut uuid = [0_u8; 16];
    getrandom::fill(&mut uuid).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        uuid[0],
        uuid[1],
        uuid[2],
        uuid[3],
        uuid[4],
        uuid[5],
        uuid[6],
        uuid[7],
        uuid[8],
        uuid[9],
        uuid[10],
        uuid[11],
        uuid[12],
        uuid[13],
        uuid[14],
        uuid[15]
    ))
}

fn allocate_port(allocated_ports: &mut Vec<u16>) -> Result<u16, ConfigError> {
    loop {
        let port = std::net::TcpListener::bind("[::]:0")
            .or_else(|_| std::net::TcpListener::bind("0.0.0.0:0"))?
            .local_addr()?
            .port();
        if !allocated_ports.contains(&port) {
            allocated_ports.push(port);
            return Ok(port);
        }
    }
}

fn allocate_udp_port(allocated_ports: &mut Vec<u16>) -> Result<u16, ConfigError> {
    loop {
        let port = std::net::UdpSocket::bind("[::]:0")
            .or_else(|_| std::net::UdpSocket::bind("0.0.0.0:0"))?
            .local_addr()?
            .port();
        if !allocated_ports.contains(&port) {
            allocated_ports.push(port);
            return Ok(port);
        }
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
    #[error("could not update deployment state: {0}")]
    StateContent(String),
}

pub struct DeploymentStore {
    root: PathBuf,
}

impl DeploymentStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn initialize(&self, config: &DeploymentConfig) -> Result<(), ConfigError> {
        self.initialize_with_artifacts(config, &[])
    }

    pub fn initialize_with_artifacts(
        &self,
        config: &DeploymentConfig,
        artifacts: &[(&str, &[u8])],
    ) -> Result<(), ConfigError> {
        config.validate()?;
        let path = self.root.join(CONFIG_RELATIVE_PATH);
        let _lock = self.operation_lock()?;
        if path.exists() {
            return Err(ConfigError::AlreadyExists);
        }
        for (name, contents) in artifacts {
            self.write_artifact_unlocked(name, contents)?;
        }
        if config.subscription_mode == SubscriptionMode::Direct {
            create_private_directory(
                &self
                    .root
                    .join(ACME_WEBROOT_RELATIVE_PATH)
                    .join(".well-known/acme-challenge"),
            )?;
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

    pub fn update_state(
        &self,
        update: impl FnOnce(Option<Vec<u8>>) -> Result<Vec<u8>, ConfigError>,
    ) -> Result<(), ConfigError> {
        let _lock = self.operation_lock()?;
        let contents = update(self.read_state_unlocked()?)?;
        Ok(atomic_write(
            &self.root.join(STATE_RELATIVE_PATH),
            &contents,
        )?)
    }

    fn read_state_unlocked(&self) -> Result<Option<Vec<u8>>, ConfigError> {
        match fs::read(self.root.join(STATE_RELATIVE_PATH)) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ConfigError::Storage(error)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn acme_webroot(&self) -> PathBuf {
        self.root.join(ACME_WEBROOT_RELATIVE_PATH)
    }

    pub fn write_artifact(&self, name: &str, contents: &[u8]) -> Result<(), ConfigError> {
        let _lock = self.operation_lock()?;
        self.write_artifact_unlocked(name, contents)
    }

    fn write_artifact_unlocked(&self, name: &str, contents: &[u8]) -> Result<(), ConfigError> {
        if name.is_empty() || Path::new(name).components().count() != 1 {
            return Err(ConfigError::InvalidValue(
                "artifact name must be a single file name",
            ));
        }
        Ok(atomic_write(
            &self.root.join(ARTIFACTS_RELATIVE_PATH).join(name),
            contents,
        )?)
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
