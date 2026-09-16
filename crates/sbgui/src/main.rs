#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, WindowBounds, WindowOptions, div, px, rgb,
    size,
};
use gpui_platform::application;

const BG: u32 = 0x0b1422;
const SURFACE: u32 = 0x111f31;
const SURFACE_2: u32 = 0x16273b;
const BORDER: u32 = 0x263b55;
const TEXT: u32 = 0xe6eff8;
const MUTED: u32 = 0x8ea3b9;
const CYAN: u32 = 0x65d9ef;
const MINT: u32 = 0x68e0ae;
const AMBER: u32 = 0xf3bd62;

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
    fn subtitle(self) -> &'static str {
        match self {
            Self::Dashboard => "快速确认代理是否正在工作，以及流量正在经过哪里。",
            Self::Proxies => "选择节点、测试延迟，管理当前的代理组。",
            Self::Connections => "查看 sing-box 当前接管的连接和流量。",
            Self::Logs => "实时查看内核日志，定位启动和分流问题。",
            Self::Settings => "订阅档案、内核版本、运行模式和自动更新。",
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
}

impl MainView {
    fn new() -> Self {
        Self {
            page: Page::Dashboard,
        }
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        div()
            .size_full()
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .child(titlebar())
            .child(
                div().flex_1().flex().child(sidebar(page, cx)).child(
                    div()
                        .flex_1()
                        .h_full()
                        .p(px(32.0))
                        .flex()
                        .flex_col()
                        .gap(px(22.0))
                        .child(page_header(page))
                        .child(page_content(page)),
                ),
            )
    }
}

fn titlebar() -> impl IntoElement {
    div()
        .h(px(46.0))
        .w_full()
        .flex()
        .items_center()
        .bg(rgb(0x0f1b2c))
        .border_b_1()
        .border_color(rgb(BORDER))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
        .child(
            div()
                .px(px(18.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                .child(div().w(px(9.0)).h(px(9.0)).rounded(px(5.0)).bg(rgb(CYAN)))
                .child(
                    div()
                        .text_size(px(15.0))
                        .text_color(rgb(TEXT))
                        .child("sbtui"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(MUTED))
                        .child("代理控制台"),
                ),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
        )
        .child(
            div()
                .mr(px(12.0))
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(5.0))
                .bg(rgb(0x18352f))
                .text_size(px(12.0))
                .text_color(rgb(MINT))
                .child("● 客户端就绪"),
        )
        .child(window_button("—", |window, _| window.minimize_window()))
        .child(window_button("□", |_, _| {}))
        .child(window_button("×", |window, _| window.remove_window()))
}

fn window_button(
    label: &'static str,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(label)
        .w(px(42.0))
        .h(px(46.0))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(16.0))
        .text_color(rgb(MUTED))
        .hover(|style| style.bg(rgb(0x1a2d43)).text_color(rgb(TEXT)))
        .on_click(move |_, window, cx| action(window, cx))
        .child(label)
}

fn sidebar(page: Page, cx: &mut Context<MainView>) -> impl IntoElement {
    div()
        .w(px(236.0))
        .h_full()
        .p(px(18.0))
        .bg(rgb(SURFACE))
        .border_r_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div().px(px(12.0)).pt(px(8.0)).pb(px(18.0)).child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(MUTED))
                    .child("工作区"),
            ),
        )
        .children(Page::all().into_iter().map(|item| {
            let active = item == page;
            div()
                .id(item.title())
                .w_full()
                .px(px(13.0))
                .py(px(11.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .gap(px(11.0))
                .bg(if active { rgb(0x164258) } else { rgb(SURFACE) })
                .text_color(if active { rgb(CYAN) } else { rgb(0xb9c8d8) })
                .hover(|style| style.bg(rgb(0x19324a)))
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.page = item;
                    cx.notify();
                }))
                .child(div().w(px(6.0)).h(px(6.0)).rounded(px(3.0)).bg(if active {
                    rgb(CYAN)
                } else {
                    rgb(0x536a82)
                }))
                .child(item.title())
        }))
        .child(div().flex_1())
        .child(
            div()
                .p(px(12.0))
                .rounded(px(7.0))
                .bg(rgb(0x0d1928))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child("数据目录"),
                )
                .child(
                    div()
                        .mt(px(5.0))
                        .text_size(px(10.0))
                        .text_color(rgb(0x6f879f))
                        .child("%APPDATA%\\sbtui"),
                ),
        )
}

