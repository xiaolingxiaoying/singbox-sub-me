use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{ConfigError, DeploymentConfig, DeploymentStore};

const SING_BOX_UNIT: &str = "etc/systemd/system/sing-box.service";
const SBCTL_UNIT: &str = "etc/systemd/system/sbctl.service";
const SBCTL_HTTP_SOCKET: &str = "etc/systemd/system/sbctl-http.socket";
const ACCOUNTING_RESET_UNIT: &str = "etc/systemd/system/sbctl-accounting-reset.service";
const ACCOUNTING_RESET_TIMER: &str = "etc/systemd/system/sbctl-accounting-reset.timer";
const OWNERSHIP_MARKER: &str = "var/lib/sbctl/ownership";
const SING_BOX_UNIT_MARKER: &str = "Description=sing-box data plane managed by sbctl";
const SBCTL_UNIT_MARKER: &str = "Description=sbctl private subscription service";
const SBCTL_HTTP_SOCKET_MARKER: &str = "Description=sbctl Direct HTTPS public listeners";
const ACCOUNTING_RESET_MARKER: &str = "Description=sbctl accounting period reset";
const CERTBOT_DEPLOY_HOOK: &str =
    "etc/letsencrypt/renewal-hooks/deploy/sbctl-certificate-deploy-hook";
const CERTBOT_DEPLOY_HOOK_MARKER: &str = "sbctl-managed Direct HTTPS certificate deploy hook";
const CERTIFICATE_GROUP: &str = crate::certificate::CERTIFICATE_GROUP;

const BACKED_UP_PATHS: &[&str] = &[
    "etc/sbctl/config.toml",
    OWNERSHIP_MARKER,
    "etc/sing-box/config.json",
    "var/lib/sbctl/state.json",
    "var/lib/sbctl/artifacts/sing-box-server.json",
    "var/lib/sbctl/artifacts/subscription-sing-box.json",
    "var/lib/sbctl/artifacts/subscription-clash.yaml",
    "var/lib/sbctl/artifacts/subscription-uri.txt",
    CERTBOT_DEPLOY_HOOK,
];

fn sing_box_unit() -> &'static str {
    "[Unit]\nDescription=sing-box data plane managed by sbctl\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser=sing-box\nGroup=sing-box\nExecStart=/usr/local/bin/sing-box run -c /etc/sing-box/config.json\nRestart=on-failure\nRestartSec=2\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=true\n\n[Install]\nWantedBy=multi-user.target\n"
}

/// Direct subscription mode runs under systemd socket activation: the
/// `sbctl-http.socket` unit owns public TCP 80/443 and passes the listeners to
/// the non-root sbctl service. External-proxy and IP-fallback modes serve only
/// high ports and never install this socket.
fn sbctl_unit(direct: bool) -> String {
    let socket_dependency = if direct {
        "\nRequires=sbctl-http.socket\nAfter=sbctl-http.socket\nSockets=sbctl-http.socket"
    } else {
        ""
    };
    format!(
        "[Unit]\nDescription=sbctl private subscription service\nAfter=network-online.target\nWants=network-online.target{socket_dependency}\n\n[Service]\nType=simple\nUser=sbctl\nGroup=sbctl\nExecStart=/usr/local/bin/sbctl serve\nRestart=on-failure\nRestartSec=2\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nProtectHome=true\nReadWritePaths=/var/lib/sbctl\n\n[Install]\nWantedBy=multi-user.target\n"
    )
}

const SBCTL_HTTP_SOCKET_CONTENTS: &str = "[Unit]\nDescription=sbctl Direct HTTPS public listeners\n\n[Socket]\nListenStream=80\nListenStream=443\nService=sbctl.service\nAccept=no\n\n[Install]\nWantedBy=sockets.target\n";

/// The Certbot renewal-hook script. Certbot runs it after every renewal in
/// Direct subscription mode; it re-validates the certificate and re-pins it so
/// the next TLS handshake serves the new certificate. A failure keeps Certbot's
/// previous certificate.
fn certbot_deploy_hook() -> &'static str {
    "#!/bin/sh\n# sbctl-managed Direct HTTPS certificate deploy hook\nset -eu\nexec /usr/local/bin/sbctl certificate verify\n"
}

