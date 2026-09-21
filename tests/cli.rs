use assert_cmd::Command;
use base64::Engine;
use predicates::prelude::*;
use std::fs;
use std::net::TcpListener;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

#[path = "cli/fixture.rs"]
mod fixture;
#[path = "cli/install.rs"]
mod install;
#[path = "cli/menu.rs"]
mod menu;
#[path = "cli/update_release.rs"]
mod update_release;

use crate::fixture::{
    free_high_tcp_port, http_get, http_request, initialize_ip_fallback_subscription,
    initialize_traffic_fixture, initialize_uninstall_fixture, read_subscription_credential,
    read_vless_uuid, refusal_message, run_traffic_set_used, seed_direct_config,
    seed_live_certificate, sing_box_check_fixture, spawn_sbctl_serve, supported_systemd_host,
    write_managed_file, write_systemctl_fixture, write_traffic_fixture,
};

#[cfg(unix)]
use crate::fixture::write_command_fixture;

#[test]
fn status_reports_an_unmanaged_host_before_installation() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "sbctl status: unmanaged (not installed)",
        ));
}

#[test]
fn configuration_initialization_persists_a_redacted_deployment_summary() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "status",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("sbctl status: configured"))
        .stdout(predicate::str::contains("mode: ip-fallback"))
        .stdout(predicate::str::contains("subscription host: 203.0.113.7"))
        .stdout(predicate::str::contains("interface: ens3"))
        .stdout(predicate::str::contains("vless-reality"));

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    assert!(config.contains("subscription_credential"));
    assert!(!config.contains("[redacted]"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "show",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "subscription credential: [redacted]",
        ))
        .stdout(predicate::str::contains("subscription_credential =").not());
}

#[test]
fn configuration_validation_rejects_an_ip_fallback_host_that_is_not_an_ip_address() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "sub.example.test",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "hysteria2",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "IP fallback subscription requires an IP address",
        ));

    assert!(!fixture.path().join("etc/sbctl/config.toml").exists());
}

#[test]
fn direct_domain_mode_generates_https_subscription_urls_without_an_http_port() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sub",
            "--format",
            "uri",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("https://sub.example.test/sub/"));

    assert!(
        fixture
            .path()
            .join("var/lib/sbctl/acme-webroot/.well-known/acme-challenge")
            .is_dir()
    );
}

#[test]
fn vless_reality_ip_fallback_exports_consistent_subscription_formats() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--proxy-host",
            "198.51.100.9",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let credential = config
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("subscription credential is persisted");

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server = fs::read_to_string(artifacts.join("sing-box-server.json"))
        .expect("sing-box server configuration is cached");
    let sing_box = fs::read_to_string(artifacts.join("subscription-sing-box.json"))
        .expect("sing-box subscription is cached");
    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    let base64_uri = fs::read_to_string(artifacts.join("subscription-base64-uri.txt"))
        .expect("Base64 URI subscription is cached");
    assert!(sing_box.contains("\"type\": \"vless\""));
    assert!(server.contains("\"private_key\""));
    assert!(sing_box.contains("198.51.100.9"));
    assert!(clash.contains("type: vless"));
    assert!(clash.contains("198.51.100.9"));
    assert!(uri.starts_with("vless://"));
    assert!(uri.contains("198.51.100.9:"));
    for value in ["www.cloudflare.com", "security=reality", "xtls-rprx-vision"] {
        assert!(uri.contains(value), "URI contains {value}");
    }
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(base64_uri.trim())
            .expect("Base64 URI subscription decodes"),
        uri.as_bytes(),
        "the Base64 URI subscription is an exact encoding of the canonical URI artifact"
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sub",
            "--format",
            "uri",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("/sub/{credential}/uri")));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sub",
            "--format",
            "base64-uri",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "/sub/{credential}/uri.txt"
        )));
}

#[test]
fn domain_nodes_export_vmess_websocket_and_hysteria2_with_independent_tls_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(&fixture, true, &["vmess"]);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("sing-box-server.json"))
            .expect("sing-box server configuration is cached"),
    )
    .expect("server configuration is JSON");
    let subscription: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("subscription-sing-box.json"))
            .expect("sing-box subscription is cached"),
    )
    .expect("subscription is JSON");
    let inbounds = server["inbounds"].as_array().expect("inbounds are present");
    let outbounds = subscription["outbounds"]
        .as_array()
        .expect("outbounds are present");
    assert_eq!(inbounds.len(), 3);
    assert_eq!(outbounds.len(), 3);
    let vmess = outbounds
        .iter()
        .find(|node| node["type"] == "vmess")
        .expect("VMess WebSocket node is exported");
    let hysteria = outbounds
        .iter()
        .find(|node| node["type"] == "hysteria2")
        .expect("Hysteria2 node is exported");
    assert_eq!(vmess["server"], "proxy.example.test");
    assert_eq!(vmess["tls"]["server_name"], "sub.example.test");
    assert_eq!(hysteria["server"], "proxy.example.test");
    assert_eq!(hysteria["tls"]["server_name"], "sub.example.test");
    assert_ne!(vmess["server_port"], hysteria["server_port"]);
    assert_ne!(vmess["uuid"], hysteria["password"]);

    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let _: serde_yaml::Value = serde_yaml::from_str(&clash).expect("Clash subscription is YAML");
    assert!(clash.contains("type: vmess"));
    assert!(clash.contains("type: hysteria2"));
    assert!(clash.contains("servername: sub.example.test"));
    assert!(clash.contains("sni: sub.example.test"));
    assert!(clash.contains(vmess["uuid"].as_str().expect("VMess UUID is text")));
    assert!(
        clash.contains(
            hysteria["password"]
                .as_str()
                .expect("Hysteria2 password is text")
        )
    );

    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    assert!(uri.contains("vmess://"));
    assert!(uri.contains("hysteria2://"));
    assert!(uri.contains("sni=sub.example.test"));
    let vmess_uri = uri
        .lines()
        .find(|line| line.starts_with("vmess://"))
        .expect("VMess URI is present");
    let vmess_payload: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(vmess_uri.trim_start_matches("vmess://"))
            .expect("VMess URI payload is base64"),
    )
    .expect("VMess URI payload is JSON");
    assert_eq!(vmess_payload["id"], vmess["uuid"]);
    assert_eq!(vmess_payload["port"], vmess["server_port"].to_string());
    assert!(
        uri.contains(
            hysteria["password"]
                .as_str()
                .expect("Hysteria2 password is text")
        )
    );
}

#[test]
fn config_init_persists_explicit_ports_for_all_five_managed_protocols() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let ports = [
        free_high_tcp_port(),
        free_high_tcp_port(),
        free_high_tcp_port(),
        free_high_tcp_port(),
        free_high_tcp_port(),
    ];

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--vless-port",
            &ports[0].to_string(),
            "--vmess-port",
            &ports[1].to_string(),
            "--hysteria2-port",
            &ports[2].to_string(),
            "--tuic-port",
            &ports[3].to_string(),
            "--anytls-port",
            &ports[4].to_string(),
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let persisted: sbctl::config::DeploymentConfig = toml::from_str(
        &fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
            .expect("configuration is persisted"),
    )
    .expect("persisted configuration is valid TOML");
    assert_eq!(persisted.vless_reality.unwrap().listen_port, ports[0]);
    assert_eq!(persisted.vmess_websocket.unwrap().listen_port, ports[1]);
    assert_eq!(persisted.hysteria2.unwrap().listen_port, ports[2]);
    assert_eq!(persisted.tuic.unwrap().listen_port, ports[3]);
    assert_eq!(persisted.anytls.unwrap().listen_port, ports[4]);
}

