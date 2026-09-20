use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rcgen::{CertificateParams, DnType, KeyPair};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;

use crate::canonical::CanonicalNode;
use crate::config::{
    CertificateMode, ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol,
    SubscriptionMode,
};

const SING_BOX_ARTIFACT: &str = "subscription-sing-box.json";
const SING_BOX_FULL_ARTIFACT: &str = "subscription-sing-box-full.json";
const CLASH_ARTIFACT: &str = "subscription-clash.yaml";
const URI_ARTIFACT: &str = "subscription-uri.txt";
const BASE64_URI_ARTIFACT: &str = "subscription-base64-uri.txt";
const SHADOWROCKET_ARTIFACT: &str = "subscription-shadowrocket.txt";
const SING_BOX_SERVER_ARTIFACT: &str = "sing-box-server.json";
const ARTIFACTS_RELATIVE_DIR: &str = "var/lib/sbctl/artifacts";
const ACTIVE_CONFIG_RELATIVE_PATH: &str = "etc/sing-box/config.json";

/// A client application version that a versioned subscription profile targets,
/// rendered as `1.12` in route paths (`sing-box-1.12.json`) and artifact names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClientVersion {
    pub major: u8,
    pub minor: u8,
}

impl ClientVersion {
    pub const fn new(major: u8, minor: u8) -> Self {
        Self { major, minor }
    }
}

impl fmt::Display for ClientVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionFormat {
    /// The historical bare-`outbounds` sing-box artifact; kept byte-compatible
    /// for existing clients.
    SingBox,
    /// The full client configuration for the latest stable sing-box.
    SingBoxFull,
    /// The full client configuration tuned for one specific sing-box minor.
    SingBoxVersion(ClientVersion),
    Clash,
    /// The mihomo variant for the previous major line (1.18.x).
    ClashLegacy(ClientVersion),
    Uri,
    Base64Uri,
    /// Base64 URI list with Shadowrocket-friendly parameters and remarks.
    Shadowrocket,
}

impl SubscriptionFormat {
    pub fn path_name(self) -> String {
        match self {
            Self::SingBox => "sing-box.json".to_owned(),
            Self::SingBoxFull => "sing-box-full.json".to_owned(),
            Self::SingBoxVersion(version) => format!("sing-box-{version}.json"),
            Self::Clash => "clash.yaml".to_owned(),
            Self::ClashLegacy(version) => format!("clash-{version}.yaml"),
            Self::Uri => "uri".to_owned(),
            Self::Base64Uri => "uri.txt".to_owned(),
            Self::Shadowrocket => "shadowrocket.txt".to_owned(),
        }
    }

    pub fn artifact_name(self) -> Cow<'static, str> {
        match self {
            Self::SingBox => Cow::Borrowed(SING_BOX_ARTIFACT),
            Self::SingBoxFull => Cow::Borrowed(SING_BOX_FULL_ARTIFACT),
            Self::SingBoxVersion(version) => {
                Cow::Owned(format!("subscription-sing-box-{version}.json"))
            }
            Self::Clash => Cow::Borrowed(CLASH_ARTIFACT),
            Self::ClashLegacy(version) => Cow::Owned(format!("subscription-clash-{version}.yaml")),
            Self::Uri => Cow::Borrowed(URI_ARTIFACT),
            Self::Base64Uri => Cow::Borrowed(BASE64_URI_ARTIFACT),
            Self::Shadowrocket => Cow::Borrowed(SHADOWROCKET_ARTIFACT),
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::SingBox | Self::SingBoxFull | Self::SingBoxVersion(_) => {
                "application/json; charset=utf-8"
            }
            Self::Clash | Self::ClashLegacy(_) => "application/yaml; charset=utf-8",
            Self::Uri | Self::Base64Uri | Self::Shadowrocket => "text/plain; charset=utf-8",
        }
    }

    /// The Chinese label + audience from the subscription matrix, used by the
    /// CLI and the index page; falls back to the raw path name.
    pub fn display_label(self) -> String {
        subscription_matrix()
            .iter()
            .find(|info| info.format == self)
            .map(|info| format!("{}（{}）", info.label, info.audience))
            .unwrap_or_else(|| self.path_name())
    }
}

/// A parsed subscription URL target behind `/sub/<credential>/...`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionRoute {
    /// An artifact-backed subscription format.
    Format(SubscriptionFormat),
    /// A scannable QR code (SVG) of the given format's subscription URL.
    Qr(SubscriptionFormat),
    /// The human-readable Chinese overview page listing every link.
    Index,
}

/// One sing-box minor version that gets its own tuned full-client profile.
/// Field differences between profiles follow the upstream changelog research
/// recorded in `docs/research/sing-box-client-version-differences.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingBoxVersionProfile {
    pub version: ClientVersion,
    pub supported: &'static str,
    pub notes: &'static str,
    /// Pre-1.12 cores only accept the legacy DNS server format (address
    /// strings plus a top-level `dns.fakeip` object); 1.12+ requires the
    /// typed server objects this tool generates for them.
    pub typed_dns: bool,
    /// Route rule actions (`sniff`, `hijack-dns`) arrived in 1.11.0; 1.10
    /// needs the legacy inbound `sniff` field plus a special `dns` outbound.
    pub route_rule_actions: bool,
    /// The AnyTLS outbound was added in sing-box 1.12.0, so 1.10/1.11 client
    /// profiles cannot contain AnyTLS nodes at all.
    pub supports_anytls: bool,
    /// `cache_file.store_dns` (optimistic DNS caching) arrived in 1.14.0;
    /// older cores must not receive the field.
    pub supports_store_dns: bool,
}

pub const CLASH_LEGACY_VERSION: ClientVersion = ClientVersion::new(1, 18);

/// Every sing-box minor from 1.10 up to the latest stable release, each with a
/// dedicated `sing-box-<major>.<minor>.json` subscription artifact. Ordered
/// ascending; the last entry is also what `sing-box-full.json` targets.
pub const SING_BOX_VERSION_PROFILES: &[SingBoxVersionProfile] = &[
    SingBoxVersionProfile {
        version: ClientVersion::new(1, 10),
        supported: ">= 1.10.0, < 1.11.0",
        notes: "旧版 DNS 服务器格式与旧版 sniff/hijack-dns 写法；不支持 AnyTLS 节点（1.12 才加入）；无 store_dns 乐观 DNS 缓存",
        typed_dns: false,
        route_rule_actions: false,
        supports_anytls: false,
        supports_store_dns: false,
    },
    SingBoxVersionProfile {
        version: ClientVersion::new(1, 11),
        supported: ">= 1.11.0, < 1.12.0",
        notes: "旧版 DNS 服务器格式；不支持 AnyTLS 节点（1.12 才加入）；无 store_dns 乐观 DNS 缓存",
        typed_dns: false,
        route_rule_actions: true,
        supports_anytls: false,
        supports_store_dns: false,
    },
    SingBoxVersionProfile {
        version: ClientVersion::new(1, 12),
        supported: ">= 1.12.0, < 1.13.0",
        notes: "DNS 服务器对象格式（legacy 格式弃用）；geoip/geosite 字段已移除，改用 rule_set；tun 用 address 合并写法；无 store_dns 乐观 DNS 缓存（1.14 才加入）",
        typed_dns: true,
        route_rule_actions: true,
        supports_anytls: true,
        supports_store_dns: false,
    },
    SingBoxVersionProfile {
        version: ClientVersion::new(1, 13),
        supported: ">= 1.13.0, < 1.14.0",
        notes: "block/dns 特殊出站与 inbound sniff 字段已移除，统一使用路由规则动作；无 store_dns 乐观 DNS 缓存（1.14 才加入）",
        typed_dns: true,
        route_rule_actions: true,
        supports_anytls: true,
        supports_store_dns: false,
    },
    SingBoxVersionProfile {
        version: ClientVersion::new(1, 14),
        supported: ">= 1.14.0",
        notes: "legacy DNS 格式与 DNS 规则 outbound 项已移除；可用 cache_file.store_dns 乐观缓存；字段与服务端运行的最新稳定版一致",
        typed_dns: true,
        route_rule_actions: true,
        supports_anytls: true,
        supports_store_dns: true,
    },
];

/// The newest sing-box version profile; `sing-box-full.json` targets it.
pub fn latest_version_profile() -> &'static SingBoxVersionProfile {
    SING_BOX_VERSION_PROFILES
        .last()
        .expect("sing-box version registry is not empty")
}

/// One row of the subscription link matrix shown by `sbctl sub` and the index
/// page. Version rows are appended dynamically from `SING_BOX_VERSION_PROFILES`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionLinkInfo {
    pub format: SubscriptionFormat,
    /// Short Chinese name shown in tables.
    pub label: String,
    /// Which client the link is for.
    pub audience: String,
    /// Extra note, e.g. what the artifact contains.
    pub note: String,
}

/// The static (non-version) rows of the subscription link matrix.
fn static_matrix_rows() -> Vec<SubscriptionLinkInfo> {
    vec![
        SubscriptionLinkInfo {
            format: SubscriptionFormat::SingBox,
            label: "sing-box 精简配置".to_owned(),
            audience: "sing-box（全版本兼容）".to_owned(),
            note: "仅 outbounds 节点列表，与历史版本逐字节一致".to_owned(),
        },
        SubscriptionLinkInfo {
            format: SubscriptionFormat::SingBoxFull,
            label: "sing-box 完整配置（最新稳定版）".to_owned(),
            audience: "sing-box 最新稳定版".to_owned(),
            note: "完整客户端配置：DNS / tun / 分流规则 / 代理组 / clash_api，与服务端运行的最新稳定版一致".to_owned(),
        },
        SubscriptionLinkInfo {
            format: SubscriptionFormat::Clash,
            label: "Clash / mihomo 配置".to_owned(),
            audience: "mihomo 现行稳定版".to_owned(),
            note: "fake-ip DNS、rule-set 分流、代理组与 AI 分流".to_owned(),
        },
        SubscriptionLinkInfo {
            format: SubscriptionFormat::ClashLegacy(CLASH_LEGACY_VERSION),
            label: "Clash / mihomo 旧版兼容".to_owned(),
            audience: "mihomo 1.18.x".to_owned(),
            note: "面向上一大版本的兼容写法（内置 GEOIP 规则）".to_owned(),
        },
        SubscriptionLinkInfo {
            format: SubscriptionFormat::Uri,
            label: "分享链接（明文）".to_owned(),
            audience: "通用".to_owned(),
            note: "每行一个 vless:// 等分享 URI".to_owned(),
        },
        SubscriptionLinkInfo {
            format: SubscriptionFormat::Base64Uri,
            label: "分享链接（Base64）".to_owned(),
            audience: "V2rayN 等".to_owned(),
            note: "明文 URI 列表整体 Base64 编码".to_owned(),
        },
        SubscriptionLinkInfo {
            format: SubscriptionFormat::Shadowrocket,
            label: "Shadowrocket 适配".to_owned(),
            audience: "Shadowrocket (iOS)".to_owned(),
            note: "URI 参数按 Shadowrocket 解析习惯适配并规范备注名".to_owned(),
        },
    ]
}

/// The ordered link matrix: static rows plus one dynamically labeled row per
/// sing-box version profile (inserted right before `sing-box-full`).
pub fn subscription_matrix() -> Vec<SubscriptionLinkInfo> {
    let latest = latest_version_profile().version;
    let mut rows: Vec<SubscriptionLinkInfo> =
        Vec::with_capacity(static_matrix_rows().len() + SING_BOX_VERSION_PROFILES.len());
    for info in static_matrix_rows() {
        if info.format == SubscriptionFormat::SingBoxFull {
            for profile in SING_BOX_VERSION_PROFILES {
                let is_latest = profile.version == latest;
                rows.push(SubscriptionLinkInfo {
                    format: SubscriptionFormat::SingBoxVersion(profile.version),
                    label: if is_latest {
                        format!("sing-box {} 适配（最新）", profile.version)
                    } else {
                        format!("sing-box {} 适配", profile.version)
                    },
                    audience: if is_latest {
                        "sing-box 最新稳定版".to_owned()
                    } else {
                        format!("sing-box {}", profile.supported)
                    },
                    note: profile.notes.to_owned(),
                });
            }
        }
        rows.push(info);
    }
    rows
}

/// One recommended format inside a per-client quick-pick row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientSubscriptionFormat {
    pub format: SubscriptionFormat,
    /// Why this client should use this format.
    pub note: String,
}

/// One row of the per-client quick-pick table: the mainstream client, its
/// recommended subscription formats in preference order, and a general note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientSubscriptionRow {
    pub client: &'static str,
    pub formats: Vec<ClientSubscriptionFormat>,
    pub note: String,
}

