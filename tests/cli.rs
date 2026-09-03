use assert_cmd::Command;
use base64::Engine;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
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
fn menu_requires_an_interactive_terminal() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .arg("menu")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("requires an interactive terminal"));
}

#[test]
fn update_check_reads_a_verified_release_manifest_without_changing_the_host() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_release_manifest(&manifest, b"candidate sbctl", b"candidate sing-box");
    let before = filesystem_snapshot(fixture.path());

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--check",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("sbctl: 0.1.1 available"))
        .stdout(predicate::str::contains("sing-box: 1.12.0 available"));

    assert_eq!(filesystem_snapshot(fixture.path()), before);
}

#[test]
fn update_rejects_an_artifact_that_does_not_match_the_fixed_manifest() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_release_manifest(&manifest, b"expected sbctl", b"expected sing-box");
    let sbctl = fixture.path().join("candidate-sbctl");
    let sing_box = fixture.path().join("candidate-sing-box");
    fs::write(&sbctl, b"unexpected sbctl").expect("candidate is written");
    fs::write(&sing_box, b"expected sing-box").expect("candidate is written");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--sbctl-artifact",
            sbctl.to_str().expect("candidate path is UTF-8"),
            "--sing-box-artifact",
            sing_box.to_str().expect("candidate path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "does not match the pinned release manifest",
        ));

    assert!(!fixture.path().join("var/lib/sbctl/rollback").exists());
    assert!(!fixture.path().join("usr/local/bin/sbctl").exists());
}

#[test]
fn failed_update_health_check_restores_the_known_good_binaries_and_keeps_a_rollback_point() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_update_fixture(&fixture);
    let manifest = fixture.path().join("release-manifest.json");
    let sbctl = command_fixture(&fixture, "candidate-sbctl", true, &[]);
    let sing_box = sing_box_check_fixture(&fixture, true, &["vless"]);
    write_release_manifest(
        &manifest,
        &fs::read(&sbctl).expect("candidate is readable"),
        &fs::read(&sing_box).expect("candidate is readable"),
    );
    write_systemctl_fixture(&fixture, false);
    let old_sbctl = b"known-good sbctl";
    let old_sing_box = b"known-good sing-box";
    write_managed_file(&fixture, "usr/local/bin/sbctl", old_sbctl);
    write_managed_file(&fixture, "usr/local/bin/sing-box", old_sing_box);
    let old_state = b"known-good accounting state";
    write_managed_file(&fixture, "var/lib/sbctl/state.json", old_state);
    let old_artifact = fs::read(
        fixture
            .path()
            .join("var/lib/sbctl/artifacts/sing-box-server.json"),
    )
    .expect("generated server artifact is readable");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--sbctl-artifact",
            sbctl.to_str().expect("candidate path is UTF-8"),
            "--sing-box-artifact",
            sing_box.to_str().expect("candidate path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("service health check failed"));

    assert_eq!(
        fs::read(fixture.path().join("usr/local/bin/sbctl")).expect("old sbctl is restored"),
        old_sbctl
    );
    assert_eq!(
        fs::read(fixture.path().join("usr/local/bin/sing-box")).expect("old sing-box is restored"),
        old_sing_box
    );
    assert_eq!(
        fs::read(fixture.path().join("var/lib/sbctl/state.json")).expect("old state is restored"),
        old_state
    );
    assert_eq!(
        fs::read(
            fixture
                .path()
                .join("var/lib/sbctl/artifacts/sing-box-server.json")
        )
        .expect("old artifact is restored"),
        old_artifact
    );
    let rollback_root = fixture.path().join("var/lib/sbctl/rollback");
    let rollback_point = fs::read_dir(&rollback_root)
        .expect("rollback directory is readable")
        .next()
        .expect("a rollback point exists")
        .expect("rollback entry is readable")
        .path();
    assert_eq!(
        fs::read(rollback_point.join("var/lib/sbctl/state.json")).expect("state is backed up"),
        old_state
    );
    assert_eq!(
        fs::read(rollback_point.join("var/lib/sbctl/artifacts/sing-box-server.json"))
            .expect("artifact is backed up"),
        old_artifact
    );
    assert!(rollback_point.join("etc/sbctl/config.toml").is_file());
}

