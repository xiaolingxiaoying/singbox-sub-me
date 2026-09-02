use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
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