fn page_header(page: Page) -> impl IntoElement {
    div()
        .flex()
        .items_end()
        .child(
            div()
                .flex_1()
                .child(
                    div()
                        .text_size(px(30.0))
                        .text_color(rgb(TEXT))
                        .child(page.title()),
                )
                .child(
                    div()
                        .mt(px(7.0))
                        .text_size(px(13.0))
                        .text_color(rgb(MUTED))
                        .child(page.subtitle()),
                ),
        )
        .child(action_button("启动内核", CYAN))
}

fn page_content(page: Page) -> gpui::Div {
    match page {
        Page::Dashboard => dashboard(),
        Page::Proxies => proxies(),
        Page::Connections => connections(),
        Page::Logs => logs(),
        Page::Settings => settings(),
    }
}

fn dashboard() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.0))
        .child(
            div()
                .flex()
                .gap(px(16.0))
                .child(metric(
                    "内核状态",
                    "未运行",
                    "在设置中导入订阅后启动",
                    AMBER,
                ))
                .child(metric("系统代理", "已关闭", "127.0.0.1:2080", MUTED))
                .child(metric("当前节点", "未选择", "代理组尚未加载", MUTED)),
        )
        .child(path_panel())
        .child(
            div()
                .flex()
                .gap(px(16.0))
                .child(traffic_panel())
                .child(quick_panel()),
        )
}

fn metric(
    title: &'static str,
    value: &'static str,
    detail: &'static str,
    color: u32,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_h(px(112.0))
        .p(px(18.0))
        .rounded(px(9.0))
        .bg(rgb(SURFACE_2))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(title),
        )
        .child(
            div()
                .mt(px(10.0))
                .text_size(px(22.0))
                .text_color(rgb(color))
                .child(value),
        )
        .child(
            div()
                .mt(px(5.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
}

fn path_panel() -> impl IntoElement {
    div()
        .p(px(20.0))
        .rounded(px(9.0))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(TEXT))
                .child("网络路径"),
        )
        .child(
            div()
                .mt(px(20.0))
                .flex()
                .items_center()
                .justify_between()
                .child(path_node("本机", "Windows 11", CYAN))
                .child(path_line())
                .child(path_node("系统代理", "已关闭", MUTED))
                .child(path_line())
                .child(path_node("当前节点", "未选择", MUTED))
                .child(path_line())
                .child(path_node("公网出口", "等待内核", MUTED)),
        )
}

fn path_node(title: &'static str, detail: &'static str, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .w(px(12.0))
                .h(px(12.0))
                .rounded(px(6.0))
                .bg(rgb(color)),
        )
        .child(div().text_size(px(12.0)).text_color(rgb(TEXT)).child(title))
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
}
fn path_line() -> impl IntoElement {
    div().h(px(1.0)).flex_1().mx(px(12.0)).bg(rgb(BORDER))
}

fn traffic_panel() -> impl IntoElement {
    div()
        .flex_1()
        .p(px(20.0))
        .rounded(px(9.0))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(TEXT))
                .child("实时流量"),
        )
        .child(
            div()
                .mt(px(18.0))
                .flex()
                .gap(px(38.0))
                .child(rate("↑ 上传", "0 B/s", MINT))
                .child(rate("↓ 下载", "0 B/s", CYAN)),
        )
        .child(
            div()
                .mt(px(18.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child("累计  上传 0 B    下载 0 B"),
        )
}
fn rate(label: &'static str, value: &'static str, color: u32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(
            div()
                .text_size(px(18.0))
                .text_color(rgb(color))
                .child(value),
        )
}

fn quick_panel() -> impl IntoElement {
    div()
        .flex_1()
        .p(px(20.0))
        .rounded(px(9.0))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .text_size(px(13.0))
                .text_color(rgb(TEXT))
                .child("快速操作"),
        )
        .child(
            div()
                .mt(px(15.0))
                .flex()
                .gap(px(8.0))
                .child(action_button("更新订阅", CYAN))
                .child(action_button("切换模式", MUTED)),
        )
        .child(
            div()
                .mt(px(12.0))
                .text_size(px(11.0))
                .text_color(rgb(MUTED))
                .child("系统代理和 TUN 操作会在执行前确认。"),
        )
}