#[test]
fn failed_candidate_configuration_check_leaves_the_known_good_binaries_untouched() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_update_fixture(&fixture);
    let manifest = fixture.path().join("release-manifest.json");
    let sbctl = command_fixture(&fixture, "candidate-sbctl", true, &[]);
    let sing_box = sing_box_check_fixture(&fixture, false, &[]);
    write_release_manifest(
        &manifest,
        &fs::read(&sbctl).expect("candidate is readable"),
        &fs::read(&sing_box).expect("candidate is readable"),
    );
    let old_sbctl = b"known-good sbctl";
    let old_sing_box = b"known-good sing-box";
    write_managed_file(&fixture, "usr/local/bin/sbctl", old_sbctl);
    write_managed_file(&fixture, "usr/local/bin/sing-box", old_sing_box);

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--sbctl-artifact",
            sbctl.to_str().expect("candidate path is UTF-8"),
            "--sing-box-artifact",
            sing_box.to_str().expect("candidate path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "sing-box candidate configuration check failed",
        ));

    assert_eq!(
        fs::read(fixture.path().join("usr/local/bin/sbctl")).expect("old sbctl is preserved"),
        old_sbctl
    );
    assert_eq!(
        fs::read(fixture.path().join("usr/local/bin/sing-box")).expect("old sing-box is preserved"),
        old_sing_box
    );
    assert!(!fixture.path().join("var/lib/sbctl/rollback").exists());
}

#[test]
fn update_check_rejects_an_unsigned_manifest_without_trusting_its_urls_or_digests() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_unsigned_release_manifest(&manifest, b"candidate sing-box");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--check",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unsigned"));

    assert!(!fixture.path().join("var/lib/sbctl/rollback").exists());
    assert!(!fixture.path().join("usr/local/bin/sbctl").exists());
}

#[test]
fn update_rejects_a_corrupted_signature_before_any_download_or_replacement() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_release_manifest(&manifest, b"candidate sbctl", b"candidate sing-box");
    corrupt_manifest_signature(&manifest);

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("signature is invalid"))
        .stderr(predicate::str::contains("download").not());

    assert!(!fixture.path().join("var/lib/sbctl/rollback").exists());
    assert!(!fixture.path().join("usr/local/bin/sbctl").exists());
}

#[test]
fn update_rejects_an_unknown_schema_version() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_lib_signed_manifest(
        &manifest,
        2,
        "0.1.1",
        b"candidate sbctl",
        "1.12.0",
        b"candidate sing-box",
        "1.12.0",
        "1.12.0",
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--check",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "schema version 2 is not supported",
        ));
}

#[test]
fn update_rejects_a_sing_box_outside_the_compatibility_matrix_before_replacement() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_update_fixture(&fixture);
    let manifest = fixture.path().join("release-manifest.json");
    let sbctl = command_fixture(&fixture, "candidate-sbctl", true, &[]);
    let sing_box = sing_box_check_fixture(&fixture, true, &["vless"]);
    let sbctl_contents = fs::read(&sbctl).expect("sbctl candidate is readable");
    let sing_box_contents = fs::read(&sing_box).expect("sing-box candidate is readable");
    write_lib_signed_manifest(
        &manifest,
        1,
        "0.1.1",
        &sbctl_contents,
        "1.99.0",
        &sing_box_contents,
        "1.12.0",
        "1.12.9",
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--sbctl-artifact",
            sbctl.to_str().expect("sbctl candidate path is UTF-8"),
            "--sing-box-artifact",
            sing_box.to_str().expect("sing-box candidate path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("outside the compatibility matrix"));

    assert!(!fixture.path().join("var/lib/sbctl/rollback").exists());
    assert!(!fixture.path().join("usr/local/bin/sbctl").exists());
    assert!(!fixture.path().join("usr/local/bin/sing-box").exists());
}

#[test]
fn update_rejects_latest_and_main_floating_versions() {
    for version in ["latest", "main"] {
        let fixture = TempDir::new().expect("temporary root is created");
        let manifest = fixture.path().join("release-manifest.json");
        write_lib_signed_manifest(
            &manifest,
            1,
            "0.1.1",
            b"candidate sbctl",
            version,
            b"candidate sing-box",
            "1.12.0",
            "1.12.0",
        );

        Command::cargo_bin("sbctl")
            .expect("sbctl binary is built")
            .args([
                "--root",
                fixture.path().to_str().expect("fixture path is UTF-8"),
                "update",
                "--check",
                "--manifest",
                manifest.to_str().expect("manifest path is UTF-8"),
            ])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("unsupported version"));
    }
}