#[test]
fn domain_nodes_export_tuic_and_anytls_with_independent_tls_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(&fixture, true, &["tuic", "anytls"]);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("sing-box-server.json"))
            .expect("sing-box server configuration is cached"),
    )
    .expect("server configuration is JSON");
    let subscription: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("subscription-sing-box.json"))
            .expect("sing-box subscription is cached"),
    )
    .expect("subscription is JSON");
    let inbounds = server["inbounds"].as_array().expect("inbounds are present");
    let outbounds = subscription["outbounds"]
        .as_array()
        .expect("outbounds are present");
    assert_eq!(inbounds.len(), 5);
    assert_eq!(outbounds.len(), 5);
    let tuic = outbounds
        .iter()
        .find(|node| node["type"] == "tuic")
        .expect("TUIC node is exported");
    let anytls = outbounds
        .iter()
        .find(|node| node["type"] == "anytls")
        .expect("AnyTLS node is exported");
    assert_eq!(tuic["server"], "proxy.example.test");
    assert_eq!(tuic["tls"]["server_name"], "sub.example.test");
    assert_eq!(anytls["server"], "proxy.example.test");
    assert_eq!(anytls["tls"]["server_name"], "sub.example.test");
    assert_ne!(tuic["server_port"], anytls["server_port"]);
    assert_ne!(tuic["uuid"], anytls["password"]);
    assert_ne!(tuic["password"], anytls["password"]);

    let configuration = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let subscription_credential = configuration
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('\"'))
        })
        .expect("subscription credential is persisted");
    assert_ne!(subscription_credential, tuic["password"]);
    assert_ne!(subscription_credential, anytls["password"]);

    let mut baseline: sbctl::config::DeploymentConfig =
        toml::from_str(&configuration).expect("configuration is valid TOML");
    baseline.enabled_protocols.retain(|protocol| {
        !matches!(
            protocol,
            sbctl::config::ManagedProtocol::Tuic | sbctl::config::ManagedProtocol::Anytls
        )
    });
    baseline.tuic = None;
    baseline.anytls = None;
    let baseline_artifacts = sbctl::subscription::generated_artifacts(&baseline, fixture.path())
        .expect("existing protocol artifacts are generated");
    let baseline_subscription = baseline_artifacts
        .iter()
        .find(|(name, _)| *name == "subscription-sing-box.json")
        .map(|(_, contents)| serde_json::from_str::<serde_json::Value>(contents))
        .expect("baseline sing-box subscription is present")
        .expect("baseline sing-box subscription is JSON");
    let existing_types = ["vless", "vmess", "hysteria2"];
    let retained_outbounds = outbounds
        .iter()
        .filter(|node| existing_types.contains(&node["type"].as_str().unwrap_or_default()))
        .collect::<Vec<_>>();
    assert_eq!(
        retained_outbounds,
        baseline_subscription["outbounds"]
            .as_array()
            .expect("baseline outbounds are present")
            .iter()
            .collect::<Vec<_>>(),
        "adding TUIC and AnyTLS preserves generated existing protocol nodes"
    );

    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let _: serde_yaml::Value = serde_yaml::from_str(&clash).expect("Clash subscription is YAML");
    assert!(clash.contains("type: tuic"));
    assert!(clash.contains("type: anytls"));
    assert!(clash.contains(tuic["uuid"].as_str().expect("TUIC UUID is text")));
    assert!(
        clash.contains(
            anytls["password"]
                .as_str()
                .expect("AnyTLS password is text")
        )
    );

    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    let tuic_uri = uri
        .lines()
        .find(|line| line.starts_with("tuic://"))
        .expect("TUIC URI is present");
    let anytls_uri = uri
        .lines()
        .find(|line| line.starts_with("anytls://"))
        .expect("AnyTLS URI is present");
    let parsed_tuic = url::Url::parse(tuic_uri).expect("TUIC URI is syntactically valid");
    assert_eq!(parsed_tuic.scheme(), "tuic");
    assert_eq!(parsed_tuic.host_str(), Some("proxy.example.test"));
    assert_eq!(
        parsed_tuic.port(),
        Some(tuic["server_port"].as_u64().expect("TUIC port") as u16)
    );
    let parsed_anytls = url::Url::parse(anytls_uri).expect("AnyTLS URI is syntactically valid");
    assert_eq!(parsed_anytls.scheme(), "anytls");
    assert_eq!(parsed_anytls.host_str(), Some("proxy.example.test"));
    assert_eq!(
        parsed_anytls.port(),
        Some(anytls["server_port"].as_u64().expect("AnyTLS port") as u16)
    );
    for credential in [
        tuic["uuid"].as_str().expect("TUIC UUID is text"),
        tuic["password"].as_str().expect("TUIC password is text"),
    ] {
        assert!(tuic_uri.contains(credential));
    }
    assert!(tuic_uri.contains("proxy.example.test"));
    assert!(tuic_uri.contains("sni=sub.example.test"));
    assert!(
        anytls_uri.contains(
            anytls["password"]
                .as_str()
                .expect("AnyTLS password is text")
        )
    );
    assert!(anytls_uri.contains("proxy.example.test"));
    assert!(anytls_uri.contains("sni=sub.example.test"));
}

#[test]
fn five_protocols_export_the_same_canonical_nodes_across_server_and_subscription_formats() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--proxy-host",
            "proxy.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let server: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("sing-box-server.json"))
            .expect("server configuration is cached"),
    )
    .expect("server configuration is JSON");
    let subscription: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(artifacts.join("subscription-sing-box.json"))
            .expect("sing-box subscription is cached"),
    )
    .expect("subscription is JSON");
    let clash = fs::read_to_string(artifacts.join("subscription-clash.yaml"))
        .expect("Clash subscription is cached");
    let uri = fs::read_to_string(artifacts.join("subscription-uri.txt"))
        .expect("URI subscription is cached");
    let inbounds = server["inbounds"].as_array().expect("inbounds are present");
    let outbounds = subscription["outbounds"]
        .as_array()
        .expect("outbounds are present");
    assert_eq!(inbounds.len(), 5);
    assert_eq!(outbounds.len(), 5);

    for outbound in outbounds {
        let kind = outbound["type"].as_str().expect("node type is present");
        let inbound = inbounds
            .iter()
            .find(|inbound| inbound["type"] == outbound["type"])
            .expect("every exported node has a server inbound");
        assert_eq!(
            outbound["server_port"], inbound["listen_port"],
            "{kind} keeps one port across the server and the client configuration"
        );
        assert_eq!(
            outbound["server"], "proxy.example.test",
            "{kind} uses the proxy host in every client format"
        );
        let expected_sni = if kind == "vless" {
            "www.cloudflare.com"
        } else {
            "sub.example.test"
        };
        assert_eq!(
            outbound["tls"]["server_name"], expected_sni,
            "{kind} uses the canonical TLS server name"
        );
        assert!(
            clash.contains(&outbound["server_port"].as_u64().expect("port").to_string()),
            "Clash carries the {kind} port"
        );
        let port_text = outbound["server_port"].as_u64().expect("port").to_string();
        if kind == "vmess" {
            let vmess_uri = uri
                .lines()
                .find(|line| line.starts_with("vmess://"))
                .expect("VMess URI is present");
            let payload: serde_json::Value = serde_json::from_slice(
                &base64::engine::general_purpose::STANDARD
                    .decode(vmess_uri.trim_start_matches("vmess://"))
                    .expect("VMess URI payload is base64"),
            )
            .expect("VMess URI payload is JSON");
            assert_eq!(payload["port"], port_text, "URI carries the vmess port");
        } else {
            assert!(uri.contains(&port_text), "URI carries the {kind} port");
        }
        assert!(uri.contains("proxy.example.test"));
        for secret in [outbound["uuid"].as_str(), outbound["password"].as_str()]
            .into_iter()
            .flatten()
        {
            assert!(
                clash.contains(secret),
                "Clash carries the same {kind} credential as the sing-box JSON"
            );
            if kind == "vmess" {
                let vmess_uri = uri
                    .lines()
                    .find(|line| line.starts_with("vmess://"))
                    .expect("VMess URI is present");
                let payload: serde_json::Value = serde_json::from_slice(
                    &base64::engine::general_purpose::STANDARD
                        .decode(vmess_uri.trim_start_matches("vmess://"))
                        .expect("VMess URI payload is base64"),
                )
                .expect("VMess URI payload is JSON");
                assert_eq!(
                    payload["id"], outbound["uuid"],
                    "URI carries the same VMess credential as the sing-box JSON"
                );
            } else {
                assert!(
                    uri.contains(secret),
                    "URI carries the same {kind} credential as the sing-box JSON"
                );
            }
        }
    }
    let _: serde_yaml::Value = serde_yaml::from_str(&clash).expect("Clash subscription is YAML");
}

