//! Interactive configuration wizard.
//!
//! The wizard reads an existing deployment, walks the administrator through
//! each field with the current value as the empty-input default, validates each
//! answer, shows a complete redacted summary, and only then produces the new
//! configuration for the caller to commit through the artifact/check/health
//! transaction. No deployment file is touched by this module: cancellation,
//! invalid input, and an unconfirmed summary all leave the existing deployment
//! exactly as it was.

use std::io;

use thiserror::Error;

use crate::config::{
    AccountingPolicy, ConfigError, DeploymentConfig, DeploymentOptions, ManagedProtocol,
    ProtocolPorts, SubscriptionMode,
};

#[derive(Debug, Error)]
pub enum WizardError {
    #[error("configuration wizard input failed: {0}")]
    Io(#[from] io::Error),
    #[error("configuration wizard produced an invalid deployment: {0}")]
    Config(#[from] ConfigError),
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WizardOutcome {
    /// The administrator cancelled before confirmation; nothing changed.
    Cancelled,
    /// The collected answers reproduce the existing configuration.
    Unchanged,
    /// A new configuration was confirmed and is ready for the commit transaction.
    Changed(DeploymentConfig),
}

/// The interactive prompt boundary. Production uses the console; tests drive a
/// script of answers through the same flow so the wizard logic is exercised
/// without a terminal.
pub trait Prompts {
    /// Ask a free-text question. `default` is displayed as the current value;
    /// an empty answer keeps the current value (or means "no value").
    fn ask(&mut self, label: &str, default: Option<&str>) -> io::Result<String>;
    /// Show a message (validation error, summary line, or header).
    fn report(&mut self, message: &str);
    /// Ask a yes/no confirmation. An empty answer selects `default`.
    fn confirm(&mut self, question: &str, default: bool) -> io::Result<bool>;
}

/// Runs the wizard. `existing` is the loaded deployment (`None` for a fresh
/// one); `default_interface` supplies the detected default-route interface as
/// the suggested default for a fresh deployment. Returns the confirmed outcome
/// without writing anything.
pub fn run<C: Prompts>(
    existing: Option<&DeploymentConfig>,
    default_interface: Option<String>,
    prompts: &mut C,
) -> Result<WizardOutcome, WizardError> {
    prompts.report("=== sbctl 配置向导 ===");

    let mode = ask_required(
        prompts,
        "订阅模式 (direct / external-proxy / ip-fallback)",
        existing
            .map(|config| config.subscription_mode.to_string())
            .or_else(|| Some(SubscriptionMode::Direct.to_string())),
        parse_mode,
    )?;

    let subscription_host = ask_required(
        prompts,
        "Subscription host",
        existing.map(|config| config.subscription_host.clone()),
        parse_host,
    )?;

    let proxy_host = ask_value(
        prompts,
        "Proxy host（留空 = 使用 Subscription host）",
        existing.and_then(|config| config.proxy_host.clone()),
        parse_optional_host,
    )?;

    let certbot_email = if mode == SubscriptionMode::Direct {
        ask_value(
            prompts,
            "Certbot 证书邮箱（仅 Direct 模式使用）",
            existing.and_then(|config| config.certbot_email.clone()),
            |value| Ok(value.to_owned()),
        )?
    } else {
        None
    };

    let http_port = if mode == SubscriptionMode::IpFallback {
        Some(ask_required(
            prompts,
            "IP fallback HTTP 端口（大于 1024）",
            existing.and_then(|config| config.http_port.map(|port| port.to_string())),
            parse_high_port,
        )?)
    } else {
        None
    };

    let listen_port = if mode == SubscriptionMode::ExternalProxy {
        Some(ask_required(
            prompts,
            "External proxy loopback 监听端口（大于 1024）",
            existing.and_then(|config| {
                config
                    .subscription_listen_port
                    .map(|port| port.to_string())
            }),
            parse_high_port,
        )?)
    } else {
        None
    };

    let interface = ask_required(
        prompts,
        "流量统计网卡",
        existing.map(|config| config.interface.clone()).or(default_interface),
        parse_interface,
    )?;

    let mut enabled_protocols = Vec::new();
    for protocol in [
        ManagedProtocol::VlessReality,
        ManagedProtocol::VmessWebsocket,
        ManagedProtocol::Hysteria2,
        ManagedProtocol::Tuic,
        ManagedProtocol::Anytls,
    ] {
        let currently_enabled =
            existing.is_none_or(|config| config.enabled_protocols.contains(&protocol));
        if ask_yes_no(
            prompts,
            &format!("启用 {protocol} 协议？"),
            currently_enabled,
        )? {
            enabled_protocols.push(protocol);
        }
    }

    let mut ports = ProtocolPorts::default();
    for protocol in &enabled_protocols {
        let current_port = existing
            .and_then(|config| config.protocol_listener_port(protocol))
            .map(|port| port.to_string());
        let port = ask_value(
            prompts,
            &format!("{protocol} 监听端口（留空 = 保持当前/自动分配）"),
            current_port,
            parse_protocol_port,
        )?;
        match protocol {
            ManagedProtocol::VlessReality => ports.vless_reality = port,
            ManagedProtocol::VmessWebsocket => ports.vmess_websocket = port,
            ManagedProtocol::Hysteria2 => ports.hysteria2 = port,
            ManagedProtocol::Tuic => ports.tuic = port,
            ManagedProtocol::Anytls => ports.anytls = port,
        }
    }

    let reality_decoy_sni = if enabled_protocols.contains(&ManagedProtocol::VlessReality) {
        Some(ask_required(
            prompts,
            "Reality decoy SNI",
            existing.and_then(|config| config.reality_decoy_sni.clone()),
            parse_hostname,
        )?)
    } else {
        None
    };

    let monthly_traffic_limit = ask_value(
        prompts,
        "每月流量上限（字节）",
        existing.map(|config| config.monthly_traffic_limit.to_string()),
        parse_u64,
    )?
    .unwrap_or(0);

    let accounting_timezone = ask_required(
        prompts,
        "Accounting timezone (IANA)",
        Some(
            existing
                .map(|config| config.accounting_timezone.clone())
                .unwrap_or_else(|| "UTC".to_owned()),
        ),
        parse_timezone,
    )?;

    let accounting_policy = ask_required(
        prompts,
        "账期策略 (natural-month / anchored-month)",
        existing
            .map(|config| config.accounting_policy.to_string())
            .or_else(|| Some(AccountingPolicy::NaturalMonth.to_string())),
        parse_policy,
    )?;

    let anchored_reset_at = if accounting_policy == AccountingPolicy::AnchoredMonth {
        Some(ask_required(
            prompts,
            "Anchored reset 首次重置时间 (YYYY-MM-DDTHH:MM)",
            existing.and_then(|config| config.anchored_reset_at.clone()),
            parse_reset_at,
        )?)
    } else {
        None
    };

    let new = DeploymentConfig::apply_options(
        existing,
        &DeploymentOptions {
            subscription_mode: mode,
            subscription_host,
            proxy_host,
            certbot_email,
            http_port,
            subscription_listen_port: listen_port,
            interface,
            enabled_protocols,
            reality_decoy_sni,
            monthly_traffic_limit,
            accounting_policy,
            accounting_timezone,
            anchored_reset_at,
            ports,
        },
    )?;

    if existing.is_some_and(|current| current == &new) {
        return Ok(WizardOutcome::Unchanged);
    }

    prompts.report("");
    prompts.report(&new.summary());
    prompts.report("");

    if !prompts.confirm("确认提交以上配置？", false)? {
        return Ok(WizardOutcome::Cancelled);
    }

    Ok(WizardOutcome::Changed(new))
}

/// Asks a required value, re-prompting until a valid answer is supplied.
fn ask_required<C: Prompts, T>(
    prompts: &mut C,
    label: &str,
    current: Option<String>,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, WizardError> {
    loop {
        if let Some(value) = ask_value(prompts, label, current.clone(), &parse)? {
            return Ok(value);
        }
        prompts.report("此项必填");
    }
}

/// Asks an optional value. An empty answer returns the current value (`None`
/// when there is none); a non-empty answer must parse, otherwise the wizard
/// re-prompts with the validation message.
fn ask_value<C: Prompts, T>(
    prompts: &mut C,
    label: &str,
    current: Option<String>,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Option<T>, WizardError> {
    loop {
        let answer = prompts.ask(label, current.as_deref())?;
        if answer.is_empty() {
            return Ok(current
                .as_deref()
                .map(|value| parse(value).expect("a persisted current value always parses")));
        }
        match parse(&answer) {
            Ok(value) => return Ok(Some(value)),
            Err(message) => prompts.report(&message),
        }
    }
}

fn ask_yes_no<C: Prompts>(prompts: &mut C, label: &str, default: bool) -> Result<bool, WizardError> {
    loop {
        let answer = prompts.ask(label, Some(if default { "y" } else { "n" }))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => prompts.report("请输入 y 或 n"),
        }
    }
}

fn parse_mode(value: &str) -> Result<SubscriptionMode, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "direct" | "1" => Ok(SubscriptionMode::Direct),
        "external-proxy" | "2" => Ok(SubscriptionMode::ExternalProxy),
        "ip-fallback" | "3" => Ok(SubscriptionMode::IpFallback),
        _ => Err("订阅模式必须是 direct、external-proxy 或 ip-fallback".to_owned()),
    }
}

fn parse_host(value: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    if crate::config::host_is_valid(&value) {
        Ok(value)
    } else {
        Err("Subscription host 必须是合法主机名或 IP 地址".to_owned())
    }
}

fn parse_optional_host(value: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    if crate::config::host_is_valid(&value) {
        Ok(value)
    } else {
        Err("Proxy host 必须是合法主机名或 IP 地址；不需要请直接回车".to_owned())
    }
}

fn parse_hostname(value: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    if crate::config::hostname_is_valid(&value) {
        Ok(value)
    } else {
        Err("Reality decoy SNI 必须是合法主机名".to_owned())
    }
}

fn parse_interface(value: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    let valid = !value.is_empty()
        && value.len() <= 15
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.');
    if valid {
        Ok(value)
    } else {
        Err("网卡必须是有效的 Linux interface 名称".to_owned())
    }
}

fn parse_high_port(value: &str) -> Result<u16, String> {
    let port = value
        .trim()
        .parse::<u16>()
        .map_err(|_| "端口必须是数字".to_owned())?;
    if port <= 1024 {
        return Err("端口必须大于 1024".to_owned());
    }
    Ok(port)
}

fn parse_protocol_port(value: &str) -> Result<u16, String> {
    let port = value
        .trim()
        .parse::<u16>()
        .map_err(|_| "端口必须是数字".to_owned())?;
    if !(10_000..=65_535).contains(&port) {
        return Err("Managed protocol 端口必须在 10000-65535".to_owned());
    }
    Ok(port)
}

fn parse_u64(value: &str) -> Result<u64, String> {
    value
        .trim()
        .parse()
        .map_err(|_| "必须是非负整数".to_owned())
}

fn parse_timezone(value: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    value
        .parse::<chrono_tz::Tz>()
        .map_err(|_| "必须是有效 IANA 时区，例如 UTC 或 Asia/Tokyo".to_owned())?;
    Ok(value)
}

fn parse_policy(value: &str) -> Result<AccountingPolicy, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "natural-month" | "natural" | "1" => Ok(AccountingPolicy::NaturalMonth),
        "anchored-month" | "anchored" | "2" => Ok(AccountingPolicy::AnchoredMonth),
        _ => Err("账期策略必须是 natural-month 或 anchored-month".to_owned()),
    }
}

