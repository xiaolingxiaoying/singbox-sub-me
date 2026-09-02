use assert_cmd::Command;
use base64::Engine;
use predicates::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

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
}

#[test]
fn domain_nodes_export_vmess_websocket_and_hysteria2_with_independent_tls_credentials() {
    let fixture = TempDir::new().expect("temporary root is created");
    let checker = sing_box_check_fixture(&fixture, true);
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
    let checker = sing_box_check_fixture(&fixture, true);
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
    let rejecting_checker = sing_box_check_fixture(&rejected, false);
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

fn sing_box_check_fixture(fixture: &TempDir, accepts_config: bool) -> PathBuf {
    #[cfg(windows)]
    {
        let path = fixture.path().join("sing-box-check.cmd");
        let script = if accepts_config {
            "@echo off\r\nfindstr /C:\"\\\"type\\\": \\\"vmess\\\"\" %3 >nul || exit /b 1\r\nexit /b 0\r\n"
        } else {
            "@echo off\r\nexit /b 1\r\n"
        };
        fs::write(&path, script).expect("checker fixture is written");
        path
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let path = fixture.path().join("sing-box-check");
        let script = if accepts_config {
            "#!/bin/sh\ngrep -q '\"type\": \"vmess\"' \"$3\"\n"
        } else {
            "#!/bin/sh\nexit 1\n"
        };
        fs::write(&path, script).expect("checker fixture is written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("checker fixture is executable");
        path
    }
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
    let mut server = ProcessCommand::new(assert_cmd::cargo::cargo_bin!("sbctl"))
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--max-requests",
            "2",
        ])
        .spawn()
        .expect("subscription service starts");

    let response = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("subscription-userinfo: upload=0; download=0; total=1000; expire="));
    assert!(response.contains("Cache-Control: no-store"));
    let rejected = http_get(
        port,
        &format!("/sub/{credential}/uri?credential={credential}"),
    );
    assert!(rejected.starts_with("HTTP/1.1 404 Not Found"));
    assert!(
        server
            .wait()
            .expect("server exits after two requests")
            .success()
    );
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = (0..50)
        .find_map(|_| match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => Some(stream),
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
                None
            }
        })
        .expect("subscription service accepts connections");
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .expect("request is sent");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response is readable");
    response
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
fn help_lists_the_safe_install_and_status_commands() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("status"));
}

#[test]
fn install_reports_a_supported_systemd_fixture_as_ready() {
    let fixture = supported_systemd_host();

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("install preflight passed"));
}

#[test]
fn install_rejects_an_existing_deployment_without_changing_its_configuration() {
    let fixture = supported_systemd_host();
    let configuration = fixture.path().join("etc/sing-box/config.json");
    fs::create_dir_all(configuration.parent().expect("config has a parent"))
        .expect("configuration directory is created");
    fs::write(&configuration, "preserve this deployment").expect("configuration is written");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Existing deployment detected"));

    assert_eq!(
        fs::read_to_string(configuration).expect("preflight leaves the config intact"),
        "preserve this deployment"
    );
}

#[test]
fn install_rejects_an_existing_sing_box_binary_without_changing_it() {
    let fixture = supported_systemd_host();
    let binary = fixture.path().join("opt/sing-box/sing-box");
    fs::create_dir_all(binary.parent().expect("binary has a parent"))
        .expect("binary directory is created");
    fs::write(&binary, "existing sing-box binary").expect("binary is written");

    assert_existing_deployment_is_preserved(&fixture, &binary, "existing sing-box binary");
}

#[test]
fn install_rejects_an_existing_sing_box_service_without_changing_it() {
    let fixture = supported_systemd_host();
    let service = fixture.path().join("etc/systemd/system/proxy.service");
    fs::create_dir_all(service.parent().expect("service has a parent"))
        .expect("systemd unit directory is created");
    let contents = "[Service]\nExecStart=/opt/sing-box/sing-box run\n";
    fs::write(&service, contents).expect("service is written");

    assert_existing_deployment_is_preserved(&fixture, &service, contents);
}

#[test]
fn install_explains_when_the_platform_is_not_supported() {
    let fixture = TempDir::new().expect("temporary root is created");
    write_os_release(&fixture, "ID=alpine\nVERSION_ID=3.20\n");
    fs::create_dir_all(fixture.path().join("run/systemd/system"))
        .expect("systemd runtime directory is created");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "requires Debian or Ubuntu with systemd",
        ));
}

fn supported_systemd_host() -> TempDir {
    let fixture = TempDir::new().expect("temporary root is created");
    write_os_release(&fixture, "ID=debian\nVERSION_ID=12\n");
    fs::create_dir_all(fixture.path().join("run/systemd/system"))
        .expect("systemd runtime directory is created");
    fixture
}

fn write_os_release(fixture: &TempDir, contents: &str) {
    let path = fixture.path().join("etc/os-release");
    fs::create_dir_all(path.parent().expect("os-release has a parent"))
        .expect("etc directory is created");
    fs::write(path, contents).expect("os-release is written");
}

fn write_traffic_fixture(fixture: &TempDir, rx: u64, tx: u64, boot_id: &str) {
    let statistics = fixture.path().join("sys/class/net/ens3/statistics");
    fs::create_dir_all(&statistics).expect("statistics directory is created");
    fs::write(statistics.join("rx_bytes"), rx.to_string()).expect("RX counter is written");
    fs::write(statistics.join("tx_bytes"), tx.to_string()).expect("TX counter is written");
    let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
    fs::create_dir_all(boot_path.parent().expect("boot ID has a parent"))
        .expect("boot ID directory is created");
    fs::write(boot_path, boot_id).expect("boot ID is written");
}

fn assert_existing_deployment_is_preserved(
    fixture: &TempDir,
    path: &std::path::Path,
    contents: &str,
) {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Existing deployment detected"));

    assert_eq!(
        fs::read_to_string(path).expect("preflight leaves the Existing deployment intact"),
        contents
    );
}