#[test]
fn regenerate_validates_before_replacing_artifacts_and_the_active_config() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    let checker = sing_box_check_fixture(&fixture, true, &["vless"]);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "install",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
            "--no-start",
        ])
        .assert()
        .success();

    // A `--no-start` fixture install never commits ownership. Seed the marker
    // so `regenerate` exercises the fully-managed active-config path.
    write_managed_file(&fixture, "var/lib/sbctl/ownership", b"sbctl-managed-v1\n");

    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let configuration = fs::read_to_string(&config_path).expect("configuration is persisted");
    let changed = configuration.replace(
        "reality_decoy_sni = \"www.cloudflare.com\"",
        "reality_decoy_sni = \"www.apple.com\"",
    );
    assert_ne!(changed, configuration, "the canonical node field is edited");
    fs::write(&config_path, changed).expect("configuration is edited");

    let artifacts = fixture.path().join("var/lib/sbctl/artifacts");
    let active = fixture.path().join("etc/sing-box/config.json");
    let artifact_names = [
        "sing-box-server.json",
        "subscription-sing-box.json",
        "subscription-clash.yaml",
        "subscription-uri.txt",
        "subscription-base64-uri.txt",
    ];
    let snapshot = || {
        let mut files = Vec::new();
        for name in artifact_names {
            files.push(fs::read(artifacts.join(name)).expect("artifact is readable"));
        }
        files.push(fs::read(&active).expect("active configuration is readable"));
        files
    };
    let before = snapshot();

    let rejecting_fixture = TempDir::new().expect("rejecting checker root is created");
    let rejecting = sing_box_check_fixture(&rejecting_fixture, false, &[]);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "regenerate",
            "--sing-box-bin",
            rejecting.to_str().expect("rejecting checker path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "sing-box configuration check failed",
        ));
    assert_eq!(
        snapshot(),
        before,
        "a rejected regeneration leaves every artifact and the active config unchanged"
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "regenerate",
            "--sing-box-bin",
            checker.to_str().expect("accepting checker path is UTF-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("regenerated and validated"));
    assert_ne!(
        snapshot(),
        before,
        "a passing regeneration atomically replaces artifacts and the active config"
    );
    for name in artifact_names {
        let contents = fs::read_to_string(artifacts.join(name)).expect("artifact is readable");
        if name == "subscription-base64-uri.txt" {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(contents.as_bytes())
                .expect("base64 URI subscription is valid standard Base64");
            let decoded = String::from_utf8(decoded).expect("base64 URI subscription is UTF-8");
            assert!(
                decoded.contains("www.apple.com"),
                "{name} carries the new canonical node field"
            );
        } else {
            assert!(
                contents.contains("www.apple.com"),
                "{name} carries the new canonical node field"
            );
        }
    }
    assert!(
        fs::read_to_string(&active)
            .expect("active configuration is readable")
            .contains("www.apple.com"),
        "the active configuration follows the regenerated server configuration"
    );
}

#[test]
fn proxy_credentials_cannot_read_the_subscription_and_the_subscription_credential_is_not_a_node_credential()
 {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port is available")
        .local_addr()
        .expect("address is available")
        .port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--listen-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--protocol",
            "vmess-websocket",
            "--protocol",
            "hysteria2",
            "--protocol",
            "tuic",
            "--protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    let configuration = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let deployment: sbctl::config::DeploymentConfig =
        toml::from_str(&configuration).expect("configuration is valid TOML");
    let subscription_credential = deployment.subscription_credential.clone();
    let node_secrets = sbctl::canonical::nodes(&deployment)
        .into_iter()
        .flat_map(|node| {
            node.secrets()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(
        !node_secrets.contains(&subscription_credential),
        "the Subscription credential is independent from every Proxy credential"
    );
    for name in [
        "sing-box-server.json",
        "subscription-sing-box.json",
        "subscription-clash.yaml",
        "subscription-uri.txt",
        "subscription-base64-uri.txt",
    ] {
        let contents =
            fs::read_to_string(fixture.path().join("var/lib/sbctl/artifacts").join(name))
                .expect("artifact is readable");
        assert!(
            !contents.contains(&subscription_credential),
            "the Subscription credential never appears in {name}"
        );
    }

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, node_secrets.len() + 1, &stderr_log);

    let authorized = http_get(port, &format!("/sub/{subscription_credential}/uri"));
    assert!(authorized.starts_with("HTTP/1.1 200 OK"));
    for secret in &node_secrets {
        let rejected = http_get(port, &format!("/sub/{secret}/uri"));
        assert!(
            rejected.starts_with("HTTP/1.1 404 Not Found"),
            "a Proxy credential must not authorize subscription retrieval"
        );
    }
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn configuration_initialization_checks_generated_sing_box_config_before_persisting() {
    let unchecked = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            unchecked.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vmess-websocket",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("require --sing-box-bin"));
    assert!(!unchecked.path().join("etc/sbctl/config.toml").exists());

    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(&fixture, true, &["vmess"]);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vmess-websocket",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();
    assert!(fixture.path().join("etc/sbctl/config.toml").is_file());

    let rejected = TempDir::new().expect("temporary root is created");
    let rejecting_checker = sing_box_check_fixture(&rejected, false, &[]);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            rejected.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vmess-websocket",
            "--sing-box-bin",
            rejecting_checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "sing-box configuration check failed",
        ));
    assert!(!rejected.path().join("etc/sbctl/config.toml").exists());
}

#[test]
fn ip_fallback_http_service_accepts_only_the_exact_credential_path_and_reports_vps_traffic() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port is available")
        .local_addr()
        .expect("address is available")
        .port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "127.0.0.1",
            "--http-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--monthly-traffic-limit",
            "1000",
        ])
        .assert()
        .success();
    let credential = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted")
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("credential is available")
        .to_owned();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let mut server = ProcessCommand::new(assert_cmd::cargo::cargo_bin!("sbctl"))
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--max-requests",
            "6",
        ])
        .spawn()
        .expect("subscription service starts");

    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let before = fs::read(&state_path).expect("state is established before subscription reads");

    let response = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("subscription-userinfo: upload=0; download=0; total=1000; expire="));
    assert!(response.contains("cache-control: no-store"));
    let rejected = http_get(
        port,
        &format!("/sub/{credential}/uri?credential={credential}"),
    );
    assert!(rejected.starts_with("HTTP/1.1 404 Not Found"));
    assert!(
        http_get(port, &format!("/sub/{credential}/bogus")).starts_with("HTTP/1.1 404 Not Found"),
        "an unknown subscription format path is a uniform 404"
    );
    assert!(
        http_get(port, &format!("/sub/{credential}/uri/extra"))
            .starts_with("HTTP/1.1 404 Not Found"),
        "a trailing path segment is a uniform 404"
    );
    assert!(
        http_get(port, "/sub/wrong-credential/uri").starts_with("HTTP/1.1 404 Not Found"),
        "an invalid Subscription credential is a uniform 404"
    );
    assert!(
        http_get(port, "/sub/uri").starts_with("HTTP/1.1 404 Not Found"),
        "a missing credential path is a uniform 404"
    );
    assert_eq!(
        fs::read(&state_path).expect("state remains readable"),
        before,
        "subscription reads must not write accounting state"
    );
    assert!(
        server
            .wait()
            .expect("server exits after the request limit")
            .success()
    );
}