fn parse_reset_at(value: &str) -> Result<String, String> {
    let value = value.trim().to_owned();
    chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M")
        .map_err(|_| "必须使用 YYYY-MM-DDTHH:MM 格式".to_owned())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;

    use super::{Prompts, WizardOutcome, run};
    use crate::config::{DeploymentConfig, ManagedProtocol, SubscriptionMode};

    struct ScriptPrompts {
        answers: VecDeque<String>,
        confirms: VecDeque<bool>,
        reports: Vec<String>,
    }

    impl ScriptPrompts {
        fn new(answers: &[&str], confirms: &[bool]) -> Self {
            Self {
                answers: answers.iter().map(|answer| (*answer).to_owned()).collect(),
                confirms: confirms.iter().copied().collect(),
                reports: Vec::new(),
            }
        }

        fn reports(&self) -> &[String] {
            &self.reports
        }
    }

    impl Prompts for ScriptPrompts {
        fn ask(&mut self, _label: &str, _default: Option<&str>) -> io::Result<String> {
            Ok(self.answers.pop_front().unwrap_or_default())
        }
        fn report(&mut self, message: &str) {
            self.reports.push(message.to_owned());
        }
        fn confirm(&mut self, _question: &str, _default: bool) -> io::Result<bool> {
            Ok(self.confirms.pop_front().unwrap_or(false))
        }
    }

    fn ip_fallback_config() -> DeploymentConfig {
        DeploymentConfig::new(
            SubscriptionMode::IpFallback,
            "203.0.113.7".into(),
            None,
            Some(2080),
            "ens3".into(),
            vec![ManagedProtocol::VlessReality],
            Some("www.cloudflare.com".into()),
        )
        .expect("an IP fallback VLESS deployment is valid")
    }

    fn empty_answers(count: usize) -> Vec<&'static str> {
        vec![""; count]
    }

    #[test]
    fn empty_answers_keep_the_existing_configuration() {
        let config = ip_fallback_config();
        let mut prompts = ScriptPrompts::new(&empty_answers(15), &[true]);

        let outcome = run(Some(&config), None, &mut prompts).expect("wizard completes");

        assert_eq!(outcome, WizardOutcome::Unchanged);
    }

    #[test]
    fn declining_the_summary_cancels_without_changing_the_deployment() {
        let config = ip_fallback_config();
        let mut answers = empty_answers(15);
        answers[1] = "198.51.100.9";
        let mut prompts = ScriptPrompts::new(&answers, &[false]);

        let outcome = run(Some(&config), None, &mut prompts).expect("wizard completes");

        assert_eq!(outcome, WizardOutcome::Cancelled);
    }

    #[test]
    fn an_invalid_answer_is_rejected_and_re_prompted() {
        let config = ip_fallback_config();
        let mut answers = empty_answers(15);
        answers[1] = "not a valid host!!";
        answers.insert(2, "198.51.100.9");
        let mut prompts = ScriptPrompts::new(&answers, &[true]);

        let outcome = run(Some(&config), None, &mut prompts).expect("wizard recovers from a bad host");

        let WizardOutcome::Changed(updated) = outcome else {
            panic!("a corrected host must produce a confirmed configuration");
        };
        assert_eq!(updated.subscription_host, "198.51.100.9");
        assert!(prompts
            .reports()
            .iter()
            .any(|message| message.contains("Subscription host 必须是合法主机名")));
    }

    #[test]
    fn a_mode_precondition_violation_fails_before_any_commit() {
        let config = ip_fallback_config();
        let mut answers = empty_answers(17);
        answers[6] = "y";
        answers[12] = "www.cloudflare.com";
        let mut prompts = ScriptPrompts::new(&answers, &[true]);

        let result = run(Some(&config), None, &mut prompts);

        assert!(result.is_err(), "VMess in IP fallback mode must be rejected");
    }

    #[test]
    fn a_fresh_deployment_uses_utc_natural_month_and_all_five_protocols() {
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
        ];
        let mut prompts = ScriptPrompts::new(&answers, &[true]);

        let outcome = run(
            None,
            Some("ens3".to_owned()),
            &mut prompts,
        )
        .expect("a fresh wizard completes");

        let WizardOutcome::Changed(config) = outcome else {
            panic!("a fresh deployment must be committed");
        };
        assert_eq!(config.subscription_mode, SubscriptionMode::Direct);
        assert_eq!(config.subscription_host, "sub.example.test");
        assert_eq!(config.interface, "ens3");
        assert_eq!(config.accounting_timezone, "UTC");
        assert_eq!(config.accounting_policy, crate::config::AccountingPolicy::NaturalMonth);
        assert_eq!(config.enabled_protocols.len(), 5);
        assert_eq!(config.certbot_email, None);
    }

    #[test]
    fn the_summary_and_prompts_never_print_credentials() {
        let config = ip_fallback_config();
        let credential = config.subscription_credential.clone();
        let mut answers = empty_answers(15);
        answers[1] = "198.51.100.9";
        let mut prompts = ScriptPrompts::new(&answers, &[false]);

        run(Some(&config), None, &mut prompts).expect("wizard completes");

        let joined = prompts.reports().join("\n");
        assert!(
            !joined.contains(&credential),
            "the wizard summary must redact the Subscription credential"
        );
        assert!(joined.contains("subscription credential: [redacted]"));
    }
}