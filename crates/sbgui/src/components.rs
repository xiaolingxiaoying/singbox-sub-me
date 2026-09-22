//! Stateless element builders shared by the pages.
//!
//! They take plain values rather than the whole window wherever they can,
//! so a page can be read without knowing everything `Sbgui` holds.

use client_core::ClientCommand;
use client_core::clash_api::Connection;
use client_core::command::SettingsPatch;
use client_core::format::{DelayLevel, delay_level, human_bytes};
use client_core::state::{ClientSnapshot, LogLevel, RouteRuleSnapshot};
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, rgb, svg,
};

use crate::state::{Page, Sbgui};
use crate::theme::{
    AMBER, BLUE, BLUE_2, BODY, BORDER, CYAN, DANGER, FAINT, LABEL, META, MINT, MUTED, PAD_CARD,
    RADIUS, ROW_X, ROW_Y, SECTION, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
};

// Small embedded SVGs keep icon weight consistent and survive standalone packaging.
pub(crate) fn icon(name: &str, color: u32, size: f32) -> impl IntoElement {
    let geometry = match name {
        "home" => "<path d='m3 10 9-7 9 7v10H3zM9 20v-7h6v7'/>",
        "subscription" => {
            "<rect x='5' y='3' width='14' height='18' rx='2'/><path d='M9 8h6M9 12h6M9 16h4'/>"
        }
        "nodes" => {
            "<rect x='8' y='8' width='8' height='8' rx='2'/><path d='M8 3H4v5m12-5h4v5M4 16v4h4m12-4v4h-4'/>"
        }
        "rules" => {
            "<path d='M4 5h16M4 12h16M4 19h16'/><circle cx='8' cy='5' r='2'/><circle cx='15' cy='12' r='2'/><circle cx='10' cy='19' r='2'/>"
        }
        "network" => "<rect x='3' y='4' width='18' height='12' rx='2'/><path d='M8 21h8m-4-5v5'/>",
        "logs" => "<path d='M5 4h14v16H5zM8 8h8m-8 4h8m-8 4h5'/>",
        "settings" => {
            "<path d='M4 7h16M4 17h16'/><circle cx='9' cy='7' r='3'/><circle cx='15' cy='17' r='3'/>"
        }
        "minimize" => "<path d='M5 12h14'/>",
        "maximize" => "<rect x='5' y='5' width='14' height='14' rx='1'/>",
        "restore" => "<path d='M8 8V5h11v11h-3'/><rect x='5' y='8' width='11' height='11' rx='1'/>",
        "close" => "<path d='m6 6 12 12M18 6 6 18'/>",
        "globe" => {
            "<circle cx='12' cy='12' r='9'/><ellipse cx='12' cy='12' rx='4' ry='9'/><path d='M3 12h18'/>"
        }
        "chevron" => "<path d='m9 5 7 7-7 7'/>",
        "check" => "<path d='m5 12 4 4L19 6'/>",
        "clock" => "<circle cx='12' cy='12' r='9'/><path d='M12 7v5l3 2'/>",
        "download" => "<path d='M12 3v12m-5-5 5 5 5-5M5 21h14'/>",
        "upload" => "<path d='M12 21V9m-5 5 5-5 5 5M5 3h14'/>",
        _ => "<circle cx='12' cy='12' r='8'/>",
    };
    let data = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24' fill='none' stroke='#{color:06x}' stroke-width='1.7' stroke-linecap='round' stroke-linejoin='round'>{geometry}</svg>"
    );
    svg()
        .data(data.as_bytes())
        .size(px(size))
        .flex_shrink_0()
        .text_color(rgb(color))
}

pub(crate) fn status_dot(color: u32) -> impl IntoElement {
    div()
        .w(px(7.0))
        .h(px(7.0))
        .flex_shrink_0()
        .rounded(px(4.0))
        .bg(rgb(color))
}

pub(crate) fn side_rate(label: &str, value: u64, color: u32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(div().text_color(rgb(FAINT)).child(label.to_owned()))
        .child(
            div()
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(color))
                .child(format!("{}/s", human_bytes(value))),
        )
}