#[test]
fn subscription_matrix_routes_serve_content_types_and_reject_bad_paths() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 17, &stderr_log);

    for (path, content_type) in [
        ("sing-box.json", "application/json; charset=utf-8"),
        ("sing-box-full.json", "application/json; charset=utf-8"),
        ("sing-box-1.12.json", "application/json; charset=utf-8"),
        ("sing-box-1.14.json", "application/json; charset=utf-8"),
        ("clash.yaml", "application/yaml; charset=utf-8"),
        ("clash-1.18.yaml", "application/yaml; charset=utf-8"),
        ("uri", "text/plain; charset=utf-8"),
        ("uri.txt", "text/plain; charset=utf-8"),
        ("shadowrocket.txt", "text/plain; charset=utf-8"),
    ] {
        let response = http_get(port, &format!("/sub/{credential}/{path}"));
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "{path} must serve 200, got: {response}"
        );
        assert!(
            response.contains(&format!("content-type: {content_type}")),
            "{path} must carry {content_type}"
        );
        assert!(
            response.contains("subscription-userinfo:"),
            "{path} must carry traffic metadata"
        );
    }

    let qr = http_get(port, &format!("/sub/{credential}/qr/uri"));
    assert!(qr.starts_with("HTTP/1.1 200 OK"), "qr route serves 200");
    assert!(qr.contains("content-type: image/svg+xml"));
    assert!(qr.contains("<svg"), "qr route serves an SVG document");
    let index = http_get(port, &format!("/sub/{credential}/index"));
    assert!(index.starts_with("HTTP/1.1 200 OK"), "index serves 200");
    assert!(index.contains("content-type: text/html; charset=utf-8"));
    assert!(
        index.contains("AnyTLS 2.2.64"),
        "the index page documents the Shadowrocket protocol floors"
    );

    for path in [
        "bogus",
        "sing-box-1.09.json",
        "clash-1.17.yaml",
        "uri/extra",
    ] {
        assert!(
            http_get(port, &format!("/sub/{credential}/{path}"))
                .starts_with("HTTP/1.1 404 Not Found"),
            "{path} must be a uniform 404"
        );
    }
    assert!(
        http_get(port, &format!("/sub/{credential}/uri?x=1")).starts_with("HTTP/1.1 404 Not Found"),
        "a query parameter must be rejected"
    );
    assert!(
        http_get(port, "/sub/wrong-credential/uri").starts_with("HTTP/1.1 404 Not Found"),
        "an invalid credential must be a uniform 404"
    );
    assert!(
        server
            .wait()
            .expect("server exits after the request limit")
            .success()
    );
}

#[test]
fn qr_all_renders_every_matrix_format_and_a_positional_format_selects_one() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let _credential = initialize_ip_fallback_subscription(&fixture, port);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    let all = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "qr", "--all"])
        .assert()
        .success();
    let stdout = String::from_utf8(all.get_output().stdout.clone()).expect("stdout is UTF-8");
    assert_eq!(
        stdout.matches("\x1b[30;47m").count(),
        sbctl::subscription::subscription_matrix().len(),
        "qr --all renders one terminal code per matrix row"
    );
    assert!(
        stdout.contains("Shadowrocket"),
        "the matrix lists Shadowrocket"
    );

    let single = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "qr", "shadowrocket"])
        .assert()
        .success();
    let stdout = String::from_utf8(single.get_output().stdout.clone()).expect("stdout is UTF-8");
    assert_eq!(
        stdout.matches("\x1b[30;47m").count(),
        1,
        "a positional format renders exactly one terminal code"
    );
}

#[test]
fn certificate_obtain_requires_a_valid_email_or_a_confirmed_no_email_path() {
    let fixture = TempDir::new().expect("temporary root is created");
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "certificate",
            "obtain",
            "--email",
            "not-an-email",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("邮箱格式无效"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "certificate", "obtain"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("请提供 --email"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "certificate", "obtain", "--no-email"])
        .write_stdin("n\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("未确认免邮箱注册"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root,
            "certificate",
            "obtain",
            "--email",
            "admin@example.com",
            "--no-email",
        ])
        .assert()
        .failure();
}

#[test]
fn subscription_degrades_to_artifact_only_for_a_missing_state_without_logging_the_credential() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    // A missing accounting state must not take the subscription offline: the
    // artifact still serves, only the traffic metadata is dropped.
    let available = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(
        available.starts_with("HTTP/1.1 200 OK"),
        "the subscription survives a missing accounting state: {available}"
    );
    assert!(
        !available.contains("subscription-userinfo:"),
        "the degraded response must not carry traffic metadata"
    );
    assert!(
        available.contains("vless://"),
        "the artifact body still serves"
    );
    let rejected = http_get(port, "/sub/wrong-credential/uri");
    assert!(
        rejected.starts_with("HTTP/1.1 404 Not Found"),
        "an invalid credential stays 404 even when state is missing"
    );
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(
        log.contains("subscription traffic metadata unavailable"),
        "a redacted diagnostic is written"
    );
    assert!(
        !log.contains(&credential),
        "the diagnostic must not contain the full Subscription credential"
    );
}

#[test]
fn subscription_degrades_to_artifact_only_for_a_corrupt_state_without_logging_the_credential() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::create_dir_all(fixture.path().join("var/lib/sbctl")).expect("state directory is created");
    fs::write(fixture.path().join("var/lib/sbctl/state.json"), "not json")
        .expect("corrupt state is written");
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let available = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(
        available.starts_with("HTTP/1.1 200 OK"),
        "the subscription survives a corrupt accounting state: {available}"
    );
    assert!(
        !available.contains("subscription-userinfo:"),
        "the degraded response must not carry traffic metadata"
    );
    http_get(port, "/sub/wrong-credential/uri");
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(!log.contains(&credential));
}

#[test]
fn subscription_degrades_to_artifact_only_for_a_schema_mismatched_state() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::create_dir_all(fixture.path().join("var/lib/sbctl")).expect("state directory is created");
    fs::write(
        fixture.path().join("var/lib/sbctl/state.json"),
        r#"{"schema_version":1,"cycle_key":"2024-02-01T00:00:00+00:00","interface":"ens3","baseline_rx":0,"baseline_tx":0,"accumulated_rx":0,"accumulated_tx":0,"boot_id":"boot-a","corrections":[]}"#,
    )
    .expect("schema-mismatched state is written");
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let available = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(
        available.starts_with("HTTP/1.1 200 OK"),
        "the subscription survives a schema-mismatched state: {available}"
    );
    assert!(
        !available.contains("subscription-userinfo:"),
        "the degraded response must not carry traffic metadata"
    );
    http_get(port, "/sub/wrong-credential/uri");
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(!log.contains(&credential));
}

#[test]
fn subscription_returns_a_redacted_503_for_a_missing_artifact() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::remove_file(
        fixture
            .path()
            .join("var/lib/sbctl/artifacts/subscription-uri.txt"),
    )
    .expect("URI artifact is removed");
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    assert!(
        http_get(port, &format!("/sub/{credential}/uri"))
            .starts_with("HTTP/1.1 503 Service Unavailable")
    );
    http_get(port, "/sub/wrong-credential/uri");
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(!log.contains(&credential));
}

#[test]
fn subscription_userinfo_total_reflects_a_total_only_correction() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    write_traffic_fixture(&fixture, 130, 260, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
            "set-used",
            "--bytes",
            "5000",
        ])
        .assert()
        .success();

    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 1, &stderr_log);
    let response = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(
        response.contains("subscription-userinfo: upload=60; download=30; total=5000; expire=")
    );
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn status_json_reports_the_current_period_without_exposing_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    write_traffic_fixture(&fixture, 130, 260, "boot-a");

    let output = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "status",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("status JSON is UTF-8");
    assert!(
        !stdout.contains(&credential),
        "status --json must not expose the Subscription credential"
    );
    let status: serde_json::Value =
        serde_json::from_str(&stdout).expect("status --json emits valid JSON");
    assert_eq!(status["configured"], true);
    assert_eq!(status["interface"], "ens3");
    assert_eq!(status["traffic"]["received"], 30);
    assert_eq!(status["traffic"]["transmitted"], 60);
    assert_eq!(status["traffic"]["total"], 90);
    let now = chrono::Utc::now().with_timezone(&chrono_tz::America::Los_Angeles);
    use chrono::Datelike;
    assert_eq!(
        status["traffic"]["accounting_period"],
        format!(
            "{:04}-{:02}-01T00:00:00{}",
            now.year(),
            now.month(),
            now.format("%:z")
        )
    );
    assert_eq!(
        status["services"]["sing-box.service"],
        "inactive or unavailable"
    );
}

#[test]
fn status_json_reports_an_unmanaged_host() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "status",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"configured\": false"));
}

