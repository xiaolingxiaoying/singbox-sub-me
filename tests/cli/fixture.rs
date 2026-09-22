//! The shared on-disk fixtures for the `cli` integration suite: temporary
//! hosts, release manifests signed and unsigned, fake systemctl, certbot and
//! sysctl binaries, and the helpers that seed a deployment under test.

use assert_cmd::Command;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

pub(crate) fn initialize_update_fixture(fixture: &TempDir) {
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

pub(crate) fn write_managed_file(fixture: &TempDir, relative: &str, contents: &[u8]) {
    let path = fixture.path().join(relative);
    fs::create_dir_all(path.parent().expect("managed path has a parent"))
        .expect("managed directory is created");
    fs::write(path, contents).expect("managed file is written");
}

pub(crate) fn write_systemctl_fixture(fixture: &TempDir, succeeds: bool) {
    let _ = command_fixture(fixture, "usr/bin/systemctl", succeeds, &[]);
}

/// Writes an executable host command that exits with the given status and,
/// when non-empty, emits `stderr_text` so diagnostics can be asserted. Only
/// its Unix callers exist, so the fixture itself is Unix-only.
#[cfg(unix)]
pub(crate) fn write_command_fixture(fixture: &TempDir, name: &str, status: u8, stderr_text: &str) {
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

/// Persists a Direct subscription configuration without starting services.
pub(crate) fn seed_direct_config(fixture: &TempDir) {
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
pub(crate) fn seed_live_certificate(fixture: &TempDir, names: &[&str]) {
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

pub(crate) fn command_fixture(
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
pub(crate) fn write_release_manifest(path: &std::path::Path, sbctl: &[u8], sing_box: &[u8]) {
    write_signed_release_manifest(path, "0.1.1", sbctl, "1.12.0", sing_box, "1.12.0", "1.12.0");
}

pub(crate) fn write_signed_release_manifest(
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
pub(crate) fn write_manifest_spec(
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
pub(crate) fn sign_release_manifest(unsigned: &std::path::Path, signed: &std::path::Path) {
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
pub(crate) fn corrupt_manifest_signature(path: &std::path::Path) {
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
pub(crate) fn write_lib_signed_manifest(
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
pub(crate) fn write_unsigned_release_manifest(path: &std::path::Path, sing_box: &[u8]) {
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

pub(crate) fn filesystem_snapshot(root: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
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

pub(crate) fn sing_box_check_fixture(
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

pub(crate) fn http_get(port: u16, path: &str) -> String {
    http_request("GET", port, path)
}

pub(crate) fn http_request(method: &str, port: u16, path: &str) -> String {
    // The service is spawned, not yet listening, when the first request fires.
    // The old 50 x 10ms budget was enough on an idle machine and was not: under
    // a loaded CI container the process can take longer than half a second to
    // reach its accept loop, and the suite failed with a panic that looked like
    // a product bug. Only the connect is retried, so a real 404/503 still
    // arrives as soon as the server is up.
    let mut attempts = 0;
    let mut stream = loop {
        attempts += 1;
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(_) if attempts < 200 => thread::sleep(Duration::from_millis(25)),
            Err(error) => panic!(
                "subscription service on port {port} never accepted a connection after {attempts} attempts: {error}"
            ),
        }
    };
    stream
        .write_all(format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .expect("request is sent");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response is readable");
    response
}

pub(crate) fn initialize_traffic_fixture(fixture: &TempDir) {
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

pub(crate) fn run_traffic_set_used(fixture: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
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

pub(crate) fn seed_service_accounts(fixture: &TempDir) {
    write_managed_file(
        fixture,
        "etc/passwd",
        b"sbctl:x:999:999::/nonexistent:/usr/sbin/nologin\nsing-box:x:998:998::/nonexistent:/usr/sbin/nologin\n",
    );
}

/// A systemctl stub that starts units successfully but reports every unit as
/// inactive, so the install health check phase fails after a successful start.
pub(crate) fn write_systemctl_health_failing_fixture(_fixture: &TempDir) {
    #[cfg(unix)]
    {
        let fixture = _fixture;
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

/// Why `serve` refuses a Direct bind: production hosts name the missing
/// socket unit, other platforms name the unsupported socket activation.
pub(crate) fn refusal_message() -> &'static str {
    #[cfg(unix)]
    {
        "requires sbctl-http.socket"
    }
    #[cfg(not(unix))]
    {
        "only supported on Unix systemd hosts"
    }
}

pub(crate) fn initialize_uninstall_fixture(fixture: &TempDir) {
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

pub(crate) fn supported_systemd_host() -> TempDir {
    let fixture = TempDir::new().expect("temporary root is created");
    write_os_release(&fixture, "ID=debian\nVERSION_ID=12\n");
    fs::create_dir_all(fixture.path().join("run/systemd/system"))
        .expect("systemd runtime directory is created");
    fixture
}

pub(crate) fn write_os_release(fixture: &TempDir, contents: &str) {
    let path = fixture.path().join("etc/os-release");
    fs::create_dir_all(path.parent().expect("os-release has a parent"))
        .expect("etc directory is created");
    fs::write(path, contents).expect("os-release is written");
}

pub(crate) fn write_traffic_fixture(fixture: &TempDir, rx: u64, tx: u64, boot_id: &str) {
    let statistics = fixture.path().join("sys/class/net/ens3/statistics");
    fs::create_dir_all(&statistics).expect("statistics directory is created");
    fs::write(statistics.join("rx_bytes"), rx.to_string()).expect("RX counter is written");
    fs::write(statistics.join("tx_bytes"), tx.to_string()).expect("TX counter is written");
    let boot_path = fixture.path().join("proc/sys/kernel/random/boot_id");
    fs::create_dir_all(boot_path.parent().expect("boot ID has a parent"))
        .expect("boot ID directory is created");
    fs::write(boot_path, boot_id).expect("boot ID is written");
}

pub(crate) fn spawn_sbctl_serve(
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

pub(crate) fn initialize_ip_fallback_subscription(fixture: &TempDir, port: u16) -> String {
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

pub(crate) fn free_high_tcp_port() -> u16 {
    // Binding an ephemeral port and releasing it does not reserve it: the OS
    // hands the same number to the next test in the same run, and two servers
    // then answer each other's clients — one saw ConnectionReset, another a 404
    // from a subscription it never asked for. Ports are therefore never handed
    // out twice inside one test process.
    static TAKEN: std::sync::OnceLock<Mutex<HashSet<u16>>> = std::sync::OnceLock::new();
    let taken = TAKEN.get_or_init(|| Mutex::new(HashSet::new()));
    loop {
        let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port is available");
        let port = listener
            .local_addr()
            .expect("ephemeral listener has an address")
            .port();
        drop(listener);
        if port < 10000 {
            continue;
        }
        let mut guard = taken.lock().expect("port bookkeeping is not poisoned");
        if guard.insert(port) {
            return port;
        }
    }
}

pub(crate) fn assert_existing_deployment_is_preserved(
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

pub(crate) fn read_subscription_credential(fixture: &TempDir) -> String {
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

pub(crate) fn read_vless_uuid(fixture: &TempDir) -> String {
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