#[test]
fn release_sign_refuses_to_sign_floating_or_invalid_manifests() {
    for (schema, sing_box_version, matrix) in [
        (1, "latest", Some(("1.12.0", "1.12.0"))),
        (2, "1.12.0", Some(("1.12.0", "1.12.0"))),
        (1, "1.12.0", None),
    ] {
        let fixture = TempDir::new().expect("temporary root is created");
        let unsigned = fixture.path().join("release-manifest.unsigned.json");
        let signed = fixture.path().join("release-manifest.json");
        let digest = |contents: &[u8]| format!("{:x}", Sha256::digest(contents));
        let matrix = match matrix {
            Some((min, max)) => {
                format!(r#","sing_box_compatibility":[{{"min":"{min}","max":"{max}"}}]"#)
            }
            None => String::new(),
        };
        fs::write(
            &unsigned,
            format!(
                r#"{{"schema":{schema},"sbctl":{{"version":"0.1.1","sha256":"{}"}},"sing_box":{{"version":"{sing_box_version}","sha256":"{}"}}{matrix}}}"#,
                digest(b"candidate sbctl"),
                digest(b"candidate sing-box"),
            ),
        )
        .expect("unsigned manifest is written");
        Command::cargo_bin("sbctl")
            .expect("sbctl binary is built")
            .args([
                "release",
                "sign",
                "--manifest",
                unsigned.to_str().expect("unsigned path is UTF-8"),
                "--private-key",
                &format!("{}/scripts/dev-signing-key.hex", env!("CARGO_MANIFEST_DIR")),
                "--output",
                signed.to_str().expect("signed path is UTF-8"),
            ])
            .assert()
            .code(2);
        assert!(!signed.exists());
    }
}

#[test]
fn sing_box_update_rejects_an_unsigned_manifest() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_unsigned_release_manifest(&manifest, b"candidate sing-box");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sing-box",
            "download",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--output",
            fixture
                .path()
                .join("downloaded-sing-box")
                .to_str()
                .expect("output path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unsigned"));

    assert!(!fixture.path().join("downloaded-sing-box").exists());
}

#[test]
fn install_with_a_signed_manifest_downloads_and_verifies_sing_box() {
    let fixture = supported_systemd_host();
    let sing_box_fixture = sing_box_check_fixture(&fixture, true, &["vless"]);
    let sing_box_contents = fs::read(&sing_box_fixture).expect("sing-box fixture is readable");

    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port is available");
    let port = listener.local_addr().expect("address is available").port();
    let served = sing_box_contents.clone();
    let server = thread::spawn(move || {
        for stream in listener.incoming().take(1) {
            let mut stream = stream.expect("client stream is accepted");
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                served.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&served);
        }
    });

    let manifest = fixture.path().join("release-manifest.json");
    write_manifest_spec(
        &manifest,
        1,
        "0.1.1",
        "https://example.test/sbctl",
        b"candidate sbctl",
        "1.12.0",
        &format!("http://127.0.0.1:{port}/sing-box"),
        &sing_box_contents,
        "1.12.0",
        "1.12.0",
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
            "--mode",
            "ip-fallback",
            "--subscription-host",
            "203.0.113.7",
            "--http-port",
            "2080",
            "--interface",
            "ens3",
            "--disable-protocol",
            "vmess-websocket",
            "--disable-protocol",
            "hysteria2",
            "--disable-protocol",
            "tuic",
            "--disable-protocol",
            "anytls",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--no-start",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("installation completed"));

    server.join().expect("byte server exits");
    assert_eq!(
        fs::read(fixture.path().join("usr/local/bin/sing-box")).expect("sing-box is installed"),
        sing_box_contents
    );
}

#[test]
fn release_verify_accepts_a_manifest_signed_with_the_built_in_key() {
    let fixture = TempDir::new().expect("temporary root is created");
    let manifest = fixture.path().join("release-manifest.json");
    write_release_manifest(&manifest, b"candidate sbctl", b"candidate sing-box");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "release",
            "verify",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "verified against the built-in public key",
        ));
}

fn initialize_update_fixture(fixture: &TempDir) {
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
}

fn write_managed_file(fixture: &TempDir, relative: &str, contents: &[u8]) {
    let path = fixture.path().join(relative);
    fs::create_dir_all(path.parent().expect("managed path has a parent"))
        .expect("managed directory is created");
    fs::write(path, contents).expect("managed file is written");
}

fn write_systemctl_fixture(fixture: &TempDir, succeeds: bool) {
    let _ = command_fixture(fixture, "usr/bin/systemctl", succeeds, &[]);
}

/// Writes an executable host command that exits with the given status and,
/// when non-empty, emits `stderr_text` so diagnostics can be asserted.
fn write_command_fixture(fixture: &TempDir, name: &str, status: u8, stderr_text: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = fixture.path().join(name);
        fs::create_dir_all(path.parent().expect("command path has a parent"))
            .expect("command directory is created");
        fs::write(
            &path,
            format!("#!/bin/sh\n{}\nexit {status}\n", {
                if stderr_text.is_empty() {
                    String::new()
                } else {
                    format!("echo {stderr_text} >&2")
                }
            }),
        )
        .expect("command fixture is written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("command fixture is executable");
    }
    #[cfg(not(unix))]
    let _ = (fixture, name, status, stderr_text);
}

/// Persists a Direct subscription configuration without starting services.
fn seed_direct_config(fixture: &TempDir) {
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
}

