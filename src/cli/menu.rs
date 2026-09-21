//! The interactive terminal menu: the top-level loop, its six sections, and
//! the rendering and confirmation helpers they share. The console prompts used
//! by the non-interactive install path live in `cli::prompt`, not here.

use crate::cli::args::{CliSubscriptionMode, InstallOptions, SingBoxCommand};
use crate::cli::commands::{
    config::{commit_config_change, regenerate, restart, run_config_wizard},
    install::install,
    serve::{print_subscription_qr, print_subscription_urls},
    status::{
        format_local_time, print_nodes, print_status, print_traffic, run_accounting_reset,
        traffic_set_used,
    },
    system::{rotate_subscription_credential, system_info},
    update::{sing_box, uninstall, update},
};
use crate::cli::prompt::ConsolePrompts;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

pub(crate) fn menu(root: &Path) -> ExitCode {
    if !io::stdin().is_terminal() {
        eprintln!("menu requires an interactive terminal");
        return ExitCode::from(2);
    }

    loop {
        clear_menu_screen();
        print_menu_header(root);
        println!();
        println!("{}", sbctl::term::green(" 1. 安装与部署"));
        println!("{}", sbctl::term::green(" 2. 节点与协议"));
        println!("{}", sbctl::term::green(" 3. 订阅中心"));
        println!("{}", sbctl::term::green(" 4. 流量与账期"));
        println!("{}", sbctl::term::green(" 5. 服务与诊断"));
        println!("{}", sbctl::term::green(" 6. 更新与卸载"));
        println!("{}", sbctl::term::green(" 0. 退出"));
        println!();
        match read_menu_choice("请选择 [0]: ") {
            Some(choice) => match choice.as_str() {
                "0" => return ExitCode::SUCCESS,
                "1" => menu_deployment(root),
                "2" => menu_protocols(root),
                "3" => menu_subscriptions(root),
                "4" => menu_traffic(root),
                "5" => menu_services(root),
                "6" => menu_updates(root),
                _ => {
                    eprintln!("无效选择，请输入 0 到 6。");
                    pause_menu();
                }
            },
            None => return ExitCode::from(2),
        }
    }
}

fn print_menu_header(root: &Path) {
    let (os, kernel, cpu, bbr) = system_info();
    let bar = sbctl::term::blue("-".repeat(70));
    println!("{bar}");
    println!(
        "{}",
        sbctl::term::white("  sbctl  ·  私有 sing-box 订阅控制面")
    );
    println!("{}", sbctl::term::blue("  快捷方式: ly"));
    println!(
        "{}",
        sbctl::term::blue("  项目: github.com/xiaolingxiaoying/singbox-sub-me")
    );
    println!("{bar}");
    println!(
        "{}",
        sbctl::term::green(format!(
            "系统: {os}   内核: {kernel}   处理器: {cpu}   BBR: {bbr}"
        ))
    );
    let store = sbctl::config::DeploymentStore::new(root);
    match store.load() {
        Ok(config) => {
            println!(
                "{}",
                sbctl::term::yellow(format!(
                    "订阅模式: {}   订阅主机: {}   协议: {} 项",
                    config.subscription_mode,
                    config.subscription_host,
                    config.enabled_protocols.len()
                ))
            );
            if let Some(port) = config.http_port {
                println!("{}", sbctl::term::yellow(format!("公网回退端口: {port}")));
            }
            if let Some(port) = config.subscription_listen_port {
                println!(
                    "{}",
                    sbctl::term::yellow(format!("反向代理回环端口: {port}"))
                );
            }
            println!(
                "{}",
                sbctl::term::green(sbctl::lifecycle::service_status(root))
            );
            match sbctl::traffic::report(&store, &config) {
                Ok(report) => {
                    let total = report.total();
                    let usage = if report.monthly_traffic_limit == 0 {
                        format!("{} / 不限", sbctl::traffic::format_gib(total))
                    } else {
                        let percent = (total as f64 / report.monthly_traffic_limit as f64) * 100.0;
                        format!(
                            "{} / {} ({percent:.1}%)",
                            sbctl::traffic::format_gib(total),
                            sbctl::traffic::format_gib(report.monthly_traffic_limit)
                        )
                    };
                    println!(
                        "{}",
                        sbctl::term::green(format!(
                            "流量: {usage}   出口网卡: {}",
                            report.interface
                        ))
                    );
                    println!(
                        "{}",
                        sbctl::term::blue(format!(
                            "刷新时间: {} ({})   客户端参考: {} ({})",
                            format_local_time(report.next_reset, &config.accounting_timezone),
                            config.accounting_timezone,
                            format_local_time(report.next_reset, &config.client_display_timezone),
                            config.client_display_timezone
                        ))
                    );
                }
                Err(error) => println!(
                    "{}",
                    sbctl::term::yellow(format!("流量: 暂不可用 ({error})"))
                ),
            }
            println!("{}", sbctl::term::green("状态: 已部署"));
        }
        Err(_) => println!("{}", sbctl::term::red("状态: 未安装，请选择 1 安装")),
    }
    println!("{bar}");
}

