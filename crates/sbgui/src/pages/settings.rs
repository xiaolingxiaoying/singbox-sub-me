//! The settings page: the seven settings sections.

use client_core::ClientCommand;
use client_core::command::SettingsPatch;
use client_core::format::age_label;
use client_core::system_proxy::TrafficMode;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb,
};

use crate::components::{panel, setting_line, setting_row_intro, toggle_line};
use crate::state::{FieldSpec, InputField, Sbgui, SettingsSection, Tone};
use crate::theme::{
    AMBER, BLUE_2, BODY, BORDER, CYAN, CYAN_DARK, FAINT, GAP_SECTION, LABEL, META, MUTED, SURFACE,
    SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_NORMAL,
};

impl Sbgui {
    // ------------------------------------------------------------- settings

    pub(crate) fn settings(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let profiles = snapshot.profiles.clone();
        let profile_rows: Vec<gpui::AnyElement> = profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let name = profile.name.clone();
                let active = profile.active;
                div()
                    .id(index)
                    .w_full()
                    .py(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(BODY))
                            .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
                            .text_color(rgb(TEXT))
                            .truncate()
                            .child(profile.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(META))
                            .text_color(rgb(MUTED))
                            .child(age_label(profile.last_updated)),
                    )
                    .child(if active {
                        // The active archive is already in use; a second
                        // "激活" button here would be a no-op.
                        div()
                            .px(px(8.0))
                            .text_size(px(META))
                            .text_color(rgb(FAINT))
                            .child("使用中")
                            .into_any_element()
                    } else {
                        self.mini_action(
                            index + 200_000,
                            "激活",
                            cx,
                            ClientCommand::SwitchProfile(name),
                        )
                        .into_any_element()
                    })
                    .into_any_element()
            })
            .collect();

        let tun_on = snapshot.traffic_mode == TrafficMode::Tun;
        let dirty_count = [
            self.field(InputField::Mirror).text != snapshot.settings.mirror,
            self.field(InputField::MixedPort).text != snapshot.settings.mixed_port.to_string(),
            self.field(InputField::TestUrl).text != snapshot.settings.test_url,
            self.field(InputField::AutoUpdateMinutes).text
                != snapshot.settings.auto_update_minutes.to_string(),
            self.field(InputField::CoreVersion).text != snapshot.settings.core_version,
        ]
        .into_iter()
        .filter(|dirty| *dirty)
        .count();

        let content = match self.settings_section {
            SettingsSection::General => panel("订阅档案")
                .w_full()
                .child(
                    div()
                        .mt(px(8.0))
                        .text_size(px(LABEL))
                        .line_height(px(19.0))
                        .text_color(rgb(MUTED))
                        .child("当前订阅与本地配置档案。订阅的添加、更新和删除请前往订阅页。"),
                )
                .child(div().mt(px(16.0)).flex().flex_col().children(profile_rows))
                .children(profiles.is_empty().then(|| {
                    div()
                        .mt(px(16.0))
                        .text_size(px(LABEL))
                        .text_color(rgb(MUTED))
                        .child("尚未导入订阅。")
                })),
            SettingsSection::Network => panel("网络与端口")
                .w_full()
                .child(setting_row_intro(
                    "流量模式",
                    "选择系统代理或 TUN 接管方式。",
                ))
                .child(setting_line("当前模式", snapshot.traffic_mode.label()))
                .child(self.edit_line(
                    "混合端口",
                    FieldSpec {
                        field: InputField::MixedPort,
                        id: "mixed-port-field",
                        placeholder: "2080",
                        width: 180.0,
                    },
                    window,
                    cx,
                ))
                .child(self.edit_line(
                    "延迟测试地址",
                    FieldSpec {
                        field: InputField::TestUrl,
                        id: "test-url-field",
                        placeholder: "https://…",
                        width: 320.0,
                    },
                    window,
                    cx,
                ))
                .child(setting_line("出站模式", snapshot.outbound_mode.label())),
            SettingsSection::Core => panel("sing-box 内核")
                .w_full()
                .child(setting_line(
                    "安装版本",
                    snapshot.core_version.as_deref().unwrap_or("未安装"),
                ))
                .child(setting_line(
                    "运行状态",
                    if snapshot.core_running {
                        "运行中"
                    } else if snapshot.starting {
                        "启动中"
                    } else {
                        "未运行"
                    },
                ))
                .child(self.edit_line(
                    "固定版本",
                    FieldSpec {
                        field: InputField::CoreVersion,
                        id: "core-version-field",
                        placeholder: "留空跟随最新",
                        width: 220.0,
                    },
                    window,
                    cx,
                ))
                .child(self.edit_line(
                    "镜像前缀",
                    FieldSpec {
                        field: InputField::Mirror,
                        id: "mirror-field",
                        placeholder: "直连",
                        width: 320.0,
                    },
                    window,
                    cx,
                ))
                .child(div().mt(px(20.0)).child(self.action(
                    "download-core",
                    "检查并更新内核",
                    Tone::Neutral,
                    cx,
                    ClientCommand::DownloadCore,
                ))),
            SettingsSection::Tun => panel("TUN")
                .w_full()
                .child(toggle_line(
                    "启用 TUN 模式",
                    tun_on,
                    "toggle-tun-settings",
                    cx,
                    SettingsPatch {
                        traffic_mode: Some(if tun_on {
                            TrafficMode::SystemProxy
                        } else {
                            TrafficMode::Tun
                        }),
                        ..Default::default()
                    },
                ))
                .child(setting_row_intro(
                    "需要重启",
                    "Windows 需要管理员权限与 wintun.dll；修改后请重启内核使配置生效。",
                ))
                .child(setting_line(
                    "当前说明",
                    if tun_on {
                        "虚拟网卡接管流量"
                    } else {
                        "使用系统代理端口"
                    },
                )),
            SettingsSection::Automation => panel("自动化")
                .w_full()
                .child(toggle_line(
                    "启动时自动启动内核",
                    snapshot.settings.auto_start,
                    "toggle-autostart",
                    cx,
                    SettingsPatch {
                        auto_start: Some(!snapshot.settings.auto_start),
                        ..Default::default()
                    },
                ))
                .child(toggle_line(
                    "内核就绪后自动开启系统代理",
                    snapshot.settings.auto_system_proxy,
                    "toggle-autoproxy",
                    cx,
                    SettingsPatch {
                        auto_system_proxy: Some(!snapshot.settings.auto_system_proxy),
                        ..Default::default()
                    },
                ))
                .child(self.edit_line(
                    "自动更新间隔（分钟）",
                    FieldSpec {
                        field: InputField::AutoUpdateMinutes,
                        id: "auto-update-field",
                        placeholder: "0 = 关闭",
                        width: 180.0,
                    },
                    window,
                    cx,
                )),
            SettingsSection::Appearance => panel("外观")
                .w_full()
                .child(setting_row_intro(
                    "界面主题",
                    "浅灰工作区、白色工作面与冷青强调色。",
                ))
                .child(setting_line("当前主题", "亮色"))
                .child(setting_line(
                    "字体",
                    "Segoe UI Variable / Microsoft YaHei UI",
                )),
            SettingsSection::Advanced => panel("高级")
                .w_full()
                .child(setting_row_intro(
                    "配置目录",
                    "GUI 与终端客户端共用同一套设置模型。",
                ))
                .child(setting_line(
                    "数据目录",
                    &self.data_dir.display().to_string(),
                ))
                .child(setting_line(
                    "设置文件",
                    // The real path: joining DATA_DIR with a Windows separator
                    // was wrong on Linux and was never an absolute path.
                    &self.data_dir.join("settings.toml").display().to_string(),
                ))
                .child(
                    div()
                        .mt(px(16.0))
                        .text_size(px(LABEL))
                        .line_height(px(19.0))
                        .text_color(rgb(AMBER))
                        .child("修改高级配置前请停止内核，并保留可恢复的配置副本。"),
                ),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(GAP_SECTION))
            // A settings row is a label and its value; the measure is capped
            // so both stay inside one eye sweep instead of drifting apart.
            .max_w(px(780.0))
            .child(div().flex().flex_wrap().gap(px(8.0)).children(
                SettingsSection::all().into_iter().map(|section| {
                    let active = section == self.settings_section;
                    div()
                        .id(format!("settings-{:?}", section))
                        .px(px(14.0))
                        .py(px(9.0))
                        .rounded(px(10.0))
                        .bg(rgb(if active { BLUE_2 } else { SURFACE }))
                        .text_size(px(LABEL))
                        .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
                        .text_color(rgb(if active { CYAN } else { MUTED }))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(SURFACE_2)))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.settings_section = section;
                            cx.notify();
                        }))
                        .child(section.label())
                }),
            ))
            .child(content)
            .children((dirty_count > 0).then(|| {
                div()
                    .px(px(16.0))
                    .py(px(12.0))
                    .rounded(px(10.0))
                    .bg(rgb(0xfff7ed))
                    .border_1()
                    .border_color(rgb(0xfed7aa))
                    .text_size(px(LABEL))
                    .text_color(rgb(AMBER))
                    .child(format!(
                        "有 {dirty_count} 项更改尚未保存；端口或内核配置可能需要重启后生效。"
                    ))
            }))
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("save-settings")
                        .px(px(18.0))
                        .py(px(10.0))
                        .rounded(px(9.0))
                        .bg(rgb(CYAN))
                        .text_size(px(LABEL))
                        .font_weight(WEIGHT_MEDIUM)
                        .text_color(rgb(SURFACE))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(CYAN_DARK)))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.save_settings(cx)))
                        .child("保存更改"),
                ),
            )
    }
}