/// Writes a self-signed certificate into the Certbot live directory that the
/// `certificate` commands validate against the subscription host.
fn seed_live_certificate(fixture: &TempDir, names: &[&str]) {
    let certificate = rcgen::generate_simple_self_signed(
        names
            .iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("a self-signed certificate is generated");
    let directory = fixture.path().join("etc/letsencrypt/live/sub.example.test");
    fs::create_dir_all(&directory).expect("certificate directory is created");
    fs::write(directory.join("fullchain.pem"), certificate.cert.pem())
        .expect("fullchain is written");
    fs::write(
        directory.join("privkey.pem"),
        certificate.signing_key.serialize_pem(),
    )
    .expect("private key is written");
}

fn command_fixture(
    fixture: &TempDir,
    name: &str,
    succeeds: bool,
    expected_protocols: &[&str],
) -> PathBuf {
    #[cfg(windows)]
    {
        let path = fixture.path().join(format!("{name}.cmd"));
        fs::create_dir_all(path.parent().expect("command path has a parent"))
            .expect("command directory is created");
        let checks = expected_protocols
            .iter()
            .map(|protocol| {
                format!("findstr /C:\"\\\"type\\\": \\\"{protocol}\\\"\" %3 >nul || exit /b 1\r\n")
            })
            .collect::<String>();
        fs::write(
            &path,
            format!(
                "@echo off\r\n{checks}exit /b {}\r\n",
                if succeeds { 0 } else { 1 }
            ),
        )
        .expect("command fixture is written");
        path
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let path = fixture.path().join(name);
        fs::create_dir_all(path.parent().expect("command path has a parent"))
            .expect("command directory is created");
        let checks = expected_protocols
            .iter()
            .map(|protocol| format!("grep -q '\"type\": \"{protocol}\"' \"$3\" || exit 1\n"))
            .collect::<String>();
        fs::write(
            &path,
            format!("#!/bin/sh\n{checks}exit {}\n", if succeeds { 0 } else { 1 }),
        )
        .expect("command fixture is written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("command fixture is executable");
        path
    }
}

/// Writes a signed schema-1 release manifest pinned to the given artifact
/// contents, signed with the development signing key through the real
/// `sbctl release sign` path so the fixtures exercise the same rules as a
/// published release.
fn write_release_manifest(path: &std::path::Path, sbctl: &[u8], sing_box: &[u8]) {
    write_signed_release_manifest(path, "0.1.1", sbctl, "1.12.0", sing_box, "1.12.0", "1.12.0");
}

fn write_signed_release_manifest(
    path: &std::path::Path,
    sbctl_version: &str,
    sbctl: &[u8],
    sing_box_version: &str,
    sing_box: &[u8],
    matrix_min: &str,
    matrix_max: &str,
) {
    write_manifest_spec(
        path,
        1,
        sbctl_version,
        "https://example.test/sbctl",
        sbctl,
        sing_box_version,
        "https://example.test/sing-box",
        sing_box,
        matrix_min,
        matrix_max,
    );
}

/// Writes a manifest with the given schema, versions, and URLs, then signs it
/// with the development signing key through the real `sbctl release sign` path.
#[allow(clippy::too_many_arguments)]
fn write_manifest_spec(
    path: &std::path::Path,
    schema: u32,
    sbctl_version: &str,
    sbctl_url: &str,
    sbctl: &[u8],
    sing_box_version: &str,
    sing_box_url: &str,
    sing_box: &[u8],
    matrix_min: &str,
    matrix_max: &str,
) {
    let digest = |contents: &[u8]| format!("{:x}", Sha256::digest(contents));
    let unsigned = path.with_extension("unsigned.tmp.json");
    fs::write(
        &unsigned,
        format!(
            r#"{{"schema":{schema},"sbctl":{{"version":"{sbctl_version}","url":"{sbctl_url}","sha256":"{}"}},"sing_box":{{"version":"{sing_box_version}","url":"{sing_box_url}","sha256":"{}"}},"sing_box_compatibility":[{{"min":"{matrix_min}","max":"{matrix_max}"}}]}}"#,
            digest(sbctl),
            digest(sing_box),
        ),
    )
    .expect("unsigned release manifest is written");
    sign_release_manifest(&unsigned, path);
}

/// Signs an already-written manifest with the development signing key through
/// the real `sbctl release sign` CLI path.
fn sign_release_manifest(unsigned: &std::path::Path, signed: &std::path::Path) {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "release",
            "sign",
            "--manifest",
            unsigned.to_str().expect("unsigned path is UTF-8"),
            "--private-key",
            &format!("{}/scripts/dev-signing-key.hex", env!("CARGO_MANIFEST_DIR")),
            "--output",
            signed.to_str().expect("output path is UTF-8"),
        ])
        .assert()
        .success();
}