pub fn install_units(
    store: &DeploymentStore,
    server_config: &str,
    direct: bool,
) -> Result<(), ConfigError> {
    let mut units = vec![
        SING_BOX_UNIT,
        SBCTL_UNIT,
        ACCOUNTING_RESET_UNIT,
        ACCOUNTING_RESET_TIMER,
    ];
    if direct {
        units.push(SBCTL_HTTP_SOCKET);
        units.push(CERTBOT_DEPLOY_HOOK);
    }
    for unit in units {
        if store.root().join(unit).exists() {
            return Err(ConfigError::AlreadyExists);
        }
    }
    store.write_active_sing_box_config(server_config.as_bytes())?;
    write_unit(store.root(), SING_BOX_UNIT, sing_box_unit())?;
    write_unit(store.root(), SBCTL_UNIT, &sbctl_unit(direct))?;
    write_unit(
        store.root(),
        ACCOUNTING_RESET_UNIT,
        "[Unit]\nDescription=sbctl accounting period reset task\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=oneshot\nUser=sbctl\nGroup=sbctl\nExecStart=/usr/local/bin/sbctl accounting-reset\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nReadWritePaths=/var/lib/sbctl\n",
    )?;
    write_unit(
        store.root(),
        ACCOUNTING_RESET_TIMER,
        "[Unit]\nDescription=sbctl accounting period reset timer\n\n[Timer]\nOnCalendar=minutely\nPersistent=true\nUnit=sbctl-accounting-reset.service\n\n[Install]\nWantedBy=timers.target\n",
    )?;
    if direct {
        write_unit(store.root(), SBCTL_HTTP_SOCKET, SBCTL_HTTP_SOCKET_CONTENTS)?;
        write_unit(store.root(), CERTBOT_DEPLOY_HOOK, certbot_deploy_hook())?;
        set_executable(&store.root().join(CERTBOT_DEPLOY_HOOK))?;
    }
    Ok(())
}

/// The final commit point of a successful installation. The ownership marker is
/// written only after the complete transaction — download verification, accounts
/// and directories, configuration and artifacts, units, daemon reload, startup,
/// and the health check — has succeeded, so a failed install never leaves a
/// marker that would make an Existing deployment look sbctl-managed.
pub fn write_ownership_marker(store: &DeploymentStore) -> Result<(), ConfigError> {
    store.write_relative_locked(OWNERSHIP_MARKER, b"sbctl-managed-v1\n")
}

