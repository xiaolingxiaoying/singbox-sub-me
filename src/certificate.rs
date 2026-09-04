//! Direct subscription mode certificate loading, validation, and renewal.
//!
//! Certificates are obtained and renewed by Debian/Ubuntu Certbot. sbctl owns
//! the loading boundary: every certificate is validated before it is served and
//! a `certbot` deploy hook re-validates and re-pins the certificate after each
//! renewal. Validation rejects expired certificates, certificates whose SAN does
//! not cover the subscription host, mismatched private keys, and TLS connections
//! whose SNI does not equal the subscription host. (ADR-0017)
//!
//! The service accounts read the certificate from a pinned copy under
//! `var/lib/sbctl/certificates/<host>/` owned by the dedicated `sbctl-cert`
//! group, so the private key is readable only by the `sbctl` daemon and the
//! `sing-box` data plane and never by unrelated host accounts.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use chrono::{DateTime, Utc};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use sha2::{Digest, Sha256};
use thiserror::Error;
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::config::{DeploymentConfig, DeploymentStore, SubscriptionMode};
use crate::runtime::Runtime;

/// The system group granted read access to the pinned certificate copy. Only
/// the sbctl daemon and the sing-box data plane are members.
pub const CERTIFICATE_GROUP: &str = "sbctl-cert";

#[derive(Debug, Error)]
pub enum CertificateError {
    #[error("certificates are managed only in direct subscription mode")]
    NotDirect,
    #[error("Certbot must be installed from the Debian or Ubuntu package repository: {0}")]
    Certbot(String),
    #[error("Certbot failed: {0}")]
    CertbotFailed(String),
    #[error("certificate for the subscription host {host} is missing or unreadable")]
    Unreadable { host: String, detail: String },
    #[error("certificate for the subscription host {host} is not a valid X.509 certificate")]
    MalformedCertificate { host: String },
    #[error(
        "certificate for the subscription host {host} is not yet valid (valid from {not_before})"
    )]
    NotYetValid { host: String, not_before: String },
    #[error("certificate for the subscription host {host} expired on {not_after}")]
    Expired { host: String, not_after: String },
    #[error("certificate for the subscription host {host} does not cover that host name")]
    SanMismatch { host: String },
    #[error(
        "certificate private key for the subscription host {host} does not match the certificate"
    )]
    KeyMismatch { host: String },
    #[error("private key file for the subscription host {host} is invalid or missing")]
    KeyInvalid { host: String },
    #[error("could not store the managed certificate copy: {0}")]
    Storage(String),
}

/// A certificate that passed every loading check. The parsed chain and key are
/// kept so the TLS acceptor can be built once per connection.
#[derive(Debug)]
pub struct ValidatedCertificate {
    pub host: String,
    pub not_before: i64,
    pub not_after: i64,
    pub san: Vec<String>,
    pub fingerprint: String,
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
    pub fullchain_pem: Vec<u8>,
    pub privkey_pem: Vec<u8>,
}

impl ValidatedCertificate {
    /// Builds a rustls server configuration that serves this certificate only
    /// when the connecting client's SNI equals the subscription host. A missing
    /// or mismatched SNI is rejected at handshake time, before any HTTP request.
    pub fn server_config(&self) -> Result<std::sync::Arc<rustls::ServerConfig>, CertificateError> {
        let provider = rustls::ServerConfig::builder().crypto_provider().clone();
        let certified = CertifiedKey::from_der(self.chain.clone(), self.key.clone_key(), &provider)
            .map_err(|_| CertificateError::KeyMismatch {
                host: self.host.clone(),
            })?;
        let resolver = SubscriptionHostResolver {
            host: self.host.clone(),
            certified: std::sync::Arc::new(certified),
        };
        Ok(std::sync::Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_cert_resolver(std::sync::Arc::new(resolver)),
        ))
    }
}

/// Serves the pinned certificate only for the exact subscription host SNI.
#[derive(Debug)]
struct SubscriptionHostResolver {
    host: String,
    certified: std::sync::Arc<CertifiedKey>,
}