pub(crate) fn pill(label: impl Into<String>, color: u32) -> impl IntoElement {
    div()
        .px(px(9.0))
        .py(px(4.0))
        .rounded(px(8.0))
        .bg(rgb(BLUE_2))
        .text_size(px(META))
        .font_weight(WEIGHT_MEDIUM)
        .text_color(rgb(color))
        .child(label.into())
}

/// A label-over-value cell, used by the overview's status strip and the
/// traffic panel's totals.
pub(crate) fn info_cell(
    label: &'static str,
    value: String,
    detail: Option<String>,
) -> impl IntoElement {
    div()
        .min_w(px(140.0))
        .child(
            div()
                .text_size(px(META))
                .text_color(rgb(FAINT))
                .child(label),
        )
        .child(
            div()
                .mt(px(6.0))
                .text_size(px(BODY))
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(TEXT))
                .child(value),
        )
        .children(detail.map(|text| {
            div()
                .mt(px(3.0))
                .text_size(px(META))
                .text_color(rgb(MUTED))
                .child(text)
        }))
}

pub(crate) fn panel(title: impl Into<String>) -> gpui::Div {
    div()
        .flex_1()
        .p(px(PAD_CARD))
        .rounded(px(RADIUS))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(SECTION))
                .font_weight(WEIGHT_SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(title.into()),
        )
}

pub(crate) fn traffic_panel(snapshot: &ClientSnapshot) -> impl IntoElement {
    let samples: Vec<(u64, u64)> = snapshot
        .traffic_history
        .iter()
        .map(|p| (p.up, p.down))
        .collect();
    let peak = snapshot.traffic_peak();
    // Two points make a line; before that the panel said "等待流量数据" under
    // an empty 170px box, which was the loudest thing on the page.
    let has_curve = samples.len() > 1;
    panel("流量趋势")
        // One legend for both rates: the metric cards used to repeat them, and
        // the 1 h / 24 h chips used to be two buttons that did nothing.
        .child(
            div()
                .mt(px(16.0))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(20.0))
                .child(legend(CYAN, "下载", snapshot.download_speed))
                .child(legend(BLUE, "上传", snapshot.upload_speed))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(META))
                        .text_color(rgb(FAINT))
                        .child("最近 5 分钟 · 本机 sing-box 实时采样"),
                ),
        )
        .child(
            div()
                .mt(px(16.0))
                .h(px(if has_curve { 180.0 } else { 96.0 }))
                .w_full()
                .when(has_curve, |plot| {
                    plot.child(traffic_chart(samples, peak.max(1)))
                })
                .when(!has_curve, |plot| {
                    plot.flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.0))
                        .bg(rgb(SURFACE_2))
                        .child(
                            div()
                                .text_size(px(LABEL))
                                .text_color(rgb(FAINT))
                                .child("内核运行并产生上下行后，这里绘制曲线。"),
                        )
                }),
        )
        .children(has_curve.then(|| {
            div()
                .mt(px(10.0))
                .flex()
                .justify_between()
                .text_size(px(META))
                .text_color(rgb(FAINT))
                .child(format!("{} 个采样点", snapshot.traffic_history.len()))
                .child("现在")
        }))
        .child(
            div()
                .mt(px(18.0))
                .pt(px(16.0))
                .border_t_1()
                .border_color(rgb(BORDER))
                .flex()
                .flex_wrap()
                .gap(px(32.0))
                .children([
                    info_cell("累计下载", human_bytes(snapshot.total_download), None),
                    info_cell("累计上传", human_bytes(snapshot.total_upload), None),
                    info_cell("5 分钟峰值", format!("{}/s", human_bytes(peak)), None),
                ]),
        )
}

fn traffic_chart(samples: Vec<(u64, u64)>, peak: u64) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let left = bounds.origin.x;
            let top = bounds.origin.y + px(2.0);
            let height = bounds.size.height - px(4.0);
            for fraction in [0.0, 0.5, 1.0] {
                let mut path = gpui::PathBuilder::stroke(px(1.0));
                let y = top + height * fraction;
                path.move_to(gpui::point(left, y));
                path.line_to(gpui::point(left + bounds.size.width, y));
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(BORDER));
                }
            }
            if samples.len() < 2 {
                return;
            }
            for upload in [false, true] {
                let mut path = gpui::PathBuilder::stroke(px(1.8));
                for (index, (up, down)) in samples.iter().enumerate() {
                    let value = if upload { *up } else { *down };
                    let x = left + bounds.size.width * (index as f32 / (samples.len() - 1) as f32);
                    let y = top + height * (1.0 - (value as f32 / peak as f32).clamp(0.0, 1.0));
                    if index == 0 {
                        path.move_to(gpui::point(x, y));
                    } else {
                        path.line_to(gpui::point(x, y));
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(if upload { BLUE } else { CYAN }));
                }
            }
        },
    )
    .size_full()
}