/// Rewrites a signed manifest with a corrupted signature value.
fn corrupt_manifest_signature(path: &std::path::Path) {
    let contents = fs::read_to_string(path).expect("manifest is readable");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&contents).expect("manifest is valid JSON");
    manifest["signature"] = serde_json::Value::String(
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            .to_owned(),
    );
    fs::write(
        path,
        serde_json::to_string(&manifest).expect("manifest is serialized"),
    )
    .expect("corrupted manifest is written");
}

/// Writes a manifest signed at the library level, bypassing `release sign`
/// field validation, so the verifier's rejection of otherwise-signed-but-
/// invalid manifests (wrong schema, floating version, empty matrix) can be
/// exercised end to end through the CLI.
#[allow(clippy::too_many_arguments)]
fn write_lib_signed_manifest(
    path: &std::path::Path,
    schema: u32,
    sbctl_version: &str,
    sbctl: &[u8],
    sing_box_version: &str,
    sing_box: &[u8],
    matrix_min: &str,
    matrix_max: &str,
) {
    use sbctl::release::{
        CompatibilityRange, ReleaseArtifact, ReleaseManifest, manifest_json_with_signature,
        parse_seed_hex, sign_manifest,
    };
    let seed_path = format!("{}/scripts/dev-signing-key.hex", env!("CARGO_MANIFEST_DIR"));
    let seed = parse_seed_hex(&fs::read_to_string(seed_path).expect("dev signing key is readable"))
        .expect("dev signing key parses");
    let digest = |contents: &[u8]| format!("{:x}", Sha256::digest(contents));
    let mut manifest = ReleaseManifest {
        schema,
        sbctl: ReleaseArtifact {
            version: sbctl_version.to_owned(),
            url: Some("https://example.test/sbctl".to_owned()),
            sha256: digest(sbctl),
        },
        sing_box: ReleaseArtifact {
            version: sing_box_version.to_owned(),
            url: Some("https://example.test/sing-box".to_owned()),
            sha256: digest(sing_box),
        },
        sing_box_compatibility: vec![CompatibilityRange {
            min: Some(matrix_min.to_owned()),
            max: Some(matrix_max.to_owned()),
        }],
        signature: None,
    };
    manifest.signature = Some(sign_manifest(&manifest, &seed).expect("manifest is signed"));
    fs::write(
        path,
        manifest_json_with_signature(&manifest).expect("signed manifest serializes"),
    )
    .expect("signed manifest is written");
}

/// Writes an unsigned schema-1 manifest so signature rejection can be tested
/// without the manifest's URLs or digests ever being trusted.
fn write_unsigned_release_manifest(path: &std::path::Path, sing_box: &[u8]) {
    let digest = |contents: &[u8]| format!("{:x}", Sha256::digest(contents));
    fs::write(
        path,
        format!(
            r#"{{"schema":1,"sbctl":{{"version":"0.1.1","sha256":"{}"}},"sing_box":{{"version":"1.12.0","sha256":"{}"}},"sing_box_compatibility":[{{"min":"1.12.0","max":"1.12.0"}}]}}"#,
            digest(b"candidate sbctl"),
            digest(sing_box),
        ),
    )
    .expect("unsigned release manifest is written");
}