fn clear_menu_screen() {
    print!("\x1b[2J\x1b[H");
    let _ = io::stdout().flush();
}

fn read_menu_choice(prompt: &str) -> Option<String> {
    print!("{}", sbctl::term::yellow(prompt));
    io::stdout().flush().ok()?;
    let mut choice = String::new();
    if io::stdin().read_line(&mut choice).ok()? == 0 {
        return None;
    }
    Some(choice.trim().to_owned())
}

fn pause_menu() {
    print!("\n{}", sbctl::term::yellow("按回车返回菜单..."));
    let _ = io::stdout().flush();
    let mut ignored = String::new();
    let _ = io::stdin().read_line(&mut ignored);
}

fn print_menu_section(title: &str) {
    println!("\n{}", sbctl::term::blue(format!("=== {title} ===")));
}

fn menu_deployment(root: &Path) {
    loop {
        clear_menu_screen();
        print_menu_header(root);
        print_menu_section("安装与部署");
        let installed = sbctl::config::DeploymentStore::new(root).load().is_ok();
        if installed {
            println!("1. 重新生成并校验现有配置工件");
            println!("2. 完整配置向导");
        } else {
            println!("1. 安装 sbctl");
            println!("2. 安装并进入完整配置向导");
        }
        println!("0. 返回");
        match read_menu_choice("请选择 [0]: ").as_deref() {
            Some("0") | None => return,
            Some("1") if installed => {
                regenerate(root, None);
            }
            Some("1") => {
                menu_install(root);
            }
            Some("2") if !installed => {
                if menu_install(root) == ExitCode::SUCCESS {
                    run_config_wizard(root, None);
                }
            }
            Some("2") => {
                run_config_wizard(root, None);
            }
            Some(_) => eprintln!("无效选择，请输入 0 到 2。"),
        }
        pause_menu();
    }
}

fn menu_protocols(root: &Path) {
    loop {
        clear_menu_screen();
        print_menu_header(root);
        print_menu_section("节点与协议");
        println!("1. 查看节点与监听端口");
        println!("2. 配置协议启停、端口、SNI 和证书");
        println!("0. 返回");
        match read_menu_choice("请选择 [0]: ").as_deref() {
            Some("0") | None => return,
            Some("1") => {
                print_nodes(root);
                pause_menu();
            }
            Some("2") => {
                run_topic_wizard(root, sbctl::wizard::ConfigurationTopic::Protocols);
                pause_menu();
            }
            Some(_) => {
                eprintln!("无效选择，请输入 0 到 2。");
                pause_menu();
            }
        }
    }
}