/// One chart legend entry: the series colour, its name, and its live value.
fn legend(color: u32, label: &'static str, value: u64) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(div().size(px(9.0)).rounded(px(5.0)).bg(rgb(color)))
        .child(
            div()
                .text_size(px(LABEL))
                .text_color(rgb(MUTED))
                .child(label),
        )
        .child(
            div()
                .text_size(px(BODY))
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(TEXT))
                .child(format!("{}/s", human_bytes(value))),
        )
}

pub(crate) fn rule_row(index: usize, rule: &RouteRuleSnapshot) -> gpui::AnyElement {
    div()
        .id(format!("rule-row-{index}"))
        .w_full()
        .px(px(ROW_X))
        .py(px(ROW_Y))
        .flex()
        .items_center()
        .gap(px(16.0))
        .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
        .hover(|style| style.bg(rgb(BLUE_2)))
        .child(
            div()
                .w(px(44.0))
                .flex_shrink_0()
                .text_size(px(META))
                .text_color(rgb(FAINT))
                .child((index + 1).to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(BODY))
                .text_color(rgb(TEXT))
                .child(rule.matcher.clone()),
        )
        .child(
            // The 状态 column used to sit here and say 已启用 on every row.
            div()
                .w(px(200.0))
                .flex_shrink_0()
                .truncate()
                .text_size(px(BODY))
                .text_color(rgb(CYAN))
                .child(rule.outbound.clone()),
        )
        .into_any_element()
}

