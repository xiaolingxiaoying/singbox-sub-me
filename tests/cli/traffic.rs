//! Traffic accounting: the VPS counters behind `traffic` and `status`, the
//! `traffic set-used` guards, and the anchored reset periods.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

use crate::fixture::{initialize_traffic_fixture, run_traffic_set_used, write_traffic_fixture};

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