fn filesystem_snapshot(root: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).expect("fixture root is readable") {
        let entry = entry.expect("fixture entry is readable");
        if entry.file_type().expect("file type is readable").is_file() {
            entries.push((
                entry.path(),
                fs::read(entry.path()).expect("file is readable"),
            ));
        }
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
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
    let baseline_artifacts = sbctl::subscription::generated_artifacts(&baseline)
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
        assert!(
            contents.contains("www.apple.com"),
            "{name} carries the new canonical node field"
        );
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

fn sing_box_check_fixture(
    fixture: &TempDir,
    accepts_config: bool,
    expected_protocols: &[&str],
) -> PathBuf {
    #[cfg(windows)]
    {
        let path = fixture.path().join("sing-box-check.cmd");
        let script = if accepts_config {
            format!(
                "@echo off\r\n{}exit /b 0\r\n",
                expected_protocols
                    .iter()
                    .map(|protocol| format!(
                        "findstr /C:\"\\\"type\\\": \\\"{protocol}\\\"\" %3 >nul || exit /b 1\r\n"
                    ))
                    .collect::<String>()
            )
        } else {
            "@echo off\r\nexit /b 1\r\n".to_owned()
        };
        fs::write(&path, script).expect("checker fixture is written");
        path
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let path = fixture.path().join("sing-box-check");
        let script = if accepts_config {
            format!(
                "#!/bin/sh\n{}",
                expected_protocols
                    .iter()
                    .map(|protocol| format!("grep -q '\"type\": \"{protocol}\"' \"$3\"\n"))
                    .collect::<String>()
            )
        } else {
            "#!/bin/sh\nexit 1\n".to_owned()
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
    assert!(response.contains("subscription-userinfo: upload=0; download=0; total=0; expire="));
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
fn subscription_returns_a_redacted_503_for_missing_state_without_logging_the_credential() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    let stderr_log = fixture.path().join("serve.err");
    let mut server = spawn_sbctl_serve(&fixture, port, 2, &stderr_log);

    let unavailable = http_get(port, &format!("/sub/{credential}/uri"));
    assert!(unavailable.starts_with("HTTP/1.1 503 Service Unavailable"));
    assert!(
        unavailable.ends_with("\r\n\r\n"),
        "the 503 has an empty redacted body"
    );
    let rejected = http_get(port, "/sub/wrong-credential/uri");
    assert!(
        rejected.starts_with("HTTP/1.1 404 Not Found"),
        "an invalid credential stays 404 even when state is missing"
    );
    assert!(server.wait().expect("server exits").success());

    let log = fs::read_to_string(&stderr_log).expect("diagnostic log is readable");
    assert!(
        log.contains("subscription request failed"),
        "a redacted diagnostic is written"
    );
    assert!(
        !log.contains(&credential),
        "the 503 diagnostic must not contain the full Subscription credential"
    );
}

#[test]
fn subscription_returns_a_redacted_503_for_corrupt_state_without_logging_the_credential() {
    let fixture = TempDir::new().expect("temporary root is created");
    let port = free_high_tcp_port();
    let credential = initialize_ip_fallback_subscription(&fixture, port);
    fs::create_dir_all(fixture.path().join("var/lib/sbctl")).expect("state directory is created");
    fs::write(fixture.path().join("var/lib/sbctl/state.json"), "not json")
        .expect("corrupt state is written");
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
fn subscription_returns_a_redacted_503_for_a_schema_mismatched_state() {
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
    let now = chrono::Utc::now();
    use chrono::Datelike;
    assert_eq!(
        status["traffic"]["accounting_period"],
        format!("{:04}-{:02}-01T00:00:00+00:00", now.year(), now.month())
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

fn initialize_traffic_fixture(fixture: &TempDir) {
    write_traffic_fixture(fixture, 100, 200, "boot-a");
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
}

fn run_traffic_set_used(fixture: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "traffic",
            "set-used",
        ])
        .args(args)
        .assert()
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
fn configuration_init_defaults_the_accounting_timezone_to_utc() {
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
    assert!(config.contains("accounting_timezone = \"UTC\""));
}

#[test]
fn anchored_month_before_the_first_reset_reports_pending_first_reset_with_zero_usage() {
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
            "traffic",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "accounting period: pending-first-reset",
        ))
        .stdout(predicate::str::contains("total: 0 bytes"))
        .stdout(predicate::str::contains(
            "next reset: 2099-01-01T00:00:00+00:00",
        ));
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
fn install_help_exposes_the_ip_fallback_http_port() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["install", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--http-port"));
}

#[test]
fn help_lists_independent_sing_box_lifecycle_commands() {
    Command::cargo_bin("sbctl")
        .expect("binary exists")
        .arg("sing-box")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("download"))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("update"))
        .stdout(predicate::str::contains("remove"));
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
fn install_defaults_to_all_managed_protocols_writes_services_and_only_lists_firewall_ports() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
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
        .success()
        .stdout(predicate::str::contains(
            "enabled protocols: vless-reality, vmess-websocket, hysteria2, tuic, anytls",
        ))
        .stdout(predicate::str::contains(
            "required firewall ports (not changed):",
        ));

    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("installation persists configuration");
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
    let sing_box_unit =
        fs::read_to_string(fixture.path().join("etc/systemd/system/sing-box.service"))
            .expect("sing-box unit is installed");
    let sbctl_unit = fs::read_to_string(fixture.path().join("etc/systemd/system/sbctl.service"))
        .expect("sbctl unit is installed");
    assert!(
        sing_box_unit
            .contains("ExecStart=/usr/local/bin/sing-box run -c /etc/sing-box/config.json")
    );
    assert!(sing_box_unit.contains("User=sing-box"));
    assert!(sing_box_unit.contains("Group=sing-box"));
    assert!(
        !sing_box_unit.contains("User=sbctl"),
        "the sing-box data plane runs under its own non-root account"
    );
    assert!(sbctl_unit.contains("User=sbctl"));
    assert!(
        sbctl_unit.contains("Requires=sbctl-http.socket"),
        "the Direct HTTPS service depends on the socket unit that owns 80/443"
    );
    let http_socket =
        fs::read_to_string(fixture.path().join("etc/systemd/system/sbctl-http.socket"))
            .expect("the Direct HTTPS socket unit is installed");
    assert!(http_socket.contains("ListenStream=80"));
    assert!(http_socket.contains("ListenStream=443"));
    let reset_timer = fs::read_to_string(
        fixture
            .path()
            .join("etc/systemd/system/sbctl-accounting-reset.timer"),
    )
    .expect("accounting reset timer is installed");
    let reset_service = fs::read_to_string(
        fixture
            .path()
            .join("etc/systemd/system/sbctl-accounting-reset.service"),
    )
    .expect("accounting reset service is installed");
    assert!(reset_timer.contains("Persistent=true"));
    assert!(reset_timer.contains("OnCalendar=minutely"));
    assert!(reset_service.contains("ExecStart=/usr/local/bin/sbctl accounting-reset"));
    assert!(reset_service.contains("User=sbctl"));
    assert!(fixture.path().join("etc/sing-box/config.json").is_file());
    assert!(!fixture.path().join("etc/ufw/user.rules").exists());

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "node",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("vless-reality: TCP"))
        .stdout(predicate::str::contains("hysteria2: UDP"))
        .stdout(predicate::str::contains("tuic: UDP"));
}

#[test]
fn install_writes_the_ownership_marker_after_services_start_and_pass_the_health_check() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_service_accounts(&fixture);
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
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(fixture.path().join("var/lib/sbctl/ownership"))
            .expect("the ownership marker is written only after the health check"),
        "sbctl-managed-v1\n"
    );
}

