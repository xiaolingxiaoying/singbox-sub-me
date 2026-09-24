//! The About destination: what this build is, and what it is running on.
//!
//! Deliberately information only. The prototype also offers 检查更新 and three
//! external links; the client has no application update channel of its own
//! (only the core download in `client_core::core`) and opening a browser is a
//! capability it does not have, so those buttons would be inert.

use gpui::{Context, Div, ParentElement, Styled, div, px, rgb};

use crate::components::icon;
use crate::state::Sbgui;
use crate::theme::{
    BODY, BORDER, CYAN, LABEL, META, MUTED, RADIUS, ROW_X, SECTION, SURFACE, TEXT, WEIGHT_MEDIUM,
    WEIGHT_SEMIBOLD,
};
use crate::tr;

impl Sbgui {
    pub(crate) fn about(&self, _cx: &mut Context<Self>) -> Div {
        let snapshot = &self.snapshot;
        let locale = self.locale;
        let core = match (snapshot.core_installed, snapshot.core_version.as_deref()) {
            (true, Some(version)) => version.to_owned(),
            (false, Some(version)) => tr!(
                locale,
                format!("{version}（未安装）"),
                format!("{version} (not installed)")
            ),
            (true, None) => {
                tr!(locale, "已安装，版本未知", "Installed, version unknown").to_owned()
            }
            _ => tr!(locale, "未安装", "Not installed").to_owned(),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .px(px(ROW_X))
                    .py(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(icon("info", CYAN, 20.0))
                            .child(
                                div()
                                    .text_size(px(SECTION))
                                    .font_weight(WEIGHT_SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .child(tr!(locale, "关于", "About")),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(BODY))
                            .line_height(px(21.0))
                            .text_color(rgb(MUTED))
                            .child(tr!(
                                locale,
                                "基于 sing-box 的私有订阅桌面客户端：内核、订阅、节点与系统代理都由同一个控制面管理，界面只渲染它的状态。",
                                "A desktop client for private subscriptions on sing-box: core, subscriptions, nodes and the system proxy share one control plane, and this window only draws its state."
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(row(tr!(locale, "应用版本", "App version"), env!("CARGO_PKG_VERSION")))
                            .child(row(tr!(locale, "内核版本", "Core version"), &core))
                            .child(row(
                                tr!(locale, "运行平台", "Platform"),
                                &format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
                            ))
                            .child(row(tr!(locale, "数据目录", "Data directory"), &self.data_dir.display().to_string())),
                    ),
            )
            .child(
                div()
                    .rounded(px(RADIUS))
                    .bg(rgb(SURFACE))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .px(px(ROW_X))
                    .py(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(LABEL))
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(rgb(MUTED))
                            .child(tr!(locale, "当前状态", "Current status")),
                    )
                    .child(row(
                        tr!(locale, "内核", "Core"),
                        if snapshot.core_running {
                            tr!(locale, "运行中", "Running")
                        } else {
                            tr!(locale, "未运行", "Not running")
                        },
                    ))
                    .child(row(tr!(locale, "订阅档案", "Subscription profiles"), &snapshot.profiles.len().to_string()))
                    .child(row(tr!(locale, "代理组", "Proxy groups"), &snapshot.proxy_groups.len().to_string()))
                    .child(row(tr!(locale, "路由规则", "Route rules"), &snapshot.rules.len().to_string()))
                    .child(row(tr!(locale, "活跃连接", "Active connections"), &snapshot.active_connections.to_string())),
            )
    }
}

fn row(label: &'static str, value: &str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .child(
            div()
                .text_size(px(META))
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(
            div()
                .text_size(px(BODY))
                .text_color(rgb(TEXT))
                .child(value.to_owned()),
        )
}
