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
    assert_eq!(
        checked,
        SING_BOX_VERSION_PROFILES.len(),
        "every profile must be validated by its own core: {checked} of {}\n\
         were checked, so a missing SING_BOX_BIN_<major>_<minor> would let a\n\
         whole minor ship unverified",
        SING_BOX_VERSION_PROFILES.len()
    );
}

/// The registry's newest minor has to be the newest *stable* upstream release.
///
/// `sing-box-full.json` targets `SING_BOX_VERSION_PROFILES.last()`, so the
/// moment sing-box ships 1.15 the server installs a kernel newer than anything
/// the registry describes, `sing-box-1.15.json` 404s, and the label claiming
/// the full artifact matches the running kernel stops being true. This turns
/// that drift into a red build while the remedy is still a five-line registry
/// entry. The CI `sing-box-profiles` job exports `SBCTL_UPSTREAM_LATEST` from
/// GitHub's `releases/latest` endpoint, which already excludes drafts and
/// prereleases.
#[test]
#[ignore = "requires SBCTL_UPSTREAM_LATEST; exported by the CI sing-box-profiles job"]
fn the_registry_tracks_the_latest_stable_sing_box_release() {
    let latest = std::env::var("SBCTL_UPSTREAM_LATEST")
        .expect("SBCTL_UPSTREAM_LATEST must be set to the latest stable sing-box version");
    let mut parts = latest.trim_start_matches('v').split('.');
    let major: u8 = parts
        .next()
        .expect("a major version")
        .parse()
        .expect("major is numeric");
    let minor: u8 = parts
        .next()
        .expect("a minor version")
        .parse()
        .expect("minor is numeric");
    let top = SING_BOX_VERSION_PROFILES
        .last()
        .expect("the registry is not empty");
    assert_eq!(
        (top.version.major, top.version.minor),
        (major, minor),
        "sing-box {major}.{minor} is the latest stable release but the version \
         registry tops out at {}.{}; add the new profile, its notes, and bump \
         the pinned cores in .github/workflows/ci.yml",
        top.version.major,
        top.version.minor
    );
}

/// The registry must be a contiguous band of minors ending at the tracked
/// latest stable, with `supported` ranges that chain and non-empty notes.
/// Catches a bad hand-edit without needing a core or the network.
#[test]
fn the_version_registry_is_a_contiguous_chained_band() {
    assert!(!SING_BOX_VERSION_PROFILES.is_empty());
    for pair in SING_BOX_VERSION_PROFILES.windows(2) {
        let (older, newer) = (&pair[0], &pair[1]);
        assert_eq!(
            (older.version.major, older.version.minor + 1),
            (newer.version.major, newer.version.minor),
            "the registry must cover every minor with no gap: {} -> {}",
            older.version,
            newer.version
        );
        assert!(
            newer.supported.contains(&format!(">= {}", older.version))
                || older.supported.contains(&format!("< {}", newer.version)),
            "the supported ranges of {} and {} must chain",
            older.version,
            newer.version
        );
    }
    for profile in SING_BOX_VERSION_PROFILES {
        assert!(
            !profile.notes.trim().is_empty(),
            "{} has no notes",
            profile.version
        );
        assert!(
            profile.supported.contains(">="),
            "{} needs a lower bound",
            profile.version
        );
    }
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