#[test]
fn install_startup_failure_rolls_back_without_leaving_an_ownership_marker() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_service_accounts(&fixture);
    write_systemctl_fixture(&fixture, false);
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
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("installation failed"));

    assert!(
        !fixture.path().join("var/lib/sbctl/ownership").exists(),
        "a failed installation must not leave an ownership marker"
    );
    assert!(!fixture.path().join("etc/sbctl/config.toml").exists());
    assert!(!fixture.path().join("usr/local/bin/sing-box").exists());
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sing-box.service")
            .exists()
    );
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl.service")
            .exists()
    );
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl-accounting-reset.timer")
            .exists()
    );
}

#[test]
fn install_health_check_failure_rolls_back_without_leaving_an_ownership_marker() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    seed_service_accounts(&fixture);
    write_systemctl_health_failing_fixture(&fixture);
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
            "--mode",
            "external-proxy",
            "--subscription-host",
            "sub.example.test",
            "--interface",
            "ens3",
            "--reality-decoy-sni",
            "www.cloudflare.com",
            "--sing-box-bin",
            checker.to_str().expect("checker path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("installation failed"));

    assert!(
        !fixture.path().join("var/lib/sbctl/ownership").exists(),
        "a failed health check must not leave an ownership marker"
    );
    assert!(!fixture.path().join("etc/sbctl/config.toml").exists());
    assert!(!fixture.path().join("usr/local/bin/sing-box").exists());
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl.service")
            .exists()
    );
}

fn seed_service_accounts(fixture: &TempDir) {
    write_managed_file(
        fixture,
        "etc/passwd",
        b"sbctl:x:999:999::/nonexistent:/usr/sbin/nologin\nsing-box:x:998:998::/nonexistent:/usr/sbin/nologin\n",
    );
}

/// A systemctl stub that starts units successfully but reports every unit as
/// inactive, so the install health check phase fails after a successful start.
fn write_systemctl_health_failing_fixture(fixture: &TempDir) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = fixture.path().join("usr/bin/systemctl");
        fs::create_dir_all(path.parent().expect("systemctl has a parent"))
            .expect("systemctl directory is created");
        fs::write(
            &path,
            "#!/bin/sh\ncase \"$1\" in\n  is-active) exit 1 ;;\n  *) exit 0 ;;\nesac\n",
        )
        .expect("systemctl fixture is written");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("systemctl fixture is executable");
    }
}

