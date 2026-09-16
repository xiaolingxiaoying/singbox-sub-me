#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, Styled, Window, WindowBounds, WindowOptions, div, px, rgb, size,
};
use gpui_platform::application;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Dashboard,
    Proxies,
    Connections,
    Logs,
    Settings,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "概览",
            Self::Proxies => "节点",
            Self::Connections => "连接",
            Self::Logs => "日志",
            Self::Settings => "设置",
        }
    }
    fn all() -> [Self; 5] {
        [
            Self::Dashboard,
            Self::Proxies,
            Self::Connections,
            Self::Logs,
            Self::Settings,
        ]
    }
}

struct MainView {
    page: Page,
    data_dir: PathBuf,
}

impl MainView {
    fn new() -> Self {
        let data_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            page: Page::Dashboard,
            data_dir: data_dir.join("sbtui"),
        }
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        div()
            .size_full()
            .bg(rgb(0x0d1726))
            .flex()
            .child(
                div()
                    .w(px(220.0))
                    .h_full()
                    .p(px(20.0))
                    .bg(rgb(0x111f32))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(22.0))
                            .text_color(rgb(0x8be9fd))
                            .child("sbtui"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(0x91a4b8))
                            .child("sing-box proxy client"),
                    )
                    .children(Page::all().into_iter().map(|item| {
                        let active = item == page;
                        div()
                            .w_full()
                            .p(px(11.0))
                            .rounded(px(6.0))
                            .bg(if active { rgb(0x16445a) } else { rgb(0x111f32) })
                            .text_color(if active { rgb(0x8be9fd) } else { rgb(0xc8d4e3) })
                            .on_mouse_up(
                                MouseButton::Left,
                                _cx.listener(move |view, _, _, cx| {
                                    view.page = item;
                                    cx.notify();
                                }),
                            )
                            .child(item.title())
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(0x72869d))
                            .child(self.data_dir.display().to_string()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .p(px(28.0))
                    .flex()
                    .flex_col()
                    .gap(px(18.0))
                    .child(
                        div()
                            .text_size(px(28.0))
                            .text_color(rgb(0xe8f1fb))
                            .child(page.title()),
                    )
                    .child(content_for(page)),
            )
    }
}

fn content_for(page: Page) -> impl IntoElement {
    match page {
        Page::Dashboard => div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(card("内核状态", "未运行", 0xf6b73c))
            .child(card("当前节点", "尚未选择节点", 0x91a4b8))
            .child(card(
                "网络路径",
                "本机 → 系统代理 → 当前节点 → 公网出口",
                0x8be9fd,
            )),
        Page::Proxies => div().child(card(
            "代理节点",
            "启动内核后将在这里显示代理组与延迟",
            0x8be9fd,
        )),
        Page::Connections => div().child(card("活动连接", "暂无活动连接", 0x91a4b8)),
        Page::Logs => div().child(card("内核日志", "暂无日志", 0x91a4b8)),
        Page::Settings => div().child(card(
            "客户端设置",
            "订阅档案、内核版本、镜像和自动更新",
            0x8be9fd,
        )),
    }
}

fn card(title: &'static str, value: &'static str, color: u32) -> impl IntoElement {
    div()
        .w_full()
        .p(px(18.0))
        .rounded(px(8.0))
        .bg(rgb(0x14243a))
        .border_1()
        .border_color(rgb(0x263b55))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(0x91a4b8))
                .child(title),
        )
        .child(
            div()
                .mt(px(8.0))
                .text_size(px(18.0))
                .text_color(rgb(color))
                .child(value),
        )
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1120.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| MainView::new()),
        )
        .unwrap();
    });
}
