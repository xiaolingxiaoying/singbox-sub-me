use std::fs;

use tempfile::TempDir;

use super::artifacts::generated_artifacts;
use crate::config::{DeploymentConfig, DeploymentStore, ManagedProtocol, SubscriptionMode};

/// Establishes the minimal traffic fixture so a subscription response can
/// read a legal accounting state and the generated URI artifact.
pub(crate) fn seed_direct_subscription(
    fixture: &TempDir,
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
pub(crate) fn seed_all_protocols(fixture: &TempDir) -> (DeploymentStore, DeploymentConfig, String) {
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
pub(crate) fn seed_single_protocol(
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

pub(crate) fn vless_config() -> DeploymentConfig {
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