/// The per-client quick-pick table for Clash Party, Clash Verge, sing-box,
/// V2rayN, and Shadowrocket. Versioned sing-box rows expand dynamically from
/// the version profile registry so a new upstream minor only needs a registry
/// entry here too.
pub fn client_subscription_matrix() -> Vec<ClientSubscriptionRow> {
    let mut sing_box_formats = vec![ClientSubscriptionFormat {
        format: SubscriptionFormat::SingBoxFull,
        note: "客户端内核为最新稳定版时使用".to_owned(),
    }];
    for profile in SING_BOX_VERSION_PROFILES {
        sing_box_formats.push(ClientSubscriptionFormat {
            format: SubscriptionFormat::SingBoxVersion(profile.version),
            note: format!("客户端内核为 {} 时使用", profile.supported),
        });
    }
    vec![
        ClientSubscriptionRow {
            client: "Clash Party",
            formats: vec![ClientSubscriptionFormat {
                format: SubscriptionFormat::Clash,
                note: "mihomo 内核订阅，导入后自动更新节点".to_owned(),
            }],
            note: "Clash Party 使用 mihomo 内核，无需关心 sing-box 版本适配。".to_owned(),
        },
        ClientSubscriptionRow {
            client: "Clash Verge",
            formats: vec![ClientSubscriptionFormat {
                format: SubscriptionFormat::Clash,
                note: "mihomo 内核订阅，导入后自动更新节点".to_owned(),
            }],
            note: "Clash Verge（Rev）使用 mihomo 内核；若内置内核较旧，可改用 clash-1.18.yaml。"
                .to_owned(),
        },
        ClientSubscriptionRow {
            client: "sing-box",
            formats: sing_box_formats,
            note: "请按客户端实际内核版本选择对应文件；1.10/1.11 不支持 AnyTLS 节点，\
                   详见各版本条目的说明。"
                .to_owned(),
        },
        ClientSubscriptionRow {
            client: "V2rayN",
            formats: vec![
                ClientSubscriptionFormat {
                    format: SubscriptionFormat::Base64Uri,
                    note: "分享链接订阅（默认内核）".to_owned(),
                },
                ClientSubscriptionFormat {
                    format: SubscriptionFormat::SingBoxFull,
                    note: "V2rayN 6.6+ 可直接导入 sing-box 完整配置（内置 sing-box 内核）"
                        .to_owned(),
                },
            ],
            note: "V2rayN 同时支持 Xray 与 sing-box 双内核，按导入方式二选一。".to_owned(),
        },
        ClientSubscriptionRow {
            client: "Shadowrocket",
            formats: vec![ClientSubscriptionFormat {
                format: SubscriptionFormat::Shadowrocket,
                note: "扫码或粘贴订阅链接，自动按 iOS 客户端习惯适配".to_owned(),
            }],
            note: "五协议均受支持，但要求 Shadowrocket ≥ 对应协议的最低版本（见总览页导入说明）。"
                .to_owned(),
        },
    ]
}

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error("external reverse-proxy subscription must bind a loopback address")]
    ExternalProxyBind,
    #[error("subscription listener port {0} is already in use")]
    ListenerUnavailable(u16),
    #[error("subscription listener failed: {0}")]
    ListenerIo(String),
    #[error("Direct HTTPS requires systemd socket activation: {0}")]
    SocketActivation(String),
    #[error("Direct HTTPS received an unexpected listener on port {0}")]
    UnexpectedDirectListener(u16),
    #[error("Direct HTTPS is missing the {0} listener")]
    MissingDirectListener(u16),
    #[error("HTTP handling failed: {0}")]
    Http(String),
    #[error("no subscription-capable Managed protocol is enabled")]
    MissingNodes,
    #[error("invalid subscription credential")]
    InvalidCredential,
    #[error("subscription artifact is unavailable: {0}")]
    Artifact(#[from] std::io::Error),
    #[error("self-signed certificate generation failed: {0}")]
    Certificate(String),
    #[error("TLS certificate could not be loaded: {0}")]
    Tls(String),
    #[error("sing-box configuration check failed: {0}")]
    Check(String),
    #[error("override template rejected: {0}")]
    Override(String),
    #[error("client compatibility: {0}")]
    ClientIncompatible(String),
    #[error(transparent)]
    Storage(#[from] ConfigError),
}

/// Regenerates the four cached artifacts from the canonical node model and
/// replaces them atomically under one operation lock. When `sing_box_bin` is
/// supplied the new server configuration is validated with `sing-box check`
/// before any file is replaced, so a failed check leaves every existing
/// artifact untouched. If any replacement fails mid-way, the already-replaced
/// files are restored to their previous complete versions. `update_active_config`
/// additionally re-syncs the active sing-box configuration consumed by the
/// managed service; reload/restart of the service is the caller's step.
pub fn regenerate(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    sing_box_bin: Option<&Path>,
    update_active_config: bool,
) -> Result<(), SubscriptionError> {
    let artifacts = generated_artifacts(config, store.root())?;
    if let Some(sing_box_bin) = sing_box_bin {
        let server = server_artifact(&artifacts)?;
        check_sing_box_config(sing_box_bin, server)?;
    }
    let _lock = store.acquire_operation_lock()?;
    let prior_artifacts = artifacts
        .iter()
        .map(|(name, _)| (name.clone(), read_artifact(store, name)))
        .collect::<Vec<_>>();
    let prior_active = if update_active_config {
        fs::read(store.root().join(ACTIVE_CONFIG_RELATIVE_PATH)).ok()
    } else {
        None
    };
    for (name, contents) in &artifacts {
        if let Err(error) = store.write_artifact_locked(name, contents.as_bytes()) {
            restore_replaced(store, &prior_artifacts, prior_active.as_deref());
            return Err(SubscriptionError::Storage(error));
        }
    }
    if update_active_config {
        let server = server_artifact(&artifacts)?;
        if let Err(error) =
            store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, server.as_bytes())
        {
            restore_replaced(store, &prior_artifacts, prior_active.as_deref());
            return Err(SubscriptionError::Storage(error));
        }
    }
    Ok(())
}

fn server_artifact(artifacts: &[(String, String)]) -> Result<&str, SubscriptionError> {
    artifacts
        .iter()
        .find(|(name, _)| name == SING_BOX_SERVER_ARTIFACT)
        .map(|(_, contents)| contents.as_str())
        .ok_or_else(|| {
            SubscriptionError::Check("no generated sing-box server configuration".to_owned())
        })
}

fn read_artifact(store: &DeploymentStore, name: &str) -> Option<Vec<u8>> {
    fs::read(store.root().join(ARTIFACTS_RELATIVE_DIR).join(name)).ok()
}

/// Best-effort rollback of already-replaced artifacts and the active
/// configuration after a mid-transaction write failure. Each write is atomic,
/// so a failed write leaves its own target on the previous complete version.
fn restore_replaced(
    store: &DeploymentStore,
    prior_artifacts: &[(String, Option<Vec<u8>>)],
    prior_active: Option<&[u8]>,
) {
    for (name, prior) in prior_artifacts.iter().rev() {
        if let Some(prior) = prior {
            let _ = store.write_artifact_locked(name, prior);
        }
    }
    if let Some(prior_active) = prior_active {
        let _ = store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, prior_active);
    }
}

/// The prior complete versions of every file a configuration transaction can
/// replace, used to restore the previous known-good deployment after a failed
/// service health check.
pub struct DeploymentSnapshot {
    pub config: Vec<u8>,
    pub artifacts: Vec<(String, Option<Vec<u8>>)>,
    pub active_config: Option<Vec<u8>>,
}

/// Validates, then atomically replaces the deployment configuration together
/// with any changed canonical artifacts and the active sing-box configuration
/// under one operation lock. The generated server configuration is checked with
/// `sing-box check` before any file is replaced, so a failed check leaves every
/// existing file untouched. A configuration-only change (one that does not alter
/// the canonical node model) skips the check and the artifact writes. The
/// returned snapshot lets the caller restore the previous deployment if the
/// subsequent service health check fails.
pub fn apply_config_transaction(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    sing_box_bin: Option<&Path>,
) -> Result<DeploymentSnapshot, SubscriptionError> {
    config.validate()?;
    let artifacts = generated_artifacts(config, store.root())?;
    let server = server_artifact(&artifacts)?;
    let _lock = store.acquire_operation_lock()?;
    let prior_artifacts = artifacts
        .iter()
        .map(|(name, _)| (name.clone(), read_artifact(store, name)))
        .collect::<Vec<_>>();
    let prior_active = fs::read(store.root().join(ACTIVE_CONFIG_RELATIVE_PATH)).ok();
    let prior_config = fs::read(store.root().join(crate::config::CONFIG_RELATIVE_PATH)).ok();

    let artifacts_changed = prior_artifacts.iter().any(|(name, prior)| {
        artifacts
            .iter()
            .find(|(artifact_name, _)| artifact_name == name)
            .is_none_or(|(_, contents)| prior.as_deref() != Some(contents.as_bytes()))
    });
    // A deployment that has no active sing-box configuration yet (configuration
    // initialized without installation) is not synced: writing the active file
    // is the installation step. Once present, it is re-synced whenever the
    // canonical node model changes or it drifted from the generated server.
    let need_active_sync = prior_active.is_some()
        && (artifacts_changed || prior_active.as_deref() != Some(server.as_bytes()));

    if artifacts_changed || need_active_sync {
        let Some(sing_box_bin) = sing_box_bin else {
            return Err(SubscriptionError::Check(
                "configuration change requires a sing-box binary for validation".to_owned(),
            ));
        };
        check_sing_box_config(sing_box_bin, server)?;
        for (name, contents) in &artifacts {
            if let Err(error) = store.write_artifact_locked(name, contents.as_bytes()) {
                restore_replaced(store, &prior_artifacts, prior_active.as_deref());
                return Err(SubscriptionError::Storage(error));
            }
        }
        if need_active_sync
            && let Err(error) =
                store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, server.as_bytes())
        {
            restore_replaced(store, &prior_artifacts, prior_active.as_deref());
            return Err(SubscriptionError::Storage(error));
        }
    }
    if let Err(error) = store.replace_locked(config) {
        restore_replaced(store, &prior_artifacts, prior_active.as_deref());
        return Err(SubscriptionError::Storage(error));
    }
    Ok(DeploymentSnapshot {
        config: prior_config.unwrap_or_default(),
        artifacts: prior_artifacts,
        active_config: prior_active,
    })
}

/// Restores a previously captured deployment snapshot after a failed service
/// health check, then restarts the managed services to return the running
/// deployment to the previous known-good configuration.
pub fn restore_config_transaction(
    store: &DeploymentStore,
    snapshot: &DeploymentSnapshot,
) -> Result<(), SubscriptionError> {
    let _lock = store.acquire_operation_lock()?;
    for (name, prior) in snapshot.artifacts.iter().rev() {
        match prior {
            Some(prior) => store.write_artifact_locked(name, prior)?,
            None => {
                let _ = fs::remove_file(store.root().join(ARTIFACTS_RELATIVE_DIR).join(name));
            }
        }
    }
    if let Some(active) = &snapshot.active_config {
        store.write_relative_locked(ACTIVE_CONFIG_RELATIVE_PATH, active)?;
    }
    store.write_relative_locked(crate::config::CONFIG_RELATIVE_PATH, &snapshot.config)?;
    Ok(())
}

pub fn generated_artifacts(
    config: &DeploymentConfig,
    root: &Path,
) -> Result<Vec<(String, String)>, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let nodes = crate::canonical::nodes(config);
    let uri = uri(config, &nodes)?;
    let mut artifacts: Vec<(String, String)> = vec![
        (
            SING_BOX_SERVER_ARTIFACT.to_owned(),
            sing_box_server(config, &nodes, root)?,
        ),
        (SING_BOX_ARTIFACT.to_owned(), sing_box(config, &nodes)?),
        (CLASH_ARTIFACT.to_owned(), clash(config, &nodes)?),
        (URI_ARTIFACT.to_owned(), uri.clone()),
        (BASE64_URI_ARTIFACT.to_owned(), base64_uri(&uri)),
        (
            SHADOWROCKET_ARTIFACT.to_owned(),
            shadowrocket(config, &nodes)?,
        ),
        (
            SING_BOX_FULL_ARTIFACT.to_owned(),
            sing_box_full(config, &nodes, latest_version_profile())?,
        ),
    ];
    for profile in SING_BOX_VERSION_PROFILES {
        // Pre-1.12 client cores have no AnyTLS outbound. A deployment whose
        // only enabled protocol is AnyTLS has no usable node for those
        // profiles, so the artifact is skipped (with a warning) instead of
        // failing the whole generation and blocking every other format.
        if !profile.supports_anytls
            && nodes
                .iter()
                .all(|node| node.protocol() == ManagedProtocol::Anytls)
        {
            eprintln!(
                "warning: sing-box {} 客户端内核不支持 AnyTLS 协议（1.12.0 才加入）；\
                 本次未生成 sing-box-{}.json，旧内核客户端将无法导入。\
                 请在部署中启用至少一个其他协议后重新生成",
                profile.version, profile.version
            );
            continue;
        }
        artifacts.push((
            SubscriptionFormat::SingBoxVersion(profile.version)
                .artifact_name()
                .into_owned(),
            sing_box_full(config, &nodes, profile)?,
        ));
    }
    artifacts.push((
        SubscriptionFormat::ClashLegacy(CLASH_LEGACY_VERSION)
            .artifact_name()
            .into_owned(),
        clash_legacy(config, &nodes)?,
    ));
    apply_client_overrides(root, &mut artifacts)?;
    Ok(artifacts)
}

/// Deep-merges the administrator's override templates into the generated
/// client artifacts. The historical bare `sing-box.json` and the URI formats
/// are deliberately untouched so their byte compatibility never changes.
fn apply_client_overrides(
    root: &Path,
    artifacts: &mut [(String, String)],
) -> Result<(), SubscriptionError> {
    let overrides = crate::override_template::Overrides::load(root)
        .map_err(|error| SubscriptionError::Override(error.to_string()))?;
    if let Some(sing_box_override) = &overrides.sing_box {
        for (name, contents) in artifacts.iter_mut() {
            if !name.starts_with("subscription-sing-box") || name == SING_BOX_ARTIFACT {
                continue;
            }
            let mut value: serde_json::Value = serde_json::from_str(contents)
                .map_err(|error| SubscriptionError::Override(error.to_string()))?;
            crate::override_template::deep_merge(&mut value, sing_box_override);
            *contents = serde_json::to_string_pretty(&value)
                .map_err(|error| SubscriptionError::Override(error.to_string()))?;
        }
    }
    if let Some(clash_override) = &overrides.clash {
        let legacy_name = SubscriptionFormat::ClashLegacy(CLASH_LEGACY_VERSION)
            .artifact_name()
            .into_owned();
        for (name, contents) in artifacts.iter_mut() {
            if name != CLASH_ARTIFACT && *name != legacy_name {
                continue;
            }
            let mut value: serde_yaml::Value = serde_yaml::from_str(contents)
                .map_err(|error| SubscriptionError::Override(error.to_string()))?;
            crate::override_template::deep_merge_yaml(&mut value, clash_override);
            *contents = serde_yaml::to_string(&value)
                .map_err(|error| SubscriptionError::Override(error.to_string()))?;
        }
    }
    Ok(())
}

pub fn check_sing_box_config(
    sing_box_binary: &Path,
    config: &str,
) -> Result<(), SubscriptionError> {
    let mut temporary = tempfile::NamedTempFile::new().map_err(SubscriptionError::Artifact)?;
    temporary
        .write_all(config.as_bytes())
        .map_err(SubscriptionError::Artifact)?;
    let status = Command::new(sing_box_binary)
        .args(["check", "-c"])
        .arg(temporary.path())
        .status()
        .map_err(SubscriptionError::Artifact)?;
    if status.success() {
        Ok(())
    } else {
        Err(SubscriptionError::Check(format!(
            "sing-box check exited with {status}"
        )))
    }
}

pub fn read_authorized(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    credential: &str,
    format: SubscriptionFormat,
) -> Result<String, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    if !constant_time_eq(
        credential.as_bytes(),
        config.subscription_credential.as_bytes(),
    ) {
        return Err(SubscriptionError::InvalidCredential);
    }
    Ok(String::from_utf8_lossy(&fs::read(
        store
            .root()
            .join("var/lib/sbctl/artifacts")
            .join(format.artifact_name().as_ref()),
    )?)
    .into_owned())
}

pub fn subscription_url(
    config: &DeploymentConfig,
    format: SubscriptionFormat,
) -> Result<String, SubscriptionError> {
    route_url(config, SubscriptionRoute::Format(format))
}

/// The full URL for any subscription route, including the QR and index pages.
pub fn route_url(
    config: &DeploymentConfig,
    route: SubscriptionRoute,
) -> Result<String, SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let prefix = match config.subscription_mode {
        SubscriptionMode::IpFallback => format!(
            "http://{}:{}",
            config.subscription_host,
            config.http_port.expect("validated IP fallback port")
        ),
        SubscriptionMode::Direct | SubscriptionMode::ExternalProxy => {
            format!("https://{}", config.subscription_host)
        }
    };
    let suffix = match route {
        SubscriptionRoute::Format(format) => format.path_name(),
        SubscriptionRoute::Qr(format) => format!("qr/{}", format.path_name()),
        SubscriptionRoute::Index => "index".to_owned(),
    };
    Ok(format!(
        "{prefix}/sub/{}/{}",
        config.subscription_credential, suffix
    ))
}

pub async fn serve(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    bind: &str,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    ensure_subscription_nodes(config)?;
    let store = Arc::new(store.clone());
    let config = Arc::new(config.clone());
    if config.subscription_mode == SubscriptionMode::Direct {
        return serve_direct_socket_activated(&store, &config, max_requests).await;
    }
    if config.subscription_mode == SubscriptionMode::ExternalProxy
        && !bind
            .parse::<SocketAddr>()
            .ok()
            .is_some_and(|address| address.ip().is_loopback())
    {
        return Err(SubscriptionError::ExternalProxyBind);
    }
    let listener = TcpListener::bind(bind).await.map_err(listener_io)?;
    serve_http_listener(listener, &store, &config, max_requests).await
}

