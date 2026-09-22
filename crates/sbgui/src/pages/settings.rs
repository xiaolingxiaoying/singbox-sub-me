//! The settings page: the seven settings sections.

use client_core::ClientCommand;
use client_core::command::SettingsPatch;
use client_core::format::age_label;
use client_core::system_proxy::TrafficMode;
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px, rgb, rgba,
};

use crate::components::{
    icon, page_head, setting_line, setting_row_intro, toggle_line, work_surface,
};
use crate::lang::{outbound_mode, traffic_mode};
use crate::state::{FieldSpec, InputField, Sbgui, SettingsSection, Tone};
use crate::theme::{
    AMBER, BODY, BORDER, BORDER_STRONG, CYAN, CYAN_DARK, FAINT, LABEL, META, MUTED, NAV_ACTIVE,
    RADIUS_CONTROL, ROW_HOVER, SECTION, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_NORMAL,
};
use crate::tr;

impl Sbgui {
    // ------------------------------------------------------------- settings

    pub(crate) fn settings(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let snapshot = &self.snapshot;
        let locale = self.locale;
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

        work_surface()
            .child(
                page_head(tr!(
                    locale,
                    "按分区修改客户端行为；端口与内核配置在重启内核后生效。",
                    "Each section changes one part of the client; ports and core settings apply after a core restart.",
                )).child(
                    self.button(
                        "save-settings",
                        tr!(locale, "保存更改", "Save changes"),
                        Tone::Accent,
                        None,
                        cx,
                        |view, cx| view.save_settings(cx),
                    ),
                ),
            )
            .children((dirty_count > 0).then(|| {
                div()
                    .mt(px(18.0))
                    .px(px(14.0))
                    .py(px(12.0))
                    .rounded(px(RADIUS_CONTROL + 2.0))
                    .bg(rgb(0xfff7ed))
                    .border_1()
                    .border_color(rgb(0xfed7aa))
                    .text_size(px(LABEL))
                    .text_color(rgb(AMBER))
                    .child(tr!(
                        locale,
                        format!("有 {dirty_count} 项更改尚未保存；端口或内核配置可能需要重启后生效。"),
                        format!(
                            "{dirty_count} change(s) not saved yet; a port or core setting may need a restart."
                        )
                    ))
            }))
            .child(
                div()
                    .mt(px(18.0))
                    .flex()
                    .flex_wrap()
                    .items_stretch()
                    .gap(px(22.0))
                    .child(self.settings_rail(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(360.0))
                            .max_w(px(760.0))
                            .p(px(20.0))
                            .rounded(px(RADIUS_CONTROL + 2.0))
                            .bg(rgb(SURFACE_2))
                            .border_1()
                            .border_color(rgb(BORDER))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(SECTION))
                                    .font_weight(WEIGHT_MEDIUM)
                                    .text_color(rgb(TEXT))
                                    .child(self.settings_section.label(locale)),
                            )
                            .child(self.settings_detail(window, cx)),
                    ),
            )
    }

    /// The seven sections as a rail rather than as a row of chips: the name and
    /// its one-line purpose stack into a 66 px row, and the selected one keeps
    /// the same inset bar the navigation uses.
    fn settings_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.locale;
        div()
            .w(px(270.0))
            .flex_shrink_0()
            .rounded(px(RADIUS_CONTROL + 2.0))
            .border_1()
            .border_color(rgb(BORDER))
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(SettingsSection::all().into_iter().map(|section| {
                let active = section == self.settings_section;
                div()
                    .id(format!("settings-{:?}", section))
                    .w_full()
                    .min_h(px(66.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    // The bar is laid out for every row and painted only when
                    // selected, so the labels stay in one column.
                    .child(
                        div()
                            .w(px(3.0))
                            .h(px(20.0))
                            .flex_shrink_0()
                            .rounded(px(2.0))
                            .bg(if active { rgb(CYAN) } else { rgba(0x0000_0000) }),
                    )
                    .bg(rgb(if active { NAV_ACTIVE } else { SURFACE }))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(if active { NAV_ACTIVE } else { ROW_HOVER })))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.settings_section = section;
                        cx.notify();
                    }))
                    .child(icon(
                        section.glyph(),
                        if active { CYAN } else { FAINT },
                        20.0,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(BODY))
                                    .font_weight(if active { WEIGHT_MEDIUM } else { WEIGHT_NORMAL })
                                    .text_color(rgb(if active { CYAN_DARK } else { TEXT }))
                                    .truncate()
                                    .child(section.label(locale)),
                            )
                            .child(
                                div()
                                    .mt(px(4.0))
                                    .text_size(px(META))
                                    .text_color(rgb(MUTED))
                                    .truncate()
                                    .child(section.blurb(locale)),
                            ),
                    )
                    .child(icon("chevron", if active { CYAN } else { FAINT }, 15.0))
            }))
    }

    /// The selected section's rows. Each one is the kit's row anatomy: the name
    /// carries the weight, the value or the control sits on the same line.
    fn settings_detail(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = &self.snapshot;
        let locale = self.locale;
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
                    .min_h(px(56.0))
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
                            .child(tr!(locale, "使用中", "In use"))
                            .into_any_element()
                    } else {
                        self.mini_action(
                            index + 200_000,
                            tr!(locale, "激活", "Activate"),
                            cx,
                            ClientCommand::SwitchProfile(name),
                        )
                        .into_any_element()
                    })
                    .into_any_element()
            })
            .collect();
        let tun_on = snapshot.traffic_mode == TrafficMode::Tun;

        match self.settings_section {
            SettingsSection::General => div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .mt(px(10.0))
                        .text_size(px(LABEL))
                        .line_height(px(19.0))
                        .text_color(rgb(MUTED))
                        .child(tr!(
                            locale,
                            "当前订阅与本地配置档案。订阅的添加、更新和删除请前往订阅页。",
                            "The active subscription and the local profiles. Add, update and delete them on the Subscriptions page.",
                        )),
                )
                .child(div().mt(px(10.0)).flex().flex_col().children(profile_rows))
                .children(profiles.is_empty().then(|| {
                    div()
                        .mt(px(16.0))
                        .text_size(px(LABEL))
                        .text_color(rgb(MUTED))
                        .child(tr!(locale, "尚未导入订阅。", "No subscription imported yet."))
                })),
            SettingsSection::Network => div()
                .flex()
                .flex_col()
                .child(setting_row_intro(
                    tr!(locale, "流量模式", "Traffic mode"),
                    tr!(locale, "选择系统代理或 TUN 接管方式。", "Choose the system proxy or TUN."),
                ))
                .child(setting_line(tr!(locale, "当前模式", "Current mode"), traffic_mode(snapshot.traffic_mode, locale)))
                .child(self.edit_line(
                    tr!(locale, "混合端口", "Mixed port"),
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
                    tr!(locale, "延迟测试地址", "Latency test URL"),
                    FieldSpec {
                        field: InputField::TestUrl,
                        id: "test-url-field",
                        placeholder: "https://…",
                        width: 320.0,
                    },
                    window,
                    cx,
                ))
                .child(setting_line(tr!(locale, "出站模式", "Outbound mode"), outbound_mode(snapshot.outbound_mode, locale))),
            SettingsSection::Core => div()
                .flex()
                .flex_col()
                .child(setting_line(
                    tr!(locale, "安装版本", "Installed version"),
                    snapshot
                        .core_version
                        .as_deref()
                        .unwrap_or(tr!(locale, "未安装", "Not installed")),
                ))
                .child(setting_line(
                    tr!(locale, "运行状态", "Runtime status"),
                    if snapshot.core_running {
                        tr!(locale, "运行中", "Running")
                    } else if snapshot.starting {
                        tr!(locale, "启动中", "Starting")
                    } else {
                        tr!(locale, "未运行", "Not running")
                    },
                ))
                .child(self.edit_line(
                    tr!(locale, "固定版本", "Pinned version"),
                    FieldSpec {
                        field: InputField::CoreVersion,
                        id: "core-version-field",
                        placeholder: tr!(locale, "留空跟随最新", "Empty follows the latest"),
                        width: 220.0,
                    },
                    window,
                    cx,
                ))
                .child(self.edit_line(
                    tr!(locale, "镜像前缀", "Mirror prefix"),
                    FieldSpec {
                        field: InputField::Mirror,
                        id: "mirror-field",
                        placeholder: tr!(locale, "直连", "Direct"),
                        width: 320.0,
                    },
                    window,
                    cx,
                ))
                .child(div().mt(px(20.0)).flex().justify_end().child(self.action(
                    "download-core",
                    tr!(locale, "检查并更新内核", "Check for core updates"),
                    Tone::Neutral,
                    cx,
                    ClientCommand::DownloadCore,
                ))),
            SettingsSection::Tun => div()
                .flex()
                .flex_col()
                .child(toggle_line(
                    tr!(locale, "启用 TUN 模式", "Enable TUN mode"),
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
                    tr!(locale, "需要重启", "Restart required"),
                    tr!(
                        locale,
                        "Windows 需要管理员权限与 wintun.dll；修改后请重启内核使配置生效。",
                        "Windows needs administrator rights and wintun.dll; restart the core to apply.",
                    ),
                ))
                .child(setting_line(
                    tr!(locale, "当前说明", "Currently"),
                    if tun_on {
                        tr!(locale, "虚拟网卡接管流量", "The virtual adapter takes traffic")
                    } else {
                        tr!(locale, "使用系统代理端口", "The system proxy port is used")
                    },
                )),
            SettingsSection::Automation => div()
                .flex()
                .flex_col()
                .child(toggle_line(
                    tr!(locale, "启动时自动启动内核", "Start the core on launch"),
                    snapshot.settings.auto_start,
                    "toggle-autostart",
                    cx,
                    SettingsPatch {
                        auto_start: Some(!snapshot.settings.auto_start),
                        ..Default::default()
                    },
                ))
                .child(toggle_line(
                    tr!(locale, "内核就绪后自动开启系统代理", "Enable the system proxy once the core is ready"),
                    snapshot.settings.auto_system_proxy,
                    "toggle-autoproxy",
                    cx,
                    SettingsPatch {
                        auto_system_proxy: Some(!snapshot.settings.auto_system_proxy),
                        ..Default::default()
                    },
                ))
                .child(self.edit_line(
                    tr!(locale, "自动更新间隔（分钟）", "Auto-update interval (min)"),
                    FieldSpec {
                        field: InputField::AutoUpdateMinutes,
                        id: "auto-update-field",
                        placeholder: tr!(locale, "0 = 关闭", "0 = off"),
                        width: 180.0,
                    },
                    window,
                    cx,
                )),
            SettingsSection::Appearance => div()
                .flex()
                .flex_col()
                .child(setting_row_intro(
                    tr!(locale, "界面主题", "Interface theme"),
                    tr!(
                        locale,
                        "浅灰工作区、白色工作面与冷青强调色。",
                        "A grey workspace, white surfaces and cool teal accents.",
                    ),
                ))
                .child(setting_line(tr!(locale, "当前主题", "Current theme"), tr!(locale, "亮色", "Light")))
                .child(setting_line(
                    tr!(locale, "字体", "Font"),
                    "Segoe UI Variable / Microsoft YaHei UI",
                )),
            SettingsSection::Advanced => div()
                .flex()
                .flex_col()
                .child(setting_row_intro(
                    tr!(locale, "配置目录", "Configuration folder"),
                    tr!(
                        locale,
                        "GUI 与终端客户端共用同一套设置模型。",
                        "The desktop and terminal clients share one settings model.",
                    ),
                ))
                .child(setting_line(
                    tr!(locale, "数据目录", "Data directory"),
                    &self.data_dir.display().to_string(),
                ))
                .child(setting_line(
                    tr!(locale, "设置文件", "Settings file"),
                    // The real path: joining DATA_DIR with a Windows separator
                    // was wrong on Linux and was never an absolute path.
                    &self.data_dir.join("settings.toml").display().to_string(),
                ))
                .child(
                    div()
                        .mt(px(20.0))
                        .px(px(14.0))
                        .py(px(12.0))
                        .rounded(px(RADIUS_CONTROL + 2.0))
                        .bg(rgb(SURFACE))
                        .border_1()
                        .border_color(rgb(BORDER_STRONG))
                        .text_size(px(LABEL))
                        .line_height(px(19.0))
                        .text_color(rgb(AMBER))
                        .child(tr!(
                            locale,
                            "修改高级配置前请停止内核，并保留可恢复的配置副本。",
                            "Stop the core before editing advanced settings, and keep a copy you can restore.",
                        )),
                ),
        }
    }
}