#[test]
fn external_proxy_mode_serves_loopback_without_touching_public_ports_or_proxy_configuration() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port is available")
        .local_addr()
        .expect("address is available")
        .port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--listen-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let credential = config
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('\"'))
        })
        .expect("credential is available");
    assert!(config.contains("subscription_listen_port"));
    assert!(!fixture.path().join("etc/caddy/Caddyfile").exists());
    assert!(!fixture.path().join("etc/nginx/nginx.conf").exists());

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();

    let mut server = ProcessCommand::new(assert_cmd::cargo::cargo_bin!("sbctl"))
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
            "--max-requests",
            "2",
        ])
        .spawn()
        .expect("loopback subscription service starts");
    let response = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("subscription-userinfo:"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "switch-mode",
            "--mode",
            "external-proxy",
            "--listen-port",
            &port.to_string(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already in use"));
    let preserved = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(preserved.starts_with("HTTP/1.1 200 OK"));
    assert!(
        fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
            .expect("configuration remains readable")
            .contains("subscription_mode = \"external-proxy\"")
    );
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn external_proxy_mode_rejects_a_managed_tcp_protocol_port_when_switching_modes() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--listen-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let config = fs::read_to_string(&config_path).expect("configuration is persisted");
    let protocol_port = config
        .lines()
        .find_map(|line| line.strip_prefix("listen_port = "))
        .expect("VLESS Reality listener port is persisted")
        .to_owned();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "switch-mode",
            "--mode",
            "external-proxy",
            "--listen-port",
            &protocol_port,
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("must not conflict"));
    assert!(
        fs::read_to_string(config_path)
            .expect("configuration remains readable")
            .contains("subscription_listen_port = 2080")
    );
}

#[test]
fn a_non_get_subscription_request_is_asked_for_get_rather_than_a_missing_route() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let response = http_request("POST", port, &format!("/sub/{credential}/uri"));
    assert!(
        response.starts_with("HTTP/1.1 405 Method Not Allowed"),
        "a probe should learn the method is wrong, not that the subscription vanished: {response}"
    );
    assert!(response.contains("allow: GET"), "{response}");
    assert!(
        http_get(port, &format!("/sub/{credential}/uri")).starts_with("HTTP/1.1 200 OK"),
        "the route itself must keep serving GET"
    );
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn configuration_validation_does_not_echo_a_secret_from_a_malformed_file() {
    let fixture = TempDir::new().expect("temporary root is created");
    let config_path = fixture.path().join("etc/sbctl/config.toml");
    fs::create_dir_all(config_path.parent().expect("config path has a parent"))
        .expect("configuration directory is created");
    let secret = "a-very-sensitive-subscription-credential";
    fs::write(
        &config_path,
        format!("subscription_credential = \"{secret}\"\nnot valid TOML"),
    )
    .expect("malformed configuration is written");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "validate",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "could not parse deployment configuration",
        ))
        .stderr(predicate::str::contains(secret).not());
}

#[test]
fn traffic_and_status_report_vps_traffic_for_the_detected_default_route_interface() {
    let fixture = TempDir::new().expect("temporary root is created");
    fs::create_dir_all(fixture.path().join("proc/net")).expect("route directory is created");
    fs::write(
        fixture.path().join("proc/net/route"),
        "Iface\tDestination\tGateway\tFlags\nens3\t00000000\t00000000\t0003\n",
    )
    .expect("default route is written");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--protocol",
            "vless-reality",
            "--monthly-traffic-limit",
            "1000",
            "--accounting-timezone",
            "UTC",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
        ])
        .assert()
        .success();

    write_traffic_fixture(&fixture, 130, 260, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("interface: ens3"))
        .stdout(predicate::str::contains("total: 90 bytes"))
        .stdout(predicate::str::contains(
            "monthly traffic limit: 1000 bytes",
        ))
        .stdout(predicate::str::contains("accounting period:"))
        .stdout(predicate::str::contains("next reset:"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "status",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("VPS traffic"))
        .stdout(predicate::str::contains("total: 90 bytes"));
}

#[test]
fn traffic_and_status_reads_do_not_write_accounting_state() {
    let fixture = TempDir::new().expect("temporary root is created");
    fs::create_dir_all(fixture.path().join("proc/net")).expect("route directory is created");
    fs::write(
        fixture.path().join("proc/net/route"),
        "Iface\tDestination\tGateway\tFlags\nens3\t00000000\t00000000\t0003\n",
    )
    .expect("default route is written");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");

    let root = fixture.path().to_str().expect("fixture path is UTF-8");
    let init_args = [
        "--root",
        root,
        "config",
        "init",
        "--mode",
        "ip-fallback",
        "--subscription-host",
        "203.0.113.7",
        "--http-port",
        "2080",
        "--protocol",
        "vless-reality",
        "--reality-decoy-sni",
        "www.cloudflare.com",
    ];
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(init_args)
        .assert()
        .success();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();

    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let snapshot = || {
        let contents = fs::read(&state_path).expect("state is readable");
        let modified = fs::metadata(&state_path)
            .expect("state metadata is readable")
            .modified()
            .expect("state modification time is readable");
        (contents, modified)
    };
    let before = snapshot();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
        ])
        .assert()
        .success();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "status",
        ])
        .assert()
        .success();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
        ])
        .assert()
        .success();

    assert_eq!(snapshot(), before, "reads must not change accounting state");
}

#[test]
fn traffic_set_used_bytes_changes_the_total_without_direction_values() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_traffic_fixture(&fixture);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();
    write_traffic_fixture(&fixture, 130, 260, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();

    run_traffic_set_used(&fixture, &["--bytes", "1000"])
        .success()
        .stdout(predicate::str::contains("accounting period:"))
        .stdout(predicate::str::contains("current received: 30 bytes"))
        .stdout(predicate::str::contains("current transmitted: 60 bytes"))
        .stdout(predicate::str::contains("current total: 90 bytes"))
        .stdout(predicate::str::contains("target received: 30 bytes"))
        .stdout(predicate::str::contains("target transmitted: 60 bytes"))
        .stdout(predicate::str::contains("target total: 1000 bytes"))
        .stdout(predicate::str::contains("next reset:"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "traffic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("received: 30 bytes"))
        .stdout(predicate::str::contains("transmitted: 60 bytes"))
        .stdout(predicate::str::contains("total: 1000 bytes"));

    write_traffic_fixture(&fixture, 134, 265, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "traffic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("total: 1009 bytes"));
}

#[test]
fn traffic_set_used_rx_tx_sets_direction_values_without_modifying_counters() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_traffic_fixture(&fixture);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();

    run_traffic_set_used(&fixture, &["--rx", "500", "--tx", "300"])
        .success()
        .stdout(predicate::str::contains("target received: 500 bytes"))
        .stdout(predicate::str::contains("target transmitted: 300 bytes"))
        .stdout(predicate::str::contains("target total: 800 bytes"));

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "traffic"])
        .assert()
        .success()
        .stdout(predicate::str::contains("received: 500 bytes"))
        .stdout(predicate::str::contains("transmitted: 300 bytes"))
        .stdout(predicate::str::contains("total: 800 bytes"));

    assert_eq!(
        fs::read_to_string(
            fixture
                .path()
                .join("sys/class/net/ens3/statistics/rx_bytes")
        )
        .expect("sysfs RX counter remains readable"),
        "100"
    );
    assert_eq!(
        fs::read_to_string(
            fixture
                .path()
                .join("sys/class/net/ens3/statistics/tx_bytes")
        )
        .expect("sysfs TX counter remains readable"),
        "200"
    );
}

#[test]
fn traffic_set_used_rejects_invalid_arguments_without_writing() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_traffic_fixture(&fixture);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let before = fs::read(&state_path).expect("state is established");

    run_traffic_set_used(&fixture, &["--bytes", "100", "--rx", "5"])
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
    run_traffic_set_used(&fixture, &["--rx", "5"])
        .code(2)
        .stderr(predicate::str::contains("--tx"));
    run_traffic_set_used(&fixture, &["--tx", "5"])
        .code(2)
        .stderr(predicate::str::contains("--rx"));
    run_traffic_set_used(&fixture, &["--bytes", "-5"])
        .code(2)
        .stderr(predicate::str::contains("unexpected argument"));
    run_traffic_set_used(&fixture, &[])
        .code(2)
        .stderr(predicate::str::contains(
            "required arguments were not provided",
        ));

    assert_eq!(
        fs::read(&state_path).expect("state remains readable"),
        before,
        "argument validation must not write accounting state"
    );
}