/// Direct subscription mode never binds 80/443 itself. systemd owns those
/// listeners through `sbctl-http.socket` and hands them to this process via
/// `LISTEN_FDS`; the two sockets are routed by their local port so TCP 80
/// serves the ACME challenge and TCP 443 serves the TLS subscription.
async fn serve_direct_socket_activated(
    store: &Arc<DeploymentStore>,
    config: &Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let listeners = crate::socket_activation::receive_listeners()
        .map_err(|error| SubscriptionError::SocketActivation(error.to_string()))?;
    let mut acme = None;
    let mut tls = None;
    for (port, listener) in listeners {
        match crate::socket_activation::direct_listener_role(port) {
            Some(crate::socket_activation::DirectListenerRole::Acme) => acme = Some(listener),
            Some(crate::socket_activation::DirectListenerRole::Tls) => tls = Some(listener),
            None => return Err(SubscriptionError::UnexpectedDirectListener(port)),
        }
    }
    let acme = tokio_listener(acme.ok_or(SubscriptionError::MissingDirectListener(80))?)?;
    let tls = tokio_listener(tls.ok_or(SubscriptionError::MissingDirectListener(443))?)?;
    tokio::try_join!(
        serve_acme_listener(acme, Arc::clone(store), max_requests),
        serve_tls_listener(tls, Arc::clone(store), Arc::clone(config), max_requests)
    )?;
    Ok(())
}

fn tokio_listener(listener: std::net::TcpListener) -> Result<TcpListener, SubscriptionError> {
    listener
        .set_nonblocking(true)
        .map_err(|error| SubscriptionError::ListenerIo(error.to_string()))?;
    TcpListener::from_std(listener)
        .map_err(|error| SubscriptionError::ListenerIo(error.to_string()))
}

/// The shared Hyper HTTP/1 builder: a bounded header size, a slow-read
/// timeout, and a Tokio timer so the timeout applies.
fn http1_builder() -> hyper::server::conn::http1::Builder {
    let mut builder = hyper::server::conn::http1::Builder::new();
    builder.max_buf_size(MAX_REQUEST_HEADER_BYTES);
    builder.timer(hyper_util::rt::TokioTimer::new());
    builder.header_read_timeout(MAX_HEADER_READ_TIME);
    builder
}

/// Bounds applied to every HTTP connection so an oversized request header, a
/// slow reader, an idle client, or connection flooding cannot exhaust the
/// process. Responses set `Connection: close`, so each request is its own
/// connection and hyper never keeps an idle connection alive.
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADER_READ_TIME: Duration = Duration::from_secs(5);
const MAX_CONNECTION_TIME: Duration = Duration::from_secs(30);
const MAX_CONCURRENT_CONNECTIONS: usize = 32;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Accepts the next connection, or returns `None` after a short poll when a
/// test-configured `max_requests` limit may have been reached by a task that
/// is already serving. Production operation (`max_requests == None`) blocks on
/// the accept until a connection arrives.
async fn accept_next(
    listener: &TcpListener,
    max_requests: Option<usize>,
) -> Result<Option<tokio::net::TcpStream>, SubscriptionError> {
    if max_requests.is_none() {
        let (stream, _) = listener.accept().await.map_err(listener_io)?;
        return Ok(Some(stream));
    }
    match tokio::time::timeout(ACCEPT_POLL_INTERVAL, listener.accept()).await {
        Ok(Ok((stream, _))) => Ok(Some(stream)),
        Ok(Err(error)) => Err(listener_io(error)),
        Err(_) => Ok(None),
    }
}

fn listener_io(error: std::io::Error) -> SubscriptionError {
    SubscriptionError::ListenerIo(error.to_string())
}

/// Accepts connections from one listener, bounding concurrency with a
/// semaphore and each connection's lifetime with a timeout. Serves at most
/// `max_requests` connections when a test supplies that limit.
async fn serve_http_listener(
    listener: TcpListener,
    store: &Arc<DeploymentStore>,
    config: &Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let store = Arc::clone(store);
        let config = Arc::clone(config);
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tokio::time::timeout(
                MAX_CONNECTION_TIME,
                serve_http_connection(TokioIo::new(stream), store, config),
            )
            .await;
            counter.fetch_add(1, Ordering::Release);
        });
    }
    Ok(())
}

/// Serves ACME HTTP-01 challenge responses from the listener on TCP 80 with
/// the same bounded connection handling as the subscription listener.
async fn serve_acme_listener(
    listener: TcpListener,
    store: Arc<DeploymentStore>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let store = Arc::clone(&store);
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tokio::time::timeout(
                MAX_CONNECTION_TIME,
                serve_acme_connection(TokioIo::new(stream), store),
            )
            .await;
            counter.fetch_add(1, Ordering::Release);
        });
    }
    Ok(())
}

/// Serves the TLS subscription listener on TCP 443. The certificate is
/// reloaded before every accepted connection, so a Certbot renewal takes
/// effect on the next handshake without signalling or restarting the service.
async fn serve_tls_listener(
    listener: TcpListener,
    store: Arc<DeploymentStore>,
    config: Arc<DeploymentConfig>,
    max_requests: Option<usize>,
) -> Result<(), SubscriptionError> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
    let counter = Arc::new(AtomicUsize::new(0));
    let mut tls = None;
    loop {
        if max_requests.is_some_and(|max| counter.load(Ordering::Acquire) >= max) {
            break;
        }
        let Some(stream) = accept_next(&listener, max_requests).await? else {
            continue;
        };
        match load_tls_config(&store, &config) {
            Ok(reloaded) => tls = Some(reloaded),
            Err(error) => {
                // A TLS-terminating listener cannot emit an HTTP 5xx: the
                // certificate is needed before the first HTTP byte. The failure
                // is instead diagnosed with a redacted log line, and the last
                // known-good configuration keeps serving until a valid
                // certificate is pinned again.
                eprintln!(
                    "Direct HTTPS certificate unavailable; connection dropped: {}",
                    redact_secret(&error.to_string(), &config.subscription_credential)
                )
            }
        }
        let Some(tls) = tls.clone() else {
            drop(stream);
            continue;
        };
        let Ok(permit) = semaphore.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let store = Arc::clone(&store);
        let config = Arc::clone(&config);
        let counter = Arc::clone(&counter);
        tokio::spawn(async move {
            let _permit = permit;
            let acceptor = TlsAcceptor::from(tls);
            let Ok(stream) = acceptor.accept(stream).await else {
                counter.fetch_add(1, Ordering::Release);
                return;
            };
            let _ = tokio::time::timeout(
                MAX_CONNECTION_TIME,
                serve_http_connection(TokioIo::new(Box::pin(stream)), store, config),
            )
            .await;
            counter.fetch_add(1, Ordering::Release);
        });
    }
    Ok(())
}

async fn serve_http_connection<S>(
    io: TokioIo<S>,
    store: Arc<DeploymentStore>,
    config: Arc<DeploymentConfig>,
) -> Result<(), SubscriptionError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let response = subscription_http_response(request, &store, &config);
        async { Ok::<_, std::convert::Infallible>(response) }
    });
    http1_builder()
        .serve_connection(io, service)
        .await
        .map_err(|error| SubscriptionError::Http(error.to_string()))
}

async fn serve_acme_connection<S>(
    io: TokioIo<S>,
    store: Arc<DeploymentStore>,
) -> Result<(), SubscriptionError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<Incoming>| {
        let response = acme_http_response(request, &store);
        async { Ok::<_, std::convert::Infallible>(response) }
    });
    http1_builder()
        .serve_connection(io, service)
        .await
        .map_err(|error| SubscriptionError::Http(error.to_string()))
}

fn acme_http_response(
    request: Request<Incoming>,
    store: &DeploymentStore,
) -> Response<Full<Bytes>> {
    let body = request
        .uri()
        .path()
        .strip_prefix("/.well-known/acme-challenge/")
        .filter(|token| !token.is_empty() && !token.contains('/') && !token.contains('?'))
        .and_then(|token| {
            fs::read_to_string(
                store
                    .acme_webroot()
                    .join(".well-known/acme-challenge")
                    .join(token),
            )
            .ok()
        });
    match body {
        Some(body) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/plain; charset=utf-8")
            .header("Cache-Control", "no-store")
            .header("Connection", "close")
            .body(Full::new(Bytes::from(body)))
            .expect("valid ACME response"),
        None => not_found_http_response(),
    }
}

fn subscription_http_response(
    request: Request<Incoming>,
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Response<Full<Bytes>> {
    if request.method() != Method::GET || request.uri().query().is_some() {
        return not_found_http_response();
    }
    let Some((credential, route)) = parse_route(request.uri().path()) else {
        return not_found_http_response();
    };
    if !constant_time_eq(
        credential.as_bytes(),
        config.subscription_credential.as_bytes(),
    ) {
        return not_found_http_response();
    }
    match route {
        SubscriptionRoute::Qr(format) => qr_http_response(config, credential, format),
        SubscriptionRoute::Index => index_http_response(store, config, credential),
        SubscriptionRoute::Format(format) => {
            let body = match read_authorized(store, config, credential, format) {
                Ok(body) => body,
                Err(error) => return unavailable_http_response(credential, &error.to_string()),
            };
            // The subscription-userinfo header is an addition to the artifact,
            // not a precondition: a broken or mid-repair accounting state must
            // not take the subscription itself offline. The failure is logged
            // redacted and the artifact is served without traffic metadata.
            let userinfo = match crate::traffic::report(store, config) {
                Ok(traffic) => {
                    // subscription-userinfo follows the common client convention:
                    // upload and download are the bytes used in the current period,
                    // while `total` is the configured monthly allowance. Keep the
                    // historical used-total value when no allowance is configured so
                    // unlimited deployments remain informative.
                    let quota = if traffic.monthly_traffic_limit > 0 {
                        traffic.monthly_traffic_limit
                    } else {
                        traffic.total()
                    };
                    Some(format!(
                        "upload={}; download={}; total={}; expire={}",
                        traffic.transmitted,
                        traffic.received,
                        quota,
                        traffic.next_reset.timestamp()
                    ))
                }
                Err(error) => {
                    eprintln!(
                        "subscription traffic metadata unavailable: {}",
                        redact_secret(&error.to_string(), credential)
                    );
                    None
                }
            };
            let mut builder = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", format.content_type())
                .header("Cache-Control", "no-store")
                .header("X-Content-Type-Options", "nosniff")
                .header("Connection", "close");
            if let Some(value) = &userinfo {
                builder = builder.header("subscription-userinfo", value);
            }
            builder
                .body(Full::new(Bytes::from(body)))
                .expect("valid subscription response")
        }
    }
}

/// A scannable SVG QR code of the given format's subscription URL. The QR
/// content is derived from the configuration only, so no artifact file is
/// needed and the code always encodes the current URL.
fn qr_http_response(
    config: &DeploymentConfig,
    credential: &str,
    format: SubscriptionFormat,
) -> Response<Full<Bytes>> {
    let body = match subscription_url(config, format)
        .map_err(|error| error.to_string())
        .and_then(|url| crate::qr::render_svg(&url))
    {
        Ok(body) => body,
        Err(error) => return unavailable_http_response(credential, &error),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "image/svg+xml")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Connection", "close")
        .body(Full::new(Bytes::from(body)))
        .expect("valid QR response")
}

/// The Chinese overview page: every subscription link with its label, the
/// matching QR code, and per-client import instructions. Self-contained HTML
/// with no external resources, so it renders offline and leaks nothing extra.
fn index_http_response(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    credential: &str,
) -> Response<Full<Bytes>> {
    let body = match crate::index_page::render(store, config) {
        Ok(body) => body,
        Err(error) => return unavailable_http_response(credential, &error.to_string()),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Connection", "close")
        .body(Full::new(Bytes::from(body)))
        .expect("valid index response")
}

/// Loads and validates the pinned certificate for Direct HTTPS. Every loading
/// check — validity period, SAN coverage, private-key match — runs before the
/// TLS acceptor is built, and the acceptor refuses connections whose SNI does
/// not equal the subscription host. The daemon reloads before every handshake,
/// so a Certbot renewal pinned by the deploy hook takes effect on the next
/// connection without a service restart.
fn load_tls_config(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<Arc<rustls::ServerConfig>, SubscriptionError> {
    crate::certificate::load_pinned(store, config)
        .map_err(|error| SubscriptionError::Tls(error.to_string()))
        .and_then(|validated| {
            validated
                .server_config()
                .map_err(|error| SubscriptionError::Tls(error.to_string()))
        })
}

/// Replaces every occurrence of a Subscription credential in a diagnostic so
/// logs and errors never expose the full secret. ADR-0013.
pub fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    text.replace(secret, "[redacted]")
}

/// A redacted 503 for state or artifact failures after a valid Subscription
/// credential authenticated. The body carries no authorization or deployment
/// details; the diagnostic log omits the credential.
fn unavailable_http_response(credential: &str, message: &str) -> Response<Full<Bytes>> {
    eprintln!(
        "subscription request failed: {}",
        redact_secret(message, credential)
    );
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header("Cache-Control", "no-store")
        .header("Connection", "close")
        .body(Full::new(Bytes::new()))
        .expect("valid unavailable response")
}

fn not_found_http_response() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header("Cache-Control", "no-store")
        .header("Connection", "close")
        .body(Full::new(Bytes::new()))
        .expect("valid not-found response")
}

fn parse_route(target: &str) -> Option<(&str, SubscriptionRoute)> {
    if target.contains('?') {
        return None;
    }
    let mut parts = target.strip_prefix("/sub/")?.split('/');
    let credential = parts.next()?;
    let route = match parts.next()? {
        "index" => SubscriptionRoute::Index,
        // The trailing check below rejects anything after the format segment.
        "qr" => SubscriptionRoute::Qr(parse_format_path(parts.next()?)?),
        segment => SubscriptionRoute::Format(parse_format_path(segment)?),
    };
    parts.next().is_none().then_some((credential, route))
}

/// Parses one subscription format path segment. Versioned segments are only
/// accepted when the version exists in the profile registry, so unknown
/// versions 404 instead of surfacing as a missing artifact.
fn parse_format_path(segment: &str) -> Option<SubscriptionFormat> {
    match segment {
        "sing-box.json" => return Some(SubscriptionFormat::SingBox),
        "sing-box-full.json" => return Some(SubscriptionFormat::SingBoxFull),
        "clash.yaml" => return Some(SubscriptionFormat::Clash),
        "uri" => return Some(SubscriptionFormat::Uri),
        "uri.txt" => return Some(SubscriptionFormat::Base64Uri),
        "shadowrocket.txt" => return Some(SubscriptionFormat::Shadowrocket),
        _ => {}
    }
    if let Some(version) = segment
        .strip_prefix("sing-box-")
        .and_then(|rest| rest.strip_suffix(".json"))
    {
        let version = parse_client_version(version)?;
        return SING_BOX_VERSION_PROFILES
            .iter()
            .any(|profile| profile.version == version)
            .then_some(SubscriptionFormat::SingBoxVersion(version));
    }
    if let Some(version) = segment
        .strip_prefix("clash-")
        .and_then(|rest| rest.strip_suffix(".yaml"))
    {
        let version = parse_client_version(version)?;
        return (version == CLASH_LEGACY_VERSION)
            .then_some(SubscriptionFormat::ClashLegacy(version));
    }
    None
}

fn parse_client_version(text: &str) -> Option<ClientVersion> {
    let (major, minor) = text.split_once('.')?;
    Some(ClientVersion::new(major.parse().ok()?, minor.parse().ok()?))
}

fn ensure_subscription_nodes(config: &DeploymentConfig) -> Result<(), SubscriptionError> {
    if !config
        .enabled_protocols
        .iter()
        .any(ManagedProtocol::has_generated_subscription_artifacts)
    {
        return Err(SubscriptionError::MissingNodes);
    }
    Ok(())
}

pub fn ensure_external_proxy_listener_available(port: u16) -> Result<(), SubscriptionError> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map(drop)
        .map_err(|_| SubscriptionError::ListenerUnavailable(port))
}

