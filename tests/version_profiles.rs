//! Real-core validation of the generated sing-box client profiles.
//!
//! This test is ignored by default because it needs sing-box binaries that are
//! not installed with the suite. The CI `sing-box-profiles` job downloads one
//! pinned core per minor release and exports `SING_BOX_BIN_<major>_<minor>`,
//! then runs `cargo test --test version_profiles -- --ignored`.

use std::path::Path;

use sbctl::config::{DeploymentConfig, ManagedProtocol, SubscriptionMode};
use sbctl::subscription::{SING_BOX_VERSION_PROFILES, SubscriptionFormat, generated_artifacts};

#[test]
#[ignore = "requires real sing-box cores; run by the CI sing-box-profiles job"]
fn generated_profiles_pass_a_real_sing_box_check() {
    let root = tempfile::tempdir().expect("temporary root is created");
    let config = DeploymentConfig::new(
        SubscriptionMode::IpFallback,
        "127.0.0.1".into(),
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
    )
    .expect("a five-protocol IP fallback deployment is valid");
    let artifacts = generated_artifacts(&config, root.path()).expect("artifacts generate");

    let mut checked = 0;
    for profile in SING_BOX_VERSION_PROFILES {
        let variable = format!(
            "SING_BOX_BIN_{}_{}",
            profile.version.major, profile.version.minor
        );
        let Ok(binary) = std::env::var(&variable) else {
            eprintln!(
                "skipping sing-box {}: {variable} is not set",
                profile.version
            );
            continue;
        };
        let name = SubscriptionFormat::SingBoxVersion(profile.version)
            .artifact_name()
            .into_owned();
        let contents = artifacts
            .iter()
            .find(|(artifact, _)| *artifact == name)
            .map(|(_, contents)| contents)
            .unwrap_or_else(|| panic!("missing profile artifact {name}"));
        sbctl::subscription::check_sing_box_config(Path::new(&binary), contents).unwrap_or_else(
            |error| panic!("sing-box {} rejected {name}: {error}", profile.version),
        );
        eprintln!("sing-box {} accepted {name}", profile.version);
        checked += 1;
    }
    assert!(
        checked >= 1,
        "no sing-box profile was checked; set SING_BOX_BIN_<major>_<minor>"
    );
}

/// The generated *server* configuration must be accepted by the latest stable
/// kernel on an IPv4-only host. This guards the fields sing-box removed in
/// 1.13 (notably inbound `domain_strategy`): the fake-kernel acceptance suite
/// cannot catch them, and a default `sbctl install` downloads the latest core.
#[test]
#[ignore = "requires the latest real sing-box core; run by the CI sing-box-profiles job"]
fn server_config_passes_the_latest_real_core_check() {
    let Ok(binary) = std::env::var("SING_BOX_BIN_1_14") else {
        eprintln!("skipping: SING_BOX_BIN_1_14 is not set");
        return;
    };
    let root = tempfile::tempdir().expect("temporary root is created");
    let mut config = DeploymentConfig::new(
        SubscriptionMode::IpFallback,
        "127.0.0.1".into(),
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
    )
    .expect("a five-protocol IP fallback deployment is valid");
    // Force the IPv4-only resolution path regardless of the CI host's routing,
    // so the generated `dns.strategy` and route `resolve` rules are exercised.
    config.ipv4_only = true;
    let artifacts = generated_artifacts(&config, root.path()).expect("artifacts generate");
    let server = artifacts
        .iter()
        .find(|(name, _)| name == "sing-box-server.json")
        .map(|(_, contents)| contents)
        .expect("the server artifact is generated");
    sbctl::subscription::check_sing_box_config(Path::new(&binary), server).unwrap_or_else(
        |error| panic!("the latest sing-box rejected the generated server config: {error}"),
    );
}
