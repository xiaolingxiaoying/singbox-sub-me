use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol};

const SING_BOX_UNIT: &str = "etc/systemd/system/sing-box.service";
const SBCTL_UNIT: &str = "etc/systemd/system/sbctl.service";
const ACCOUNTING_RESET_UNIT: &str = "etc/systemd/system/sbctl-accounting-reset.service";
const ACCOUNTING_RESET_TIMER: &str = "etc/systemd/system/sbctl-accounting-reset.timer";
const OWNERSHIP_MARKER: &str = "var/lib/sbctl/ownership";
const SING_BOX_UNIT_MARKER: &str = "Description=sing-box data plane managed by sbctl";
const SBCTL_UNIT_MARKER: &str = "Description=sbctl private subscription service";
const ACCOUNTING_RESET_MARKER: &str = "Description=sbctl accounting period reset";

const BACKED_UP_PATHS: &[&str] = &[
    "etc/sbctl/config.toml",
    OWNERSHIP_MARKER,
    "etc/sing-box/config.json",
    "var/lib/sbctl/state.json",
    "var/lib/sbctl/artifacts/sing-box-server.json",
    "var/lib/sbctl/artifacts/subscription-sing-box.json",
    "var/lib/sbctl/artifacts/subscription-clash.yaml",
    "var/lib/sbctl/artifacts/subscription-uri.txt",
];

pub fn install_units(store: &DeploymentStore, server_config: &str) -> Result<(), ConfigError> {
    for unit in [
        SING_BOX_UNIT,
        SBCTL_UNIT,
        ACCOUNTING_RESET_UNIT,
        ACCOUNTING_RESET_TIMER,
    ] {
        if store.root().join(unit).exists() {
            return Err(ConfigError::AlreadyExists);
        }
    }
    store.write_active_sing_box_config(server_config.as_bytes())?;
    write_unit(
        store.root(),
        SING_BOX_UNIT,
        "[Unit]\nDescription=sing-box data plane managed by sbctl\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nExecStart=/usr/local/bin/sing-box run -c /etc/sing-box/config.json\nRestart=on-failure\nRestartSec=2\nNoNewPrivileges=true\nPrivateTmp=true\n\n[Install]\nWantedBy=multi-user.target\n",
    )?;
    write_unit(
        store.root(),
        SBCTL_UNIT,
        "[Unit]\nDescription=sbctl private subscription service\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser=sbctl\nGroup=sbctl\nExecStart=/usr/local/bin/sbctl serve\nRestart=on-failure\nRestartSec=2\nNoNewPrivileges=true\nPrivateTmp=true\nProtectSystem=strict\nReadWritePaths=/var/lib/sbctl\n\n[Install]\nWantedBy=multi-user.target\n",
    )?;
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

pub fn start_services(root: &Path) -> Result<(), String> {
    ensure_daemon_account(root)?;
    prepare_daemon_storage(root)?;
    systemctl(root, &["daemon-reload"])?;
    systemctl(
        root,
        &[
            "enable",
            "--now",
            "sing-box.service",
            "sbctl.service",
            "sbctl-accounting-reset.timer",
        ],
    )
}

/// Removes only files created by a failed fresh installation. Preflight has
/// already established that no sing-box deployment existed at these paths.
pub fn rollback_fresh_installation(root: &Path) {
    let _ = systemctl(
        root,
        &[
            "disable",
            "--now",
            "sbctl-accounting-reset.timer",
            "sbctl.service",
            "sing-box.service",
        ],
    );
    let _ = systemctl(root, &["daemon-reload"]);
    for relative in [
        "etc/systemd/system/sbctl.service",
        "etc/systemd/system/sing-box.service",
        "etc/systemd/system/sbctl-accounting-reset.service",
        "etc/systemd/system/sbctl-accounting-reset.timer",
        "etc/sing-box/config.json",
        "usr/local/bin/sing-box",
        "etc/sbctl/config.toml",
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

    if sbctl_unit_owned {
        remove_file_if_present(&root.join(SBCTL_UNIT))?;
        remove_file_if_present(&root.join("usr/local/bin/sbctl"))?;
    }
    if sing_box_unit_owned {
        remove_file_if_present(&root.join(SING_BOX_UNIT))?;
        remove_file_if_present(&root.join("usr/local/bin/sing-box"))?;
    }
    if reset_timer_owned {
        remove_file_if_present(&root.join(ACCOUNTING_RESET_TIMER))?;
        remove_file_if_present(&root.join(ACCOUNTING_RESET_UNIT))?;
    }
    if sbctl_unit_owned || sing_box_unit_owned || reset_timer_owned {
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

pub fn service_status(root: &Path) -> String {
    [
        "sing-box.service",
        "sbctl.service",
        "sbctl-accounting-reset.timer",
    ]
    .into_iter()
    .map(
        |unit| match systemctl(root, &["is-active", "--quiet", unit]) {
            Ok(()) => format!("{unit}: active"),
            Err(_) => format!("{unit}: inactive or unavailable"),
        },
    )
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
    config
        .enabled_protocols
        .iter()
        .map(|protocol| match protocol {
            ManagedProtocol::VlessReality => format!(
                "vless-reality: TCP {}",
                config
                    .vless_reality
                    .as_ref()
                    .expect("validated")
                    .listen_port
            ),
            ManagedProtocol::VmessWebsocket => format!(
                "vmess-websocket: TCP {}",
                config
                    .vmess_websocket
                    .as_ref()
                    .expect("validated")
                    .listen_port
            ),
            ManagedProtocol::Hysteria2 => format!(
                "hysteria2: UDP {}",
                config.hysteria2.as_ref().expect("validated").listen_port
            ),
            ManagedProtocol::Tuic => format!(
                "tuic: UDP {}",
                config.tuic.as_ref().expect("validated").listen_port
            ),
            ManagedProtocol::Anytls => format!(
                "anytls: TCP {}",
                config.anytls.as_ref().expect("validated").listen_port
            ),
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

fn ensure_daemon_account(root: &Path) -> Result<(), String> {
    let passwd = root.join("etc/passwd");
    if fs::read_to_string(&passwd)
        .ok()
        .is_some_and(|contents| contents.lines().any(|line| line.starts_with("sbctl:")))
    {
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
            "sbctl",
        ])
        .status()
        .map_err(|error| format!("could not create sbctl service account: {error}"))?;
    status.success().then_some(()).ok_or_else(|| {
        format!("could not create sbctl service account: useradd exited with {status}")
    })
}

fn prepare_daemon_storage(root: &Path) -> Result<(), String> {
    // Fixture roots intentionally do not have real passwd entries or ownership
    // metadata. Only change ownership when operating on the live host root.
    if root != Path::new("/") {
        return Ok(());
    }
    let status = Command::new("chown")
        .args(["-R", "sbctl:sbctl", "/etc/sbctl", "/var/lib/sbctl"])
        .status()
        .map_err(|error| format!("could not prepare sbctl service storage: {error}"))?;
    status.success().then_some(()).ok_or_else(|| {
        format!("could not prepare sbctl service storage: chown exited with {status}")
    })
}