fn sing_box(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    Ok(
        serde_json::to_string_pretty(&json!({"outbounds": client_outbounds(config, nodes)}))
            .expect("JSON values serialize"),
    )
}

/// The five-protocol outbound list shared by the bare and full sing-box client
/// artifacts, so the two can never drift apart on protocol fields.
fn client_outbounds(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Vec<Value> {
    let skip_verify = client_skip_cert_verify(config);
    nodes
        .iter()
        .map(|node| match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => json!({"type": "vless", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "flow": "xtls-rprx-vision",
                "tls": {"enabled": true, "server_name": decoy_sni, "utls": {"enabled": true, "fingerprint": "chrome"},
                    "reality": {"enabled": true, "public_key": public_key, "short_id": short_id}}}),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => json!({"type": "vmess", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "security": "auto", "alter_id": 0,
                "transport": {"type": "ws", "path": path},
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify}}),
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => json!({"type": "hysteria2", "tag": node.tag(), "server": host,
                "server_port": port, "password": password,
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify,
                    "alpn": ["h3"]}}),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => json!({"type": "tuic", "tag": node.tag(), "server": host,
                "server_port": port, "uuid": uuid, "password": password,
                "congestion_control": "bbr", "udp_relay_mode": "native",
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify,
                    "alpn": ["h3"]}}),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => json!({"type": "anytls", "tag": node.tag(), "server": host,
                "server_port": port, "password": password,
                "idle_session_check_interval": "30s", "idle_session_timeout": "30s",
                "min_idle_session": 5,
                "tls": {"enabled": true, "server_name": tls_server_name, "insecure": skip_verify}}),
        })
        .collect()
}

/// Group tags shared by the full sing-box client profile and the clash
/// artifact so panel screenshots and docs read the same everywhere.
pub const SELECTOR_TAG: &str = "🚀节点选择";
pub const AUTO_TAG: &str = "♻️自动选择";
const DIRECT_TAG: &str = "direct";

/// Domains that must never be routed through the selector, kept in one place
/// for the sing-box full profile, the clash artifact, and their overrides.
pub const AI_DOMAIN_SUFFIXES: &[&str] = &[
    "chatgpt.com",
    "openai.com",
    "oaistatic.com",
    "oaiusercontent.com",
    "x.com",
    "twitter.com",
    "twimg.com",
];

/// Domains that must not receive a fake IP: LAN names, OS connectivity
/// checks, and NTP servers, matching the sing-box-yg client defaults.
const FAKE_IP_FILTER_SUFFIXES: &[&str] = &[
    "lan",
    "local",
    "msftconnecttest.com",
    "msftncsi.com",
    "captive.apple.com",
    "time.windows.com",
    "time.apple.com",
    "time.android.com",
    "ntp.org",
];

/// The full sing-box client configuration for one version profile: log, DNS
/// (fake-ip with a direct resolver and a proxied DoH fallback), the tun
/// inbound, grouped outbounds, rule-set routing, and the clash API used by
/// dashboards and sbtui. Field differences between sing-box versions are
/// concentrated here, guided by the changelog research in
/// `docs/research/sing-box-client-version-differences.md`.
fn sing_box_full(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
    profile: &SingBoxVersionProfile,
) -> Result<String, SubscriptionError> {
    // Pre-1.12 client cores have no AnyTLS outbound, so those profiles must
    // silently drop the node; refuse to generate an empty-node artifact and
    // say exactly which knob to turn instead.
    let compatible_nodes: Vec<CanonicalNode> = nodes
        .iter()
        .filter(|node| profile.supports_anytls || node.protocol() != ManagedProtocol::Anytls)
        .cloned()
        .collect();
    if compatible_nodes.is_empty() {
        return Err(SubscriptionError::ClientIncompatible(format!(
            "sing-box {} 客户端内核不支持 AnyTLS 协议（1.12.0 才加入）；\
             请在部署中启用至少一个其他协议，否则请移除 sing-box-{}.json 适配",
            profile.version, profile.version
        )));
    }
    let nodes = &compatible_nodes;
    let node_tags: Vec<&str> = nodes.iter().map(CanonicalNode::tag).collect();
    let mut outbounds = client_outbounds(config, nodes);
    let mut selector_members: Vec<&str> = vec![AUTO_TAG, DIRECT_TAG];
    selector_members.extend(node_tags.iter().copied());
    outbounds.push(json!({
        "type": "selector",
        "tag": SELECTOR_TAG,
        "outbounds": selector_members,
        "interrupt_exist_connections": false
    }));
    outbounds.push(json!({
        "type": "urltest",
        "tag": AUTO_TAG,
        "outbounds": node_tags,
        "url": config.client_latency_probe_url,
        "interval": "5m",
        "tolerance": 50,
        "idle_timeout": "30m"
    }));
    outbounds.push(json!({"type": "direct", "tag": DIRECT_TAG}));

    let fake_ip = config.client_dns_mode == crate::config::ClientDnsMode::FakeIp;
    // 1.12+ requires typed DNS server objects; 1.10/1.11 only accept the
    // legacy address-string format, with fake-ip as a special `fakeip`
    // address plus a top-level dns.fakeip object (removed in 1.14).
    let mut dns = if profile.typed_dns {
        let mut dns_servers = vec![
            json!({"type": "udp", "tag": "dns-direct", "server": "223.5.5.5"}),
            json!({"type": "https", "tag": "dns-proxy", "server": "1.1.1.1", "detour": SELECTOR_TAG}),
        ];
        if fake_ip {
            dns_servers.push(json!({
                "type": "fakeip",
                "tag": "dns-fakeip",
                "inet4_range": "198.18.0.0/15",
                "inet6_range": "fc00::/18"
            }));
        }
        json!({"servers": dns_servers})
    } else {
        let mut dns_servers = vec![
            json!({"tag": "dns-direct", "address": "223.5.5.5"}),
            json!({"tag": "dns-proxy", "address": "https://1.1.1.1/dns-query", "detour": SELECTOR_TAG}),
        ];
        if fake_ip {
            dns_servers.push(json!({"tag": "dns-fakeip", "address": "fakeip"}));
        }
        let mut dns = json!({"servers": dns_servers});
        if fake_ip {
            dns["fakeip"] = json!({
                "enabled": true,
                "inet4_range": "198.18.0.0/15",
                "inet6_range": "fc00::/18"
            });
        }
        dns
    };

    let mut dns_rules = Vec::new();
    if !profile.typed_dns {
        // Pre-1.12 cores have no route.default_domain_resolver; the legacy
        // `outbound: any` DNS rule (removed in 1.14) resolves proxy server
        // domains through direct DNS instead.
        dns_rules.push(json!({"outbound": "any", "server": "dns-direct"}));
    }
    dns_rules.push(json!({"clash_mode": "Direct", "server": "dns-direct"}));
    dns_rules.push(json!({"clash_mode": "Global", "server": "dns-proxy"}));
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        dns_rules.push(json!({"rule_set": ["geosite-cn"], "server": "dns-direct"}));
    }
    dns_rules.push(json!({
        "domain_suffix": FAKE_IP_FILTER_SUFFIXES,
        "server": "dns-direct"
    }));
    if fake_ip {
        dns_rules.push(json!({"query_type": ["A", "AAAA"], "server": "dns-fakeip"}));
    }
    // `independent_cache` is deprecated in 1.14 and removed in 1.16, and
    // brings no benefit here, so the DNS object stays lean across versions.
    dns["rules"] = json!(dns_rules);
    dns["final"] = json!("dns-proxy");

    let legacy_route = !profile.route_rule_actions;
    let mut tun = json!({
        "type": "tun",
        "tag": "tun-in",
        "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
        "mtu": 9000,
        "auto_route": true,
        "strict_route": true,
        "stack": "mixed"
    });
    if legacy_route {
        // 1.10 has no route rule actions; protocol sniffing is configured on
        // the inbound and DNS is hijacked through a special `dns` outbound.
        tun["sniff"] = json!(true);
    }

    let mut route_rules = Vec::new();
    if !legacy_route {
        route_rules.push(json!({"action": "sniff"}));
    }
    route_rules.push(if legacy_route {
        json!({"protocol": "dns", "outbound": "dns-out"})
    } else {
        json!({"protocol": "dns", "action": "hijack-dns"})
    });
    route_rules.push(json!({"ip_is_private": true, "outbound": DIRECT_TAG}));
    route_rules.push(json!({
        "domain_suffix": AI_DOMAIN_SUFFIXES,
        "outbound": SELECTOR_TAG
    }));
    let mut rule_sets: Vec<Value> = Vec::new();
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        route_rules.push(json!({"rule_set": ["geosite-cn", "geoip-cn"], "outbound": DIRECT_TAG}));
        rule_sets.push(remote_rule_set(
            "geosite-cn",
            &format!("{}/geosite/cn.srs", sing_box_rule_set_base(config)),
        ));
        rule_sets.push(remote_rule_set(
            "geoip-cn",
            &format!("{}/geoip/cn.srs", sing_box_rule_set_base(config)),
        ));
    }
    if legacy_route {
        outbounds.push(json!({"type": "dns", "tag": "dns-out"}));
    }
    let mut route = json!({
        "rules": route_rules,
        "rule_set": rule_sets,
        "final": SELECTOR_TAG,
        "auto_detect_interface": true
    });
    if profile.typed_dns {
        route["default_domain_resolver"] = json!({"server": "dns-direct"});
    }

    let mut cache_file = json!({"enabled": true, "store_fakeip": fake_ip});
    if profile.supports_store_dns {
        cache_file["store_dns"] = json!(true);
    }

    Ok(serde_json::to_string_pretty(&json!({
        "log": {"level": "info", "timestamp": true},
        "dns": dns,
        "inbounds": [tun],
        "outbounds": outbounds,
        "route": route,
        "experimental": {
            "clash_api": {
                "external_controller": "127.0.0.1:9090",
                "default_mode": "rule"
            },
            "cache_file": cache_file
        }
    }))
    .expect("JSON values serialize"))
}

/// The rule-set base for sing-box artifacts: the configured source root plus
/// the `@sing` branch that carries the `.srs` binary rule-sets.
fn sing_box_rule_set_base(config: &DeploymentConfig) -> String {
    format!(
        "{}@sing/geo",
        config.client_rule_set_base_url.trim_end_matches('/')
    )
}

fn remote_rule_set(tag: &str, url: &str) -> Value {
    json!({
        "type": "remote",
        "tag": tag,
        "format": "binary",
        "url": url,
        // Deprecated in 1.14 (moved to route.http_clients) but only removed
        // in 1.16, so every profile in the registry still accepts it.
        "download_detour": SELECTOR_TAG,
        "update_interval": "1d"
    })
}

/// Hosts without an IPv6 route cannot dial the AAAA addresses the default
/// resolution strategy prefers, so their server configuration pins IPv4 even
/// when the deployment never opted in; the explicit flag forces the same
/// restriction on dual-stack hosts.
fn ipv4_only_required(config: &DeploymentConfig) -> bool {
    config.ipv4_only || !host_has_ipv6_route()
}

/// UDP `connect` performs a route lookup without sending a packet, which makes
/// it a cheap probe for an IPv6 default route.
fn host_has_ipv6_route() -> bool {
    let Ok(socket) = std::net::UdpSocket::bind("[::]:0") else {
        return false;
    };
    socket.connect("[2001:4860:4860::8888]:443").is_ok()
}

fn sing_box_server(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
    root: &Path,
) -> Result<String, SubscriptionError> {
    let certificate = certificate_tls_config(config, root)?;
    let mut inbounds = Vec::new();
    let mut tags = Vec::new();
    for node in nodes {
        tags.push(node.tag());
        inbounds.push(match &node {
            CanonicalNode::VlessReality {
                port,
                uuid,
                private_key,
                short_id,
                decoy_sni,
                ..
            } => json!({"type": "vless", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "flow": "xtls-rprx-vision"}],
                "tls": {"enabled": true, "server_name": decoy_sni, "reality": {"enabled": true,
                    "handshake": {"server": decoy_sni, "server_port": 443}, "private_key": private_key,
                    "short_id": [short_id]}}}),
            CanonicalNode::VmessWebsocket {
                port,
                tls_server_name,
                uuid,
                path,
                ..
            } => json!({"type": "vmess", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "alterId": 0}],
                "transport": {"type": "ws", "path": path},
                "tls": server_tls(tls_server_name, &certificate, &[])}),
            CanonicalNode::Hysteria2 {
                port,
                tls_server_name,
                password,
                ..
            } => json!({"type": "hysteria2", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"password": password}],
                "tls": server_tls(tls_server_name, &certificate, &["h3"])}),
            CanonicalNode::Tuic {
                port,
                tls_server_name,
                uuid,
                password,
                ..
            } => json!({"type": "tuic", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"uuid": uuid, "password": password}],
                "tls": server_tls(tls_server_name, &certificate, &["h3"])}),
            CanonicalNode::Anytls {
                port,
                tls_server_name,
                password,
                ..
            } => json!({"type": "anytls", "tag": node.tag(), "listen": "::",
                "listen_port": port, "users": [{"password": password}],
                "tls": server_tls(tls_server_name, &certificate, &[])}),
        });
    }
    let mut server = json!({
        // Connection-level debug logging would expose proxied destinations.
        "log": {"level": "info"},
        "inbounds": inbounds
    });
    if ipv4_only_required(config) {
        // The inbound `domain_strategy` field was deprecated in 1.11 and
        // removed in 1.13, so the destination pin now lives on a route action
        // (the documented migration). Pinning the default DNS strategy keeps
        // every other lookup, including the Reality camouflage handshake that
        // dials its decoy independently of the inbound destination, on IPv4.
        server["dns"] = json!({"strategy": "ipv4_only"});
        server["route"] = json!({
            "rules": tags
                .iter()
                .map(|tag| json!({"inbound": tag, "action": "resolve", "strategy": "ipv4_only"}))
                .collect::<Vec<_>>()
        });
    }
    Ok(serde_json::to_string_pretty(&server).expect("JSON values serialize"))
}

