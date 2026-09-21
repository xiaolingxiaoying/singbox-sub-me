//! `sbctl certificate`: obtaining, verifying and renewing the TLS
//! certificate, and the modes that refuse to touch it.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[cfg(unix)]
use std::fs;

use crate::fixture::{
    read_subscription_credential, seed_direct_config, seed_live_certificate,
    supported_systemd_host, write_traffic_fixture,
};

#[cfg(unix)]
use crate::fixture::write_command_fixture;

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
