use std::borrow::Cow;
use std::fmt;

use super::artifacts::{
    BASE64_URI_ARTIFACT, CLASH_ARTIFACT, SHADOWROCKET_ARTIFACT, SING_BOX_ARTIFACT,
    SING_BOX_FULL_ARTIFACT, URI_ARTIFACT,
};

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
