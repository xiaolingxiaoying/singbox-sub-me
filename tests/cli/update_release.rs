//! The self-update release path: manifest verification, signature and
//! schema rejection, and the rollback behaviour of a failed update.

use assert_cmd::Command;
use predicates::prelude::*;
/// Taken from the implementation so a moved rollback location fails the suite
/// instead of quietly turning every "no rollback point" assertion into a tautology.
use sbctl::update::ROLLBACK_ROOT;
use sha2::{Digest, Sha256};
use std::fs;
use tempfile::TempDir;

use crate::fixture::{
    command_fixture, corrupt_manifest_signature, filesystem_snapshot, initialize_update_fixture,
    sing_box_check_fixture, write_lib_signed_manifest, write_managed_file, write_release_manifest,
    write_systemctl_fixture, write_unsigned_release_manifest,
};

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

    assert!(!fixture.path().join(ROLLBACK_ROOT).exists());
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
    let rollback_root = fixture.path().join(ROLLBACK_ROOT);
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
    assert!(!fixture.path().join(ROLLBACK_ROOT).exists());
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

    assert!(!fixture.path().join(ROLLBACK_ROOT).exists());
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

    assert!(!fixture.path().join(ROLLBACK_ROOT).exists());
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

    assert!(!fixture.path().join(ROLLBACK_ROOT).exists());
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