pub(crate) fn log_table_header() -> impl IntoElement {
    div()
        .px(px(ROW_X))
        .py(px(10.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .bg(rgb(SURFACE_2))
        .border_b_1()
        .border_color(rgb(BORDER))
        .text_size(px(META))
        .text_color(rgb(MUTED))
        .child(div().w(px(76.0)).child("级别"))
        .child(div().w(px(110.0)).child("来源"))
        .child(div().flex_1().child("内容"))
}

pub(crate) fn log_row(index: usize, source: &str, line: &str, wrap: bool) -> gpui::AnyElement {
    let color = level_color(line);
    let level = if color == DANGER {
        "Error"
    } else if color == AMBER {
        "Warn"
    } else if color == FAINT {
        "Debug"
    } else {
        "Info"
    };
    div()
        .id(format!("log-row-{index}"))
        .w_full()
        .px(px(ROW_X))
        .py(px(ROW_Y))
        .flex()
        .items_start()
        .gap(px(12.0))
        .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
        .hover(|style| style.bg(rgb(BLUE_2)))
        .text_size(px(LABEL))
        .line_height(px(19.0))
        // The 序号 column is gone: a line number nobody cites is decoration,
        // and the level no longer needs a filled pill to be read as a level.
        .child(
            div()
                .w(px(76.0))
                .flex_shrink_0()
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(color))
                .child(level),
        )
        .child(
            div()
                .w(px(110.0))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(MUTED))
                .child(source.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .font_family("Cascadia Mono, Consolas")
                .when(!wrap, |row| {
                    row.whitespace_nowrap().overflow_hidden().text_ellipsis()
                })
                .text_color(rgb(if color == TEXT { TEXT } else { color }))
                .child(line.to_owned()),
        )
        .into_any_element()
}

pub(crate) fn connection_header() -> impl IntoElement {
    div()
        .min_w(px(920.0))
        .px(px(ROW_X))
        .py(px(10.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .bg(rgb(SURFACE_2))
        .text_size(px(META))
        .text_color(rgb(MUTED))
        .child(div().w(px(150.0)).child("应用 / 入口"))
        .child(div().flex_1().child("远程目标"))
        .child(div().w(px(64.0)).child("协议"))
        .child(div().w(px(150.0)).child("命中规则"))
        .child(div().w(px(130.0)).child("累计流量"))
        .child(div().w(px(100.0)).child("建立时间"))
        .child(div().w(px(48.0)).child(""))
}

pub(crate) fn connection_row(
    index: usize,
    connection: &Connection,
    cx: &mut Context<Sbgui>,
) -> gpui::AnyElement {
    let id = connection.id.clone();
    let detail_id = connection.id.clone();
    let application = if !connection.metadata.process.is_empty() {
        connection.metadata.process.clone()
    } else if !connection.metadata.process_path.is_empty() {
        connection.metadata.process_path.clone()
    } else {
        "系统代理".to_owned()
    };
    div()
        .id(index)
        .min_w(px(920.0))
        .px(px(ROW_X))
        .py(px(ROW_Y))
        .flex()
        .items_center()
        .gap(px(12.0))
        .bg(rgb(SURFACE))
        .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
        .font_family("Cascadia Mono, Consolas")
        .text_size(px(LABEL))
        .text_color(rgb(TEXT))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(BLUE_2)))
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
            view.selected_connection = Some(detail_id.clone());
            cx.notify();
        }))
        // 状态 and 使用节点 left the row for the detail card: every row here is
        // active, and the chain is the least readable column in a fixed width.
        .child(
            div()
                .w(px(150.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(status_dot(MINT))
                .child(div().truncate().child(application)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(connection_target(connection)),
        )
        .child(
            div()
                .w(px(64.0))
                .flex_shrink_0()
                .text_color(rgb(MUTED))
                .child(connection.metadata.network.to_uppercase()),
        )
        .child(
            div()
                .w(px(150.0))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(MUTED))
                .child(if connection.rule.is_empty() {
                    "未匹配".to_owned()
                } else {
                    connection.rule.clone()
                }),
        )
        .child(
            div()
                .w(px(130.0))
                .flex_shrink_0()
                .text_color(rgb(CYAN))
                .child(format!(
                    "↓ {} ↑ {}",
                    human_bytes(connection.download),
                    human_bytes(connection.upload)
                )),
        )
        .child(
            div()
                .w(px(100.0))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(MUTED))
                .child(if connection.start.is_empty() {
                    "刚刚".to_owned()
                } else {
                    connection.start.clone()
                }),
        )
        .child(
            div()
                .id(index + 300_000)
                .w(px(48.0))
                .flex_shrink_0()
                .text_color(rgb(DANGER))
                .hover(|style| style.text_color(rgb(0xffb0ab)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    view.send(ClientCommand::CloseConnection(id.clone()));
                    cx.notify();
                }))
                .child("断开"),
        )
        .into_any_element()
}

pub(crate) fn setting_line(label: &str, value: &str) -> impl IntoElement {
    div()
        .w_full()
        .py(px(14.0))
        .flex()
        .items_center()
        .gap(px(16.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .w(px(160.0))
                .flex_shrink_0()
                .text_size(px(BODY))
                .text_color(rgb(TEXT))
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(LABEL))
                .text_color(rgb(MUTED))
                .truncate()
                .child(value.to_owned()),
        )
}

pub(crate) fn setting_row_intro(label: &str, detail: &str) -> impl IntoElement {
    div()
        .mt(px(18.0))
        .child(
            div()
                .text_size(px(BODY))
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(TEXT))
                .child(label.to_owned()),
        )
        .child(
            div()
                .mt(px(4.0))
                .text_size(px(LABEL))
                .line_height(px(19.0))
                .text_color(rgb(MUTED))
                .child(detail.to_owned()),
        )
}