#[test]
fn traffic_set_used_rejects_corrupted_state_without_overwriting_it() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_traffic_fixture(&fixture);
    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    fs::create_dir_all(state_path.parent().expect("state has a parent"))
        .expect("state directory is created");
    fs::write(&state_path, b"not json").expect("corrupted state is written");

    run_traffic_set_used(&fixture, &["--bytes", "1000"])
        .code(2)
        .stderr(predicate::str::contains("state is corrupted"));

    assert_eq!(
        fs::read(&state_path).expect("corrupted state remains readable"),
        b"not json"
    );
}

#[test]
fn traffic_set_used_rejects_a_target_below_the_current_total() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_traffic_fixture(&fixture);
    let root = fixture.path().to_str().expect("fixture path is UTF-8");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();
    write_traffic_fixture(&fixture, 130, 260, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["--root", root, "accounting-reset"])
        .assert()
        .success();
    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let before = fs::read(&state_path).expect("state is established");

    run_traffic_set_used(&fixture, &["--bytes", "50"])
        .code(2)
        .stderr(predicate::str::contains(
            "below the currently reported total",
        ));

    assert_eq!(
        fs::read(&state_path).expect("state remains readable"),
        before,
        "a rejected correction must not change accounting state"
    );
}

#[test]
fn traffic_set_used_requires_established_state() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_traffic_fixture(&fixture);

    run_traffic_set_used(&fixture, &["--bytes", "1000"])
        .code(2)
        .stderr(predicate::str::contains(
            "accounting state has not been established",
        ));
    assert!(!fixture.path().join("var/lib/sbctl/state.json").exists());
}

#[test]
fn traffic_without_established_state_is_a_diagnosable_error() {
    let fixture = TempDir::new().expect("temporary root is created");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "accounting state has not been established for the current period",
        ));
    assert!(!fixture.path().join("var/lib/sbctl/state.json").exists());
}

#[test]
fn accounting_reset_establishes_state_once_and_repeated_resets_do_not_reestablish_it() {
    let fixture = TempDir::new().expect("temporary root is created");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("accounting period:"));

    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let first = fs::read(&state_path).expect("state is established by the reset task");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let second = fs::read(&state_path).expect("state remains readable");

    assert_eq!(
        first, second,
        "a repeated reset must not reestablish the period"
    );
}

#[test]
fn configuration_init_defaults_the_refresh_and_client_display_timezones() {
    let fixture = TempDir::new().expect("temporary root is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    assert!(config.contains("accounting_timezone = \"America/Los_Angeles\""));
    assert!(config.contains("client_display_timezone = \"Asia/Shanghai\""));
}

#[test]
fn anchored_month_before_the_first_reset_starts_a_trackable_current_period() {
    let fixture = TempDir::new().expect("temporary root is created");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--accounting-policy",
            "anchored-month",
            "--accounting-timezone",
            "UTC",
            "--anchored-reset-at",
            "2099-01-01T00:00",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("accounting period: pending-first-reset").not())
        .stdout(predicate::str::contains("total: 0 bytes"))
        // The anchored day is the 1st, so the schedule boundary at the start
        // of the coming month lies before the 2099 first anchor and ends the
        // period: the reported next reset is the near boundary, never the
        // anchor year.
        .stdout(predicate::str::contains("next reset: 2099").not())
        .stdout(predicate::str::contains("next reset: "));
}

#[test]
fn anchored_reset_rejects_nonexistent_and_ambiguous_dst_local_times() {
    let fixture = TempDir::new().expect("temporary root is created");
    let anchored_init = |reset_at: &str| {
        let mut command = Command::cargo_bin("sbctl").expect("sbctl binary is built");
        command
            .arg("--root")
            .arg(fixture.path())
            .arg("config")
            .arg("init")
            .arg("--mode")
            .arg("ip-fallback")
            .arg("--subscription-host")
            .arg("203.0.113.7")
            .arg("--http-port")
            .arg("2080")
            .arg("--interface")
            .arg("ens3")
            .arg("--protocol")
            .arg("vless-reality")
            .arg("--reality-decoy-sni")
            .arg("www.cloudflare.com")
            .arg("--accounting-policy")
            .arg("anchored-month")
            .arg("--accounting-timezone")
            .arg("America/New_York")
            .arg("--anchored-reset-at")
            .arg(reset_at);
        command
    };

    anchored_init("2024-03-10T02:30")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "does not exist in the accounting timezone",
        ));
    anchored_init("2024-11-03T01:30")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "ambiguous in the accounting timezone",
        ));
    assert!(!fixture.path().join("etc/sbctl/config.toml").exists());
}

#[test]
fn direct_serve_refuses_to_bind_public_ports_without_socket_activation() {
    let fixture = TempDir::new().expect("temporary root is created");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "direct",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(refusal_message()));
}

#[test]
fn external_proxy_serve_rejects_a_non_loopback_bind() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("an ephemeral port is available")
        .local_addr()
        .expect("address is available")
        .port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--listen-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
            "--bind",
            &format!("0.0.0.0:{port}"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("must bind a loopback address"));
}

#[test]
fn uninstall_removes_the_direct_https_socket_unit() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    write_systemctl_fixture(&fixture, true);
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
            "--no-start",
        ])
        .assert()
        .success();
    assert!(
        fixture
            .path()
            .join("etc/systemd/system/sbctl-http.socket")
            .exists()
    );

    // A `--no-start` fixture install defers startup and the health check, so
    // it never writes the ownership marker. Seed it here so the uninstall flow
    // recognizes the fixture as an sbctl-managed deployment.
    write_managed_file(&fixture, "var/lib/sbctl/ownership", b"sbctl-managed-v1\n");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "uninstall",
        ])
        .assert()
        .success();

    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl-http.socket")
            .exists(),
        "uninstall removes the Direct HTTPS socket unit"
    );
    assert!(
        !fixture
            .path()
            .join("etc/letsencrypt/renewal-hooks/deploy/sbctl-certificate-deploy-hook")
            .exists(),
        "uninstall removes the sbctl-owned Certbot deploy hook"
    );
}

#[test]
fn certificate_verify_pins_a_valid_certificate_without_exposing_the_credential() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_direct_config(&fixture);
    let credential = read_subscription_credential(&fixture);
    seed_live_certificate(&fixture, &["sub.example.test"]);

    let output = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "certificate",
            "verify",
        ])
        .output()
        .expect("verify output is captured");
    assert!(
        output.status.success(),
        "a valid certificate verifies: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("valid until"));
    assert!(stdout.contains("fingerprint:"));
    assert!(
        !stdout.contains(&credential),
        "verify output must not expose the credential"
    );

    let pinned = fixture
        .path()
        .join("var/lib/sbctl/certificates/sub.example.test");
    assert!(
        pinned.join("fullchain.pem").is_file(),
        "the daemon copy is pinned"
    );
    assert!(
        pinned.join("privkey.pem").is_file(),
        "the private key copy is pinned"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(pinned.join("privkey.pem"))
            .expect("pinned key has metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o640, "the pinned key is group-readable only");
    }
}

#[test]
fn certificate_verify_rejects_a_certificate_that_does_not_cover_the_host_without_exposing_the_credential()
 {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_direct_config(&fixture);
    let credential = read_subscription_credential(&fixture);
    seed_live_certificate(&fixture, &["other.example.test"]);

    let output = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "certificate",
            "verify",
        ])
        .output()
        .expect("verify output is captured");
    assert!(!output.status.success(), "a SAN mismatch is rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not cover that host name"));
    assert!(
        !stderr.contains(&credential),
        "the credential must not leak into diagnostics"
    );
}

// The fake certbot is a POSIX shell fixture; there is no certbot on Windows.
#[cfg(unix)]
#[test]
fn certificate_obtain_fails_with_a_redacted_diagnostic_when_certbot_fails() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_direct_config(&fixture);
    let credential = read_subscription_credential(&fixture);
    write_command_fixture(&fixture, "usr/bin/certbot", 1, "certbot boom\n");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "certificate",
            "obtain",
            "--email",
            "admin@example.test",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Certbot failed: certbot boom"))
        .stderr(predicate::str::contains("certificate operation failed"))
        .stderr(predicate::str::contains(&credential).not())
        .stdout(predicate::str::contains(&credential).not());
    assert!(
        !fixture
            .path()
            .join("var/lib/sbctl/certificates/sub.example.test/privkey.pem")
            .exists(),
        "a failed obtain must not pin a certificate"
    );
}