fn menu_subscriptions(root: &Path) {
    loop {
        clear_menu_screen();
        print_menu_header(root);
        print_menu_section("订阅中心");
        println!("1. 查看全部订阅链接与二维码");
        println!("2. 显示 sing-box 完整配置二维码");
        println!("3. 显示 sing-box 精简配置二维码");
        println!("4. 显示 Clash/Mihomo 二维码");
        println!("5. 显示 Clash 1.18 兼容二维码");
        println!("6. 显示 URI 二维码");
        println!("7. 显示 Base64 URI 二维码（V2rayN）");
        println!("8. 显示 Shadowrocket 二维码");
        println!("9. 配置订阅入口");
        println!("10. 轮换订阅凭据（旧链接立即失效）");
        println!("11. 重新生成订阅工件");
        println!("12. 客户端模板配置（DNS / 分流规则档位 / 延迟探测）");
        println!("0. 返回");
        match read_menu_choice("请选择 [0]: ").as_deref() {
            Some("0") | None => return,
            Some("1") => {
                print_subscription_urls(root, None);
                pause_menu();
            }
            Some("2") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::SingBoxFull),
                    false,
                );
                pause_menu();
            }
            Some("3") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::SingBox),
                    false,
                );
                pause_menu();
            }
            Some("4") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::Clash),
                    false,
                );
                pause_menu();
            }
            Some("5") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::ClashLegacy(
                        sbctl::subscription::CLASH_LEGACY_VERSION,
                    )),
                    false,
                );
                pause_menu();
            }
            Some("6") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::Uri),
                    false,
                );
                pause_menu();
            }
            Some("7") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::Base64Uri),
                    false,
                );
                pause_menu();
            }
            Some("8") => {
                print_subscription_qr(
                    root,
                    Some(sbctl::subscription::SubscriptionFormat::Shadowrocket),
                    false,
                );
                pause_menu();
            }
            Some("9") => {
                run_topic_wizard(root, sbctl::wizard::ConfigurationTopic::Subscription);
                pause_menu();
            }
            Some("10") => {
                if confirm_menu_action("确认轮换订阅凭据？旧订阅 URL 将立即失效") {
                    rotate_subscription_credential(root);
                }
                pause_menu();
            }
            Some("11") => {
                if confirm_menu_action("确认重新生成并校验订阅工件？") {
                    regenerate(root, None);
                }
                pause_menu();
            }
            Some("12") => {
                run_topic_wizard(root, sbctl::wizard::ConfigurationTopic::ClientTemplate);
                pause_menu();
            }
            Some(_) => {
                eprintln!("无效选择，请输入 0 到 12。");
                pause_menu();
            }
        }
    }
}

fn menu_traffic(root: &Path) {
    loop {
        clear_menu_screen();
        print_menu_header(root);
        print_menu_section("流量与账期");
        println!("1. 查看本周期流量");
        println!("2. 配置上限、已用量、时区、刷新规则和出口网卡");
        println!("3. 修正本周期总流量（GiB）");
        println!("4. 修正本周期 RX/TX 流量（GiB）");
        println!("5. 立即建立/刷新当前账期");
        println!("0. 返回");
        match read_menu_choice("请选择 [0]: ").as_deref() {
            Some("0") | None => return,
            Some("1") => {
                print_traffic(root);
                pause_menu();
            }
            Some("2") => {
                run_topic_wizard(root, sbctl::wizard::ConfigurationTopic::Traffic);
                pause_menu();
            }
            Some("3") => {
                menu_total_traffic_correction(root);
                pause_menu();
            }
            Some("4") => {
                menu_direction_traffic_correction(root);
                pause_menu();
            }
            Some("5") => {
                if confirm_menu_action("确认建立/刷新当前账期？") {
                    run_accounting_reset(root);
                }
                pause_menu();
            }
            Some(_) => {
                eprintln!("无效选择，请输入 0 到 5。");
                pause_menu();
            }
        }
    }
}

fn menu_services(root: &Path) {
    loop {
        clear_menu_screen();
        print_menu_header(root);
        print_menu_section("服务与诊断");
        println!("1. 查看完整部署状态");
        println!("2. 校验配置并重启服务");
        println!("3. 查看运行日志");
        println!("0. 返回");
        match read_menu_choice("请选择 [0]: ").as_deref() {
            Some("0") | None => return,
            Some("1") => {
                print_status(root);
                pause_menu();
            }
            Some("2") => {
                if confirm_menu_action("确认校验配置并重启服务？") {
                    restart(root, None);
                }
                pause_menu();
            }
            Some("3") => {
                menu_logs();
                pause_menu();
            }
            Some(_) => {
                eprintln!("无效选择，请输入 0 到 3。");
                pause_menu();
            }
        }
    }
}

fn menu_updates(root: &Path) {
    loop {
        clear_menu_screen();
        print_menu_header(root);
        print_menu_section("更新与卸载");
        println!("1. 更新 sbctl");
        println!("2. 更新 / 切换 sing-box 内核");
        println!("3. 卸载服务和二进制（保留备份）");
        println!("4. 彻底清理 sbctl 数据");
        println!("0. 返回");
        match read_menu_choice("请选择 [0]: ").as_deref() {
            Some("0") | None => return,
            Some("1") => {
                if confirm_menu_action("确认更新 sbctl？将自动校验签名 manifest") {
                    update(root, false, None, None, None);
                }
                pause_menu();
            }
            Some("2") => {
                if confirm_menu_action("确认更新 / 切换 sing-box 内核？") {
                    sing_box(
                        root,
                        SingBoxCommand::Update {
                            manifest: None,
                            artifact: None,
                        },
                    );
                }
                pause_menu();
            }
            Some("3") => {
                if confirm_menu_action("确认卸载服务和二进制并保留备份？") {
                    uninstall(root, false);
                    return;
                }
                pause_menu();
            }
            Some("4") => {
                if confirm_menu_action("确认彻底清理 sbctl 数据？此操作不可恢复")
                    && confirm_menu_action("再次确认：删除配置、状态、证书和备份？")
                {
                    uninstall(root, true);
                    return;
                }
                pause_menu();
            }
            Some(_) => {
                eprintln!("无效选择，请输入 0 到 4。");
                pause_menu();
            }
        }
    }
}

