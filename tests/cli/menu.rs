//! The interactive menu entry point and the `--help` surface that lists the
//! safe commands.

use assert_cmd::Command;
use predicates::prelude::*;

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
fn install_help_exposes_the_guided_first_install_path() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["install", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--guided"));
}

#[test]
fn guided_install_refuses_individual_configuration_flags() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args([
            "install",
            "--guided",
            "--subscription-host",
            "sub.example.test",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "--guided 不能与单项安装配置参数同时使用",
        ));
}

#[test]
fn guided_install_refuses_a_separate_mode_argument() {
    Command::cargo_bin("sbctl")
        .expect("sbctl binary is built")
        .args(["install", "--guided", "--mode", "ip-fallback"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
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