#[cfg(unix)]
#[test]
fn certificate_obtain_runs_certbot_and_pins_the_renewed_certificate() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_direct_config(&fixture);
    let credential = read_subscription_credential(&fixture);
    seed_live_certificate(&fixture, &["sub.example.test"]);
    write_command_fixture(&fixture, "usr/bin/certbot", 0, "");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "certificate",
            "obtain",
            "--email",
            "admin@example.test",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("certificate operation completed"));
    assert!(
        fixture
            .path()
            .join("var/lib/sbctl/certificates/sub.example.test/privkey.pem")
            .is_file(),
        "obtain pins the certificate for the daemon"
    );
    let _ = credential;
}

#[test]
fn certificate_commands_refuse_non_direct_modes_without_touching_any_certificate_path() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--listen-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "certificate",
            "verify",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "managed only in direct subscription mode",
        ));
    assert!(
        !fixture.path().join("etc/letsencrypt").exists(),
        "External proxy mode never writes the Certificate-managed tree"
    );
}

#[test]
fn uninstall_stops_and_removes_managed_services_and_binaries_but_preserves_root_readable_backup_and_data()
 {
    let fixture = supported_systemd_host();
    initialize_uninstall_fixture(&fixture);
    write_systemctl_fixture(&fixture, true);
    let unrelated_service = fixture.path().join("etc/systemd/system/unrelated.service");
    let proxy_configuration = fixture.path().join("etc/nginx/nginx.conf");
    let firewall_rules = fixture.path().join("etc/ufw/user.rules");
    write_managed_file(
        &fixture,
        "etc/systemd/system/unrelated.service",
        b"preserve service",
    );
    write_managed_file(&fixture, "etc/nginx/nginx.conf", b"preserve proxy");
    write_managed_file(&fixture, "etc/ufw/user.rules", b"preserve firewall");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "uninstall",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("backup preserved at"));

    assert!(!fixture.path().join("usr/local/bin/sbctl").exists());
    assert!(!fixture.path().join("usr/local/bin/sing-box").exists());
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl.service")
            .exists()
    );
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sing-box.service")
            .exists()
    );
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl-accounting-reset.timer")
            .exists()
    );
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl-accounting-reset.service")
            .exists()
    );
    assert!(fixture.path().join("etc/sbctl/config.toml").is_file());
    assert!(fixture.path().join("var/lib/sbctl/state.json").is_file());
    let backup = fixture
        .path()
        .join("var/backups/sbctl")
        .read_dir()
        .expect("backup directory exists")
        .next()
        .expect("backup is created")
        .expect("backup entry is readable")
        .path();
    let original_config = fs::read(fixture.path().join("etc/sbctl/config.toml"))
        .expect("deployment configuration remains available");
    let backed_up_config = fs::read(backup.join("etc/sbctl/config.toml"))
        .expect("deployment configuration is backed up");
    assert_eq!(backed_up_config, original_config);
    assert!(
        String::from_utf8_lossy(&backed_up_config).contains("subscription_credential"),
        "the backup retains the subscription credential"
    );
    assert_eq!(
        fs::read(backup.join("var/lib/sbctl/state.json")).expect("traffic state is backed up"),
        b"managed traffic state"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            fs::metadata(&backup)
                .expect("backup directory metadata is readable")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(backup.join("etc/sbctl/config.toml"))
                .expect("backup configuration metadata is readable")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::read(unrelated_service).expect("unrelated service survives"),
        b"preserve service"
    );
    assert_eq!(
        fs::read(proxy_configuration).expect("proxy configuration survives"),
        b"preserve proxy"
    );
    assert_eq!(
        fs::read(firewall_rules).expect("firewall rules survive"),
        b"preserve firewall"
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "uninstall",
            "--purge",
        ])
        .assert()
        .success();
    assert!(!fixture.path().join("etc/sing-box/config.json").exists());
    assert!(!fixture.path().join("etc/sing-box").exists());
}

#[test]
fn uninstall_purge_removes_only_sbctl_owned_persistent_data() {
    let fixture = supported_systemd_host();
    initialize_uninstall_fixture(&fixture);
    write_systemctl_fixture(&fixture, true);
    let unrelated_state = fixture.path().join("var/lib/unrelated/state");
    let proxy_configuration = fixture.path().join("etc/nginx/nginx.conf");
    write_managed_file(&fixture, "var/lib/unrelated/state", b"preserve state");
    write_managed_file(&fixture, "etc/nginx/nginx.conf", b"preserve proxy");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "uninstall",
            "--purge",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("persistent sbctl data purged"));

    assert!(!fixture.path().join("etc/sbctl/config.toml").exists());
    assert!(!fixture.path().join("var/lib/sbctl").exists());
    assert!(!fixture.path().join("etc/sing-box/config.json").exists());
    assert!(!fixture.path().join("etc/sing-box").exists());
    assert_eq!(
        fs::read(unrelated_state).expect("unrelated state survives"),
        b"preserve state"
    );
    assert_eq!(
        fs::read(proxy_configuration).expect("proxy configuration survives"),
        b"preserve proxy"
    );
}

#[test]
fn uninstall_does_not_touch_a_manual_sing_box_deployment_without_sbctl_ownership_markers() {
    let fixture = supported_systemd_host();
    initialize_uninstall_fixture(&fixture);
    write_systemctl_fixture(&fixture, true);
    let manual_unit = fixture.path().join("etc/systemd/system/sing-box.service");
    let manual_binary = fixture.path().join("usr/local/bin/sing-box");
    let manual_configuration = fixture.path().join("etc/sing-box/config.json");
    write_managed_file(
        &fixture,
        "etc/systemd/system/sing-box.service",
        b"manual sing-box service",
    );
    write_managed_file(
        &fixture,
        "usr/local/bin/sing-box",
        b"manual sing-box binary",
    );
    write_managed_file(
        &fixture,
        "etc/sing-box/config.json",
        b"manual sing-box configuration",
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "uninstall",
            "--purge",
        ])
        .assert()
        .success();

    assert_eq!(
        fs::read(manual_unit).expect("manual unit survives"),
        b"manual sing-box service"
    );
    assert_eq!(
        fs::read(manual_binary).expect("manual binary survives"),
        b"manual sing-box binary"
    );
    assert_eq!(
        fs::read(manual_configuration).expect("manual configuration survives"),
        b"manual sing-box configuration"
    );
}

#[test]
fn credential_rotate_invalidates_the_old_url_and_keeps_proxy_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    write_systemctl_fixture(&fixture, true);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "127.0.0.1",
            "--http-port",
            &port.to_string(),
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    let old_credential = read_subscription_credential(&fixture);
    let old_proxy_uuid = read_vless_uuid(&fixture);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let uri_before = fs::read(
        fixture
            .path()
            .join("var/lib/sbctl/artifacts/subscription-uri.txt"),
    )
    .expect("URI artifact is readable");

    let output = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "credential",
            "rotate",
        ])
        .output()
        .expect("rotation output is captured");
    assert!(output.status.success(), "rotation succeeds");
    let rotate_stdout = String::from_utf8_lossy(&output.stdout);
    assert!(rotate_stdout.contains("rotated"));
    assert!(
        !rotate_stdout.contains(&old_credential),
        "rotation output must not expose the old Subscription credential"
    );

    let new_credential = read_subscription_credential(&fixture);
    assert_ne!(
        new_credential, old_credential,
        "rotation generates a fresh credential"
    );
    assert!(
        !rotate_stdout.contains(&new_credential),
        "rotation output must not print the complete new Subscription credential; run 'sbctl sub' for URLs"
    );
    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is readable");
    assert!(
        config.contains(&old_proxy_uuid),
        "Proxy credential is unchanged by Subscription rotation"
    );
    assert_eq!(
        fs::read(
            fixture
                .path()
                .join("var/lib/sbctl/artifacts/subscription-uri.txt"),
        )
        .expect("URI artifact remains readable"),
        uri_before,
        "rotation must not alter the generated proxy artifacts"
    );

    let stderr_log = fixture.path().join("rotate-serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);
    assert!(
        http_get(port, &format!("/sub/{old_credential}/uri")).starts_with("HTTP/1.1 404 Not Found"),
        "the previous Subscription URL must immediately stop working"
    );
    assert!(
        http_get(port, &format!("/sub/{new_credential}/uri")).starts_with("HTTP/1.1 200 OK"),
        "the new Subscription URL is usable"
    );
    assert!(server.wait().expect("server exits").success());
}