fn proxies() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(section_title("代理组", "启动内核后加载选择器和节点"))
        .child(empty_state(
            "暂无代理组",
            "导入订阅并启动 sing-box，节点和延迟会显示在这里。",
            "去设置导入订阅",
        ))
}
fn connections() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(section_title("活动连接", "按下载、上传、主机或目标排序"))
        .child(empty_state(
            "暂无活动连接",
            "内核运行后，经过本机代理的连接会实时显示在这里。",
            "启动内核",
        ))
}
fn logs() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(section_title("内核日志", "显示 cache/core.log 的实时尾部"))
        .child(
            div()
                .min_h(px(280.0))
                .p(px(18.0))
                .rounded(px(9.0))
                .bg(rgb(0x09111d))
                .border_1()
                .border_color(rgb(BORDER))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(rgb(0x69839d))
                        .child("等待 sing-box 启动……"),
                ),
        )
}
fn settings() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(setting_row(
            "订阅档案",
            "尚未导入",
            "在这里新增、编辑或激活订阅链接",
        ))
        .child(setting_row(
            "sing-box 内核",
            "未安装",
            "支持固定版本和 GitHub 镜像",
        ))
        .child(setting_row(
            "流量模式",
            "系统代理",
            "可切换为 TUN；需要管理员权限",
        ))
        .child(setting_row(
            "自动更新",
            "关闭",
            "修改 settings.toml 中的 auto_update_minutes",
        ))
}

fn section_title(title: &'static str, detail: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .child(div().text_size(px(15.0)).text_color(rgb(TEXT)).child(title))
                .child(
                    div()
                        .mt(px(4.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(detail),
                ),
        )
        .child(action_button("刷新", MUTED))
}
fn setting_row(title: &'static str, value: &'static str, detail: &'static str) -> impl IntoElement {
    div()
        .p(px(18.0))
        .rounded(px(8.0))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .child(div().text_size(px(13.0)).text_color(rgb(TEXT)).child(title))
                .child(
                    div()
                        .mt(px(5.0))
                        .text_size(px(11.0))
                        .text_color(rgb(MUTED))
                        .child(detail),
                ),
        )
        .child(
            div()
                .mr(px(16.0))
                .text_size(px(13.0))
                .text_color(rgb(CYAN))
                .child(value),
        )
        .child(action_button("编辑", MUTED))
}
fn empty_state(
    title: &'static str,
    detail: &'static str,
    action: &'static str,
) -> impl IntoElement {
    div()
        .min_h(px(220.0))
        .p(px(28.0))
        .rounded(px(9.0))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .w(px(10.0))
                .h(px(10.0))
                .rounded(px(5.0))
                .bg(rgb(MUTED)),
        )
        .child(
            div()
                .mt(px(6.0))
                .text_size(px(16.0))
                .text_color(rgb(TEXT))
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
        .child(action_button(action, CYAN))
}
fn action_button(label: &'static str, color: u32) -> impl IntoElement {
    div()
        .id(label)
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(5.0))
        .bg(rgb(if color == CYAN { 0x164258 } else { 0x1a2b3e }))
        .border_1()
        .border_color(rgb(if color == CYAN { 0x2d7188 } else { BORDER }))
        .text_size(px(12.0))
        .text_color(rgb(color))
        .hover(|style| style.bg(rgb(0x21516a)))
        .on_click(|_, _, _| {})
        .child(label)
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                titlebar: None,
                is_movable: true,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| MainView::new()),
        )
        .unwrap();
    });
}
