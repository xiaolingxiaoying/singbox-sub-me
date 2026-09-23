//! The Settings tab: the profile list and the run-environment summary, plus the two
//! helpers the rest of the UI borrows — the routing-rules listing and OSC 52 copy.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem, Paragraph};

use crate::app::App;
use crate::format::{age_label, human_bytes, usage_label};
use crate::style::{CYAN, panel};
use crate::{settings, system_proxy};

pub(crate) fn draw_settings(frame: &mut Frame, area: ratatui::prelude::Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);
    let items: Vec<ListItem> = app
        .snapshot
        .profiles
        .iter()
        .map(|profile| {
            ListItem::new(format!(
                "{}{}（更新于 {}）",
                if profile.active { "✓ " } else { "  " },
                profile.name,
                age_label(profile.last_updated)
            ))
        })
        .collect();
    frame.render_stateful_widget(
        List::new(items)
            .block(panel("订阅档案 · Enter 激活"))
            .highlight_style(
                Style::default()
                    .fg(Color::Rgb(8, 18, 28))
                    .bg(CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
        columns[0],
        &mut app.profiles_list,
    );
    let settings = &app.snapshot.settings;
    let core_path = settings::core_path(&app.dir);
    let info = vec![
        Line::from(format!(
            "内核: {}",
            if app.snapshot.core_installed {
                core_path.display().to_string()
            } else {
                "未安装".into()
            }
        )),
        Line::from(format!(
            "版本: {}   镜像: {}",
            app.snapshot
                .core_version
                .clone()
                .unwrap_or_else(|| "未检测".into()),
            if settings.mirror.is_empty() {
                "直连"
            } else {
                settings.mirror.as_str()
            }
        )),
        Line::from(format!(
            "运行版本: {}   内存: {}",
            app.snapshot
                .core_runtime_version
                .clone()
                .unwrap_or_else(|| "未运行".into()),
            if app.snapshot.core_running {
                human_bytes(app.snapshot.memory_used)
            } else {
                "-".to_owned()
            }
        )),
        Line::from(format!(
            "模式: {}   混合端口: {}   延迟地址: {}",
            app.snapshot.traffic_mode.label(),
            settings.mixed_port,
            settings.test_url
        )),
        Line::from(format!(
            "自动更新: {}   启动内核: {}   自动系统代理: {}",
            if settings.auto_update_minutes == 0 {
                "关".to_owned()
            } else {
                format!("{} 分钟", settings.auto_update_minutes)
            },
            if settings.auto_start { "开" } else { "关" },
            if settings.auto_system_proxy {
                "开"
            } else {
                "关"
            }
        )),
        Line::from(format!(
            "系统代理后端: {}   订阅用量: {}",
            system_proxy::platform_label(),
            usage_label(app.snapshot.subscription_usage.as_ref())
        )),
        Line::from(""),
        Line::from("档案: n 新增 ｜ f 本地文件 ｜ e 改链接 ｜ Delete 删除 ｜ Enter 激活"),
        Line::from("内核: v 改版本 ｜ r 改镜像 ｜ d 下载 ｜ u 更新订阅"),
        Line::from("选项: a 自动更新 ｜ P 混合端口 ｜ U 延迟地址 ｜ g 启动内核 ｜ y 自动代理"),
    ];
    frame.render_widget(Paragraph::new(info).block(panel("运行环境")), columns[1]);
}

/// Copies text to the terminal clipboard via the OSC 52 escape sequence, which
/// works over SSH and needs no platform-specific clipboard dependency.
pub(crate) fn copy_to_clipboard_osc52(text: &str) {
    use base64::Engine;
    use std::io::Write;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    print!("\x1b]52;c;{encoded}\x07");
    let _ = std::io::stdout().flush();
}