/// Copies the binary that successfully validated the generated configuration to
/// the exact path used by the managed service.
pub fn install_checked_sing_box(root: &Path, candidate: &Path) -> Result<(), ConfigError> {
    let destination = root.join("usr/local/bin/sing-box");
    let parent = destination.parent().expect("binary path has a parent");
    fs::create_dir_all(parent)?;
    fs::copy(candidate, &destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub fn restart_sing_box_service(root: &Path) -> Result<(), String> {
    systemctl(root, &["restart", "sing-box.service"])?;
    systemctl(root, &["is-active", "--quiet", "sing-box.service"])
}

pub fn remove_managed_sing_box(root: &Path) -> Result<(), String> {
    if !unit_has_marker(root, SING_BOX_UNIT, SING_BOX_UNIT_MARKER)? {
        return Err("sing-box is not an sbctl-managed deployment".to_owned());
    }
    systemctl(root, &["disable", "--now", "sing-box.service"])?;
    remove_file_if_present(&root.join(SING_BOX_UNIT))?;
    remove_file_if_present(&root.join("usr/local/bin/sing-box"))?;
    systemctl(root, &["daemon-reload"])
}

pub fn start_services(root: &Path, direct: bool) -> Result<(), String> {
    ensure_daemon_accounts(root)?;
    if direct {
        ensure_certificate_group(root)?;
    }
    prepare_daemon_storage(root, direct)?;
    systemctl(root, &["daemon-reload"])?;
    let mut arguments = vec!["enable", "--now"];
    arguments.extend(managed_units(direct));
    systemctl(root, &arguments)
}

/// The units an installation transaction enables. Direct subscription mode
/// additionally owns the socket unit that holds public TCP 80/443.
fn managed_units(direct: bool) -> Vec<&'static str> {
    let mut units = vec![
        "sing-box.service",
        "sbctl.service",
        "sbctl-accounting-reset.timer",
    ];
    if direct {
        units.push("sbctl-http.socket");
    }
    units
}

/// The health check phase of an installation: verifies that every unit the
/// transaction enabled reports active. The ownership marker is written only
/// after this passes, so a unit that starts but immediately fails keeps the
/// installation rolled back instead of leaving a misleading deployment.
pub fn check_service_health(root: &Path, direct: bool) -> Result<(), String> {
    for unit in managed_units(direct) {
        systemctl(root, &["is-active", "--quiet", unit])?;
    }
    Ok(())
}

/// Removes only files created by a failed fresh installation. Preflight has
/// already established that no sing-box deployment existed at these paths.
pub fn rollback_fresh_installation(root: &Path) {
    let _ = systemctl(
        root,
        &[
            "disable",
            "--now",
            "sbctl-http.socket",
            "sbctl-accounting-reset.timer",
            "sbctl.service",
            "sing-box.service",
        ],
    );
    let _ = systemctl(root, &["daemon-reload"]);
    for relative in [
        "etc/systemd/system/sbctl-http.socket",
        "etc/systemd/system/sbctl.service",
        "etc/systemd/system/sing-box.service",
        "etc/systemd/system/sbctl-accounting-reset.service",
        "etc/systemd/system/sbctl-accounting-reset.timer",
        "etc/sing-box/config.json",
        "usr/local/bin/sing-box",
        "etc/sbctl/config.toml",
        CERTBOT_DEPLOY_HOOK,
        OWNERSHIP_MARKER,
        "var/lib/sbctl/artifacts/sing-box-server.json",
        "var/lib/sbctl/artifacts/subscription-sing-box.json",
        "var/lib/sbctl/artifacts/subscription-clash.yaml",
        "var/lib/sbctl/artifacts/subscription-uri.txt",
    ] {
        let _ = fs::remove_file(root.join(relative));
    }
}

/// Removes only paths created by sbctl. A normal uninstall leaves persistent
/// data in place and first makes a root-readable backup; --purge removes the
/// explicitly owned configuration and state instead.
pub fn uninstall(root: &Path, purge: bool) -> Result<Option<std::path::PathBuf>, String> {
    if !root.join("etc/sbctl/config.toml").is_file() || !root.join(OWNERSHIP_MARKER).is_file() {
        return Err("no sbctl-managed deployment configuration was found".to_owned());
    }

    let backup = (!purge).then(|| backup_persistent_data(root)).transpose()?;
    let sbctl_unit_owned = unit_has_marker(root, SBCTL_UNIT, SBCTL_UNIT_MARKER)?;
    let sing_box_unit_owned = unit_has_marker(root, SING_BOX_UNIT, SING_BOX_UNIT_MARKER)?;
    let reset_timer_owned = unit_has_marker(root, ACCOUNTING_RESET_TIMER, ACCOUNTING_RESET_MARKER)?;
    let http_socket_owned = unit_has_marker(root, SBCTL_HTTP_SOCKET, SBCTL_HTTP_SOCKET_MARKER)?;
    let deploy_hook_owned = unit_has_marker(root, CERTBOT_DEPLOY_HOOK, CERTBOT_DEPLOY_HOOK_MARKER)?;
    let sing_box_config_owned = sing_box_unit_owned || !root.join(SING_BOX_UNIT).exists();
    if sbctl_unit_owned {
        systemctl(root, &["disable", "--now", "sbctl.service"])?;
    }
    if sing_box_unit_owned {
        systemctl(root, &["disable", "--now", "sing-box.service"])?;
    }
    if reset_timer_owned {
        systemctl(root, &["disable", "--now", "sbctl-accounting-reset.timer"])?;
    }
    if http_socket_owned {
        systemctl(root, &["disable", "--now", "sbctl-http.socket"])?;
    }

    if sbctl_unit_owned {
        remove_file_if_present(&root.join(SBCTL_UNIT))?;
        remove_file_if_present(&root.join("usr/local/bin/sbctl"))?;
        remove_file_if_present(&root.join("usr/local/bin/ly"))?;
    }
    if sing_box_unit_owned {
        remove_file_if_present(&root.join(SING_BOX_UNIT))?;
        remove_file_if_present(&root.join("usr/local/bin/sing-box"))?;
    }
    if reset_timer_owned {
        remove_file_if_present(&root.join(ACCOUNTING_RESET_TIMER))?;
        remove_file_if_present(&root.join(ACCOUNTING_RESET_UNIT))?;
    }
    if http_socket_owned {
        remove_file_if_present(&root.join(SBCTL_HTTP_SOCKET))?;
    }
    if deploy_hook_owned {
        remove_file_if_present(&root.join(CERTBOT_DEPLOY_HOOK))?;
    }
    if sbctl_unit_owned || sing_box_unit_owned || reset_timer_owned || http_socket_owned {
        systemctl(root, &["daemon-reload"])?;
    }

    if purge {
        // A prior non-purge uninstall removes the unit but deliberately keeps
        // persistent configuration. Conversely, a replacement non-sbctl unit
        // signals a hand-managed deployment and must leave its config alone.
        if sing_box_config_owned {
            remove_file_if_present(&root.join("etc/sing-box/config.json"))?;
            remove_empty_directory_if_present(&root.join("etc/sing-box"))?;
        }
        remove_file_if_present(&root.join("etc/sbctl/config.toml"))?;
        remove_directory_if_present(&root.join("var/lib/sbctl"))?;
    }
    Ok(backup)
}

fn unit_has_marker(root: &Path, relative: &str, marker: &str) -> Result<bool, String> {
    match fs::read_to_string(root.join(relative)) {
        Ok(contents) => Ok(contents.contains(marker)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "could not inspect {}: {error}",
            root.join(relative).display()
        )),
    }
}

