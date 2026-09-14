//! Real-core validation of the generated clash/mihomo artifacts.
//!
//! Ignored by default because it needs a mihomo binary that is not installed
//! with the suite. The CI `mihomo-profiles` job downloads a pinned mihomo and
//! sets `MIHOMO_BIN`, then runs `cargo test --test clash_mihomo -- --ignored`.

use std::fs;
use std::process::Command;

use sbctl::config::{DeploymentConfig, ManagedProtocol, SubscriptionMode};
use sbctl::subscription::{CLASH_LEGACY_VERSION, SubscriptionFormat, generated_artifacts};

#[test]
#[ignore = "requires a real mihomo binary; run by the CI mihomo-profiles job"]
fn generated_clash_artifacts_load_in_a_real_mihomo() {
    let Ok(binary) = std::env::var("MIHOMO_BIN") else {
        panic!("MIHOMO_BIN must point at a mihomo binary");
    };
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

    for format in [
        SubscriptionFormat::Clash,
        SubscriptionFormat::ClashLegacy(CLASH_LEGACY_VERSION),
    ] {
        let name = format.artifact_name().into_owned();
        let contents = artifacts
            .iter()
            .find(|(artifact, _)| *artifact == name)
            .map(|(_, contents)| contents.clone())
            .unwrap_or_else(|| panic!("missing clash artifact {name}"));
        let config_path = root.path().join(&name);
        fs::write(&config_path, contents).expect("clash artifact is written");
        let output = Command::new(&binary)
            .arg("-t")
            .arg("-d")
            .arg(root.path())
            .arg("-f")
            .arg(&config_path)
            .output()
            .expect("mihomo runs");
        assert!(
            output.status.success(),
            "mihomo rejected {name}:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("mihomo accepted {name}");
    }
}