fn run_topic_wizard(root: &Path, topic: sbctl::wizard::ConfigurationTopic) {
    let store = sbctl::config::DeploymentStore::new(root);
    let config = match store.load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("读取部署配置失败: {error}");
            return;
        }
    };
    let mut prompts = ConsolePrompts;
    match sbctl::wizard::run_topic(&config, topic, &mut prompts) {
        Ok(sbctl::wizard::WizardOutcome::Cancelled) => {
            println!("配置未应用，现有部署保持不变");
        }
        Ok(sbctl::wizard::WizardOutcome::Unchanged) => {
            println!("配置未发生变化");
        }
        Ok(sbctl::wizard::WizardOutcome::Changed(new)) => {
            let _ = commit_config_change(root, &store, &new, None);
        }
        Err(error) => eprintln!("配置失败: {error}"),
    }
}

fn menu_total_traffic_correction(root: &Path) {
    let store = sbctl::config::DeploymentStore::new(root);
    if let Ok(config) = store.load()
        && let Ok(report) = sbctl::traffic::report(&store, &config)
    {
        println!(
            "当前已用总流量: {}",
            sbctl::traffic::format_gib(report.total())
        );
    }
    let Some(input) = read_menu_choice("目标总流量（GiB，支持小数；输入 0 表示 0）: ")
    else {
        return;
    };
    match sbctl::traffic::parse_traffic_amount(&input) {
        Ok(bytes) => {
            traffic_set_used(root, Some(bytes), None, None);
        }
        Err(error) => eprintln!("流量输入无效: {error}"),
    }
}

fn menu_direction_traffic_correction(root: &Path) {
    let Some(rx) = read_menu_choice("目标 RX 接收流量（GiB）: ") else {
        return;
    };
    let Some(tx) = read_menu_choice("目标 TX 发送流量（GiB）: ") else {
        return;
    };
    match (
        sbctl::traffic::parse_traffic_amount(&rx),
        sbctl::traffic::parse_traffic_amount(&tx),
    ) {
        (Ok(rx), Ok(tx)) => {
            traffic_set_used(root, None, Some(rx), Some(tx));
        }
        (Err(error), _) | (_, Err(error)) => eprintln!("流量输入无效: {error}"),
    }
}

fn menu_install(root: &Path) -> ExitCode {
    match sbctl::config::DeploymentStore::new(root).load() {
        Ok(_) => {
            eprintln!("已安装。请返回上级菜单选择完整配置向导或重新生成配置工件。");
            ExitCode::from(2)
        }
        Err(sbctl::config::ConfigError::Missing) => install(
            root,
            InstallOptions {
                mode: CliSubscriptionMode::Direct,
                subscription_host: None,
                proxy_host: None,
                http_port: None,
                interface: None,
                reality_decoy_sni: None,
                protocol_sni: None,
                disable_protocol: Vec::new(),
                vless_port: None,
                vmess_port: None,
                hysteria2_port: None,
                tuic_port: None,
                anytls_port: None,
                sing_box_bin: None,
                manifest: None,
                no_start: false,
            },
        ),
        Err(error) => {
            eprintln!("检查部署状态失败: {error}");
            ExitCode::from(2)
        }
    }
}

fn menu_logs() {
    let status = std::process::Command::new("journalctl")
        .args([
            "-u",
            "sbctl.service",
            "-u",
            "sing-box.service",
            "-n",
            "50",
            "--no-pager",
        ])
        .status();
    if let Err(error) = status {
        eprintln!("查看日志失败: {error}（需要 systemd）");
    }
}

pub(crate) fn confirm_menu_action(prompt: &str) -> bool {
    print!("{prompt} [y/N]: ");
    if io::stdout().flush().is_err() {
        return false;
    }
    let mut answer = String::new();
    io::stdin().read_line(&mut answer).is_ok()
        && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