pub fn restart_services(root: &Path) -> Result<(), String> {
    systemctl(root, &["restart", "sing-box.service", "sbctl.service"])?;
    for unit in ["sing-box.service", "sbctl.service"] {
        systemctl(root, &["is-active", "--quiet", unit])?;
    }
    Ok(())
}

pub fn service_status_entries(root: &Path) -> Vec<(&'static str, String)> {
    [
        "sing-box.service",
        "sbctl.service",
        "sbctl-http.socket",
        "sbctl-accounting-reset.timer",
    ]
    .into_iter()
    .map(|unit| {
        let state = match systemctl(root, &["is-active", "--quiet", unit]) {
            Ok(()) => "active".to_owned(),
            Err(_) => "inactive or unavailable".to_owned(),
        };
        (unit, state)
    })
    .collect()
}

pub fn service_status(root: &Path) -> String {
    service_status_entries(root)
        .into_iter()
        .map(|(unit, state)| format!("{unit}: {state}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn required_firewall_ports(config: &DeploymentConfig) -> Vec<String> {
    let mut ports = Vec::new();
    if matches!(
        config.subscription_mode,
        crate::config::SubscriptionMode::Direct
    ) {
        ports.extend(["TCP 80 (ACME)", "TCP 443 (subscription)"].map(str::to_owned));
    }
    if let Some(node) = &config.vless_reality {
        ports.push(format!("TCP {} (VLESS Reality)", node.listen_port));
    }
    if let Some(node) = &config.vmess_websocket {
        ports.push(format!("TCP {} (VMess WebSocket)", node.listen_port));
    }
    if let Some(node) = &config.hysteria2 {
        ports.push(format!("UDP {} (Hysteria2)", node.listen_port));
    }
    if let Some(node) = &config.tuic {
        ports.push(format!("UDP {} (TUIC v5)", node.listen_port));
    }
    if let Some(node) = &config.anytls {
        ports.push(format!("TCP {} (AnyTLS)", node.listen_port));
    }
    ports
}

pub fn enabled_nodes(config: &DeploymentConfig) -> String {
    crate::canonical::nodes(config)
        .iter()
        .map(|node| {
            format!(
                "{}: {} {}",
                node.protocol(),
                node.transport().to_uppercase(),
                node.port()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_unit(root: &Path, relative_path: &str, contents: &str) -> Result<(), ConfigError> {
    let path = root.join(relative_path);
    let parent = path.parent().expect("unit path has parent");
    fs::create_dir_all(parent)?;
    fs::write(path, contents)?;
    Ok(())
}

fn set_executable(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn backup_persistent_data(root: &Path) -> Result<std::path::PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let destination = root.join("var/backups/sbctl").join(timestamp.to_string());
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    set_private_directory_permissions(&destination)?;
    for relative in BACKED_UP_PATHS {
        let source = root.join(relative);
        if !source.is_file() {
            continue;
        }
        let target = destination.join(relative);
        let parent = target
            .parent()
            .expect("backup target for a managed file has a parent");
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        set_private_directory_permissions(parent)?;
        fs::copy(&source, &target).map_err(|error| error.to_string())?;
        set_root_readable_file_permissions(&target)?;
    }
    Ok(destination)
}

fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not remove {}: {error}", path.display())),
    }
}

fn remove_directory_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not remove {}: {error}", path.display())),
    }
}

fn remove_empty_directory_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(format!("could not remove {}: {error}", path.display())),
    }
}

