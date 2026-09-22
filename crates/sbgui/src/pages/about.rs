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
    BODY, BORDER, CYAN, FAINT, LABEL, META, MUTED, RADIUS, ROW_X, SECTION, SURFACE, TEXT,
    WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
};

impl Sbgui {
    pub(crate) fn about(&self, _cx: &mut Context<Self>) -> Div {
        let snapshot = &self.snapshot;
        let core = match (snapshot.core_installed, snapshot.core_version.as_deref()) {
            (true, Some(version)) => version.to_owned(),
            (false, Some(version)) => format!("{version}（未安装）"),
            (true, None) => "已安装，版本未知".to_owned(),
            _ => "未安装".to_owned(),
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
                                    .child("关于"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(BODY))
                            .line_height(px(21.0))
                            .text_color(rgb(MUTED))
                            .child("基于 sing-box 的私有订阅桌面客户端：内核、订阅、节点与系统代理都由同一个控制面管理，界面只渲染它的状态。"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(row("应用版本", env!("CARGO_PKG_VERSION")))
                            .child(row("内核版本", &core))
                            .child(row(
                                "运行平台",
                                &format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
                            ))
                            .child(row("数据目录", &self.data_dir.display().to_string())),
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
                            .text_color(rgb(FAINT))
                            .child("当前状态"),
                    )
                    .child(row(
                        "内核",
                        if snapshot.core_running {
                            "运行中"
                        } else {
                            "未运行"
                        },
                    ))
                    .child(row("订阅档案", &snapshot.profiles.len().to_string()))
                    .child(row("代理组", &snapshot.proxy_groups.len().to_string()))
                    .child(row("路由规则", &snapshot.rules.len().to_string()))
                    .child(row("活跃连接", &snapshot.active_connections.to_string())),
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
                .text_color(rgb(FAINT))
                .child(label),
        )
        .child(
            div()
                .text_size(px(BODY))
                .text_color(rgb(TEXT))
                .child(value.to_owned()),
        )
}