/// The `proxies:` block plus the two historical groups shared by the current
/// and the legacy clash artifacts, so protocol fields cannot drift apart.
fn clash_proxies(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let skip = client_skip_cert_verify(config);
    let mut proxies = String::from("proxies:\n");
    for node in nodes {
        let entry = match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => format!(
                "  - name: {}\n    type: vless\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    network: tcp\n    udp: true\n    flow: xtls-rprx-vision\n    tls: true\n    servername: {decoy_sni}\n    client-fingerprint: chrome\n    reality-opts:\n      public-key: {public_key}\n      short-id: {short_id}\n",
                node.tag()
            ),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => format!(
                "  - name: {}\n    type: vmess\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    alterId: 0\n    cipher: auto\n    tls: true\n    servername: {tls_server_name}\n    skip-cert-verify: {skip}\n    network: ws\n    ws-opts:\n      path: {path}\n      headers:\n        Host: {tls_server_name}\n",
                node.tag()
            ),
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => format!(
                "  - name: {}\n    type: hysteria2\n    server: {host}\n    port: {port}\n    password: {password}\n    sni: {tls_server_name}\n    skip-cert-verify: {skip}\n",
                node.tag()
            ),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => format!(
                "  - name: {}\n    type: tuic\n    server: {host}\n    port: {port}\n    uuid: {uuid}\n    password: {password}\n    sni: {tls_server_name}\n    alpn:\n      - h3\n    skip-cert-verify: {skip}\n",
                node.tag()
            ),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => format!(
                "  - name: {}\n    type: anytls\n    server: {host}\n    port: {port}\n    password: {password}\n    client-fingerprint: chrome\n    udp: true\n    idle-session-check-interval: 30\n    idle-session-timeout: 30\n    tls: true\n    sni: {tls_server_name}\n    skip-cert-verify: {skip}\n",
                node.tag()
            ),
        };
        proxies.push_str(&entry);
    }
    proxies.push_str(concat!(
        "mode: rule\n",
        "proxy-groups:\n",
        "  - name: 🌍选择代理节点\n",
        "    type: select\n",
        // The selector holds DIRECT, so latency tests must use a URL that is
        // reachable without a proxy; gstatic would time out from China.
        // aliyun.com answers with a redirect, which mihomo counts as success.
        "    url: http://aliyun.com/generate_204\n",
        "    interval: 300\n",
        "    proxies:\n",
        "      - ♻️自动选择\n",
        "      - DIRECT\n",
    ));
    for node in nodes {
        proxies.push_str(&format!("      - {}\n", node.tag()));
    }
    proxies.push_str(concat!(
        "  - name: ♻️自动选择\n",
        "    type: url-test\n",
        "    url: http://www.gstatic.com/generate_204\n",
        "    interval: 300\n",
        "    tolerance: 50\n",
        "    proxies:\n",
    ));
    for node in nodes {
        proxies.push_str(&format!("      - {}\n", node.tag()));
    }
    proxies.push_str("  - name: 🎯全球直连\n    type: select\n    proxies:\n      - DIRECT\n");
    for node in nodes {
        proxies.push_str(&format!("      - {}\n", node.tag()));
    }
    Ok(proxies)
}

/// The `dns:` block shared by the current and legacy clash artifacts; the
/// fake-ip filter keeps LAN names, OS connectivity checks, and NTP on real
/// DNS answers so captive-portal detection keeps working.
fn clash_dns(config: &DeploymentConfig) -> String {
    let mode = match config.client_dns_mode {
        crate::config::ClientDnsMode::FakeIp => "fake-ip",
        crate::config::ClientDnsMode::RedirHost => "redir-host",
    };
    let mut dns = format!(
        "dns:\n  enable: true\n  ipv6: false\n  enhanced-mode: {mode}\n  fake-ip-range: 198.18.0.1/16\n  fake-ip-filter:\n"
    );
    for suffix in FAKE_IP_FILTER_SUFFIXES {
        dns.push_str(&format!("    - '+.{suffix}'\n"));
    }
    dns.push_str(concat!(
        "  use-hosts: false\n  use-system-hosts: false\n",
        "  nameserver:\n    - 'https://1.1.1.1/dns-query#🌍选择代理节点'\n",
        "    - 'https://8.8.8.8/dns-query#🌍选择代理节点'\n",
        "  proxy-server-nameserver:\n    - https://223.5.5.5/dns-query\n",
    ));
    dns
}

fn clash(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
    let mut output = clash_proxies(config, nodes)?;
    // The AI suffix rules must precede the CN rule-set so OpenAI/X domains
    // never fall into geosite-cn's direct verdict.
    output.push_str("rules:\n");
    for suffix in AI_DOMAIN_SUFFIXES {
        output.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},🌍选择代理节点\n"));
    }
    if config.client_rule_profile == crate::config::ClientRuleProfile::Standard {
        let base = format!(
            "{}@meta/geo",
            config.client_rule_set_base_url.trim_end_matches('/')
        );
        output.push_str(&format!(
            concat!(
                "  - RULE-SET,geosite-private,🎯全球直连\n",
                "  - RULE-SET,geoip-private,🎯全球直连\n",
                "  - RULE-SET,geosite-cn,🎯全球直连\n",
                "  - RULE-SET,geoip-cn,🎯全球直连\n",
                "  - MATCH,🌍选择代理节点\n",
                "rule-providers:\n",
                "  geosite-private:\n",
                "    type: http\n",
                "    behavior: domain\n",
                "    format: mrs\n",
                "    url: {base}/geosite/private.mrs\n",
                "    path: ./ruleset/geosite-private.mrs\n",
                "    interval: 86400\n",
                "  geoip-private:\n",
                "    type: http\n",
                "    behavior: ipcidr\n",
                "    format: mrs\n",
                "    url: {base}/geoip/private.mrs\n",
                "    path: ./ruleset/geoip-private.mrs\n",
                "    interval: 86400\n",
                "  geosite-cn:\n",
                "    type: http\n",
                "    behavior: domain\n",
                "    format: mrs\n",
                "    url: {base}/geosite/cn.mrs\n",
                "    path: ./ruleset/geosite-cn.mrs\n",
                "    interval: 86400\n",
                "  geoip-cn:\n",
                "    type: http\n",
                "    behavior: ipcidr\n",
                "    format: mrs\n",
                "    url: {base}/geoip/cn.mrs\n",
                "    path: ./ruleset/geoip-cn.mrs\n",
                "    interval: 86400\n",
            ),
            base = base
        ));
    } else {
        output.push_str(concat!(
            "  - GEOIP,LAN,DIRECT\n",
            "  - GEOIP,CN,DIRECT\n",
            "  - MATCH,🌍选择代理节点\n",
        ));
    }
    output.push_str(&clash_dns(config));
    Ok(output)
}

/// The mihomo 1.18.x compatibility artifact: same node and group layout as the
/// current artifact, but with the pre-rule-set built-in GEOIP rules that the
/// previous major line shipped everywhere.
fn clash_legacy(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let mut output = clash_proxies(config, nodes)?;
    output.push_str("rules:\n");
    for suffix in AI_DOMAIN_SUFFIXES {
        output.push_str(&format!("  - DOMAIN-SUFFIX,{suffix},🌍选择代理节点\n"));
    }
    output.push_str(concat!(
        "  - GEOIP,LAN,DIRECT\n",
        "  - GEOIP,CN,DIRECT\n",
        "  - MATCH,🌍选择代理节点\n",
    ));
    output.push_str(&clash_dns(config));
    Ok(output)
}

/// The Shadowrocket-adapted Base64 URI list (research:
/// `docs/research/sing-box-client-version-differences.md` §6). Differences
/// from the plain `uri` rendering: passwords and SNI values are always
/// percent-encoded (Shadowrocket 2.2.44 fixed URI password decoding, which
/// implies special characters must arrive encoded), TUIC carries
/// `udp_relay_mode`, and AnyTLS follows the official anytls-go scheme with
/// the path slash and without the non-standard `security` parameter.
fn shadowrocket(
    config: &DeploymentConfig,
    nodes: &[CanonicalNode],
) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    for node in nodes {
        match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => uris.push_str(&format!("vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={}&fp=chrome&pbk={public_key}&sid={short_id}&type=tcp#{}\n", percent_encode(decoy_sni), node.tag())),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => {
                let payload = json!({"v": "2", "ps": node.tag(), "add": host, "port": port.to_string(), "id": uuid, "aid": "0", "scy": "auto", "net": "ws", "type": "none", "host": tls_server_name, "path": path, "tls": "tls", "sni": tls_server_name});
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&payload).expect("JSON values serialize"));
                uris.push_str(&format!("vmess://{encoded}\n"));
            }
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "hysteria2://{}@{}:{}?insecure={}&sni={}#{}\n",
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{}:{}@{}:{}?congestion_control=bbr&udp_relay_mode=native&alpn=h3&insecure={}&sni={}#{}\n",
                percent_encode(uuid),
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{}@{}:{}/?insecure={}&sni={}#{}\n",
                percent_encode(password),
                host,
                port,
                insecure,
                percent_encode(tls_server_name),
                node.tag()
            )),
        }
    }
    Ok(base64_uri(&uris))
}

/// Percent-encodes everything outside the RFC 3986 unreserved set so secrets
/// with special characters survive URI parsing in every client.
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn uri(config: &DeploymentConfig, nodes: &[CanonicalNode]) -> Result<String, SubscriptionError> {
    let insecure = if client_skip_cert_verify(config) {
        1
    } else {
        0
    };
    let mut uris = String::new();
    for node in nodes {
        match &node {
            CanonicalNode::VlessReality {
                host,
                port,
                uuid,
                public_key,
                short_id,
                decoy_sni,
                ..
            } => uris.push_str(&format!("vless://{uuid}@{host}:{port}?encryption=none&flow=xtls-rprx-vision&security=reality&sni={decoy_sni}&fp=chrome&pbk={public_key}&sid={short_id}&type=tcp#{}\n", node.tag())),
            CanonicalNode::VmessWebsocket {
                host,
                port,
                tls_server_name,
                uuid,
                path,
            } => {
                let payload = json!({"v": "2", "ps": node.tag(), "add": host, "port": port.to_string(), "id": uuid, "aid": "0", "scy": "auto", "net": "ws", "type": "none", "host": tls_server_name, "path": path, "tls": "tls", "sni": tls_server_name});
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&payload).expect("JSON values serialize"));
                uris.push_str(&format!("vmess://{encoded}\n"));
            }
            CanonicalNode::Hysteria2 {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "hysteria2://{password}@{host}:{port}?insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Tuic {
                host,
                port,
                tls_server_name,
                uuid,
                password,
            } => uris.push_str(&format!(
                "tuic://{uuid}:{password}@{host}:{port}?congestion_control=bbr&alpn=h3&insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
            CanonicalNode::Anytls {
                host,
                port,
                tls_server_name,
                password,
            } => uris.push_str(&format!(
                "anytls://{password}@{host}:{port}?security=tls&insecure={insecure}&sni={tls_server_name}#{}\n",
                node.tag()
            )),
        }
    }
    Ok(uris)
}

fn base64_uri(uri: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(uri.as_bytes())
}

/// The certificate path written into the sing-box server configuration for the
/// TLS-terminating Managed protocols. Direct subscription mode uses the pinned
/// copy that the deploy hook grants to the `sbctl` and `sing-box` accounts.
/// External proxy mode leaves certificate management entirely to the existing
/// reverse proxy and its own Certbot setup.
/// The Managed protocol listeners present this certificate to their clients.
/// `SelfSigned` mode generates a long-lived self-signed certificate (sing-box-yg
/// style, never expires, no ACME dependency) that clients are told to skip
/// verifying; `Domain` mode uses the administrator-managed certificate.
fn certificate_tls_config(
    config: &DeploymentConfig,
    root: &Path,
) -> Result<Value, SubscriptionError> {
    let (certificate_path, key_path) = match config.certificate_mode {
        CertificateMode::SelfSigned => ensure_self_signed_certificate(config, root)?,
        CertificateMode::Domain => {
            if config.subscription_mode == SubscriptionMode::Direct {
                let directory = crate::config::DeploymentStore::certificate_directory_absolute(
                    &config.subscription_host,
                );
                (
                    directory
                        .join("fullchain.pem")
                        .to_string_lossy()
                        .into_owned(),
                    directory.join("privkey.pem").to_string_lossy().into_owned(),
                )
            } else {
                (
                    format!(
                        "/etc/letsencrypt/live/{}/fullchain.pem",
                        config.subscription_host
                    ),
                    format!(
                        "/etc/letsencrypt/live/{}/privkey.pem",
                        config.subscription_host
                    ),
                )
            }
        }
    };
    Ok(
        json!({"enabled": true, "server_name": config.protocol_server_name(),
        "certificate_path": certificate_path,
        "key_path": key_path}),
    )
}

/// Generates and pins a long-lived self-signed certificate for the subscription
/// host, or reuses the pinned copy. The certificate stays valid for 36500 days,
/// matching the sing-box-yg default, so the proxy listeners never break on an
/// expired administrator-managed certificate. Files are created private
/// (directory 0750, key and certificate 0640) so the TLS private key is never
/// world-readable, even before the daemon-storage preparation runs.
fn ensure_self_signed_certificate(
    config: &DeploymentConfig,
    root: &Path,
) -> Result<(String, String), SubscriptionError> {
    let server_name = config.protocol_server_name();
    let directory = self_signed_certificate_directory(root, server_name);
    let certificate_path = directory.join("cert.pem");
    let key_path = directory.join("key.pem");
    if certificate_path.is_file() && key_path.is_file() {
        return Ok((
            certificate_path.to_string_lossy().into_owned(),
            key_path.to_string_lossy().into_owned(),
        ));
    }
    let key_pair =
        KeyPair::generate().map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    let mut params = CertificateParams::new(vec![server_name.to_owned()])
        .map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    params
        .distinguished_name
        .push(DnType::CommonName, server_name.to_owned());
    params
        .distinguished_name
        .push(DnType::OrganizationName, "sbctl");
    let certificate = params
        .self_signed(&key_pair)
        .map_err(|error| SubscriptionError::Certificate(error.to_string()))?;
    fs::create_dir_all(&directory).map_err(SubscriptionError::Artifact)?;
    restrict_directory_permissions(&directory)?;
    write_private_file(&certificate_path, certificate.pem().as_bytes())?;
    write_private_file(&key_path, key_pair.serialize_pem().as_bytes())?;
    Ok((
        certificate_path.to_string_lossy().into_owned(),
        key_path.to_string_lossy().into_owned(),
    ))
}

/// The directory holding the long-lived self-signed certificate for a protocol
/// SNI. The live host uses the absolute path consumed by the generated sing-box
/// configuration and the service accounts; a fixture root keeps every write
/// inside that root so tests and `--root` operations never touch host storage.
fn self_signed_certificate_directory(root: &Path, server_name: &str) -> std::path::PathBuf {
    if root == Path::new("/") {
        Path::new(crate::config::CERTIFICATES_ABSOLUTE_PATH).join(server_name)
    } else {
        root.join(crate::config::CERTIFICATES_RELATIVE_PATH)
            .join(server_name)
    }
}

fn restrict_directory_permissions(directory: &Path) -> Result<(), SubscriptionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o750))
            .map_err(SubscriptionError::Artifact)?;
    }
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<(), SubscriptionError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o640)
            .open(path)
            .and_then(|mut file| file.write_all(contents))
            .map_err(SubscriptionError::Artifact)
    }
    #[cfg(not(unix))]
    fs::write(path, contents).map_err(SubscriptionError::Artifact)
}