fn set_private_directory_permissions(path: &Path) -> Result<(), String> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn set_root_readable_file_permissions(path: &Path) -> Result<(), String> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn systemctl(root: &Path, args: &[&str]) -> Result<(), String> {
    let command = root.join("usr/bin/systemctl");
    #[cfg(windows)]
    let command = if command.is_file() {
        command
    } else {
        root.join("usr/bin/systemctl.cmd")
    };
    let program = if command.is_file() {
        command
    } else {
        "systemctl".into()
    };
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("systemctl {} exited with {status}", args.join(" ")))
}

fn ensure_daemon_accounts(root: &Path) -> Result<(), String> {
    for account in ["sbctl", "sing-box"] {
        ensure_daemon_account(root, account)?;
    }
    Ok(())
}

fn ensure_daemon_account(root: &Path, account: &str) -> Result<(), String> {
    let passwd = root.join("etc/passwd");
    if fs::read_to_string(&passwd).ok().is_some_and(|contents| {
        contents
            .lines()
            .any(|line| line.starts_with(&format!("{account}:")))
    }) {
        return Ok(());
    }
    let candidate = root.join("usr/sbin/useradd");
    let program = if candidate.is_file() {
        candidate
    } else {
        "useradd".into()
    };
    let status = Command::new(program)
        .args([
            "--system",
            "--no-create-home",
            "--shell",
            "/usr/sbin/nologin",
            account,
        ])
        .status()
        .map_err(|error| format!("could not create {account} service account: {error}"))?;
    status.success().then_some(()).ok_or_else(|| {
        format!("could not create {account} service account: useradd exited with {status}")
    })
}

/// Creates the shared certificate group and adds both service accounts to it.
/// The deploy hook stores the pinned private key as `root:{group} 0640`, so
/// only the sbctl daemon and the sing-box data plane can read it while the
/// subscription credential and node credentials stay private to their owners.
fn ensure_certificate_group(root: &Path) -> Result<(), String> {
    let group_file = root.join("etc/group");
    let groups = fs::read_to_string(&group_file).unwrap_or_default();
    let group_exists = groups
        .lines()
        .any(|line| line.starts_with(&format!("{CERTIFICATE_GROUP}:")));
    if !group_exists {
        run_account_command(root, "groupadd", &["--system", CERTIFICATE_GROUP])?;
    }
    for account in ["sbctl", "sing-box"] {
        let is_member = groups.lines().any(|line| {
            line.starts_with(&format!("{CERTIFICATE_GROUP}:"))
                && line
                    .split(':')
                    .nth(3)
                    .unwrap_or_default()
                    .split(',')
                    .any(|member| member == account)
        });
        if !is_member {
            run_account_command(root, "usermod", &["-aG", CERTIFICATE_GROUP, account])?;
        }
    }
    Ok(())
}

fn run_account_command(root: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let candidate = root.join("usr/sbin").join(program);
    let executable = if candidate.is_file() {
        candidate
    } else {
        program.into()
    };
    let status = Command::new(executable)
        .args(args)
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("{program} {} exited with {status}", args.join(" ")))
}

