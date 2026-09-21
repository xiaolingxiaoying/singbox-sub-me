//! Shared console prompts. These are used by both the non-interactive
//! install path and the interactive menu, so they live apart from `cli::menu`.

use crate::cli::args::CliManagedProtocol;
use std::io::{self, IsTerminal, Write};

pub(crate) fn select_protocols(
    disabled: &[CliManagedProtocol],
) -> Result<Vec<sbctl::config::ManagedProtocol>, sbctl::config::ConfigError> {
    let defaults = [
        CliManagedProtocol::VlessReality,
        CliManagedProtocol::VmessWebsocket,
        CliManagedProtocol::Hysteria2,
        CliManagedProtocol::Tuic,
        CliManagedProtocol::Anytls,
    ];
    let mut selected = Vec::new();
    for protocol in defaults {
        let disabled_by_flag = disabled
            .iter()
            .any(|disabled| std::mem::discriminant(disabled) == std::mem::discriminant(&protocol));
        if !disabled_by_flag && (!io::stdin().is_terminal() || confirm_protocol(&protocol)?) {
            selected.push(protocol.into());
        }
    }
    Ok(selected)
}

pub(crate) fn protocol_ports(
    vless_reality: Option<u16>,
    vmess_websocket: Option<u16>,
    hysteria2: Option<u16>,
    tuic: Option<u16>,
    anytls: Option<u16>,
) -> sbctl::config::ProtocolPorts {
    sbctl::config::ProtocolPorts {
        vless_reality,
        vmess_websocket,
        hysteria2,
        tuic,
        anytls,
    }
}

fn confirm_protocol(protocol: &CliManagedProtocol) -> Result<bool, sbctl::config::ConfigError> {
    let name = match protocol {
        CliManagedProtocol::VlessReality => "vless-reality",
        CliManagedProtocol::VmessWebsocket => "vmess-websocket",
        CliManagedProtocol::Hysteria2 => "hysteria2",
        CliManagedProtocol::Tuic => "tuic",
        CliManagedProtocol::Anytls => "anytls",
    };
    print!("Enable {name} [Y/n]: ");
    io::stdout()
        .flush()
        .map_err(sbctl::config::ConfigError::Storage)?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(sbctl::config::ConfigError::Storage)?;
    Ok(!matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "n" | "no"
    ))
}

pub(crate) fn required_install_value(
    value: Option<String>,
    label: &str,
) -> Result<String, sbctl::config::ConfigError> {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        return Ok(value);
    }
    print!("{label}: ");
    io::stdout()
        .flush()
        .map_err(sbctl::config::ConfigError::Storage)?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(sbctl::config::ConfigError::Storage)?;
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value).ok_or_else(|| {
        sbctl::config::ConfigError::StateContent(format!(
            "{label} is required; supply it with the matching --flag or run interactively"
        ))
    })
}

pub(crate) struct ConsolePrompts;

impl sbctl::wizard::Prompts for ConsolePrompts {
    fn ask(&mut self, label: &str, default: Option<&str>) -> io::Result<String> {
        print!("{label}");
        if let Some(default) = default {
            print!(" [{}]", default);
        }
        print!(": ");
        io::stdout().flush()?;
        let mut answer = String::new();
        let read = io::stdin().read_line(&mut answer)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "wizard input ended before the prompt was answered",
            ));
        }
        Ok(answer.trim().to_owned())
    }

    fn report(&mut self, message: &str) {
        println!("{message}");
    }

    fn confirm(&mut self, question: &str, default: bool) -> io::Result<bool> {
        loop {
            print!("{question} [{}]: ", if default { "Y/n" } else { "y/N" });
            io::stdout().flush()?;
            let mut answer = String::new();
            let read = io::stdin().read_line(&mut answer)?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "wizard input ended before the confirmation was answered",
                ));
            }
            match answer.trim().to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => eprintln!("请输入 y 或 n"),
            }
        }
    }
}
