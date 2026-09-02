use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::{ConfigError, DeploymentConfig, DeploymentStore, ManagedProtocol};

const SING_BOX_UNIT: &str = "etc/systemd/system/sing-box.service";
const SBCTL_UNIT: &str = "etc/systemd/system/sbctl.service";

pub fn install_units(store: &DeploymentStore, server_config: &str) -> Result<(), ConfigError> {
    for unit in [SING_BOX_UNIT, SBCTL_UNIT] {
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
    )
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

pub fn start_services(root: &Path) -> Result<(), String> {
    ensure_daemon_account(root)?;
    systemctl(root, &["daemon-reload"])?;
    systemctl(
        root,
        &["enable", "--now", "sing-box.service", "sbctl.service"],
    )
}

/// Removes only files created by a failed fresh installation. Preflight has
/// already established that no sing-box deployment existed at these paths.
pub fn rollback_fresh_installation(root: &Path) {
    let _ = systemctl(
        root,
        &["disable", "--now", "sbctl.service", "sing-box.service"],
    );
    let _ = systemctl(root, &["daemon-reload"]);
    for relative in [
        "etc/systemd/system/sbctl.service",
        "etc/systemd/system/sing-box.service",
        "etc/sing-box/config.json",
        "usr/local/bin/sing-box",
        "etc/sbctl/config.toml",
        "var/lib/sbctl/artifacts/sing-box-server.json",
        "var/lib/sbctl/artifacts/subscription-sing-box.json",
        "var/lib/sbctl/artifacts/subscription-clash.yaml",
        "var/lib/sbctl/artifacts/subscription-uri.txt",
    ] {
        let _ = fs::remove_file(root.join(relative));
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
    ["sing-box.service", "sbctl.service"]
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