#[test]
fn config_wizard_with_empty_answers_leaves_an_existing_deployment_unchanged() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let before = fs::read(&config_path).expect("configuration is readable");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
        ])
        .write_stdin("\n".repeat(25))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "deployment configuration is unchanged",
        ));

    assert_eq!(
        fs::read(&config_path).expect("configuration remains readable"),
        before,
        "empty answers must keep every current value"
    );
}

#[test]
fn config_wizard_cancelled_leaves_the_existing_deployment_unchanged() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let before = fs::read(&config_path).expect("configuration is readable");
    let mut answers = vec![String::new(); 17];
    answers[1] = "198.51.100.9".into();
    answers.push("n".into());
    let input = answers.join("\n") + "\n";

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
        ])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "configuration wizard cancelled; the existing deployment is unchanged",
        ));

    assert_eq!(
        fs::read(&config_path).expect("configuration remains readable"),
        before,
        "an unconfirmed summary must not change the deployment"
    );
}

#[test]
fn config_wizard_without_input_aborts_without_changing_the_deployment() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let before = fs::read(&config_path).expect("configuration is readable");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
        ])
        .write_stdin("")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("wizard input ended"));

    assert_eq!(
        fs::read(&config_path).expect("configuration remains readable"),
        before,
        "an interrupted wizard must not change the deployment"
    );
}

#[test]
fn config_wizard_rejects_an_ambiguous_dst_anchored_reset_before_committing() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let before = fs::read(&config_path).expect("configuration is readable");
    let mut answers = vec![String::new(); 18];
    answers[14] = "America/New_York".into();
    answers[16] = "anchored-month".into();
    answers[17] = "2024-11-03T01:30".into();
    answers.push("y".into());
    let input = answers.join("\n") + "\n";

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
        ])
        .write_stdin(input)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "anchored reset time is ambiguous in the accounting timezone",
        ));

    assert_eq!(
        fs::read(&config_path).expect("configuration remains readable"),
        before,
        "a DST-ambiguous schedule must be rejected before any commit"
    );
}

#[test]
fn config_wizard_commits_a_timezone_change_and_establishes_new_accounting_state() {
    let fixture = TempDir::new().expect("temporary root is created");
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    write_systemctl_fixture(&fixture, true);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--accounting-timezone",
            "UTC",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "accounting-reset",
        ])
        .assert()
        .success();
    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let state_before = fs::read_to_string(&state_path).expect("state is established");
    assert!(
        state_before.contains("+00:00"),
        "the initial UTC period is established"
    );

    let mut answers = vec![String::new(); 17];
    answers[14] = "Asia/Tokyo".into();
    answers.push("y".into());
    let input = answers.join("\n") + "\n";
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
        ])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "deployment configuration committed",
        ));

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is committed");
    assert!(config.contains("accounting_timezone = \"Asia/Tokyo\""));
    let state_after = fs::read_to_string(&state_path).expect("new state is established");
    assert!(
        state_after.contains("+09:00"),
        "changing the accounting timezone establishes a new accounting state"
    );
    assert_ne!(state_after, state_before);
}

#[test]
fn config_wizard_creates_a_new_deployment_with_secure_defaults() {
    let fixture = TempDir::new().expect("temporary root is created");
    fs::create_dir_all(fixture.path().join("proc/net")).expect("route directory is created");
    fs::write(
        fixture.path().join("proc/net/route"),
        "Iface\tDestination\tGateway\tFlags\nens3\t00000000\t00000000\t0003\n",
    )
    .expect("route fixture is written");
    fs::create_dir_all(fixture.path().join("sys/class/net/ens3"))
        .expect("interface fixture is created");
    let checker = sing_box_check_fixture(
        &fixture,
        true,
        &["vless", "vmess", "hysteria2", "tuic", "anytls"],
    );
    let mut answers = vec![String::new(); 24];
    answers[1] = "sub.example.test".into();
    // Confirm the no-email ACME path (the wizard asks for an email and, when
    // it is empty, requires an explicit confirmation).
    answers[4] = "y".into();
    answers[17] = "www.cloudflare.com".into();
    answers[23] = "y".into();
    let input = answers.join("\n") + "\n";

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "deployment configuration committed",
        ));

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("a fresh wizard deployment is committed");
    assert!(config.contains("subscription_mode = \"direct\""));
    assert!(config.contains("subscription_host = \"sub.example.test\""));
    assert!(config.contains("interface = \"ens3\""));
    assert!(config.contains("accounting_timezone = \"America/Los_Angeles\""));
    assert!(config.contains("client_display_timezone = \"Asia/Shanghai\""));
    assert!(config.contains("accounting_policy = \"natural-month\""));
    for protocol in [
        "vless-reality",
        "vmess-websocket",
        "hysteria2",
        "tuic",
        "anytls",
    ] {
        assert!(
            config.contains(protocol),
            "{protocol} is enabled by default"
        );
    }
}

#[test]
fn config_wizard_output_does_not_leak_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "init",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--protocol",
            "vless-reality",
            "--reality-decoy-sni",
            "www.cloudflare.com",
        ])
        .assert()
        .success();
    let credential = read_subscription_credential(&fixture);
    let proxy_uuid = read_vless_uuid(&fixture);
    let mut answers = vec![String::new(); 17];
    answers[1] = "198.51.100.9".into();
    answers.push("n".into());
    let input = answers.join("\n") + "\n";

    let output = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "config",
            "wizard",
        ])
        .write_stdin(input)
        .output()
        .expect("wizard output is captured");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for secret in [&credential, &proxy_uuid] {
        assert!(!stdout.contains(secret), "stdout must not expose {secret}");
        assert!(!stderr.contains(secret), "stderr must not expose {secret}");
    }
    assert!(stdout.contains("subscription credential: [redacted]"));
}

#[test]
fn system_status_reports_the_kernel_congestion_control_from_the_fixture() {
    let fixture = TempDir::new().expect("temporary root is created");
    let cc = fixture
        .path()
        .join("proc/sys/net/ipv4/tcp_congestion_control");
    let qdisc = fixture.path().join("proc/sys/net/core/default_qdisc");
    fs::create_dir_all(cc.parent().expect("congestion control has a parent")).unwrap();
    fs::create_dir_all(qdisc.parent().expect("qdisc has a parent")).unwrap();
    fs::write(&cc, "cubic\n").unwrap();
    fs::write(&qdisc, "fq_codel\n").unwrap();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "system",
            "status",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("tcp_congestion_control=cubic"))
        .stdout(predicate::str::contains("default_qdisc=fq_codel"));
}

// The fake sysctl is a POSIX shell fixture; sysctl does not exist on Windows.
#[cfg(unix)]
#[test]
fn system_bbr_applies_and_persists_the_drop_in_without_touching_sing_box() {
    let fixture = TempDir::new().expect("temporary root is created");
    let cc = fixture
        .path()
        .join("proc/sys/net/ipv4/tcp_congestion_control");
    let qdisc = fixture.path().join("proc/sys/net/core/default_qdisc");
    fs::create_dir_all(cc.parent().expect("congestion control has a parent")).unwrap();
    fs::create_dir_all(qdisc.parent().expect("qdisc has a parent")).unwrap();
    fs::write(&cc, "cubic\n").unwrap();
    fs::write(&qdisc, "fq_codel\n").unwrap();
    let sysctl = fixture.path().join("usr/bin/sysctl");
    fs::create_dir_all(sysctl.parent().expect("sysctl has a parent")).unwrap();
    fs::write(&sysctl, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sysctl, fs::Permissions::from_mode(0o700)).unwrap();
    }

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "system",
            "bbr",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("tcp_congestion_control=bbr"));

    let drop_in = fs::read_to_string(fixture.path().join("etc/sysctl.d/99-sbctl-bbr.conf"))
        .expect("the sysctl drop-in is persisted");
    assert!(drop_in.contains("net.ipv4.tcp_congestion_control=bbr"));
    assert!(drop_in.contains("net.core.default_qdisc=fq"));
}
