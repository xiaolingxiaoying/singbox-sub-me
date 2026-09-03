use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use chrono::{Datelike, LocalResult, TimeZone, Timelike};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use x25519_dalek::{X25519_BASEPOINT_BYTES, x25519};

pub const CONFIG_RELATIVE_PATH: &str = "etc/sbctl/config.toml";
pub const STATE_RELATIVE_PATH: &str = "var/lib/sbctl/state.json";
const ARTIFACTS_RELATIVE_PATH: &str = "var/lib/sbctl/artifacts";
const ACME_WEBROOT_RELATIVE_PATH: &str = "var/lib/sbctl/acme-webroot";
const MIN_PROTOCOL_PORT: u16 = 10_000;
const MAX_PROTOCOL_PORT: u16 = 65_535;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DeploymentConfig {
    pub subscription_mode: SubscriptionMode,
    pub subscription_host: String,
    pub proxy_host: Option<String>,
    pub http_port: Option<u16>,
    #[serde(default)]
    pub subscription_listen_port: Option<u16>,
    /// Administrator contact for Direct-mode Certbot issuance; never printed.
    #[serde(default)]
    pub certbot_email: Option<String>,
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
    #[serde(default)]
    pub tuic: Option<TuicCredentials>,
    #[serde(default)]
    pub anytls: Option<AnytlsCredentials>,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TuicCredentials {
    pub listen_port: u16,
    pub uuid: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct AnytlsCredentials {
    pub listen_port: u16,
    pub password: String,
}

/// Optional administrator-selected listener ports for the Managed protocols.
/// A missing value keeps the existing random high-port allocation behavior.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProtocolPorts {
    pub vless_reality: Option<u16>,
    pub vmess_websocket: Option<u16>,
    pub hysteria2: Option<u16>,
    pub tuic: Option<u16>,
    pub anytls: Option<u16>,
}

/// The complete set of administrator-selected deployment choices carried by the
/// interactive wizard and rebuilt into a `DeploymentConfig`. Optional fields
/// are `None` when the deployment does not use that feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentOptions {
    pub subscription_mode: SubscriptionMode,
    pub subscription_host: String,
    pub proxy_host: Option<String>,
    pub certbot_email: Option<String>,
    pub http_port: Option<u16>,
    pub subscription_listen_port: Option<u16>,
    pub interface: String,
    pub enabled_protocols: Vec<ManagedProtocol>,
    pub reality_decoy_sni: Option<String>,
    pub monthly_traffic_limit: u64,
    pub accounting_policy: AccountingPolicy,
    pub accounting_timezone: String,
    pub anchored_reset_at: Option<String>,
    pub ports: ProtocolPorts,
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
            Self::VlessReality | Self::VmessWebsocket | Self::Hysteria2 | Self::Tuic | Self::Anytls
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
        Self::new_with_ports(
            subscription_mode,
            subscription_host,
            proxy_host,
            http_port,
            interface,
            enabled_protocols,
            reality_decoy_sni,
            ProtocolPorts::default(),
        )
    }

    // Keep the compatibility constructor's positional shape explicit while the
    // protocol port bundle remains grouped in `ProtocolPorts`.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_ports(
        subscription_mode: SubscriptionMode,
        subscription_host: String,
        proxy_host: Option<String>,
        http_port: Option<u16>,
        interface: String,
        enabled_protocols: Vec<ManagedProtocol>,
        reality_decoy_sni: Option<String>,
        ports: ProtocolPorts,
    ) -> Result<Self, ConfigError> {
        let subscription_credential = generate_subscription_credential()?;
        let mut allocated_ports = Vec::new();
        validate_requested_ports(&enabled_protocols, &ports)?;
        let vless_reality = enabled_protocols
            .contains(&ManagedProtocol::VlessReality)
            .then(|| generate_vless_reality_credentials(&mut allocated_ports, ports.vless_reality))
            .transpose()?;
        let vmess_websocket = enabled_protocols
            .contains(&ManagedProtocol::VmessWebsocket)
            .then(|| {
                generate_vmess_websocket_credentials(&mut allocated_ports, ports.vmess_websocket)
            })
            .transpose()?;
        let hysteria2 = enabled_protocols
            .contains(&ManagedProtocol::Hysteria2)
            .then(|| generate_hysteria2_credentials(&mut allocated_ports, ports.hysteria2))
            .transpose()?;
        let tuic = enabled_protocols
            .contains(&ManagedProtocol::Tuic)
            .then(|| generate_tuic_credentials(&mut allocated_ports, ports.tuic))
            .transpose()?;
        let anytls = enabled_protocols
            .contains(&ManagedProtocol::Anytls)
            .then(|| generate_anytls_credentials(&mut allocated_ports, ports.anytls))
            .transpose()?;
        let subscription_listen_port =
            (subscription_mode == SubscriptionMode::ExternalProxy).then_some(2080);
        let config = Self {
            subscription_mode,
            subscription_host,
            proxy_host,
            http_port,
            subscription_listen_port,
            interface,
            enabled_protocols,
            reality_decoy_sni,
            subscription_credential,
            monthly_traffic_limit: 0,
            accounting_policy: AccountingPolicy::NaturalMonth,
            accounting_timezone: default_accounting_timezone(),
            anchored_reset_at: None,
            certbot_email: None,
            vless_reality,
            vmess_websocket,
            hysteria2,
            tuic,
            anytls,
        };
        config.validate()?;
        Ok(config)
    }

    /// Rebuilds a deployment from a complete set of administrator-selected
    /// options, preserving every existing Proxy credential and the Subscription
    /// credential when an existing deployment is being edited. Protocols that
    /// remain enabled keep their credentials (with an optionally changed port);
    /// newly enabled protocols receive fresh credentials; a fresh deployment
    /// allocates every credential and the Subscription credential.
    pub fn apply_options(
        existing: Option<&DeploymentConfig>,
        options: &DeploymentOptions,
    ) -> Result<Self, ConfigError> {
        let DeploymentOptions {
            subscription_mode,
            subscription_host,
            proxy_host,
            certbot_email,
            http_port,
            subscription_listen_port,
            interface,
            enabled_protocols,
            reality_decoy_sni,
            monthly_traffic_limit,
            accounting_policy,
            accounting_timezone,
            anchored_reset_at,
            ports,
        } = options;
        validate_requested_ports(enabled_protocols, ports)?;
        let mut allocated_ports = Vec::new();
        let vless_reality = build_protocol_credentials(
            existing.and_then(|config| config.vless_reality.as_ref()),
            enabled_protocols.contains(&ManagedProtocol::VlessReality),
            ports.vless_reality,
            &mut allocated_ports,
            generate_vless_reality_credentials,
            |credentials| credentials.listen_port,
            |credentials, port| credentials.listen_port = port,
        )?;
        let vmess_websocket = build_protocol_credentials(
            existing.and_then(|config| config.vmess_websocket.as_ref()),
            enabled_protocols.contains(&ManagedProtocol::VmessWebsocket),
            ports.vmess_websocket,
            &mut allocated_ports,
            generate_vmess_websocket_credentials,
            |credentials| credentials.listen_port,
            |credentials, port| credentials.listen_port = port,
        )?;
        let hysteria2 = build_protocol_credentials(
            existing.and_then(|config| config.hysteria2.as_ref()),
            enabled_protocols.contains(&ManagedProtocol::Hysteria2),
            ports.hysteria2,
            &mut allocated_ports,
            generate_hysteria2_credentials,
            |credentials| credentials.listen_port,
            |credentials, port| credentials.listen_port = port,
        )?;
        let tuic = build_protocol_credentials(
            existing.and_then(|config| config.tuic.as_ref()),
            enabled_protocols.contains(&ManagedProtocol::Tuic),
            ports.tuic,
            &mut allocated_ports,
            generate_tuic_credentials,
            |credentials| credentials.listen_port,
            |credentials, port| credentials.listen_port = port,
        )?;
        let anytls = build_protocol_credentials(
            existing.and_then(|config| config.anytls.as_ref()),
            enabled_protocols.contains(&ManagedProtocol::Anytls),
            ports.anytls,
            &mut allocated_ports,
            generate_anytls_credentials,
            |credentials| credentials.listen_port,
            |credentials, port| credentials.listen_port = port,
        )?;
        let subscription_credential = match existing {
            Some(config) => config.subscription_credential.clone(),
            None => generate_subscription_credential()?,
        };
        let config = Self {
            subscription_mode: subscription_mode.clone(),
            subscription_host: subscription_host.clone(),
            proxy_host: proxy_host.clone(),
            http_port: *http_port,
            subscription_listen_port: *subscription_listen_port,
            interface: interface.clone(),
            enabled_protocols: enabled_protocols.clone(),
            reality_decoy_sni: reality_decoy_sni.clone(),
            subscription_credential,
            monthly_traffic_limit: *monthly_traffic_limit,
            accounting_policy: accounting_policy.clone(),
            accounting_timezone: accounting_timezone.clone(),
            anchored_reset_at: anchored_reset_at.clone(),
            certbot_email: certbot_email.clone(),
            vless_reality,
            vmess_websocket,
            hysteria2,
            tuic,
            anytls,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn protocol_listener_port(&self, protocol: &ManagedProtocol) -> Option<u16> {
        match protocol {
            ManagedProtocol::VlessReality => self.vless_reality.as_ref().map(|node| node.listen_port),
            ManagedProtocol::VmessWebsocket => {
                self.vmess_websocket.as_ref().map(|node| node.listen_port)
            }
            ManagedProtocol::Hysteria2 => self.hysteria2.as_ref().map(|node| node.listen_port),
            ManagedProtocol::Tuic => self.tuic.as_ref().map(|node| node.listen_port),
            ManagedProtocol::Anytls => self.anytls.as_ref().map(|node| node.listen_port),
        }
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
        let listener_ports = self
            .vless_reality
            .as_ref()
            .map(|node| node.listen_port)
            .into_iter()
            .chain(self.vmess_websocket.as_ref().map(|node| node.listen_port))
            .chain(self.hysteria2.as_ref().map(|node| node.listen_port))
            .chain(self.tuic.as_ref().map(|node| node.listen_port))
            .chain(self.anytls.as_ref().map(|node| node.listen_port))
            .collect::<Vec<_>>();
        for (index, port) in listener_ports.iter().enumerate() {
            if !(MIN_PROTOCOL_PORT..=MAX_PROTOCOL_PORT).contains(port) {
                return Err(ConfigError::InvalidValue(
                    "Managed protocol ports must be in 10000-65535",
                ));
            }
            if listener_ports[..index].contains(port) {
                return Err(ConfigError::InvalidValue(
                    "Managed protocol ports must be unique across TCP and UDP",
                ));
            }
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
        validate_enabled_credentials(
            &self.enabled_protocols,
            ManagedProtocol::Tuic,
            self.tuic.is_some(),
            "TUIC requires generated node credentials",
        )?;
        validate_enabled_credentials(
            &self.enabled_protocols,
            ManagedProtocol::Anytls,
            self.anytls.is_some(),
            "AnyTLS requires generated node credentials",
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
        let accounting_timezone =
            self.accounting_timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|_| {
                    ConfigError::InvalidValue("accounting timezone must be a named IANA timezone")
                })?;
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
                let reset = chrono::NaiveDateTime::parse_from_str(reset_at, "%Y-%m-%dT%H:%M")
                    .map_err(|_| {
                        ConfigError::InvalidValue("anchored reset time must use YYYY-MM-DDTHH:MM")
                    })?;
                validate_anchored_reset_local_time(accounting_timezone, reset)?;
            }
            _ => {}
        }
        match self.subscription_mode {
            SubscriptionMode::IpFallback => {
                if self.subscription_listen_port.is_some() {
                    return Err(ConfigError::InvalidValue(
                        "only external reverse-proxy subscription configures a listener port",
                    ));
                }
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
                if self.protocol_listener_ports().contains(&port) {
                    return Err(ConfigError::InvalidValue(
                        "IP fallback HTTP port must not conflict with a Managed protocol port",
                    ));
                }
                if self.enabled_protocols.iter().any(|protocol| {
                    matches!(
                        protocol,
                        ManagedProtocol::VmessWebsocket
                            | ManagedProtocol::Hysteria2
                            | ManagedProtocol::Tuic
                            | ManagedProtocol::Anytls
                    )
                }) {
                    return Err(ConfigError::InvalidValue(
                        "VMess WebSocket, Hysteria2, TUIC, and AnyTLS require a domain subscription mode",
                    ));
                }
            }
            SubscriptionMode::Direct => {
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
                if self.subscription_listen_port.is_some() {
                    return Err(ConfigError::InvalidValue(
                        "only external reverse-proxy subscription configures a listener port",
                    ));
                }
            }
            SubscriptionMode::ExternalProxy => {
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
                let Some(port) = self.subscription_listen_port else {
                    return Err(ConfigError::InvalidValue(
                        "external reverse-proxy subscription requires a loopback listener port",
                    ));
                };
                if port <= 1024 {
                    return Err(ConfigError::InvalidValue(
                        "external reverse-proxy listener port must be higher than 1024",
                    ));
                }
                if self.protocol_listener_ports().contains(&port) {
                    return Err(ConfigError::InvalidValue(
                        "external reverse-proxy listener port must not conflict with a Managed protocol port",
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
        if let Some(port) = self.subscription_listen_port {
            lines.push(format!("loopback subscription port: {port}"));
        }
        if let Some(sni) = &self.reality_decoy_sni {
            lines.push(format!("Reality decoy SNI: {sni}"));
        }
        if let Some(reset_at) = &self.anchored_reset_at {
            lines.push(format!("anchored reset: {reset_at}"));
        }
        lines.join("\n")
    }

    fn protocol_listener_ports(&self) -> Vec<u16> {
        [
            self.vless_reality.as_ref().map(|node| node.listen_port),
            self.vmess_websocket.as_ref().map(|node| node.listen_port),
            self.hysteria2.as_ref().map(|node| node.listen_port),
            self.tuic.as_ref().map(|node| node.listen_port),
            self.anytls.as_ref().map(|node| node.listen_port),
        ]
        .into_iter()
        .flatten()
        .collect()
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

/// Rejects an anchored reset instant whose local time is skipped or repeated by
/// a DST transition in the accounting timezone, so the schedule is unambiguous.
fn validate_anchored_reset_local_time(
    timezone: chrono_tz::Tz,
    reset: chrono::NaiveDateTime,
) -> Result<(), ConfigError> {
    match timezone.with_ymd_and_hms(
        reset.year(),
        reset.month(),
        reset.day(),
        reset.hour(),
        reset.minute(),
        0,
    ) {
        LocalResult::Single(_) => Ok(()),
        LocalResult::Ambiguous(_, _) => Err(ConfigError::InvalidValue(
            "anchored reset time is ambiguous in the accounting timezone",
        )),
        LocalResult::None => Err(ConfigError::InvalidValue(
            "anchored reset time does not exist in the accounting timezone",
        )),
    }
}

fn validate_requested_ports(
    enabled_protocols: &[ManagedProtocol],
    ports: &ProtocolPorts,
) -> Result<(), ConfigError> {
    let requested = [
        (ManagedProtocol::VlessReality, ports.vless_reality),
        (ManagedProtocol::VmessWebsocket, ports.vmess_websocket),
        (ManagedProtocol::Hysteria2, ports.hysteria2),
        (ManagedProtocol::Tuic, ports.tuic),
        (ManagedProtocol::Anytls, ports.anytls),
    ];
    let mut values = Vec::new();
    for (protocol, port) in requested {
        if let Some(port) = port {
            if !enabled_protocols.contains(&protocol) {
                return Err(ConfigError::InvalidValue(
                    "cannot specify a port for a disabled Managed protocol",
                ));
            }
            if !(MIN_PROTOCOL_PORT..=MAX_PROTOCOL_PORT).contains(&port) {
                return Err(ConfigError::InvalidValue(
                    "Managed protocol ports must be in 10000-65535",
                ));
            }
            if values.contains(&port) {
                return Err(ConfigError::InvalidValue(
                    "Managed protocol ports must be unique across TCP and UDP",
                ));
            }
            values.push(port);
        }
    }
    Ok(())
}

/// A fresh high-entropy Subscription credential. `credential rotate` and new
/// deployments each call this so the URL-safe 256-bit secret is always generated
/// by the same path.
pub fn generate_subscription_credential() -> Result<String, ConfigError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Reuses an existing protocol credential when the protocol stays enabled,
/// changing only its listener port when requested, or generates a fresh one.
fn build_protocol_credentials<T, G>(
    existing: Option<&T>,
    enabled: bool,
    requested_port: Option<u16>,
    allocated_ports: &mut Vec<u16>,
    mut generate: G,
    get_port: impl Fn(&T) -> u16,
    set_port: impl Fn(&mut T, u16),
) -> Result<Option<T>, ConfigError>
where
    T: Clone,
    G: FnMut(&mut Vec<u16>, Option<u16>) -> Result<T, ConfigError>,
{
    if !enabled {
        return Ok(None);
    }
    if let Some(current) = existing {
        let current_port = get_port(current);
        let port = apply_existing_port(current_port, requested_port, allocated_ports)?;
        let mut updated = current.clone();
        set_port(&mut updated, port);
        return Ok(Some(updated));
    }
    generate(allocated_ports, requested_port).map(Some)
}

fn apply_existing_port(
    current_port: u16,
    requested_port: Option<u16>,
    allocated_ports: &mut Vec<u16>,
) -> Result<u16, ConfigError> {
    let port = match requested_port {
        Some(requested) if requested != current_port => requested,
        _ => current_port,
    };
    if allocated_ports.contains(&port) {
        return Err(ConfigError::InvalidValue(
            "Managed protocol ports must be unique across TCP and UDP",
        ));
    }
    if port != current_port {
        ensure_protocol_port_available(port)?;
    }
    allocated_ports.push(port);
    Ok(port)
}

fn generate_vless_reality_credentials(
    allocated_ports: &mut Vec<u16>,
    requested_port: Option<u16>,
) -> Result<VlessRealityCredentials, ConfigError> {
    let mut private = [0_u8; 32];
    let mut short_id = [0_u8; 8];
    getrandom::fill(&mut private).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    getrandom::fill(&mut short_id).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    let public = x25519(private, X25519_BASEPOINT_BYTES);
    let listen_port = allocate_port(allocated_ports, requested_port)?;
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
    requested_port: Option<u16>,
) -> Result<VmessWebsocketCredentials, ConfigError> {
    let mut path = [0_u8; 16];
    getrandom::fill(&mut path).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(VmessWebsocketCredentials {
        listen_port: allocate_port(allocated_ports, requested_port)?,
        uuid: generate_uuid()?,
        path: format!(
            "/{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(path)
        ),
    })
}

fn generate_hysteria2_credentials(
    allocated_ports: &mut Vec<u16>,
    requested_port: Option<u16>,
) -> Result<Hysteria2Credentials, ConfigError> {
    let mut password = [0_u8; 32];
    getrandom::fill(&mut password).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(Hysteria2Credentials {
        listen_port: allocate_udp_port(allocated_ports, requested_port)?,
        password: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(password),
    })
}

fn generate_tuic_credentials(
    allocated_ports: &mut Vec<u16>,
    requested_port: Option<u16>,
) -> Result<TuicCredentials, ConfigError> {
    let mut password = [0_u8; 32];
    getrandom::fill(&mut password).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(TuicCredentials {
        listen_port: allocate_udp_port(allocated_ports, requested_port)?,
        uuid: generate_uuid()?,
        password: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(password),
    })
}

fn generate_anytls_credentials(
    allocated_ports: &mut Vec<u16>,
    requested_port: Option<u16>,
) -> Result<AnytlsCredentials, ConfigError> {
    let mut password = [0_u8; 32];
    getrandom::fill(&mut password).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    Ok(AnytlsCredentials {
        listen_port: allocate_port(allocated_ports, requested_port)?,
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

fn allocate_port(
    allocated_ports: &mut Vec<u16>,
    requested_port: Option<u16>,
) -> Result<u16, ConfigError> {
    if let Some(port) = requested_port {
        if allocated_ports.contains(&port) {
            return Err(ConfigError::InvalidValue(
                "Managed protocol ports must be unique",
            ));
        }
        ensure_protocol_port_available(port)?;
        allocated_ports.push(port);
        return Ok(port);
    }
    loop {
        let port = random_protocol_port()?;
        if !allocated_ports.contains(&port) && ensure_protocol_port_available(port).is_ok() {
            allocated_ports.push(port);
            return Ok(port);
        }
    }
}

fn allocate_udp_port(
    allocated_ports: &mut Vec<u16>,
    requested_port: Option<u16>,
) -> Result<u16, ConfigError> {
    if let Some(port) = requested_port {
        if allocated_ports.contains(&port) {
            return Err(ConfigError::InvalidValue(
                "Managed protocol ports must be unique",
            ));
        }
        ensure_protocol_port_available(port)?;
        allocated_ports.push(port);
        return Ok(port);
    }
    loop {
        let port = random_protocol_port()?;
        if !allocated_ports.contains(&port) && ensure_protocol_port_available(port).is_ok() {
            allocated_ports.push(port);
            return Ok(port);
        }
    }
}

fn random_protocol_port() -> Result<u16, ConfigError> {
    let mut bytes = [0_u8; 4];
    getrandom::fill(&mut bytes).map_err(|error| ConfigError::Randomness(error.to_string()))?;
    let span = u32::from(MAX_PROTOCOL_PORT - MIN_PROTOCOL_PORT) + 1;
    Ok(MIN_PROTOCOL_PORT + (u32::from_le_bytes(bytes) % span) as u16)
}

fn ensure_protocol_port_available(port: u16) -> Result<(), ConfigError> {
    let tcp_available = std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port));
    if tcp_available.is_err() {
        return Err(ConfigError::PortUnavailable(port));
    }
    let udp_available = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port));
    if udp_available.is_err() {
        return Err(ConfigError::PortUnavailable(port));
    }
    Ok(())
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
    #[error("Managed protocol port {0} is already in use")]
    PortUnavailable(u16),
    #[error("configuration storage failed: {0}")]
    Storage(#[from] io::Error),
    #[error("could not update deployment state: {0}")]
    StateContent(String),
    #[error("VPS traffic state is corrupted: {0}")]
    StateCorrupt(String),
    #[error("VPS traffic state schema version {0} is not supported")]
    StateSchemaMismatch(u32),
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

    pub fn replace(&self, config: &DeploymentConfig) -> Result<(), ConfigError> {
        config.validate()?;
        let path = self.root.join(CONFIG_RELATIVE_PATH);
        let _lock = self.operation_lock()?;
        if !path.exists() {
            return Err(ConfigError::Missing);
        }
        let contents = toml::to_string_pretty(config)?;
        Ok(atomic_write(&path, contents.as_bytes())?)
    }

    /// Replaces the persisted configuration while an operation lock is already
    /// held. Multi-file lifecycle transactions use this so the configuration,
    /// artifacts, and active sing-box configuration commit together.
    pub fn replace_locked(&self, config: &DeploymentConfig) -> Result<(), ConfigError> {
        config.validate()?;
        let path = self.root.join(CONFIG_RELATIVE_PATH);
        if !path.exists() {
            return Err(ConfigError::Missing);
        }
        let contents = toml::to_string_pretty(config)?;
        Ok(atomic_write(&path, contents.as_bytes())?)
    }

    pub fn write_state(&self, contents: &[u8]) -> Result<(), ConfigError> {
        self.atomic_write_managed(&self.root.join(STATE_RELATIVE_PATH), contents)
    }

    /// Read the complete accounting state without acquiring the operation lock.
    /// Writers commit via temporary file and atomic rename, so a concurrent
    /// reader observes either the previous or the next complete version.
    pub fn read_state(&self) -> Result<Option<Vec<u8>>, ConfigError> {
        self.read_state_unlocked()
    }

    pub fn update_state(
        &self,
        update: impl FnOnce(Option<Vec<u8>>) -> Result<Vec<u8>, ConfigError>,
    ) -> Result<(), ConfigError> {
        let _lock = self.operation_lock()?;
        let prior = self.read_state_unlocked()?;
        let contents = update(prior.clone())?;
        if prior.as_deref() == Some(contents.as_slice()) {
            return Ok(());
        }
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

    /// Replaces a cached artifact while an operation lock is already held.
    /// Callers use the locked write helpers only while the guard from
    /// `acquire_operation_lock` lives.
    pub fn write_artifact_locked(&self, name: &str, contents: &[u8]) -> Result<(), ConfigError> {
        self.write_artifact_unlocked(name, contents)
    }

    /// Replaces the configuration consumed by the sing-box systemd unit.
    /// Callers validate this exact content before invoking this operation.
    pub fn write_active_sing_box_config(&self, contents: &[u8]) -> Result<(), ConfigError> {
        self.atomic_write_managed(&self.root.join("etc/sing-box/config.json"), contents)
    }

    /// Serializes a multi-file lifecycle operation with configuration and state
    /// writers. Callers use the locked write helpers only while this guard lives.
    pub fn acquire_operation_lock(&self) -> Result<OperationLock, ConfigError> {
        self.operation_lock()
    }

    pub fn write_relative_locked(
        &self,
        relative: &str,
        contents: &[u8],
    ) -> Result<(), ConfigError> {
        let path = safe_managed_path(&self.root, relative)?;
        Ok(atomic_write(&path, contents)?)
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

pub struct OperationLock(File);

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

fn safe_managed_path(root: &Path, relative: &str) -> Result<PathBuf, ConfigError> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ConfigError::InvalidValue(
            "managed path must be a relative path",
        ));
    }
    Ok(root.join(path))
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

/// Public host-format check used by the interactive wizard's per-item
/// validation. Accepts a hostname or an IP address.
pub fn host_is_valid(value: &str) -> bool {
    validate_host("host", value).is_ok()
}

/// Public hostname-only check used for fields that cannot be an IP address,
/// such as the Reality decoy SNI.
pub fn hostname_is_valid(value: &str) -> bool {
    validate_hostname("hostname", value).is_ok()
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
    use std::net::TcpListener;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::{
        AccountingPolicy, DeploymentConfig, DeploymentOptions, DeploymentStore, ManagedProtocol,
        ProtocolPorts, SubscriptionMode,
    };

    #[test]
    fn read_state_returns_the_complete_version_or_none_without_writing() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());

        assert_eq!(
            store.read_state().expect("missing state reads as None"),
            None
        );
        store
            .write_state(b"complete state")
            .expect("state is committed");
        assert_eq!(
            store
                .read_state()
                .expect("complete state reads as Some")
                .as_deref(),
            Some(b"complete state".as_slice())
        );
    }

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

    #[test]
    fn requested_protocol_ports_are_preserved_in_the_generated_credentials() {
        let vless_port = free_port();
        let vmess_port = free_port();
        let hysteria2_port = free_port();
        let tuic_port = free_port();
        let anytls_port = free_port();
        let config = DeploymentConfig::new_with_ports(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![
                ManagedProtocol::VlessReality,
                ManagedProtocol::VmessWebsocket,
                ManagedProtocol::Hysteria2,
                ManagedProtocol::Tuic,
                ManagedProtocol::Anytls,
            ],
            Some("www.cloudflare.com".into()),
            ProtocolPorts {
                vless_reality: Some(vless_port),
                vmess_websocket: Some(vmess_port),
                hysteria2: Some(hysteria2_port),
                tuic: Some(tuic_port),
                anytls: Some(anytls_port),
            },
        )
        .expect("explicit protocol ports are valid");

        assert_eq!(config.vless_reality.unwrap().listen_port, vless_port);
        assert_eq!(config.vmess_websocket.unwrap().listen_port, vmess_port);
        assert_eq!(config.hysteria2.unwrap().listen_port, hysteria2_port);
        assert_eq!(config.tuic.unwrap().listen_port, tuic_port);
        assert_eq!(config.anytls.unwrap().listen_port, anytls_port);
    }

    #[test]
    fn requested_protocol_ports_reject_duplicates_and_disabled_protocols() {
        let duplicate = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality, ManagedProtocol::Hysteria2],
            Some("www.cloudflare.com".into()),
            ProtocolPorts {
                vless_reality: Some(12001),
                hysteria2: Some(12001),
                ..ProtocolPorts::default()
            },
        );
        assert!(duplicate.is_err());

        let disabled = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
            ProtocolPorts {
                vmess_websocket: Some(12002),
                ..ProtocolPorts::default()
            },
        );
        assert!(disabled.is_err());
    }

    #[test]
    fn requested_protocol_ports_reject_a_port_below_the_canonical_high_range() {
        let result = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
            ProtocolPorts {
                vless_reality: Some(5000),
                ..ProtocolPorts::default()
            },
        );

        assert!(matches!(
            result,
            Err(super::ConfigError::InvalidValue(
                "Managed protocol ports must be in 10000-65535"
            ))
        ));
    }

    #[test]
    fn a_hand_edited_config_rejects_an_out_of_range_or_duplicate_protocol_port() {
        let config = DeploymentConfig::new(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![ManagedProtocol::VlessReality, ManagedProtocol::Hysteria2],
            Some("www.cloudflare.com".into()),
        )
        .expect("a base deployment is valid");

        let mut low_port = config.clone();
        low_port.vless_reality.as_mut().unwrap().listen_port = 5000;
        assert!(matches!(
            low_port.validate(),
            Err(super::ConfigError::InvalidValue(
                "Managed protocol ports must be in 10000-65535"
            ))
        ));

        let mut duplicate = config.clone();
        let hysteria_port = duplicate.hysteria2.as_ref().unwrap().listen_port;
        duplicate.vless_reality.as_mut().unwrap().listen_port = hysteria_port;
        assert!(matches!(
            duplicate.validate(),
            Err(super::ConfigError::InvalidValue(
                "Managed protocol ports must be unique across TCP and UDP"
            ))
        ));
    }

    #[test]
    fn automatically_allocated_protocol_ports_are_in_the_upstream_high_port_range() {
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("automatic port allocation succeeds");

        let port = config.vless_reality.expect("VLESS node exists").listen_port;
        assert!((10000..=65535).contains(&port));
    }

    #[test]
    fn an_explicitly_requested_port_is_rejected_when_already_listening() {
        let listener = TcpListener::bind("0.0.0.0:0").expect("test listener binds");
        let port = listener
            .local_addr()
            .expect("test listener has an address")
            .port();
        if port < 10000 {
            return;
        }

        let result = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
            ProtocolPorts {
                vless_reality: Some(port),
                ..ProtocolPorts::default()
            },
        );

        assert!(matches!(
            result,
            Err(super::ConfigError::PortUnavailable(_))
        ));
    }

    fn free_port() -> u16 {
        loop {
            let listener = TcpListener::bind("127.0.0.1:0").expect("test port binds");
            let port = listener
                .local_addr()
                .expect("test port has an address")
                .port();
            if port >= 10000 {
                return port;
            }
        }
    }

    #[test]
    fn accounting_timezone_defaults_to_utc() {
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("a default deployment is valid");

        assert_eq!(config.accounting_timezone, "UTC");
    }

    #[test]
    fn anchored_reset_rejects_a_nonexistent_dst_local_time() {
        let config = anchored_reset_config("America/New_York", "2024-03-10T02:30")
            .expect("base anchored config is valid");

        assert!(matches!(
            config.validate(),
            Err(super::ConfigError::InvalidValue(
                "anchored reset time does not exist in the accounting timezone"
            ))
        ));
    }

    #[test]
    fn anchored_reset_rejects_an_ambiguous_dst_local_time() {
        let config = anchored_reset_config("America/New_York", "2024-11-03T01:30")
            .expect("base anchored config is valid");

        assert!(matches!(
            config.validate(),
            Err(super::ConfigError::InvalidValue(
                "anchored reset time is ambiguous in the accounting timezone"
            ))
        ));
    }

    #[test]
    fn anchored_reset_accepts_a_stable_dst_local_time() {
        let config = anchored_reset_config("America/New_York", "2024-06-15T09:30")
            .expect("a stable anchored reset is valid");

        assert!(config.validate().is_ok());
    }

    fn anchored_reset_config(
        timezone: &str,
        reset_at: &str,
    ) -> Result<DeploymentConfig, super::ConfigError> {
        let mut config = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
            ProtocolPorts::default(),
        )?;
        config.accounting_policy = AccountingPolicy::AnchoredMonth;
        config.accounting_timezone = timezone.to_owned();
        config.anchored_reset_at = Some(reset_at.to_owned());
        Ok(config)
    }

    #[test]
    fn apply_options_with_unchanged_values_preserves_the_existing_configuration() {
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            Some("198.51.100.9".into()),
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an existing deployment is valid");
        let source = config.clone();

        let rebuilt = DeploymentConfig::apply_options(
            Some(&config),
            &DeploymentOptions {
                subscription_mode: source.subscription_mode,
                subscription_host: source.subscription_host.clone(),
                proxy_host: source.proxy_host.clone(),
                certbot_email: source.certbot_email.clone(),
                http_port: source.http_port,
                subscription_listen_port: source.subscription_listen_port,
                interface: source.interface.clone(),
                enabled_protocols: source.enabled_protocols.clone(),
                reality_decoy_sni: source.reality_decoy_sni.clone(),
                monthly_traffic_limit: source.monthly_traffic_limit,
                accounting_policy: source.accounting_policy,
                accounting_timezone: source.accounting_timezone.clone(),
                anchored_reset_at: source.anchored_reset_at.clone(),
                ports: ProtocolPorts {
                    vless_reality: source.vless_reality.as_ref().map(|node| node.listen_port),
                    ..ProtocolPorts::default()
                },
            },
        )
        .expect("rebuilding with unchanged values is valid");

        assert_eq!(rebuilt, config);
    }

    #[test]
    fn apply_options_preserves_proxy_credentials_when_changing_only_a_port() {
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an existing deployment is valid");
        let source = config.clone();
        let current = config.vless_reality.clone().expect("VLESS node exists");
        let new_port = free_port();
        if new_port == current.listen_port {
            return;
        }

        let rebuilt = DeploymentConfig::apply_options(
            Some(&config),
            &DeploymentOptions {
                subscription_mode: source.subscription_mode,
                subscription_host: source.subscription_host.clone(),
                proxy_host: source.proxy_host.clone(),
                certbot_email: source.certbot_email.clone(),
                http_port: source.http_port,
                subscription_listen_port: source.subscription_listen_port,
                interface: source.interface.clone(),
                enabled_protocols: source.enabled_protocols.clone(),
                reality_decoy_sni: source.reality_decoy_sni.clone(),
                monthly_traffic_limit: source.monthly_traffic_limit,
                accounting_policy: source.accounting_policy,
                accounting_timezone: source.accounting_timezone.clone(),
                anchored_reset_at: source.anchored_reset_at.clone(),
                ports: ProtocolPorts {
                    vless_reality: Some(new_port),
                    ..ProtocolPorts::default()
                },
            },
        )
        .expect("a changed listener port is valid");

        let updated = rebuilt.vless_reality.expect("VLESS node remains enabled");
        assert_eq!(updated.listen_port, new_port);
        assert_eq!(updated.uuid, current.uuid);
        assert_eq!(updated.private_key, current.private_key);
        assert_eq!(updated.public_key, current.public_key);
        assert_eq!(updated.short_id, current.short_id);
    }

    #[test]
    fn apply_options_for_a_new_deployment_generates_fresh_credentials() {
        let rebuilt = DeploymentConfig::apply_options(
            None,
            &DeploymentOptions {
                subscription_mode: SubscriptionMode::Direct,
                subscription_host: "sub.example.test".into(),
                proxy_host: None,
                certbot_email: None,
                http_port: None,
                subscription_listen_port: None,
                interface: "ens3".into(),
                enabled_protocols: vec![
                    ManagedProtocol::VlessReality,
                    ManagedProtocol::VmessWebsocket,
                ],
                reality_decoy_sni: Some("www.cloudflare.com".into()),
                monthly_traffic_limit: 0,
                accounting_policy: AccountingPolicy::NaturalMonth,
                accounting_timezone: "UTC".into(),
                anchored_reset_at: None,
                ports: ProtocolPorts::default(),
            },
        )
        .expect("a fresh deployment from options is valid");

        assert_eq!(rebuilt.subscription_credential.len(), 43);
        assert!(rebuilt.vless_reality.is_some());
        assert!(rebuilt.vmess_websocket.is_some());
        assert!(rebuilt.hysteria2.is_none());
    }

    #[test]
    fn apply_options_keeps_the_existing_subscription_credential() {
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an existing deployment is valid");
        let credential = config.subscription_credential.clone();
        let source = config.clone();

        let rebuilt = DeploymentConfig::apply_options(
            Some(&config),
            &DeploymentOptions {
                subscription_mode: source.subscription_mode,
                subscription_host: source.subscription_host.clone(),
                proxy_host: source.proxy_host.clone(),
                certbot_email: source.certbot_email.clone(),
                http_port: source.http_port,
                subscription_listen_port: source.subscription_listen_port,
                interface: source.interface.clone(),
                enabled_protocols: source.enabled_protocols.clone(),
                reality_decoy_sni: source.reality_decoy_sni.clone(),
                monthly_traffic_limit: source.monthly_traffic_limit,
                accounting_policy: source.accounting_policy,
                accounting_timezone: source.accounting_timezone.clone(),
                anchored_reset_at: source.anchored_reset_at.clone(),
                ports: ProtocolPorts {
                    vless_reality: source.vless_reality.as_ref().map(|node| node.listen_port),
                    ..ProtocolPorts::default()
                },
            },
        )
        .expect("editing preserves the subscription credential");

        assert_eq!(rebuilt.subscription_credential, credential);
    }

    #[test]
    fn apply_options_rejects_mode_preconditions_before_returning_a_config() {
        let result = DeploymentConfig::apply_options(
            None,
            &DeploymentOptions {
                subscription_mode: SubscriptionMode::IpFallback,
                subscription_host: "203.0.113.7".into(),
                proxy_host: None,
                certbot_email: None,
                http_port: Some(2080),
                subscription_listen_port: None,
                interface: "ens3".into(),
                enabled_protocols: vec![
                    ManagedProtocol::VlessReality,
                    ManagedProtocol::VmessWebsocket,
                ],
                reality_decoy_sni: Some("www.cloudflare.com".into()),
                monthly_traffic_limit: 0,
                accounting_policy: AccountingPolicy::NaturalMonth,
                accounting_timezone: "UTC".into(),
                anchored_reset_at: None,
                ports: ProtocolPorts::default(),
            },
        );

        assert!(matches!(
            result,
            Err(super::ConfigError::InvalidValue(
                "VMess WebSocket, Hysteria2, TUIC, and AnyTLS require a domain subscription mode"
            ))
        ));
    }
}