impl ResolvesServerCert for SubscriptionHostResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<std::sync::Arc<CertifiedKey>> {
        (client_hello.server_name() == Some(self.host.as_str()))
            .then(|| std::sync::Arc::clone(&self.certified))
    }
}

/// The Certbot live directory a certificate is renewed into.
fn live_directory(store: &DeploymentStore, config: &DeploymentConfig) -> PathBuf {
    store
        .root()
        .join("etc/letsencrypt/live")
        .join(&config.subscription_host)
}

/// The sbctl-owned copy that both service accounts read from.
fn pinned_directory(store: &DeploymentStore, config: &DeploymentConfig) -> PathBuf {
    store.certificate_directory(&config.subscription_host)
}

/// Validates the currently renewed certificate in the Certbot live directory.
pub fn load(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<ValidatedCertificate, CertificateError> {
    load_at(store, config, Utc::now())
}

/// Validates the certificate with an explicit instant so tests can exercise
/// expiry and not-yet-valid states without forging certificates.
pub fn load_at(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<ValidatedCertificate, CertificateError> {
    load_directory_at(&live_directory(store, config), config, now)
}

/// Loads and validates the pinned copy served by the sbctl daemon. This is the
/// copy the deploy hook re-creates after each Certbot renewal.
pub fn load_pinned(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<ValidatedCertificate, CertificateError> {
    load_directory_at(&pinned_directory(store, config), config, Utc::now())
}

fn load_directory_at(
    directory: &Path,
    config: &DeploymentConfig,
    now: DateTime<Utc>,
) -> Result<ValidatedCertificate, CertificateError> {
    require_direct(config)?;
    let host = config.subscription_host.clone();
    let fullchain_pem = fs::read(directory.join("fullchain.pem")).map_err(|error| {
        CertificateError::Unreadable {
            host: host.clone(),
            detail: error.to_string(),
        }
    })?;
    let privkey_pem =
        fs::read(directory.join("privkey.pem")).map_err(|error| CertificateError::Unreadable {
            host: host.clone(),
            detail: error.to_string(),
        })?;
    let mut chain_reader = BufReader::new(&fullchain_pem[..]);
    let chain = rustls_pemfile::certs(&mut chain_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CertificateError::MalformedCertificate { host: host.clone() })?;
    let end_entity = chain
        .first()
        .ok_or_else(|| CertificateError::MalformedCertificate { host: host.clone() })?;
    let (_, parsed) = X509Certificate::from_der(end_entity.as_ref())
        .map_err(|_| CertificateError::MalformedCertificate { host: host.clone() })?;

    let now_ts = now.timestamp();
    let not_before = parsed.validity().not_before.timestamp();
    let not_after = parsed.validity().not_after.timestamp();
    if now_ts < not_before {
        return Err(CertificateError::NotYetValid {
            host: host.clone(),
            not_before: parsed.validity().not_before.to_string(),
        });
    }
    if now_ts > not_after {
        return Err(CertificateError::Expired {
            host: host.clone(),
            not_after: parsed.validity().not_after.to_string(),
        });
    }

    let san = certificate_names(&parsed);
    if !san.iter().any(|name| dns_name_matches(name, &host)) {
        return Err(CertificateError::SanMismatch { host: host.clone() });
    }

    let key = rustls_pemfile::private_key(&mut BufReader::new(&privkey_pem[..]))
        .map_err(|_| CertificateError::KeyInvalid { host: host.clone() })?
        .ok_or_else(|| CertificateError::KeyInvalid { host: host.clone() })?;
    let provider = rustls::ServerConfig::builder().crypto_provider().clone();
    CertifiedKey::from_der(chain.clone(), key.clone_key(), &provider)
        .map_err(|_| CertificateError::KeyMismatch { host: host.clone() })?;

    let fingerprint = sha256_hex(end_entity.as_ref());
    Ok(ValidatedCertificate {
        host,
        not_before,
        not_after,
        san,
        fingerprint,
        chain,
        key,
        fullchain_pem,
        privkey_pem,
    })
}

/// Obtains a certificate with Certbot's webroot authenticator, then validates
/// and pins it so the daemon and sing-box can serve it immediately.
pub fn obtain(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    email: &str,
) -> Result<ValidatedCertificate, CertificateError> {
    obtain_with_runtime(&Runtime::live(store.root()), store, config, email)
}

pub fn obtain_with_runtime<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
    store: &DeploymentStore,
    config: &DeploymentConfig,
    email: &str,
) -> Result<ValidatedCertificate, CertificateError> {
    require_direct(config)?;
    let (status, output) = runtime
        .run_command_output(
            "certbot",
            &[
                "certonly",
                "--webroot",
                "--webroot-path",
                &store.acme_webroot().to_string_lossy(),
                "--domain",
                &config.subscription_host,
                "--email",
                email,
                "--agree-tos",
                "--non-interactive",
                "--keep-until-expiring",
            ],
        )
        .map_err(|error| CertificateError::Certbot(error.to_string()))?;
    certbot_result(status, output)?;
    publish(store, config)
}

/// Renews certificates with Certbot and re-validates and re-pins them. The
/// daemon loads the pinned copy before every TLS handshake, so a successful
/// renewal takes effect on the next connection without a service restart.
pub fn renew(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<ValidatedCertificate, CertificateError> {
    renew_with_runtime(&Runtime::live(store.root()), store, config)
}

pub fn renew_with_runtime<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<ValidatedCertificate, CertificateError> {
    require_direct(config)?;
    let (status, output) = runtime
        .run_command_output(
            "certbot",
            &["renew", "--cert-name", &config.subscription_host],
        )
        .map_err(|error| CertificateError::Certbot(error.to_string()))?;
    certbot_result(status, output)?;
    publish(store, config)
}

/// The Certbot deploy hook entry point. Certbot runs this after every renewal;
/// it validates the freshly renewed certificate and re-pins it with private-key
/// permissions readable only by the sbctl and sing-box service accounts. A
/// failure makes Certbot keep the previous certificate.
pub fn deploy_hook(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<ValidatedCertificate, CertificateError> {
    publish(store, config)
}

/// Pins the already-issued certificate during installation, but only when one
/// actually exists. A fresh Direct deployment has no certificate yet: the units
/// and the socket-activated HTTPS listener are installed first, then the
/// certificate is obtained and pinned afterwards via `certificate obtain`.
/// Returns the validated certificate when present, or `Ok(None)` when none has
/// been issued yet, so a not-yet-obtained certificate never blocks an install.
pub fn pin_if_present(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<Option<ValidatedCertificate>, CertificateError> {
    if !live_directory(store, config)
        .join("fullchain.pem")
        .is_file()
    {
        return Ok(None);
    }
    publish(store, config).map(Some)
}

fn publish(
    store: &DeploymentStore,
    config: &DeploymentConfig,
) -> Result<ValidatedCertificate, CertificateError> {
    let validated = load(store, config)?;
    pin(store, config, &validated)?;
    Ok(validated)
}

/// Copies the validated certificate into the sbctl-owned pinned directory. The
/// copy is atomic (temporary file + rename) and created private, so a renewal
/// interrupted mid-write never exposes a truncated private key or a file the
/// service accounts cannot read.
pub fn pin(
    store: &DeploymentStore,
    config: &DeploymentConfig,
    validated: &ValidatedCertificate,
) -> Result<(), CertificateError> {
    let directory = pinned_directory(store, config);
    fs::create_dir_all(&directory).map_err(|error| CertificateError::Storage(error.to_string()))?;
    atomic_write_certificate(&directory, "fullchain.pem", &validated.fullchain_pem)?;
    atomic_write_certificate(&directory, "privkey.pem", &validated.privkey_pem)?;
    restrict_certificate_permissions(store, &directory)
}

fn atomic_write_certificate(
    directory: &Path,
    name: &str,
    contents: &[u8],
) -> Result<(), CertificateError> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let temporary = directory.join(format!(".{name}.{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| CertificateError::Storage(error.to_string()))?;
        let result = (|| {
            file.write_all(contents)
                .map_err(|error| CertificateError::Storage(error.to_string()))?;
            file.sync_all()
                .map_err(|error| CertificateError::Storage(error.to_string()))?;
            drop(file);
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o640))
                .map_err(|error| CertificateError::Storage(error.to_string()))?;
            fs::rename(&temporary, directory.join(name))
                .map_err(|error| CertificateError::Storage(error.to_string()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
    #[cfg(not(unix))]
    {
        fs::write(directory.join(name), contents)
            .map_err(|error| CertificateError::Storage(error.to_string()))
    }
}

/// Restricts the pinned certificate files to the service accounts: the host
/// directory is group-only and the files are group-readable. The group
/// ownership is applied only on the live host where the real `sbctl-cert`
/// group exists; fixture roots keep the writing user's ownership.
fn restrict_certificate_permissions(
    store: &DeploymentStore,
    directory: &Path,
) -> Result<(), CertificateError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o750))
            .map_err(|error| CertificateError::Storage(error.to_string()))?;
        if store.root() == Path::new("/") {
            let status = Command::new("chgrp")
                .args(["-R", CERTIFICATE_GROUP, &directory.to_string_lossy()])
                .status()
                .map_err(|error| CertificateError::Storage(error.to_string()))?;
            if !status.success() {
                return Err(CertificateError::Storage(format!(
                    "chgrp {} exited with {status}",
                    CERTIFICATE_GROUP
                )));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (store, directory);
    Ok(())
}

/// A redacted summary used by `status --json` and certificate diagnostics. The
/// summary never includes the private key, the Subscription credential, or a
/// full deployment path.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CertificateStatus {
    pub host: String,
    pub state: &'static str,
    pub not_after: Option<i64>,
    pub not_before: Option<i64>,
    pub san: Vec<String>,
    pub fingerprint: Option<String>,
    pub error: Option<String>,
}

/// Reports certificate state without failing: a missing or invalid certificate
/// becomes an `error` state so the rest of the status report stays available.
pub fn status(store: &DeploymentStore, config: &DeploymentConfig) -> CertificateStatus {
    match load(store, config) {
        Ok(validated) => CertificateStatus {
            host: validated.host.clone(),
            state: "ok",
            not_after: Some(validated.not_after),
            not_before: Some(validated.not_before),
            san: validated.san.clone(),
            fingerprint: Some(validated.fingerprint.clone()),
            error: None,
        },
        Err(error) => CertificateStatus {
            host: config.subscription_host.clone(),
            state: "error",
            not_after: None,
            not_before: None,
            san: Vec::new(),
            fingerprint: None,
            error: Some(error.to_string()),
        },
    }
}

fn require_direct(config: &DeploymentConfig) -> Result<(), CertificateError> {
    (config.subscription_mode == SubscriptionMode::Direct)
        .then_some(())
        .ok_or(CertificateError::NotDirect)
}

fn certbot_result(status: ExitStatus, output: String) -> Result<(), CertificateError> {
    if status.success() {
        return Ok(());
    }
    let diagnostic = redact_private_keys(output.trim()).to_owned();
    Err(CertificateError::CertbotFailed(if diagnostic.is_empty() {
        format!("exited with {status}")
    } else {
        diagnostic
    }))
}

/// Strips any PEM private-key block from a Certbot diagnostic so a failure can
/// never echo the private key into logs or errors.
fn redact_private_keys(text: &str) -> String {
    let mut redacted = String::with_capacity(text.len());
    let mut in_block = false;
    for line in text.lines() {
        if line.starts_with("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_block = true;
            redacted.push_str("[redacted private key block]\n");
            continue;
        }
        if in_block {
            if line.starts_with("-----END") {
                in_block = false;
            }
            continue;
        }
        redacted.push_str(line);
        redacted.push('\n');
    }
    redacted
}

/// Collects every host name a certificate asserts, from the Subject Alternative
/// Name extension and, as a fallback, the Subject Common Name.
fn certificate_names(certificate: &X509Certificate) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(Some(san)) = certificate.subject_alternative_name() {
        for name in &san.value.general_names {
            if let GeneralName::DNSName(dns) = name {
                names.push((*dns).to_owned());
            }
        }
    }
    if let Ok(Some(common_name)) = certificate
        .subject()
        .iter_common_name()
        .next()
        .map(|attribute| attribute.as_str())
        .transpose()
    {
        names.push(common_name.to_owned());
    }
    names
}

/// Matches a host name against a certificate name, supporting a single leftmost
/// wildcard label (`*.example.test` covers `sub.example.test`).
fn dns_name_matches(name: &str, host: &str) -> bool {
    if name == host {
        return true;
    }
    let Some(suffix) = name.strip_prefix("*.") else {
        return false;
    };
    let Some(prefix) = host.strip_suffix(suffix) else {
        return false;
    };
    prefix.ends_with('.') && !prefix[..prefix.len() - 1].contains('.')
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::{CERTIFICATE_GROUP, CertificateError, dns_name_matches, load_at, pin};
    use crate::config::{DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode};
    use chrono::{TimeZone, Utc};
    use rcgen::generate_simple_self_signed;
    use std::fs;
    use tempfile::TempDir;

    /// A Direct VLESS deployment whose subscription host matches the generated
    /// certificates in the tests.
    fn direct_store(fixture: &TempDir) -> (DeploymentStore, DeploymentConfig) {
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
        (store, config)
    }

    fn live_directory(fixture: &TempDir) -> std::path::PathBuf {
        fixture.path().join("etc/letsencrypt/live/sub.example.test")
    }

    fn write_certificate(fixture: &TempDir, names: &[&str]) {
        let certificate = generate_simple_self_signed(
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
        )
        .expect("cert");
        fs::create_dir_all(live_directory(fixture)).expect("certificate directory is created");
        fs::write(
            live_directory(fixture).join("fullchain.pem"),
            certificate.cert.pem(),
        )
        .expect("fullchain is written");
        fs::write(
            live_directory(fixture).join("privkey.pem"),
            certificate.signing_key.serialize_pem(),
        )
        .expect("private key is written");
    }

    fn mid_2025() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 6, 1, 12, 0, 0).unwrap()
    }

    #[test]
    fn valid_certificate_for_the_subscription_host_loads() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["sub.example.test"]);
        let (store, config) = direct_store(&fixture);

        let validated = load_at(&store, &config, mid_2025()).expect("certificate is valid");
        assert_eq!(validated.host, "sub.example.test");
        assert!(validated.san.contains(&"sub.example.test".to_owned()));
        assert!(validated.fingerprint.len() >= 40);
    }

    #[test]
    fn a_wildcard_certificate_covers_the_subscription_host() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["*.example.test"]);
        let (store, config) = direct_store(&fixture);

        assert!(load_at(&store, &config, mid_2025()).is_ok());
    }

    #[test]
    fn an_expired_certificate_is_rejected_before_loading() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["sub.example.test"]);
        let (store, config) = direct_store(&fixture);

        let error = load_at(
            &store,
            &config,
            Utc.with_ymd_and_hms(4097, 1, 1, 0, 0, 0).unwrap(),
        )
        .expect_err("an expired certificate is rejected");
        assert!(matches!(error, CertificateError::Expired { .. }));
    }

    #[test]
    fn a_not_yet_valid_certificate_is_rejected_before_loading() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["sub.example.test"]);
        let (store, config) = direct_store(&fixture);

        let error = load_at(
            &store,
            &config,
            Utc.with_ymd_and_hms(1974, 1, 1, 0, 0, 0).unwrap(),
        )
        .expect_err("a not-yet-valid certificate is rejected");
        assert!(matches!(error, CertificateError::NotYetValid { .. }));
    }

    #[test]
    fn a_certificate_whose_san_misses_the_host_is_rejected() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["other.example.test"]);
        let (store, config) = direct_store(&fixture);

        let error = load_at(&store, &config, mid_2025()).expect_err("a SAN mismatch is rejected");
        assert!(matches!(error, CertificateError::SanMismatch { .. }));
    }

    #[test]
    fn a_mismatched_private_key_is_rejected() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["sub.example.test"]);
        fs::write(
            live_directory(&fixture).join("privkey.pem"),
            generate_simple_self_signed(vec!["unrelated.example.test".into()])
                .expect("a second certificate")
                .signing_key
                .serialize_pem(),
        )
        .expect("the wrong private key is written");
        let (store, config) = direct_store(&fixture);

        let error =
            load_at(&store, &config, mid_2025()).expect_err("a mismatched private key is rejected");
        assert!(matches!(error, CertificateError::KeyMismatch { .. }));
    }

    #[test]
    fn missing_certificate_files_are_a_diagnosable_unreadable_error() {
        let fixture = TempDir::new().expect("temporary root is created");
        let (store, config) = direct_store(&fixture);

        let error = load_at(&store, &config, mid_2025()).expect_err("missing files are rejected");
        assert!(matches!(error, CertificateError::Unreadable { .. }));
    }

    #[test]
    fn non_direct_modes_never_load_certificates() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["sub.example.test"]);
        let store = DeploymentStore::new(fixture.path());
        let config = DeploymentConfig::new(
            SubscriptionMode::ExternalProxy,
            "sub.example.test".into(),
            None,
            None,
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an external-proxy deployment is valid");

        let error = load_at(&store, &config, mid_2025()).expect_err("external proxy is rejected");
        assert!(matches!(error, CertificateError::NotDirect));
    }

    #[test]
    fn pin_stores_a_restricted_copy_that_loads_as_pinned() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_certificate(&fixture, &["sub.example.test"]);
        let (store, config) = direct_store(&fixture);
        let validated = load_at(&store, &config, mid_2025()).expect("certificate is valid");

        pin(&store, &config, &validated).expect("the certificate is pinned");

        let pinned = store.certificate_directory("sub.example.test");
        assert!(pinned.join("fullchain.pem").is_file());
        assert!(pinned.join("privkey.pem").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(pinned.join("privkey.pem"))
                .expect("pinned key has metadata")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o640,
                "the private key is group-readable only"
            );
            let directory_mode = fs::metadata(&pinned)
                .expect("pinned directory has metadata")
                .permissions()
                .mode();
            assert_eq!(
                directory_mode & 0o777,
                0o750,
                "the pinned directory is not world-listable"
            );
        }
        let pinned_validated = crate::certificate::load_pinned(&store, &config)
            .expect("the pinned copy is independently loadable");
        assert_eq!(pinned_validated.fingerprint, validated.fingerprint);
    }

    #[test]
    fn certbot_diagnostics_never_include_a_private_key_block() {
        let diagnostics = "Renewal failed.\n-----BEGIN PRIVATE KEY-----\nZm9vYmFy\n-----END PRIVATE KEY-----\n\nMore output\n";
        let redacted = super::redact_private_keys(diagnostics);
        assert!(!redacted.contains("BEGIN PRIVATE KEY"));
        assert!(redacted.contains("[redacted private key block]"));
        assert!(redacted.contains("More output"));
    }

    #[test]
    fn dns_name_matching_supports_exact_and_single_label_wildcard_names() {
        assert!(dns_name_matches("sub.example.test", "sub.example.test"));
        assert!(dns_name_matches("*.example.test", "sub.example.test"));
        assert!(!dns_name_matches("*.example.test", "a.b.example.test"));
        assert!(!dns_name_matches("*.example.test", "example.test"));
        assert!(!dns_name_matches("sub.example.test", "other.example.test"));
    }

    #[test]
    fn the_certificate_group_is_the_single_shared_reader_group() {
        assert_eq!(CERTIFICATE_GROUP, "sbctl-cert");
    }
}