#[test]
fn external_proxy_install_does_not_install_the_direct_https_socket() {
    let fixture = supported_systemd_host();
    write_traffic_fixture(&fixture, 100, 200, "boot-a");
    let checker = sing_box_check_fixture(&fixture, true, &["vless"]);

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "install",
            "--mode",
            "external-proxy",
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

    let sbctl_unit = fs::read_to_string(fixture.path().join("etc/systemd/system/sbctl.service"))
        .expect("sbctl unit is installed");
    assert!(
        !sbctl_unit.contains("sbctl-http.socket"),
        "an external reverse proxy keeps owning public 80/443"
    );
    assert!(
        !fixture
            .path()
            .join("etc/systemd/system/sbctl-http.socket")
            .exists(),
        "no socket unit is installed outside Direct subscription mode"
    );
    assert!(
        fixture
            .path()
            .join("etc/systemd/system/sing-box.service")
            .is_file()
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
        .stderr(predicate::str::contains("requires sbctl-http.socket"));
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
fn direct_install_writes_the_certbot_deploy_hook_but_external_proxy_does_not() {
    let direct = supported_systemd_host();
    write_traffic_fixture(&direct, 100, 200, "boot-a");
    let checker = sing_box_check_fixture(&direct, true, &["vless"]);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            direct.path().to_str().expect("fixture path is UTF-8"),
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
    let hook = direct
        .path()
        .join("etc/letsencrypt/renewal-hooks/deploy/sbctl-certificate-deploy-hook");
    assert!(
        hook.is_file(),
        "Direct install writes the Certbot deploy hook"
    );
    let hook_text = fs::read_to_string(&hook).expect("deploy hook is readable");
    assert!(hook_text.contains("sbctl certificate verify"));
    assert!(hook_text.contains("set -eu"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&hook)
            .expect("deploy hook has metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "the deploy hook is executable");
    }

    let external = supported_systemd_host();
    write_traffic_fixture(&external, 100, 200, "boot-a");
    let checker = sing_box_check_fixture(&external, true, &["vless"]);
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            external.path().to_str().expect("fixture path is UTF-8"),
            "install",
            "--mode",
            "external-proxy",
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
        !external
            .path()
            .join("etc/letsencrypt/renewal-hooks/deploy/sbctl-certificate-deploy-hook")
            .exists(),
        "External proxy mode never installs or touches the sbctl deploy hook"
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

fn initialize_uninstall_fixture(fixture: &TempDir) {
    write_traffic_fixture(fixture, 100, 200, "boot-a");
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
    write_managed_file(fixture, "usr/local/bin/sbctl", b"managed sbctl binary");
    write_managed_file(
        fixture,
        "usr/local/bin/sing-box",
        b"managed sing-box binary",
    );
    write_managed_file(
        fixture,
        "etc/sing-box/config.json",
        b"managed sing-box configuration",
    );
    write_managed_file(
        fixture,
        "etc/systemd/system/sbctl.service",
        b"Description=sbctl private subscription service",
    );
    write_managed_file(
        fixture,
        "etc/systemd/system/sing-box.service",
        b"Description=sing-box data plane managed by sbctl",
    );
    write_managed_file(
        fixture,
        "etc/systemd/system/sbctl-accounting-reset.timer",
        b"Description=sbctl accounting period reset timer\n[Timer]\nPersistent=true\n",
    );
    write_managed_file(
        fixture,
        "etc/systemd/system/sbctl-accounting-reset.service",
        b"Description=sbctl accounting period reset task\n[Service]\nExecStart=/usr/local/bin/sbctl accounting-reset\n",
    );
    write_managed_file(
        fixture,
        "var/lib/sbctl/state.json",
        b"managed traffic state",
    );
    write_managed_file(fixture, "var/lib/sbctl/ownership", b"sbctl-managed-v1\n");
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

fn spawn_sbctl_serve(
    fixture: &TempDir,
    port: u16,
    max_requests: usize,
    stderr_log: &PathBuf,
) -> std::process::Child {
    let stderr_file = fs::File::create(stderr_log).expect("stderr log is created");
    ProcessCommand::new(assert_cmd::cargo::cargo_bin!("sbctl"))
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--max-requests",
            &max_requests.to_string(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_file))
        .spawn()
        .expect("subscription service starts")
}

fn initialize_ip_fallback_subscription(fixture: &TempDir, port: u16) -> String {
    write_traffic_fixture(fixture, 100, 200, "boot-a");
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
    fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted")
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("credential is available")
        .to_owned()
}

fn free_high_tcp_port() -> u16 {
    loop {
        let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port is available");
        let port = listener
            .local_addr()
            .expect("ephemeral listener has an address")
            .port();
        if port >= 10000 {
            return port;
        }
    }
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

fn read_subscription_credential(fixture: &TempDir) -> String {
    fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted")
        .lines()
        .find_map(|line| {
            line.strip_prefix("subscription_credential = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("subscription credential is available")
        .to_owned()
}

fn read_vless_uuid(fixture: &TempDir) -> String {
    let config = fs::read_to_string(fixture.path().join("etc/sbctl/config.toml"))
        .expect("configuration is persisted");
    let section = config
        .split("[vless_reality]")
        .nth(1)
        .expect("VLESS Reality section exists");
    section
        .lines()
        .find_map(|line| {
            line.strip_prefix("uuid = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .expect("VLESS proxy credential is available")
        .to_owned()
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
    let mut answers = vec![String::new(); 15];
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
    let mut answers = vec![String::new(); 16];
    answers[13] = "America/New_York".into();
    answers[14] = "anchored-month".into();
    answers[15] = "2024-11-03T01:30".into();
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

    let mut answers = vec![String::new(); 15];
    answers[13] = "Asia/Tokyo".into();
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
    let answers = [
        "",
        "sub.example.test",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "www.cloudflare.com",
        "",
        "",
        "",
        "y",
    ];
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
    assert!(config.contains("accounting_timezone = \"UTC\""));
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
    let mut answers = vec![String::new(); 15];
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