pub(crate) fn toggle_line(
    label: &'static str,
    on: bool,
    id: &'static str,
    cx: &mut Context<Sbgui>,
    patch: SettingsPatch,
) -> impl IntoElement {
    div()
        .w_full()
        .py(px(16.0))
        .flex()
        .items_center()
        .gap(px(16.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(BODY))
                .text_color(rgb(TEXT))
                .child(label),
        )
        .child(
            div()
                .id(id)
                .w(px(46.0))
                .h(px(26.0))
                .flex_shrink_0()
                .p(px(3.0))
                .rounded(px(13.0))
                .flex()
                .items_center()
                .when(on, |control| control.justify_end())
                .when(!on, |control| control.justify_start())
                .bg(rgb(if on { CYAN } else { 0xc7cbd1 }))
                .cursor_pointer()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.send(ClientCommand::UpdateSettings(patch.clone()));
                    cx.notify();
                }))
                .child(
                    div()
                        .w(px(20.0))
                        .h(px(20.0))
                        .rounded(px(10.0))
                        .bg(rgb(SURFACE)),
                ),
        )
}

pub(crate) fn empty_state(
    title: &'static str,
    detail: &'static str,
    action: Option<&'static str>,
    cx: &mut Context<Sbgui>,
) -> impl IntoElement {
    let button = action.map(|label| {
        let label: &'static str = label;
        div()
            .id(label)
            .mt(px(6.0))
            .px(px(16.0))
            .py(px(9.0))
            .rounded(px(9.0))
            .bg(rgb(BLUE_2))
            .border_1()
            .border_color(rgb(BLUE_2))
            .text_size(px(LABEL))
            .font_weight(WEIGHT_MEDIUM)
            .text_color(rgb(CYAN))
            .hover(|style| style.bg(rgb(SURFACE_2)))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if label.contains("启动") {
                    view.send(ClientCommand::StartCore);
                } else {
                    view.page = if label.contains("订阅") {
                        Page::Subscriptions
                    } else {
                        Page::Settings
                    };
                }
                cx.notify();
            }))
            .child(label)
    });
    div()
        .min_h(px(240.0))
        .p(px(40.0))
        .rounded(px(RADIUS))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(SECTION))
                .font_weight(WEIGHT_SEMIBOLD)
                .text_color(rgb(TEXT))
                .child(title),
        )
        .child(
            div()
                .max_w(px(420.0))
                .text_size(px(BODY))
                .line_height(px(21.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
        .children(button)
}

/// The empty case inside a panel that already owns the border and the surface,
/// so the two boxes no longer nest inside each other.
pub(crate) fn inline_empty(title: &'static str, detail: &'static str) -> impl IntoElement {
    div()
        .w_full()
        .min_h(px(180.0))
        .p(px(32.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(BODY))
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(TEXT))
                .child(title),
        )
        .child(
            div()
                .max_w(px(420.0))
                .text_size(px(LABEL))
                .line_height(px(19.0))
                .text_color(rgb(MUTED))
                .child(detail),
        )
}

fn level_color(line: &str) -> u32 {
    // Derived from the shared level judgement, so a Chinese client event that
    // the filter treats as an error is not painted as plain info text.
    match client_core::state::log_level_of(line) {
        LogLevel::Error => DANGER,
        LogLevel::Warn => AMBER,
        LogLevel::Debug => FAINT,
        LogLevel::Info => TEXT,
    }
}

pub(crate) fn delay_color(delay: Option<u64>) -> u32 {
    match delay_level(delay) {
        DelayLevel::Fast => MINT,
        DelayLevel::Slow => AMBER,
        DelayLevel::Timeout => DANGER,
        DelayLevel::Unknown => CYAN,
    }
}

pub(crate) fn clean_proxy_label(value: &str) -> String {
    value
        .trim_start_matches(|character: char| {
            !character.is_ascii_alphanumeric() && !('\u{4e00}'..='\u{9fff}').contains(&character)
        })
        .trim()
        .to_owned()
}

pub(crate) fn connection_target(connection: &Connection) -> String {
    let host = if connection.metadata.destination_host.is_empty() {
        connection.metadata.destination_ip.as_str()
    } else {
        connection.metadata.destination_host.as_str()
    };
    if host.is_empty() {
        "—".to_owned()
    } else if connection.metadata.destination_port.is_empty() {
        host.to_owned()
    } else {
        format!("{host}:{}", connection.metadata.destination_port)
    }
}

pub(crate) fn connection_chain(connection: &Connection) -> String {
    if connection.chains.is_empty() {
        "直连".to_owned()
    } else {
        connection
            .chains
            .iter()
            .map(|value| clean_proxy_label(value))
            .collect::<Vec<_>>()
            .join(" → ")
    }
}
