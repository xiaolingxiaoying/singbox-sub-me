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

#[cfg(unix)]
use crate::fixture::write_systemctl_restart_race_fixture;
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

#[cfg(unix)]
#[test]
fn official_sing_box_digest_mismatch_aborts_before_changing_the_host() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().expect("temporary root is created");
    initialize_update_fixture(&fixture);
    let curl = fixture.path().join("usr/bin/curl");
    fs::create_dir_all(curl.parent().expect("curl has a parent"))
        .expect("fake curl directory is created");
    fs::write(
        &curl,
        r##"#!/bin/sh
set -eu
output=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        *) url=$1; shift ;;
    esac
done
case "$url" in
    */releases/latest) printf '%s' '{"tag_name":"v1.14.1"}' > "$output" ;;
    */releases/tags/v1.14.1) printf '%s' '{"assets":[{"name":"sing-box-1.14.1-linux-amd64.tar.gz","digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}]}' > "$output" ;;
    */sing-box-1.14.1-linux-amd64.tar.gz) printf '%s' 'tampered archive' > "$output" ;;
    *) echo "unexpected URL: $url" >&2; exit 1 ;;
esac
"##,
    )
    .expect("fake curl is written");
    fs::set_permissions(&curl, fs::Permissions::from_mode(0o700)).expect("fake curl is executable");

    write_managed_file(&fixture, "usr/local/bin/sbctl", b"known-good sbctl");
    write_managed_file(&fixture, "usr/local/bin/sing-box", b"known-good sing-box");
    write_systemctl_fixture_recording_calls(&fixture);
    let before = filesystem_snapshot(fixture.path());
    let path = format!(
        "{}:{}",
        fixture.path().join("usr/bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sing-box",
            "update",
        ])
        .env("PATH", path)
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "official sing-box archive does not match GitHub's SHA-256 digest",
        ));

    assert_eq!(filesystem_snapshot(fixture.path()), before);
    assert!(!fixture.path().join(ROLLBACK_ROOT).exists());
    assert!(!fixture.path().join(".systemctl-calls").exists());
}

#[cfg(unix)]
#[test]
fn successful_signed_update_installs_both_candidates_and_keeps_a_rollback_point() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = TempDir::new().expect("temporary root is created");
    initialize_update_fixture(&fixture);
    let manifest = fixture.path().join("release-manifest.json");
    let candidate_sbctl = command_fixture(&fixture, "candidate-sbctl", true, &[]);
    let candidate_sing_box = sing_box_check_fixture(&fixture, true, &["vless"]);
    let candidate_sbctl_bytes = fs::read(&candidate_sbctl).expect("candidate sbctl is readable");
    let candidate_sing_box_bytes =
        fs::read(&candidate_sing_box).expect("candidate sing-box is readable");
    write_release_manifest(&manifest, &candidate_sbctl_bytes, &candidate_sing_box_bytes);

    let old_sbctl = b"known-good sbctl";
    let old_sing_box = b"known-good sing-box";
    write_managed_file(&fixture, "usr/local/bin/sbctl", old_sbctl);
    write_managed_file(&fixture, "usr/local/bin/sing-box", old_sing_box);
    write_managed_file(
        &fixture,
        "var/lib/sbctl/state.json",
        b"known-good accounting state",
    );
    let config_path = fixture.path().join("etc/sbctl/config.toml");
    let state_path = fixture.path().join("var/lib/sbctl/state.json");
    let server_artifact_path = fixture
        .path()
        .join("var/lib/sbctl/artifacts/sing-box-server.json");
    let old_config = fs::read(&config_path).expect("configuration is readable");
    let old_state = fs::read(&state_path).expect("accounting state is readable");
    let old_server_artifact = fs::read(&server_artifact_path).expect("server artifact is readable");
    write_systemctl_fixture_recording_calls(&fixture);

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "update",
            "--manifest",
            manifest.to_str().expect("manifest path is UTF-8"),
            "--sbctl-artifact",
            candidate_sbctl.to_str().expect("candidate path is UTF-8"),
            "--sing-box-artifact",
            candidate_sing_box
                .to_str()
                .expect("candidate path is UTF-8"),
        ])
        .assert()
        .success();

    let installed_sbctl = fixture.path().join("usr/local/bin/sbctl");
    let installed_sing_box = fixture.path().join("usr/local/bin/sing-box");
    assert_eq!(
        fs::read(&installed_sbctl).expect("updated sbctl is readable"),
        candidate_sbctl_bytes
    );
    assert_eq!(
        fs::read(&installed_sing_box).expect("updated sing-box is readable"),
        candidate_sing_box_bytes
    );
    assert_eq!(
        fs::read(&config_path).expect("configuration remains readable"),
        old_config
    );
    assert_eq!(
        fs::read(&state_path).expect("accounting state remains readable"),
        old_state
    );
    assert_eq!(
        fs::read(&server_artifact_path).expect("server artifact remains readable"),
        old_server_artifact
    );
    assert_eq!(
        fs::metadata(&installed_sbctl)
            .expect("updated sbctl metadata is readable")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(&installed_sing_box)
            .expect("updated sing-box metadata is readable")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );

    let rollback_root = fixture.path().join(ROLLBACK_ROOT);
    let rollback_point = fs::read_dir(&rollback_root)
        .expect("rollback directory is readable")
        .next()
        .expect("a successful update keeps a rollback point")
        .expect("rollback entry is readable")
        .path();
    assert_eq!(
        fs::read(rollback_point.join("usr/local/bin/sbctl")).expect("sbctl is backed up"),
        old_sbctl
    );
    assert_eq!(
        fs::read(rollback_point.join("usr/local/bin/sing-box")).expect("sing-box is backed up"),
        old_sing_box
    );
    let systemctl_calls = fs::read_to_string(fixture.path().join(".systemctl-calls"))
        .expect("service restart calls are recorded");
    assert!(systemctl_calls.contains("restart sing-box.service sbctl.service"));
    assert!(systemctl_calls.contains("is-active --quiet sing-box.service"));
    assert!(systemctl_calls.contains("is-active --quiet sbctl.service"));
}

