//! The managed-host lifecycle around install: `status`, `serve`, the
//! external-proxy modes, `uninstall`, credential rotation and `system`.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::net::TcpListener;
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

use crate::fixture::{
    free_high_tcp_port, http_get, initialize_ip_fallback_subscription,
    initialize_uninstall_fixture, read_subscription_credential, read_vless_uuid, refusal_message,
    sing_box_check_fixture, spawn_sbctl_serve, supported_systemd_host, write_managed_file,
    write_systemctl_fixture, write_traffic_fixture,
};

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