pub fn prepare_daemon_storage(root: &Path, direct: bool) -> Result<(), String> {
    // Fixture roots intentionally do not have real passwd entries or ownership
    // metadata. Only change ownership when operating on the live host root.
    if root != Path::new("/") {
        return Ok(());
    }
    let status = Command::new("chown")
        .args(["-R", "sbctl:sbctl", "/etc/sbctl", "/var/lib/sbctl"])
        .status()
        .map_err(|error| format!("could not prepare sbctl service storage: {error}"))?;
    if !status.success() {
        return Err(format!(
            "could not prepare sbctl service storage: chown exited with {status}"
        ));
    }
    // The generated sing-box configuration is written root-only (0600). The
    // sing-box data plane runs as its own account, so the file must be owned
    // and readable by that account while staying private to the host.
    let status = Command::new("chown")
        .args(["sing-box:sing-box", "/etc/sing-box/config.json"])
        .status()
        .map_err(|error| format!("could not grant sing-box service config access: {error}"))?;
    if !status.success() {
        return Err(format!(
            "could not grant sing-box service config access: chown exited with {status}"
        ));
    }
    let status = Command::new("chmod")
        .args(["0640", "/etc/sing-box/config.json"])
        .status()
        .map_err(|error| format!("could not restrict sing-box service config: {error}"))?;
    status.success().then_some(()).ok_or_else(|| {
        format!("could not restrict sing-box service config: chmod exited with {status}")
    })?;
    if direct {
        grant_certificate_storage(root)?;
    } else {
        grant_self_signed_certificate_access(root)?;
    }
    Ok(())
}

/// In self-signed certificate mode the sing-box data plane reads the pinned
/// certificate under /var/lib/sbctl/certificates. Grant the sing-box service
/// account traversal of the storage root and read access to the pinned
/// certificate copy, keeping the private key group-readable only.
fn grant_self_signed_certificate_access(root: &Path) -> Result<(), String> {
    let status = Command::new("chmod")
        .args(["0755", "/var/lib/sbctl"])
        .status()
        .map_err(|error| format!("could not grant certificate traversal: {error}"))?;
    if !status.success() {
        return Err(format!(
            "could not grant certificate traversal: chmod exited with {status}"
        ));
    }
    let certificates = root.join("var/lib/sbctl/certificates");
    if certificates.is_dir() {
        let status = Command::new("chown")
            .args(["-R", "root:sing-box", &certificates.to_string_lossy()])
            .status()
            .map_err(|error| format!("could not grant certificate file access: {error}"))?;
        if !status.success() {
            return Err(format!(
                "could not grant certificate file access: chown exited with {status}"
            ));
        }
        let status = Command::new("find")
            .args([
                &certificates.to_string_lossy(),
                "-type",
                "d",
                "-exec",
                "chmod",
                "0755",
                "{}",
                "+",
            ])
            .status()
            .map_err(|error| format!("could not restrict certificate directories: {error}"))?;
        if !status.success() {
            return Err(format!(
                "could not restrict certificate directories: find exited with {status}"
            ));
        }
        let status = Command::new("find")
            .args([
                &certificates.to_string_lossy(),
                "-type",
                "f",
                "-exec",
                "chmod",
                "0640",
                "{}",
                "+",
            ])
            .status()
            .map_err(|error| format!("could not restrict certificate files: {error}"))?;
        if !status.success() {
            return Err(format!(
                "could not restrict certificate files: find exited with {status}"
            ));
        }
    }
    Ok(())
}

/// Both service accounts read the pinned certificate copy under
/// /var/lib/sbctl/certificates in Direct subscription mode. Grant the shared
/// certificate group traversal of the storage root while every sbctl-owned
/// state file stays 0600, so sing-box gains access to the certificate and
/// nothing else. External proxy mode never touches certificate storage.
fn grant_certificate_storage(root: &Path) -> Result<(), String> {
    let status = Command::new("chgrp")
        .args([CERTIFICATE_GROUP, "/var/lib/sbctl"])
        .status()
        .map_err(|error| format!("could not grant certificate storage access: {error}"))?;
    if !status.success() {
        return Err(format!(
            "could not grant certificate storage access: chgrp exited with {status}"
        ));
    }
    let status = Command::new("chmod")
        .args(["0750", "/var/lib/sbctl"])
        .status()
        .map_err(|error| format!("could not restrict certificate storage: {error}"))?;
    if !status.success() {
        return Err(format!(
            "could not restrict certificate storage: chmod exited with {status}"
        ));
    }
    let certificates = root.join(crate::config::CERTIFICATES_RELATIVE_PATH);
    if certificates.is_dir() {
        let status = Command::new("chgrp")
            .args(["-R", CERTIFICATE_GROUP, &certificates.to_string_lossy()])
            .status()
            .map_err(|error| format!("could not grant certificate copy access: {error}"))?;
        if !status.success() {
            return Err(format!(
                "could not grant certificate copy access: chgrp exited with {status}"
            ));
        }
    }
    Ok(())
}