#[cfg(unix)]
fn write_systemctl_fixture_recording_calls(fixture: &TempDir) {
    use std::os::unix::fs::PermissionsExt;

    let path = fixture.path().join("usr/bin/systemctl");
    fs::create_dir_all(path.parent().expect("systemctl has a parent"))
        .expect("systemctl directory is created");
    fs::write(
        &path,
        "#!/bin/sh\nroot=$(CDPATH= cd -- \"$(dirname -- \"$0\")/../..\" && pwd)\nprintf '%s\\n' \"$*\" >> \"$root/.systemctl-calls\"\nexit 0\n",
    )
    .expect("systemctl fixture is written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .expect("systemctl fixture is executable");
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

/// The VPS 2026-09-22 P0 regression: a candidate that passes `sing-box check`
/// but whose service exits immediately must fail the update, restore the
/// known-good binary, and observe the restored unit as stable. The systemctl
/// stub reports active once right after `restart` — the `Type=simple` fork
/// window — so a single `is-active` probe would commit the broken candidate
/// while `Restart=on-failure` loops it.
#[cfg(unix)]
#[test]
fn standalone_sing_box_update_rolls_back_a_candidate_that_crashes_on_start() {
    let fixture = TempDir::new().expect("temporary root is created");
    initialize_update_fixture(&fixture);
    let candidate = sing_box_check_fixture(&fixture, true, &[]);
    let mut contents = fs::read(&candidate).expect("candidate is readable");
    contents.extend_from_slice(b"\n# CRASHES-ON-RUN\n");
    fs::write(&candidate, &contents).expect("candidate is written");

    write_systemctl_restart_race_fixture(&fixture, "CRASHES-ON-RUN");
    let old_sing_box = b"known-good sing-box";
    write_managed_file(&fixture, "usr/local/bin/sing-box", old_sing_box);
    write_managed_file(&fixture, "usr/local/bin/sbctl", b"known-good sbctl");

    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "--root",
            fixture.path().to_str().expect("fixture path is UTF-8"),
            "sing-box",
            "update",
            "--artifact",
            candidate.to_str().expect("candidate path is UTF-8"),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("service health check failed"));

    assert_eq!(
        fs::read(fixture.path().join("usr/local/bin/sing-box")).expect("old sing-box is restored"),
        old_sing_box
    );
    assert!(fixture.path().join(ROLLBACK_ROOT).exists());
    assert_eq!(
        fs::read_to_string(fixture.path().join(".systemctl-unit-state"))
            .expect("the stub recorded the unit state"),
        "stable",
        "the rollback must restart the unit on the restored binary"
    );
    for probe in 0..3 {
        let status = std::process::Command::new(fixture.path().join("usr/bin/systemctl"))
            .args(["is-active", "--quiet", "sing-box.service"])
            .status()
            .expect("the stub runs");
        assert!(
            status.success(),
            "the restored unit must stay active (probe {probe})"
        );
    }
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