/// Clients connecting to a self-signed certificate must be told to skip
/// verification; the domain certificate is verified normally.
fn client_skip_cert_verify(config: &DeploymentConfig) -> bool {
    config.certificate_mode == CertificateMode::SelfSigned
}

fn server_tls(tls_server_name: &str, certificate: &Value, alpn: &[&str]) -> Value {
    let mut tls = json!({"enabled": true, "server_name": tls_server_name,
        "certificate_path": certificate["certificate_path"],
        "key_path": certificate["key_path"]});
    if !alpn.is_empty() {
        tls["alpn"] = json!(alpn);
    }
    tls
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut different = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        different |= usize::from(*left.get(index).unwrap_or(&0) ^ *right.get(index).unwrap_or(&0));
    }
    different == 0
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{
        SING_BOX_VERSION_PROFILES, SubscriptionFormat, clash, clash_legacy,
        client_subscription_matrix, generated_artifacts, latest_version_profile, regenerate,
        shadowrocket, sing_box, sing_box_full, uri,
    };
    use crate::config::{
        DeploymentConfig, DeploymentStore, ManagedProtocol, ProtocolPorts, SubscriptionMode,
    };

    async fn http_get(port: u16, path: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("subscription service accepts connections");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .expect("request is sent");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("response is readable");
        response
    }

    /// Establishes the minimal traffic fixture so a subscription response can
    /// read a legal accounting state and the generated URI artifact.
    fn seed_direct_subscription(fixture: &TempDir) -> (DeploymentStore, DeploymentConfig, String) {
        let statistics = fixture.path().join("sys/class/net/ens3/statistics");
        fs::create_dir_all(&statistics).expect("statistics directory is created");
        fs::write(statistics.join("rx_bytes"), "100\n").expect("RX counter is written");
        fs::write(statistics.join("tx_bytes"), "200\n").expect("TX counter is written");
        let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
        fs::create_dir_all(boot_path.parent().expect("boot ID has a parent"))
            .expect("boot ID directory is created");
        fs::write(boot_path, "boot-a").expect("boot ID is written");
        let store = DeploymentStore::new(fixture.path());
        let config = DeploymentConfig::new(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("a Direct VLESS deployment is valid");
        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");
        let references = artifacts
            .iter()
            .map(|(name, contents)| (name.clone(), contents.as_bytes()))
            .collect::<Vec<_>>();
        store
            .initialize_with_artifacts(&config, &references)
            .expect("subscription deployment is initialized");
        crate::traffic::reset(&store, &config).expect("accounting state is established");
        let credential = config.subscription_credential.clone();
        (store, config, credential)
    }

    /// A Direct deployment with every Managed protocol enabled, used to verify
    /// per-version client compatibility (AnyTLS availability, DNS format).
    fn seed_all_protocols(fixture: &TempDir) -> (DeploymentStore, DeploymentConfig, String) {
        let statistics = fixture.path().join("sys/class/net/ens3/statistics");
        fs::create_dir_all(&statistics).expect("statistics directory is created");
        fs::write(statistics.join("rx_bytes"), "100\n").expect("RX counter is written");
        fs::write(statistics.join("tx_bytes"), "200\n").expect("TX counter is written");
        let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
        fs::create_dir_all(boot_path.parent().expect("boot ID has a parent"))
            .expect("boot ID directory is created");
        fs::write(boot_path, "boot-a").expect("boot ID is written");
        let store = DeploymentStore::new(fixture.path());
        let config = DeploymentConfig::new(
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
        )
        .expect("a five-protocol deployment is valid");
        let credential = config.subscription_credential.clone();
        (store, config, credential)
    }

    /// A Direct deployment with exactly one Managed protocol enabled.
    fn seed_single_protocol(
        fixture: &TempDir,
        protocol: ManagedProtocol,
    ) -> (DeploymentStore, DeploymentConfig, String) {
        let statistics = fixture.path().join("sys/class/net/ens3/statistics");
        fs::create_dir_all(&statistics).expect("statistics directory is created");
        fs::write(statistics.join("rx_bytes"), "100\n").expect("RX counter is written");
        fs::write(statistics.join("tx_bytes"), "200\n").expect("TX counter is written");
        let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
        fs::create_dir_all(boot_path.parent().expect("boot ID has a parent"))
            .expect("boot ID directory is created");
        fs::write(boot_path, "boot-a").expect("boot ID is written");
        let store = DeploymentStore::new(fixture.path());
        let config = DeploymentConfig::new(
            SubscriptionMode::Direct,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![protocol],
            Some("www.cloudflare.com".into()),
        )
        .expect("a single-protocol deployment is valid");
        let credential = config.subscription_credential.clone();
        (store, config, credential)
    }

    #[test]
    fn parse_route_maps_every_matrix_route_and_rejects_malformed_paths() {
        use super::{SubscriptionFormat, SubscriptionRoute, parse_route};
        let credential = "cred";
        let cases = [
            (
                "sing-box.json",
                SubscriptionRoute::Format(SubscriptionFormat::SingBox),
            ),
            (
                "sing-box-full.json",
                SubscriptionRoute::Format(SubscriptionFormat::SingBoxFull),
            ),
            (
                "clash.yaml",
                SubscriptionRoute::Format(SubscriptionFormat::Clash),
            ),
            (
                "clash-1.18.yaml",
                SubscriptionRoute::Format(SubscriptionFormat::ClashLegacy(
                    super::CLASH_LEGACY_VERSION,
                )),
            ),
            ("uri", SubscriptionRoute::Format(SubscriptionFormat::Uri)),
            (
                "uri.txt",
                SubscriptionRoute::Format(SubscriptionFormat::Base64Uri),
            ),
            (
                "shadowrocket.txt",
                SubscriptionRoute::Format(SubscriptionFormat::Shadowrocket),
            ),
            ("qr/uri", SubscriptionRoute::Qr(SubscriptionFormat::Uri)),
            (
                "qr/sing-box-full.json",
                SubscriptionRoute::Qr(SubscriptionFormat::SingBoxFull),
            ),
            ("index", SubscriptionRoute::Index),
        ];
        for (path, route) in cases {
            let target = format!("/sub/{credential}/{path}");
            assert_eq!(
                parse_route(&target),
                Some((credential, route)),
                "route {path} must parse"
            );
        }
        for profile in super::SING_BOX_VERSION_PROFILES {
            let target = format!("/sub/{credential}/sing-box-{}.json", profile.version);
            assert_eq!(
                parse_route(&target),
                Some((
                    credential,
                    SubscriptionRoute::Format(SubscriptionFormat::SingBoxVersion(profile.version)),
                )),
                "version profile {} must parse",
                profile.version
            );
        }
    }

    #[test]
    fn parse_route_rejects_query_unknown_and_trailing_paths() {
        use super::parse_route;
        for target in [
            "/sub/cred/uri?credential=cred",
            "/sub/cred/bogus",
            "/sub/cred/sing-box-1.09.json",
            "/sub/cred/clash-1.17.yaml",
            "/sub/cred/uri/extra",
            "/sub/cred/qr",
            "/sub/cred/qr/index",
            "/sub/cred",
            "/sub/",
            "/other/cred/uri",
        ] {
            assert!(parse_route(target).is_none(), "must reject {target}");
        }
        // An empty credential parses but can never match the real one, so the
        // handler still returns a uniform 404 before reading any artifact.
        assert_eq!(
            parse_route("/sub//uri"),
            Some((
                "",
                super::SubscriptionRoute::Format(super::SubscriptionFormat::Uri)
            ))
        );
    }

    #[test]
    fn route_url_builds_matrix_links_for_formats_qr_and_index() {
        use super::{SubscriptionFormat, SubscriptionRoute, route_url, subscription_url};
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, credential) = seed_direct_subscription(&fixture);
        let base = format!("https://sub.example.test/sub/{credential}");
        assert_eq!(
            subscription_url(&config, SubscriptionFormat::SingBox).expect("url builds"),
            format!("{base}/sing-box.json")
        );
        assert_eq!(
            route_url(&config, SubscriptionRoute::Qr(SubscriptionFormat::Uri))
                .expect("qr url builds"),
            format!("{base}/qr/uri")
        );
        assert_eq!(
            route_url(&config, SubscriptionRoute::Index).expect("index url builds"),
            format!("{base}/index")
        );
    }

    #[test]
    fn the_bare_sing_box_artifact_stays_outbounds_only_and_legacy_uri_forms_are_stable() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_direct_subscription(&fixture);
        let snapshot =
            |artifacts: Vec<(String, String)>| -> std::collections::BTreeMap<String, String> {
                artifacts.into_iter().collect()
            };
        let first =
            snapshot(generated_artifacts(&config, fixture.path()).expect("artifacts generate"));
        let second =
            snapshot(generated_artifacts(&config, fixture.path()).expect("artifacts regenerate"));
        for name in [
            "subscription-sing-box.json",
            "subscription-uri.txt",
            "subscription-base64-uri.txt",
        ] {
            assert_eq!(first[name], second[name], "{name} must be deterministic");
        }
        let bare: serde_json::Value = serde_json::from_str(&first["subscription-sing-box.json"])
            .expect("bare artifact is JSON");
        let object = bare.as_object().expect("bare artifact is a JSON object");
        assert_eq!(
            object.len(),
            1,
            "the bare sing-box artifact must stay outbounds-only"
        );
        assert!(object.contains_key("outbounds"));
        let base64 = first["subscription-base64-uri.txt"].clone();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(base64.trim())
                .expect("base64 artifact decodes"),
            first["subscription-uri.txt"].as_bytes(),
            "the base64 artifact must stay the exact URI artifact"
        );
    }

    #[test]
    fn client_overrides_merge_into_full_profiles_but_never_the_bare_artifact() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_direct_subscription(&fixture);
        let overrides = fixture.path().join("etc/sbctl/overrides");
        fs::create_dir_all(&overrides).expect("override directory is created");
        fs::write(
            overrides.join("sing-box-override.json"),
            r#"{"route":{"rules":[{"domain_suffix":["novixlink"],"outbound":"🚀节点选择"}]}}"#,
        )
        .expect("sing-box override is written");
        fs::write(
            overrides.join("clash-override.yaml"),
            "rules:\n  - DOMAIN-SUFFIX,novixlink,🚀选择代理节点\n",
        )
        .expect("clash override is written");
        let artifacts = generated_artifacts(&config, fixture.path())
            .expect("artifacts generate with overrides");
        let get = |name: &str| {
            artifacts
                .iter()
                .find(|(artifact, _)| artifact == name)
                .map(|(_, contents)| contents.clone())
                .unwrap_or_else(|| panic!("missing artifact {name}"))
        };

        let full: serde_json::Value =
            serde_json::from_str(&get("subscription-sing-box-full.json")).expect("full is JSON");
        assert_eq!(
            full["route"]["rules"][0]["domain_suffix"][0], "novixlink",
            "the override rule must be prepended to the generated route rules"
        );
        let versioned: serde_json::Value =
            serde_json::from_str(&get("subscription-sing-box-1.12.json"))
                .expect("versioned profile is JSON");
        assert_eq!(
            versioned["route"]["rules"][0]["domain_suffix"][0],
            "novixlink"
        );

        let bare: serde_json::Value =
            serde_json::from_str(&get("subscription-sing-box.json")).expect("bare is JSON");
        assert!(
            bare.get("route").is_none(),
            "the historical bare artifact must never gain override fields"
        );

        for name in ["subscription-clash.yaml", "subscription-clash-1.18.yaml"] {
            let clash: serde_yaml::Value =
                serde_yaml::from_str(&get(name)).expect("clash artifact is YAML");
            let rules = clash["rules"].as_sequence().expect("clash has rules");
            assert!(
                rules[0]
                    .as_str()
                    .expect("the first rule is a string")
                    .contains("novixlink"),
                "{name} must prepend the override rule"
            );
        }
    }

    #[test]
    fn an_invalid_override_aborts_regeneration_and_preserves_the_previous_artifacts() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, _) = seed_direct_subscription(&fixture);
        regenerate(&store, &config, None, false).expect("baseline artifacts regenerate");
        let baseline = artifact(&store, "subscription-sing-box-full.json");
        let overrides = fixture.path().join("etc/sbctl/overrides");
        fs::create_dir_all(&overrides).expect("override directory is created");
        fs::write(overrides.join("sing-box-override.json"), "{ not valid json")
            .expect("invalid override is written");

        let error = regenerate(&store, &config, None, false).expect_err("invalid override aborts");
        assert!(
            matches!(error, super::SubscriptionError::Override(_)),
            "unexpected error: {error}"
        );
        assert_eq!(
            artifact(&store, "subscription-sing-box-full.json"),
            baseline,
            "a rejected override must not touch the served artifacts"
        );
    }

    #[test]
    fn minimal_rule_profile_drops_remote_rule_sets_while_standard_keeps_them() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, mut config, _) = seed_direct_subscription(&fixture);
        let full = |config: &DeploymentConfig| {
            let artifacts =
                generated_artifacts(config, fixture.path()).expect("artifacts generate");
            let contents = artifacts
                .iter()
                .find(|(name, _)| name == "subscription-sing-box-full.json")
                .map(|(_, contents)| contents.clone())
                .expect("full artifact exists");
            serde_json::from_str::<serde_json::Value>(&contents).expect("full artifact is JSON")
        };

        config.client_rule_profile = crate::config::ClientRuleProfile::Standard;
        let standard = full(&config);
        assert!(
            standard["route"]["rule_set"]
                .as_array()
                .expect("rule_set is an array")
                .iter()
                .any(|rule| rule["tag"] == "geosite-cn"),
            "standard must reference remote rule-sets"
        );
        assert!(
            standard["route"]["rules"]
                .as_array()
                .expect("rules is an array")
                .iter()
                .any(|rule| rule.get("rule_set").is_some()),
            "standard must route through rule_set"
        );

        config.client_rule_profile = crate::config::ClientRuleProfile::Minimal;
        let minimal = full(&config);
        assert!(
            minimal["route"]["rule_set"]
                .as_array()
                .expect("rule_set is an array")
                .is_empty(),
            "minimal must not reference remote rule-sets"
        );
        for rule in minimal["route"]["rules"]
            .as_array()
            .expect("rules is an array")
        {
            assert!(
                rule.get("rule_set").is_none(),
                "minimal rules stay built-in"
            );
        }
        for rule in minimal["dns"]["rules"]
            .as_array()
            .expect("dns rules is an array")
        {
            assert!(
                rule.get("rule_set").is_none(),
                "minimal DNS rules stay built-in"
            );
        }
    }

    #[test]
    fn version_profiles_only_carry_store_dns_where_the_changelog_allows_it() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_direct_subscription(&fixture);
        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");
        for profile in SING_BOX_VERSION_PROFILES {
            let name = SubscriptionFormat::SingBoxVersion(profile.version)
                .artifact_name()
                .into_owned();
            let contents = artifacts
                .iter()
                .find(|(artifact, _)| *artifact == name)
                .map(|(_, contents)| contents)
                .unwrap_or_else(|| panic!("missing profile artifact {name}"));
            let value: serde_json::Value = serde_json::from_str(contents).expect("profile is JSON");
            let has_store_dns = value["experimental"]["cache_file"]
                .get("store_dns")
                .is_some();
            assert_eq!(
                has_store_dns, profile.supports_store_dns,
                "store_dns mismatch for {}",
                profile.version
            );
        }
    }

    #[test]
    fn pre_anytls_client_profiles_drop_the_anytls_node_and_say_so() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_all_protocols(&fixture);
        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");
        let parsed = |name: &str| -> serde_json::Value {
            let contents = artifacts
                .iter()
                .find(|(artifact, _)| artifact == name)
                .map(|(_, contents)| contents.clone())
                .unwrap_or_else(|| panic!("missing profile artifact {name}"));
            serde_json::from_str(&contents).expect("profile is JSON")
        };
        let has_node = |value: &serde_json::Value, tag: &str| {
            value["outbounds"]
                .as_array()
                .expect("outbounds is an array")
                .iter()
                .any(|outbound| outbound["tag"] == tag)
        };

        for profile in SING_BOX_VERSION_PROFILES {
            let name = SubscriptionFormat::SingBoxVersion(profile.version)
                .artifact_name()
                .into_owned();
            let value = parsed(&name);
            assert_eq!(
                has_node(&value, "sbctl-anytls"),
                profile.supports_anytls,
                "AnyTLS node presence mismatch for {name}"
            );
        }

        // 1.10: legacy DNS servers, top-level fakeip, inbound sniff, a special
        // dns outbound, and no route/domain-resolver fields.
        let legacy = parsed("subscription-sing-box-1.10.json");
        for server in legacy["dns"]["servers"].as_array().expect("dns servers") {
            assert!(server.get("address").is_some(), "1.10 DNS must be legacy");
            assert!(server.get("type").is_none(), "1.10 DNS must not be typed");
        }
        assert!(
            legacy["dns"]["fakeip"]["enabled"]
                .as_bool()
                .unwrap_or(false),
            "1.10 fake-ip must use the top-level dns.fakeip object"
        );
        assert!(
            !legacy["route"].get("default_domain_resolver").is_some(),
            "1.10 has no route.default_domain_resolver"
        );
        assert!(
            has_node(&legacy, "dns-out"),
            "1.10 hijacks DNS through a special dns outbound"
        );
        assert_eq!(
            legacy["inbounds"][0]["sniff"], true,
            "1.10 sniffs at the inbound"
        );

        // 1.11: legacy DNS but rule actions are available; no dns outbound.
        let one_eleven = parsed("subscription-sing-box-1.11.json");
        assert!(
            one_eleven["dns"]["servers"]
                .as_array()
                .expect("dns servers")
                .iter()
                .all(|server| server.get("type").is_none()),
            "1.11 DNS must stay legacy"
        );
        assert!(
            !has_node(&one_eleven, "dns-out"),
            "1.11 hijacks DNS through the hijack-dns rule action"
        );
        let rules = one_eleven["route"]["rules"]
            .as_array()
            .expect("route rules");
        assert!(
            rules.iter().any(|rule| rule["action"] == "hijack-dns"),
            "1.11 route rules use actions"
        );

        // 1.12+: typed DNS and the domain resolver default.
        let typed = parsed("subscription-sing-box-1.14.json");
        assert!(
            typed["dns"]["servers"]
                .as_array()
                .expect("dns servers")
                .iter()
                .all(|server| server.get("type").is_some()),
            "1.14 DNS must be typed"
        );
        assert_eq!(
            typed["route"]["default_domain_resolver"]["server"], "dns-direct",
            "1.14 resolves outbound server domains through default_domain_resolver"
        );
    }

    #[test]
    fn an_anytls_only_deployment_skips_pre_anytls_profiles_with_a_warning() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (_store, config, _) = seed_single_protocol(&fixture, ManagedProtocol::Anytls);
        let artifacts =
            generated_artifacts(&config, fixture.path()).expect("other formats still generate");
        let names: Vec<&str> = artifacts.iter().map(|(name, _)| name.as_str()).collect();
        assert!(
            !names.contains(&"subscription-sing-box-1.10.json"),
            "the 1.10 profile must be skipped for an AnyTLS-only deployment"
        );
        assert!(
            !names.contains(&"subscription-sing-box-1.11.json"),
            "the 1.11 profile must be skipped for an AnyTLS-only deployment"
        );
        assert!(
            names.contains(&"subscription-clash.yaml")
                && names.contains(&"subscription-sing-box-full.json"),
            "the formats AnyTLS supports must still generate"
        );
    }

    #[test]
    fn the_client_matrix_covers_the_mainstream_clients() {
        let rows = client_subscription_matrix();
        let clients: Vec<&str> = rows.iter().map(|row| row.client).collect();
        for client in [
            "Clash Party",
            "Clash Verge",
            "sing-box",
            "V2rayN",
            "Shadowrocket",
        ] {
            assert!(
                clients.contains(&client),
                "the client matrix must cover {client}"
            );
        }
        let sing_box_row = rows
            .iter()
            .find(|row| row.client == "sing-box")
            .expect("the sing-box row exists");
        // One recommendation per version profile plus sing-box-full.
        assert_eq!(
            sing_box_row.formats.len(),
            SING_BOX_VERSION_PROFILES.len() + 1,
            "the sing-box row must recommend one format per supported version"
        );
    }

    #[test]
    fn no_domain_ip_fallback_artifacts_use_the_fake_protocol_sni_and_insecure_tls() {
        let config = DeploymentConfig::new_with_ports(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![
                ManagedProtocol::VlessReality,
                ManagedProtocol::VmessWebsocket,
                ManagedProtocol::Hysteria2,
                ManagedProtocol::Tuic,
                ManagedProtocol::Anytls,
            ],
            Some("www.cloudflare.com".into()),
            ProtocolPorts::default(),
        )
        .expect("a no-domain deployment with all five protocols is valid");
        let nodes = crate::canonical::nodes(&config);

        let sing_box = sing_box(&config, &nodes).expect("sing-box artifacts generate");
        let clash = clash(&config, &nodes).expect("clash artifacts generate");
        let clash_legacy = clash_legacy(&config, &nodes).expect("legacy clash generates");
        let shadowrocket = shadowrocket(&config, &nodes).expect("shadowrocket generates");
        let sing_box_full = sing_box_full(&config, &nodes, latest_version_profile())
            .expect("full config generates");
        let uri = uri(&config, &nodes).expect("uri artifacts generate");

        assert!(
            clash.contains("  - name: 🌍选择代理节点\n    type: select\n    url: http://aliyun.com/generate_204\n    interval: 300\n    proxies:\n      - ♻️自动选择\n      - DIRECT\n"),
            "clash subscription exposes the manual selection group"
        );
        assert!(
            clash.contains("  - name: ♻️自动选择\n    type: url-test\n    url: http://www.gstatic.com/generate_204\n    interval: 300\n    tolerance: 50\n"),
            "clash subscription exposes the automatic selection group"
        );
        assert!(
            clash
                .contains("  - name: 🎯全球直连\n    type: select\n    proxies:\n      - DIRECT\n"),
            "clash subscription exposes the direct selection group"
        );
        for rule in [
            "  - DOMAIN-SUFFIX,chatgpt.com,🌍选择代理节点\n",
            "  - DOMAIN-SUFFIX,x.com,🌍选择代理节点\n",
            "  - RULE-SET,geosite-cn,🎯全球直连\n",
            "  - RULE-SET,geoip-cn,🎯全球直连\n",
            "  - MATCH,🌍选择代理节点\n",
        ] {
            assert!(
                clash.contains(rule),
                "clash subscription carries rule: {rule}"
            );
        }
        assert!(
            clash.contains(
                "url: https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/geo/geosite/cn.mrs"
            ),
            "clash rule-providers reference the meta-branch rule-set base URL"
        );
        assert!(
            clash.contains("enhanced-mode: fake-ip\n"),
            "clash subscription defaults to fake-ip DNS"
        );
        // The legacy mihomo variant keeps the built-in GEOIP rules the 1.18
        // line shipped, so old cores never need the remote rule-providers.
        for rule in [
            "  - RULE-SET,geosite-cn,🎯全球直连\n",
            "  - GEOIP,LAN,DIRECT\n",
            "  - GEOIP,CN,DIRECT\n",
            "  - MATCH,🌍选择代理节点\n",
        ] {
            assert_eq!(
                clash_legacy.contains(rule),
                matches!(
                    rule,
                    "  - GEOIP,LAN,DIRECT\n"
                        | "  - GEOIP,CN,DIRECT\n"
                        | "  - MATCH,🌍选择代理节点\n"
                ),
                "legacy clash keeps GEOIP rules and skips rule-sets: {rule}"
            );
        }

        for artifact in [&sing_box, &clash, &uri] {
            assert!(
                artifact.contains("www.bing.com"),
                "artifact carries the default fake protocol SNI"
            );
            assert!(
                artifact.contains("www.cloudflare.com"),
                "artifact carries the Reality decoy SNI"
            );
            assert!(
                artifact.contains("203.0.113.7"),
                "artifact addresses the VPS IP rather than a domain"
            );
        }
        assert!(
            sing_box.contains("\"insecure\": true"),
            "sing-box clients skip certificate verification"
        );
        assert!(
            clash.contains("skip-cert-verify: true"),
            "clash clients skip certificate verification"
        );
        assert!(
            uri.contains("insecure=1"),
            "URI clients skip certificate verification"
        );
        // The Shadowrocket artifact decodes to URIs with the SR-specific
        // adaptations: encoded passwords and the official anytls scheme.
        {
            use base64::Engine as _;
            let decoded = String::from_utf8(
                base64::engine::general_purpose::STANDARD
                    .decode(&shadowrocket)
                    .expect("shadowrocket artifact is valid base64"),
            )
            .expect("shadowrocket URIs are UTF-8");
            assert!(
                decoded.contains("anytls://") && decoded.contains("/?insecure="),
                "shadowrocket anytls URI follows the official scheme: {decoded}"
            );
            assert!(
                decoded.contains("tuic://") && decoded.contains("udp_relay_mode=native"),
                "shadowrocket tuic URI carries udp_relay_mode"
            );
            assert!(
                !decoded.contains("security=tls"),
                "shadowrocket URIs drop the non-standard security parameter"
            );
        }
        // The full sing-box client profile carries DNS, tun, groups, routing,
        // rule-sets, and the clash API for dashboards.
        for fragment in [
            "\"tun\"",
            "🚀节点选择",
            "♻️自动选择",
            "\"selector\"",
            "\"urltest\"",
            "geosite-cn",
            "geoip-cn",
            "clash_api",
            "cache_file",
            "\"fakeip\"",
            "223.5.5.5",
            "chatgpt.com",
        ] {
            assert!(
                sing_box_full.contains(fragment),
                "full sing-box client profile carries {fragment}"
            );
        }
    }

    #[test]
    fn self_signed_certificates_are_generated_inside_the_deployment_root_with_private_permissions()
    {
        let fixture = TempDir::new().expect("temporary root is created");
        let config = DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::Hysteria2],
            None,
        )
        .expect("an IP fallback Hysteria2 deployment is valid");

        let artifacts = generated_artifacts(&config, fixture.path()).expect("artifacts generate");

        let server: serde_json::Value = serde_json::from_str(
            &artifacts
                .iter()
                .find(|(name, _)| *name == "sing-box-server.json")
                .map(|(_, contents)| contents.clone())
                .expect("server artifact is present"),
        )
        .expect("server configuration is JSON");
        let certificate_path = server["inbounds"][0]["tls"]["certificate_path"]
            .as_str()
            .expect("the TLS inbound references a certificate path");
        assert!(
            certificate_path.starts_with(fixture.path().to_str().expect("fixture path is UTF-8")),
            "the self-signed certificate is written inside the deployment root: {certificate_path}"
        );
        let directory = fixture
            .path()
            .join("var/lib/sbctl/certificates/www.bing.com");
        assert!(directory.join("key.pem").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key_mode = fs::metadata(directory.join("key.pem"))
                .expect("private key exists")
                .permissions()
                .mode();
            assert_eq!(
                key_mode & 0o777,
                0o640,
                "the TLS private key is never world-readable"
            );
            let directory_mode = fs::metadata(&directory)
                .expect("certificate directory exists")
                .permissions()
                .mode();
            assert_eq!(directory_mode & 0o777, 0o750);
        }
    }

    #[tokio::test]
    async fn acme_listener_serves_the_challenge_and_rejects_every_other_path() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let challenge = store.acme_webroot().join(".well-known/acme-challenge");
        fs::create_dir_all(&challenge).expect("challenge directory is created");
        fs::write(challenge.join("token-1"), "challenge-body").expect("challenge is written");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_acme_listener(
            listener,
            Arc::new(store),
            Some(1),
        ));

        let served = http_get(port, "/.well-known/acme-challenge/token-1").await;
        assert!(served.starts_with("HTTP/1.1 200 OK"), "challenge is served");
        assert!(served.contains("challenge-body"));
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn acme_listener_returns_404_for_a_foreign_or_malformed_challenge_path() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_acme_listener(
            listener,
            Arc::new(store),
            Some(3),
        ));

        let missing = http_get(port, "/.well-known/acme-challenge/unknown").await;
        assert!(missing.starts_with("HTTP/1.1 404 Not Found"));
        let traversal = http_get(port, "/.well-known/acme-challenge/../config.toml").await;
        assert!(traversal.starts_with("HTTP/1.1 404 Not Found"));
        let wrong_root = http_get(port, "/sub/anything/uri").await;
        assert!(wrong_root.starts_with("HTTP/1.1 404 Not Found"));
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_serves_the_subscription_after_a_real_handshake() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, credential) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store),
            Arc::new(config),
            Some(1),
        ));

        let response = tls_get(port, &format!("/sub/{credential}/uri")).await;
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "TLS subscription is served"
        );
        assert!(response.contains("vless://"));
        assert!(response.contains("subscription-userinfo:"));
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn a_broken_accounting_state_degrades_the_userinfo_header_not_the_subscription() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, credential) = seed_direct_subscription(&fixture);
        store
            .write_state(b"not json")
            .expect("the accounting state is corrupted");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let store = Arc::new(store.clone());
        let config = Arc::new(config);
        let handler = tokio::spawn(async move {
            super::serve_http_listener(listener, &store, &config, Some(1)).await
        });

        let response = http_get(port, &format!("/sub/{credential}/uri")).await;
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "the subscription must survive a broken accounting state: {response}"
        );
        assert!(
            !response.contains("subscription-userinfo:"),
            "the degraded response must not carry traffic metadata"
        );
        assert!(
            response.contains("vless://"),
            "the artifact body still serves"
        );
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_serves_base64_uri_with_the_standard_traffic_headers() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, credential) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store.clone()),
            Arc::new(config),
            Some(1),
        ));

        let response = tls_get(port, &format!("/sub/{credential}/uri.txt")).await;
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("content-type: text/plain; charset=utf-8"));
        assert!(response.contains("subscription-userinfo:"));
        let (_, body) = response
            .split_once("\r\n\r\n")
            .expect("the response separates headers and body");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(body.trim())
                .expect("the response body is standard Base64"),
            fs::read(
                store
                    .root()
                    .join("var/lib/sbctl/artifacts/subscription-uri.txt")
            )
            .expect("the canonical URI artifact is readable")
        );
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_rejects_a_handshake_whose_sni_is_not_the_subscription_host() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, _) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store),
            Arc::new(config),
            Some(1),
        ));

        let handshake = tls_handshake_sni(port, "attacker.example.test").await;
        assert!(
            handshake.is_err(),
            "an SNI mismatch is rejected before any HTTP request"
        );
        handler.await.expect("handler completes").expect("no error");
    }

    #[tokio::test]
    async fn direct_tls_listener_rejects_a_handshake_without_an_sni() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config, _) = seed_direct_subscription(&fixture);
        seed_direct_certificate(&fixture, &store, &config);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral listener is available");
        let port = listener.local_addr().expect("listener address").port();

        let handler = tokio::spawn(super::serve_tls_listener(
            listener,
            Arc::new(store),
            Arc::new(config),
            Some(1),
        ));

        let handshake = tls_handshake_sni(port, "").await;
        assert!(
            handshake.is_err(),
            "a missing SNI is rejected before any HTTP request"
        );
        handler.await.expect("handler completes").expect("no error");
    }

    /// Writes a valid certificate into the Certbot live directory and pins it
    /// into the sbctl-owned copy that the daemon actually serves.
    fn seed_direct_certificate(
        fixture: &TempDir,
        store: &DeploymentStore,
        config: &DeploymentConfig,
    ) {
        let certificate_directory = fixture.path().join("etc/letsencrypt/live/sub.example.test");
        fs::create_dir_all(&certificate_directory).expect("certificate directory is created");
        let certificate = rcgen::generate_simple_self_signed(vec!["sub.example.test".into()])
            .expect("a self-signed certificate is generated");
        fs::write(
            certificate_directory.join("fullchain.pem"),
            certificate.cert.pem(),
        )
        .expect("fullchain is written");
        fs::write(
            certificate_directory.join("privkey.pem"),
            certificate.signing_key.serialize_pem(),
        )
        .expect("private key is written");
        let validated =
            crate::certificate::load(store, config).expect("the fixture certificate is valid");
        crate::certificate::pin(store, config, &validated)
            .expect("the certificate is pinned for the daemon");
    }

    /// Opens a TLS connection that accepts any certificate and returns the
    /// response to a single GET request. Certificates are verified separately
    /// by the deploy hook and the certificate ticket; this test exercises the
    /// listener's TLS termination path, not certificate trust.
    async fn tls_get(port: u16, path: &str) -> String {
        let mut stream = tls_connect(port, "sub.example.test")
            .await
            .expect("TLS handshake completes");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: sub.example.test\r\n\r\n").as_bytes())
            .await
            .expect("request is sent");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("response is readable");
        response
    }

    /// Opens a TLS connection with a caller-supplied SNI and returns whether
    /// the handshake completed. An empty `sni` connects without a DNS SNI (an
    /// IP server name is used, which rustls omits from the ClientHello).
    async fn tls_handshake_sni(port: u16, sni: &str) -> Result<(), std::io::Error> {
        tls_connect(port, sni).await.map(|_| ())
    }

    async fn tls_connect(
        port: u16,
        sni: &str,
    ) -> Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>, std::io::Error> {
        use rustls::client::danger::{
            HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
        };
        use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
        use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
        use tokio_rustls::TlsConnector;

        #[derive(Debug)]
        struct AcceptsEverything;
        impl ServerCertVerifier for AcceptsEverything {
            fn verify_server_cert(
                &self,
                _end_entity: &CertificateDer<'_>,
                _intermediates: &[CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: UnixTime,
            ) -> Result<ServerCertVerified, rustls::Error> {
                Ok(ServerCertVerified::assertion())
            }
            fn verify_tls12_signature(
                &self,
                _message: &[u8],
                _cert: &CertificateDer<'_>,
                _dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn verify_tls13_signature(
                &self,
                _message: &[u8],
                _cert: &CertificateDer<'_>,
                _dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                Ok(HandshakeSignatureValid::assertion())
            }
            fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
                vec![
                    SignatureScheme::ECDSA_NISTP256_SHA256,
                    SignatureScheme::ECDSA_NISTP384_SHA384,
                    SignatureScheme::ED25519,
                    SignatureScheme::RSA_PSS_SHA256,
                    SignatureScheme::RSA_PSS_SHA384,
                    SignatureScheme::RSA_PSS_SHA512,
                    SignatureScheme::RSA_PKCS1_SHA256,
                    SignatureScheme::RSA_PKCS1_SHA384,
                    SignatureScheme::RSA_PKCS1_SHA512,
                ]
            }
        }

        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptsEverything))
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("TLS listener accepts connections");
        let server_name = if sni.is_empty() {
            ServerName::try_from("203.0.113.7".to_owned()).expect("a valid IP server name")
        } else {
            ServerName::try_from(sni.to_owned()).expect("valid server name")
        };
        connector.connect(server_name, stream).await
    }

    fn checker(fixture: &TempDir, accepts: bool) -> PathBuf {
        #[cfg(windows)]
        let path = fixture.path().join("sing-box-check.cmd");
        #[cfg(not(windows))]
        let path = fixture.path().join("sing-box-check");
        fs::write(
            &path,
            #[cfg(windows)]
            if accepts {
                "@exit /b 0\r\n"
            } else {
                "@exit /b 1\r\n"
            },
            #[cfg(not(windows))]
            if accepts {
                "#!/bin/sh\nexit 0\n"
            } else {
                "#!/bin/sh\nexit 1\n"
            },
        )
        .expect("checker is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("checker is executable");
        }
        path
    }

    fn vless_config() -> DeploymentConfig {
        DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an IP fallback VLESS deployment is valid")
    }

    #[test]
    fn ipv4_only_survives_persistence_and_pins_resolution() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let mut config = vless_config();
        config.ipv4_only = true;
        store.initialize(&config).unwrap();
        let config = store.load().unwrap();
        let artifacts = generated_artifacts(&config, fixture.path()).unwrap();
        let server: serde_json::Value = serde_json::from_str(
            &artifacts
                .iter()
                .find(|(name, _)| *name == "sing-box-server.json")
                .unwrap()
                .1,
        )
        .unwrap();
        // The legacy inbound field was removed in sing-box 1.13: resolution is
        // now pinned through a route action plus the default DNS strategy.
        assert!(server["inbounds"][0].get("domain_strategy").is_none());
        assert_eq!(server["dns"]["strategy"], "ipv4_only");
        let rules = server["route"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|rule| {
            rule["inbound"] == "sbctl-vless-reality"
                && rule["action"] == "resolve"
                && rule["strategy"] == "ipv4_only"
        }));
    }

    #[test]
    fn server_artifact_pins_info_logging() {
        let fixture = TempDir::new().unwrap();
        let store = DeploymentStore::new(fixture.path());
        let config = vless_config();
        store.initialize(&config).unwrap();
        let config = store.load().unwrap();
        let artifacts = generated_artifacts(&config, fixture.path()).unwrap();
        let server: serde_json::Value = serde_json::from_str(
            &artifacts
                .iter()
                .find(|(name, _)| *name == "sing-box-server.json")
                .unwrap()
                .1,
        )
        .unwrap();
        assert_eq!(server["log"]["level"], "info");
    }

    fn write_old_artifacts(store: &DeploymentStore) {
        for (name, contents) in [
            ("sing-box-server.json", "old server".as_bytes()),
            ("subscription-sing-box.json", "old sing-box".as_bytes()),
            ("subscription-clash.yaml", "old clash".as_bytes()),
            ("subscription-uri.txt", "old uri".as_bytes()),
            ("subscription-base64-uri.txt", "old Base64 URI".as_bytes()),
        ] {
            store
                .write_artifact(name, contents)
                .expect("an old artifact is committed");
        }
    }

    fn artifact(store: &DeploymentStore, name: &str) -> Vec<u8> {
        fs::read(store.root().join("var/lib/sbctl/artifacts").join(name))
            .expect("artifact is readable")
    }

    #[test]
    fn regenerate_with_a_failed_check_leaves_artifacts_and_active_config_unchanged() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        store
            .write_relative_locked("etc/sing-box/config.json", b"old active config")
            .expect("old active config is committed");
        let rejecting = checker(&fixture, false);

        let result = regenerate(&store, &vless_config(), Some(&rejecting), true);
        assert!(
            result.is_err(),
            "a rejected check must fail the regeneration"
        );
        for (name, old) in [
            ("sing-box-server.json", "old server".as_bytes()),
            ("subscription-sing-box.json", "old sing-box".as_bytes()),
            ("subscription-clash.yaml", "old clash".as_bytes()),
            ("subscription-uri.txt", "old uri".as_bytes()),
        ] {
            assert_eq!(
                artifact(&store, name),
                old,
                "{name} stays on the old complete version"
            );
        }
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            b"old active config"
        );
    }

    #[test]
    fn regenerate_with_a_passing_check_replaces_all_artifacts_and_active_config() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        store
            .write_relative_locked("etc/sing-box/config.json", b"old active config")
            .expect("old active config is committed");
        let config = vless_config();
        let accepting = checker(&fixture, true);

        regenerate(&store, &config, Some(&accepting), true)
            .expect("a passing check allows the regeneration");
        let expected =
            generated_artifacts(&config, fixture.path()).expect("new artifacts are generated");
        for (name, contents) in &expected {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is replaced by the complete new version"
            );
        }
        let server = expected
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents)
            .expect("server artifact is present");
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            server.as_bytes(),
            "the active sing-box configuration is re-synced"
        );
    }

    #[test]
    fn regenerate_without_active_config_sync_leaves_it_untouched() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        store
            .write_relative_locked("etc/sing-box/config.json", b"old active config")
            .expect("old active config is committed");
        let accepting = checker(&fixture, true);

        regenerate(&store, &vless_config(), Some(&accepting), false)
            .expect("artifacts are regenerated without the active config");
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            b"old active config"
        );
    }

    #[test]
    fn regenerate_restores_earlier_artifacts_when_a_later_replacement_fails() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        write_old_artifacts(&store);
        let accepting = checker(&fixture, true);

        let blocked = store
            .root()
            .join("var/lib/sbctl/artifacts/subscription-uri.txt");
        fs::remove_file(&blocked).expect("blocked artifact is removed");
        fs::create_dir(&blocked).expect("blocked artifact is replaced by a directory");

        let result = regenerate(&store, &vless_config(), Some(&accepting), true);
        assert!(result.is_err(), "a blocked artifact fails the regeneration");
        assert_eq!(
            artifact(&store, "sing-box-server.json"),
            "old server".as_bytes(),
            "an earlier replaced artifact is restored after a later write failure"
        );
        assert_eq!(
            artifact(&store, "subscription-sing-box.json"),
            "old sing-box".as_bytes(),
            "an earlier replaced artifact is restored after a later write failure"
        );
    }

    #[test]
    fn redact_secret_replaces_every_occurrence_of_the_credential() {
        let secret = "deadbeef-credential";
        let message = format!("subscription artifact failed: {secret}; retry with {secret}");
        assert_eq!(
            super::redact_secret(&message, secret),
            "subscription artifact failed: [redacted]; retry with [redacted]"
        );
    }

    #[test]
    fn redact_secret_leaves_unrelated_text_untouched() {
        assert_eq!(
            super::redact_secret("subscription artifact failed: no such file", "secret"),
            "subscription artifact failed: no such file"
        );
    }

    fn write_initial_deployment(store: &DeploymentStore, config: &DeploymentConfig) {
        let artifacts = generated_artifacts(config, store.root()).expect("artifacts generate");
        let references = artifacts
            .iter()
            .map(|(name, contents)| (name.clone(), contents.as_bytes()))
            .collect::<Vec<_>>();
        store
            .initialize_with_artifacts(config, &references)
            .expect("initial deployment is written");
        let server = artifacts
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents.as_bytes())
            .expect("server artifact exists");
        store
            .write_relative_locked("etc/sing-box/config.json", server)
            .expect("active config is written");
    }

    fn persisted_config(store: &DeploymentStore) -> Vec<u8> {
        fs::read(store.root().join("etc/sbctl/config.toml")).expect("config is readable")
    }

    #[test]
    fn apply_config_transaction_with_a_failed_check_leaves_everything_unchanged() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let mut new = old.clone();
        new.subscription_host = "198.51.100.9".into();
        let rejecting = checker(&fixture, false);

        let result = super::apply_config_transaction(&store, &new, Some(&rejecting));

        assert!(
            result.is_err(),
            "a rejected check must fail the transaction"
        );
        let expected =
            generated_artifacts(&old, fixture.path()).expect("old artifacts are generated");
        for (name, contents) in &expected {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} stays on the old complete version"
            );
        }
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&old)
                .expect("old config serializes")
                .as_bytes()
        );
    }

    #[test]
    fn apply_config_transaction_replaces_config_artifacts_and_active_config_together() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let mut new = old.clone();
        new.subscription_host = "198.51.100.9".into();
        let accepting = checker(&fixture, true);

        let snapshot =
            super::apply_config_transaction(&store, &new, Some(&accepting)).expect("transaction");

        let expected =
            generated_artifacts(&new, fixture.path()).expect("new artifacts are generated");
        for (name, contents) in &expected {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is replaced by the new complete version"
            );
        }
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&new)
                .expect("new config serializes")
                .as_bytes()
        );
        let server = expected
            .iter()
            .find(|(name, _)| *name == "sing-box-server.json")
            .map(|(_, contents)| contents.as_bytes())
            .expect("server artifact exists");
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            server
        );
        assert_eq!(
            snapshot.config,
            toml::to_string_pretty(&old)
                .expect("old serializes")
                .as_bytes()
                .to_vec()
        );
    }

    #[test]
    fn apply_config_transaction_skips_the_check_for_a_config_only_change() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let artifacts_before =
            generated_artifacts(&old, fixture.path()).expect("old artifacts are generated");
        let active_before = fs::read(store.root().join("etc/sing-box/config.json"))
            .expect("active config is readable");
        let mut new = old.clone();
        new.monthly_traffic_limit = 1_000_000;

        super::apply_config_transaction(&store, &new, None)
            .expect("config-only change needs no check");

        for (name, contents) in &artifacts_before {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is untouched by a config-only change"
            );
        }
        assert_eq!(
            fs::read(store.root().join("etc/sing-box/config.json"))
                .expect("active config is readable"),
            active_before
        );
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&new)
                .expect("new config serializes")
                .as_bytes()
        );
    }

    #[test]
    fn restore_config_transaction_returns_the_previous_deployment() {
        let fixture = TempDir::new().expect("temporary root is created");
        let store = DeploymentStore::new(fixture.path());
        let old = vless_config();
        write_initial_deployment(&store, &old);
        let mut new = old.clone();
        new.subscription_host = "198.51.100.9".into();
        let accepting = checker(&fixture, true);

        let snapshot =
            super::apply_config_transaction(&store, &new, Some(&accepting)).expect("transaction");
        super::restore_config_transaction(&store, &snapshot).expect("restore succeeds");

        let old_artifacts =
            generated_artifacts(&old, fixture.path()).expect("old artifacts are generated");
        for (name, contents) in &old_artifacts {
            assert_eq!(
                artifact(&store, name),
                contents.as_bytes(),
                "{name} is restored"
            );
        }
        assert_eq!(
            persisted_config(&store),
            toml::to_string_pretty(&old)
                .expect("old config serializes")
                .as_bytes()
        );
    }
}
