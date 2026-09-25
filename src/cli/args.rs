//! The clap surface of the `sbctl` binary: every argument type, the subcommand
//! enums, and the `--format` value parser shared by `sub` and `qr`.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "sbctl",
    version,
    about = "Manage a private sing-box deployment"
)]
pub(crate) struct Cli {
    #[arg(long, global = true, hide = true, value_name = "PATH")]
    pub(crate) root: Option<PathBuf>,
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Interactively install a fresh sbctl deployment.
    Install {
        #[command(flatten)]
        options: InstallOptions,
    },
    /// Open the interactive management menu for an installed deployment.
    #[command(alias = "m")]
    Menu,
    /// Show whether sbctl currently manages a deployment.
    Status {
        /// Emit a machine-readable JSON status report.
        #[arg(long)]
        json: bool,
    },
    /// Reconcile and show VPS traffic, or apply an explicit traffic correction.
    Traffic {
        #[command(subcommand)]
        command: Option<TrafficCommand>,
    },
    /// List the generated Managed protocol listeners without exposing credentials.
    Node {
        /// Also print each node's native share link (`vless://…` and friends).
        /// Those carry the proxy credentials, so they reach this terminal only
        /// and are never written to the journal.
        #[arg(long)]
        uri: bool,
    },
    /// Validate the active sing-box configuration and restart both managed services.
    Restart {
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Remove the sbctl-managed services and binaries; preserve data unless --purge is supplied.
    Uninstall {
        /// Also remove persistent data explicitly owned by sbctl.
        #[arg(long)]
        purge: bool,
    },
    /// Verify or apply a signed, fixed-version release manifest and its artifacts.
    Update {
        /// Display versions from the signed manifest without downloading or changing the host.
        #[arg(long)]
        check: bool,
        /// Signed release manifest containing the allowed artifact hashes.
        /// Omit to fetch and verify the latest signed manifest for this host.
        #[arg(long, value_name = "PATH")]
        manifest: Option<PathBuf>,
        /// Optional local sbctl candidate artifact; otherwise download from the manifest.
        #[arg(long, value_name = "PATH")]
        sbctl_artifact: Option<PathBuf>,
        /// Optional local sing-box candidate artifact; otherwise download from the manifest.
        #[arg(long, value_name = "PATH")]
        sing_box_artifact: Option<PathBuf>,
    },
    /// Manage the sing-box data-plane binary independently from sbctl.
    #[command(name = "sing-box")]
    SingBox {
        #[command(subcommand)]
        command: SingBoxCommand,
    },
    /// Maintainer tooling for signed release manifests.
    #[command(hide = true)]
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    /// Print every subscription link with its label and QR link, or one raw
    /// link when `--format` is given.
    Sub {
        #[arg(long, value_name = "FORMAT", value_parser = parse_cli_format)]
        format: Option<sbctl::subscription::SubscriptionFormat>,
    },
    /// Print a terminal QR code for a generated subscription representation.
    /// Pass a format (default `sing-box-full`) or `--all` for the whole matrix.
    Qr {
        /// Subscription format to render; defaults to `sing-box-full`.
        #[arg(value_name = "FORMAT", value_parser = parse_cli_format)]
        format: Option<sbctl::subscription::SubscriptionFormat>,
        /// Render a terminal QR code for every format in the subscription matrix.
        #[arg(long, conflicts_with = "format")]
        all: bool,
    },
    /// Rotate the Subscription credential so previous subscription URLs stop working.
    Credential {
        #[command(subcommand)]
        command: CredentialCommand,
    },
    /// Run the subscription service.
    Serve {
        /// Socket address; defaults to the configured public IP and HTTP port.
        #[arg(long)]
        bind: Option<String>,
        /// Stop after this many requests (useful for supervised health checks).
        #[arg(long, hide = true)]
        max_requests: Option<usize>,
    },
    /// Obtain or renew a Direct subscription mode certificate with Certbot.
    Certificate {
        #[command(subcommand)]
        command: CertificateCommand,
    },
    /// Create, inspect, and validate the persistent deployment configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Run the periodic accounting reset task (managed by the systemd timer).
    #[command(name = "accounting-reset", hide = true)]
    AccountingReset,
    /// Host system tuning helpers (require root; never touch sing-box).
    System {
        #[command(subcommand)]
        command: SystemCommand,
    },
    /// Validate and atomically replace the canonical protocol artifacts and
    /// the active sing-box configuration from the persisted deployment.
    #[command(name = "regenerate", hide = true)]
    Regenerate {
        /// sing-box binary used to validate the regenerated server configuration.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
}

/// The `install` command's arguments. They live here, flattened into
/// [`Command::Install`], so the entry point hands them to the handler instead
/// of destructuring sixteen fields on both sides.
#[derive(Debug, Args)]
pub(crate) struct InstallOptions {
    #[arg(
        long,
        value_enum,
        default_value_t = CliSubscriptionMode::Direct,
        conflicts_with = "guided"
    )]
    pub(crate) mode: CliSubscriptionMode,
    /// Collect all installation settings in the configuration wizard before
    /// creating services or persistent state.
    #[arg(long)]
    pub(crate) guided: bool,
    #[arg(long)]
    pub(crate) subscription_host: Option<String>,
    #[arg(long)]
    pub(crate) proxy_host: Option<String>,
    /// Public HTTP port used only by IP fallback subscription mode.
    #[arg(long)]
    pub(crate) http_port: Option<u16>,
    #[arg(long)]
    pub(crate) interface: Option<String>,
    #[arg(long)]
    pub(crate) reality_decoy_sni: Option<String>,
    /// Fake TLS server name for the certificate-based protocols in a
    /// no-domain deployment (defaults to www.bing.com).
    #[arg(long)]
    pub(crate) protocol_sni: Option<String>,
    /// Explicitly omit a Managed protocol; all five are enabled by default.
    #[arg(long, value_enum)]
    pub(crate) disable_protocol: Vec<CliManagedProtocol>,
    /// Optional listener ports for the five Managed protocols.
    #[arg(long)]
    pub(crate) vless_port: Option<u16>,
    #[arg(long)]
    pub(crate) vmess_port: Option<u16>,
    #[arg(long)]
    pub(crate) hysteria2_port: Option<u16>,
    #[arg(long)]
    pub(crate) tuic_port: Option<u16>,
    #[arg(long)]
    pub(crate) anytls_port: Option<u16>,
    #[arg(long, value_name = "PATH")]
    pub(crate) sing_box_bin: Option<PathBuf>,
    /// Signed release manifest used to download and verify the data plane.
    #[arg(long, value_name = "PATH")]
    pub(crate) manifest: Option<PathBuf>,
    /// Create units and configuration without starting services (acceptance fixture use).
    #[arg(long, hide = true)]
    pub(crate) no_start: bool,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Create the initial deployment configuration without overwriting one.
    Init {
        #[arg(long, value_enum)]
        mode: CliSubscriptionMode,
        #[arg(long)]
        subscription_host: String,
        #[arg(long)]
        proxy_host: Option<String>,
        #[arg(long)]
        http_port: Option<u16>,
        /// Loopback HTTP port used by an external reverse proxy.
        #[arg(long)]
        listen_port: Option<u16>,
        /// Linux network interface; defaults to the detected default-route interface.
        #[arg(long = "interface")]
        interface: Option<String>,
        #[arg(long = "protocol", value_enum, required = true)]
        protocols: Vec<CliManagedProtocol>,
        /// Optional listener ports for the five Managed protocols.
        #[arg(long)]
        vless_port: Option<u16>,
        #[arg(long)]
        vmess_port: Option<u16>,
        #[arg(long)]
        hysteria2_port: Option<u16>,
        #[arg(long)]
        tuic_port: Option<u16>,
        #[arg(long)]
        anytls_port: Option<u16>,
        #[arg(long)]
        reality_decoy_sni: Option<String>,
        #[arg(long)]
        protocol_sni: Option<String>,
        #[arg(long, default_value_t = 0)]
        monthly_traffic_limit: u64,
        #[arg(long, value_enum, default_value_t = CliAccountingPolicy::NaturalMonth)]
        accounting_policy: CliAccountingPolicy,
        /// Named IANA timezone used for VPS accounting resets.
        #[arg(long)]
        accounting_timezone: Option<String>,
        /// Named IANA timezone used for human-readable client reset times.
        #[arg(long)]
        client_display_timezone: Option<String>,
        /// Required for anchored-month: YYYY-MM-DDTHH:MM in the accounting timezone.
        #[arg(long)]
        anchored_reset_at: Option<String>,
        /// sing-box binary used to validate VMess WebSocket or Hysteria2 server configuration.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Change only the subscription delivery mode while retaining generated nodes and credentials.
    SwitchMode {
        #[arg(long, value_enum)]
        mode: CliSubscriptionMode,
        /// Required when switching to external-proxy mode.
        #[arg(long)]
        listen_port: Option<u16>,
    },
    /// Display the persisted deployment summary without exposing credentials.
    Show,
    /// Parse and validate the persisted deployment configuration.
    Validate,
    /// Open the interactive configuration wizard for a new or existing deployment.
    Wizard {
        /// sing-box binary used to validate regenerated protocol configuration.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Manage the server-side client override templates merged into
    /// subscription artifacts (etc/sbctl/overrides/).
    Override {
        #[command(subcommand)]
        command: OverrideCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum OverrideCommand {
    /// Show the override files and which artifacts they affect.
    Show,
    /// Open $EDITOR on one override template; saving regenerates artifacts.
    Edit {
        /// Which template to edit: sing-box or clash.
        #[arg(value_enum)]
        target: CliOverrideTarget,
        /// sing-box binary used to validate the merged client profile.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Validate the override templates and run the merged sing-box profile
    /// through a real `sing-box check` when a core is available.
    Validate {
        /// sing-box binary used to validate the merged client profile.
        #[arg(long, value_name = "PATH")]
        sing_box_bin: Option<PathBuf>,
    },
    /// Delete both override templates and regenerate the artifacts.
    Clear,
}

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum CliOverrideTarget {
    #[value(name = "sing-box")]
    SingBox,
    #[value(name = "clash")]
    Clash,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CredentialCommand {
    /// Generate a fresh Subscription credential; the previous URLs stop working immediately.
    Rotate,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TrafficCommand {
    /// Show the current accounting period's VPS traffic.
    Show,
    /// Set the reported VPS traffic for the current accounting period.
    #[command(group(
        clap::ArgGroup::new("correction")
            .required(true)
            .multiple(true)
            .args(["bytes", "rx", "tx"])
    ))]
    SetUsed {
        /// Target reported total VPS traffic in bytes; only increases the total.
        #[arg(long, value_name = "TOTAL", conflicts_with_all = ["rx", "tx"])]
        bytes: Option<u64>,
        /// Target reported received bytes; requires --tx.
        #[arg(long, value_name = "BYTES", requires = "tx", conflicts_with = "bytes")]
        rx: Option<u64>,
        /// Target reported transmitted bytes; requires --rx.
        #[arg(long, value_name = "BYTES", requires = "rx", conflicts_with = "bytes")]
        tx: Option<u64>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CertificateCommand {
    /// Obtain a certificate using Certbot's webroot authenticator.
    Obtain {
        /// ACME registration email (only used for expiry notices).
        #[arg(long, value_name = "EMAIL", conflicts_with = "no_email")]
        email: Option<String>,
        /// Register without an email via --register-unsafely-without-email
        /// (requires an interactive confirmation).
        #[arg(long)]
        no_email: bool,
    },
    /// Renew certificates and safely reload the sbctl service if they changed.
    Renew,
    /// Validate the certificate and re-pin it for the service accounts. This is
    /// the Certbot deploy hook and the recommended post-renewal check.
    Verify,
    /// Show the pinned certificate's validity, SANs, and deploy-hook state.
    Status,
}

/// Host system tuning helpers. These require root and never touch the sing-box
/// deployment; they configure kernel-level TCP acceleration only.
#[derive(Debug, Subcommand)]
pub(crate) enum SystemCommand {
    /// Enable BBR + FQ TCP congestion control (kernel 4.9+) and persist it.
    Bbr,
    /// Show the current kernel congestion control and queueing discipline.
    Status,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ReleaseCommand {
    /// Sign an unsigned manifest with the given Ed25519 signing key seed.
    Sign {
        /// Manifest JSON to sign; any existing `signature` field is replaced.
        #[arg(long, value_name = "PATH")]
        manifest: PathBuf,
        /// File containing the hex-encoded Ed25519 signing key seed.
        #[arg(long, value_name = "PATH")]
        private_key: PathBuf,
        /// Where to write the signed manifest.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,
    },
    /// Verify a manifest against the built-in first-release public key.
    Verify {
        #[arg(long, value_name = "PATH")]
        manifest: PathBuf,
    },
    /// Generate a fresh release signing keypair. The secret seed is written to
    /// a 0600 file and never printed; only the public key is shown.
    Keygen {
        /// Directory in which to write `sbctl-release-secret.hex`.
        #[arg(long, value_name = "DIR", default_value = ".")]
        output: PathBuf,
    },
}

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum CliSubscriptionMode {
    Direct,
    ExternalProxy,
    IpFallback,
}

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum CliManagedProtocol {
    VlessReality,
    VmessWebsocket,
    Hysteria2,
    Tuic,
    Anytls,
}

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum CliAccountingPolicy {
    NaturalMonth,
    AnchoredMonth,
}

/// Parses a subscription format id for `sbctl sub`/`sbctl qr`. Versioned ids
/// (`sing-box-1.12`, `clash-1.18`) are accepted for any registered version so
/// a new upstream minor only needs a registry entry.
fn parse_cli_format(text: &str) -> Result<sbctl::subscription::SubscriptionFormat, String> {
    use sbctl::subscription::{ClientVersion, SubscriptionFormat};
    match text {
        "sing-box" => Ok(SubscriptionFormat::SingBox),
        "sing-box-full" => Ok(SubscriptionFormat::SingBoxFull),
        "clash" => Ok(SubscriptionFormat::Clash),
        "uri" => Ok(SubscriptionFormat::Uri),
        "base64-uri" => Ok(SubscriptionFormat::Base64Uri),
        "shadowrocket" => Ok(SubscriptionFormat::Shadowrocket),
        other => {
            let parse_version = |value: &str| -> Option<ClientVersion> {
                let (major, minor) = value.split_once('.')?;
                Some(ClientVersion::new(major.parse().ok()?, minor.parse().ok()?))
            };
            if let Some(version) = other.strip_prefix("sing-box-") {
                let version = parse_version(version).ok_or_else(|| {
                    format!("invalid sing-box version in '{other}'; expected e.g. sing-box-1.12")
                })?;
                return Ok(SubscriptionFormat::SingBoxVersion(version));
            }
            if let Some(version) = other.strip_prefix("clash-") {
                let version = parse_version(version).ok_or_else(|| {
                    format!("invalid clash version in '{other}'; expected e.g. clash-1.18")
                })?;
                return Ok(SubscriptionFormat::ClashLegacy(version));
            }
            Err(format!(
                "unknown subscription format '{other}'; expected sing-box, sing-box-full, \
sing-box-<version>, clash, clash-<version>, uri, base64-uri, or shadowrocket"
            ))
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum SingBoxCommand {
    /// Download the pinned sing-box artifact and verify its digest.
    Download {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Install a verified sing-box artifact at /usr/local/bin/sing-box.
    Install {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact: PathBuf,
    },
    /// Download (when needed), verify, and replace the managed sing-box binary.
    Update {
        /// Omit to fetch and verify the latest signed manifest for this host.
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long)]
        artifact: Option<PathBuf>,
    },
    /// Remove only the sbctl-owned sing-box binary and service.
    Remove,
}

impl From<CliSubscriptionMode> for sbctl::config::SubscriptionMode {
    fn from(mode: CliSubscriptionMode) -> Self {
        match mode {
            CliSubscriptionMode::Direct => Self::Direct,
            CliSubscriptionMode::ExternalProxy => Self::ExternalProxy,
            CliSubscriptionMode::IpFallback => Self::IpFallback,
        }
    }
}

impl From<CliManagedProtocol> for sbctl::config::ManagedProtocol {
    fn from(protocol: CliManagedProtocol) -> Self {
        match protocol {
            CliManagedProtocol::VlessReality => Self::VlessReality,
            CliManagedProtocol::VmessWebsocket => Self::VmessWebsocket,
            CliManagedProtocol::Hysteria2 => Self::Hysteria2,
            CliManagedProtocol::Tuic => Self::Tuic,
            CliManagedProtocol::Anytls => Self::Anytls,
        }
    }
}

impl From<CliAccountingPolicy> for sbctl::config::AccountingPolicy {
    fn from(policy: CliAccountingPolicy) -> Self {
        match policy {
            CliAccountingPolicy::NaturalMonth => Self::NaturalMonth,
            CliAccountingPolicy::AnchoredMonth => Self::AnchoredMonth,
        }
    }
}
