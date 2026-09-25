//! `sbctl config`: initialization, validation, regeneration and the
//! interactive wizard, and the accounting anchors they persist.

use assert_cmd::Command;
use base64::Engine;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

use crate::fixture::{
    free_high_tcp_port, read_subscription_credential, read_vless_uuid, sing_box_check_fixture,
    supported_systemd_host, write_managed_file, write_systemctl_fixture, write_traffic_fixture,
};

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
        ))
        .stdout(predicate::str::contains("防火墙端口核对"))
        .stdout(predicate::str::contains("sudo ufw allow 2080/tcp"));

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
        ))
        .stdout(predicate::str::contains("防火墙端口核对"))
        .stdout(predicate::str::contains("sudo ufw allow 80/tcp"))
        .stdout(predicate::str::contains("sudo ufw allow 443/tcp"))
        .stdout(predicate::str::contains("# vless-reality"))
        .stdout(predicate::str::contains("# vmess-websocket"))
        .stdout(predicate::str::contains("# hysteria2"))
        .stdout(predicate::str::contains("# tuic"))
        .stdout(predicate::str::contains("# anytls"));

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
