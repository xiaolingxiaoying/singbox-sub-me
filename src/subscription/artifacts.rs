use crate::config::ConfigError;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use thiserror::Error;

use super::profile::{
    CLASH_LEGACY_VERSION, SING_BOX_VERSION_PROFILES, SubscriptionFormat, SubscriptionRoute,
    latest_version_profile,
};
use super::render::{
    clash, clash_legacy, ensure_subscription_nodes, shadowrocket, sing_box, sing_box_full,
    sing_box_server, uri,
};
use super::{base64_uri, constant_time_eq};
use crate::config::{DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode};

pub(super) const SING_BOX_ARTIFACT: &str = "subscription-sing-box.json";
pub(super) const SING_BOX_FULL_ARTIFACT: &str = "subscription-sing-box-full.json";
pub(super) const CLASH_ARTIFACT: &str = "subscription-clash.yaml";
pub(super) const URI_ARTIFACT: &str = "subscription-uri.txt";
pub(super) const BASE64_URI_ARTIFACT: &str = "subscription-base64-uri.txt";
pub(super) const SHADOWROCKET_ARTIFACT: &str = "subscription-shadowrocket.txt";
const SING_BOX_SERVER_ARTIFACT: &str = "sing-box-server.json";
const ARTIFACTS_RELATIVE_DIR: &str = "var/lib/sbctl/artifacts";
const ACTIVE_CONFIG_RELATIVE_PATH: &str = "etc/sing-box/config.json";

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
    if let Err(error) = remove_stale_artifacts(store, &artifacts) {
        eprintln!("warning: superseded subscription artifacts could not be removed: {error}");
    }
    Ok(DeploymentSnapshot {
        config: prior_config.unwrap_or_default(),
        artifacts: prior_artifacts,
        active_config: prior_active,
    })
}

/// Removes superseded subscription artifacts.
///
/// A skipped version profile — an AnyTLS-only deployment, or a minor later
/// dropped from the registry — otherwise leaves its previous file on disk,
/// still reachable at a valid URL and handing a client stale hosts and
/// credentials with no way to tell it is out of date.
fn remove_stale_artifacts(
    store: &DeploymentStore,
    current: &[(String, String)],
) -> Result<(), std::io::Error> {
    let Ok(entries) = fs::read_dir(store.root().join("var/lib/sbctl/artifacts")) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Only names this generator owns are eligible; anything else an
        // administrator placed in the directory is left alone.
        if !(name.starts_with("subscription-") || name == SING_BOX_SERVER_ARTIFACT) {
            continue;
        }
        if current.iter().any(|(kept, _)| *kept == name) {
            continue;
        }
        fs::remove_file(entry.path())?;
    }
    Ok(())
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
    let contents = fs::read(
        store
            .root()
            .join("var/lib/sbctl/artifacts")
            .join(format.artifact_name().as_ref()),
    )?;
    // A corrupted artifact must not go out with replacement characters silently
    // spliced into a client's configuration; the caller turns this into the
    // redacted 503.
    String::from_utf8(contents).map_err(|_| {
        SubscriptionError::Artifact(std::io::Error::other(
            "the stored subscription artifact is not valid UTF-8",
        ))
    })
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
            crate::canonical::uri_host(&config.subscription_host),
            config.http_port.expect("validated IP fallback port")
        ),
        SubscriptionMode::Direct | SubscriptionMode::ExternalProxy => {
            format!(
                "https://{}",
                crate::canonical::uri_host(&config.subscription_host)
            )
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

#[cfg(test)]
mod tests {
    use base64::Engine;
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::{generated_artifacts, regenerate};
    use crate::config::{DeploymentConfig, DeploymentStore, ManagedProtocol};
    use crate::subscription::test_support::{
        seed_direct_subscription, seed_single_protocol, vless_config,
    };

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
