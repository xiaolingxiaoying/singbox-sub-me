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

#[cfg(unix)]
#[test]
fn certificate_renew_pins_the_certificate_written_by_certbot() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_direct_config(&fixture);
    seed_live_certificate(&fixture, &["sub.example.test"]);

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "certificate",
            "verify",
        ])
        .assert()
        .success();

    let renewed = rcgen::generate_simple_self_signed(vec!["sub.example.test".into()])
        .expect("a replacement certificate is generated");
    let renewed_fullchain = renewed.cert.pem();
    let renewed_private_key = renewed.signing_key.serialize_pem();
    let root = fixture.path();
    fs::write(
        root.join("var/lib/sbctl/renewed-fullchain.pem"),
        &renewed_fullchain,
    )
    .expect("replacement fullchain is staged");
    fs::write(
        root.join("var/lib/sbctl/renewed-privkey.pem"),
        &renewed_private_key,
    )
    .expect("replacement private key is staged");

    let certbot = root.join("usr/bin/certbot");
    fs::create_dir_all(certbot.parent().expect("certbot has a parent"))
        .expect("certbot fixture directory is created");
    fs::write(
        &certbot,
        r#"#!/bin/sh
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
printf '%s\n' "$*" > "$root/.certbot-args"
cp "$root/var/lib/sbctl/renewed-fullchain.pem" "$root/etc/letsencrypt/live/sub.example.test/fullchain.pem"
cp "$root/var/lib/sbctl/renewed-privkey.pem" "$root/etc/letsencrypt/live/sub.example.test/privkey.pem"
"#,
    )
    .expect("certbot renewal fixture is written");
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&certbot, fs::Permissions::from_mode(0o700))
            .expect("certbot fixture is executable");
    }

    let renewal_output = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root.to_str().expect("fixture path is UTF-8"),
            "certificate",
            "renew",
        ])
        .output()
        .expect("renew output is captured");
    assert!(
        renewal_output.status.success(),
        "a valid renewed certificate is published: {}",
        String::from_utf8_lossy(&renewal_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&renewal_output.stdout).contains("certificate operation completed")
    );
    assert_eq!(
        fs::read_to_string(root.join(".certbot-args")).expect("Certbot arguments are recorded"),
        "renew --cert-name sub.example.test --non-interactive\n"
    );

    let pinned = root.join("var/lib/sbctl/certificates/sub.example.test");
    assert_eq!(
        fs::read(pinned.join("fullchain.pem")).expect("renewed fullchain is pinned"),
        renewed_fullchain.as_bytes()
    );
    assert_eq!(
        fs::read(pinned.join("privkey.pem")).expect("renewed private key is pinned"),
        renewed_private_key.as_bytes()
    );

    // `certificate status` reports hook presence as part of the deployment
    // health summary. Model the hook installed by the real installer.
    let hook = root.join(sbctl::lifecycle::CERTBOT_DEPLOY_HOOK_RELATIVE_PATH);
    fs::create_dir_all(hook.parent().expect("deploy hook has a parent"))
        .expect("deploy hook directory is created");
    fs::write(&hook, "#!/bin/sh\nexec sbctl certificate verify\n")
        .expect("deploy hook fixture is written");

    let status = Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            root.to_str().expect("fixture path is UTF-8"),
            "certificate",
            "status",
        ])
        .output()
        .expect("certificate status output is captured");
    assert!(status.status.success());
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(status.contains("状态: 有效"));
    assert!(status.contains("Certbot deploy hook: 已安装"));
}

#[test]
fn certificate_commands_refuse_non_direct_modes_without_touching_any_certificate_path() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    // External-proxy configuration checks the listener port is free, so two
    // tests sharing one fixed port fail whenever the runner overlaps them.
    let listen_port = crate::fixture::free_high_tcp_port().to_string();
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
            &listen_port,
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
