//! Stateless element builders shared by the pages.
//!
//! They take plain values rather than the whole window wherever they can,
//! so a page can be read without knowing everything `Sbgui` holds.

use client_core::ClientCommand;
use client_core::clash_api::Connection;
use client_core::command::SettingsPatch;
use client_core::format::{DelayLevel, delay_level, human_bytes};
use client_core::state::{LogLevel, RouteRuleSnapshot};
use gpui::prelude::FluentBuilder;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px, rgb, svg,
};

use crate::state::{Page, Sbgui};
use crate::theme::{
    AMBER, BLUE, BLUE_2, BODY, BORDER, CYAN, DANGER, DISPLAY, FAINT, GAP_ITEM, LABEL, META, MINT,
    MUTED, PAD_SURFACE_X, PAD_SURFACE_Y, RADIUS, RADIUS_CONTROL, ROW_HOVER, ROW_X, ROW_Y, SECTION,
    SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
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
        "chevron_down" => "<path d='m5 9 7 7 7-7'/>",
        "check" => "<path d='m5 12 4 4L19 6'/>",
        "clock" => "<circle cx='12' cy='12' r='9'/><path d='M12 7v5l3 2'/>",
        "download" => "<path d='M12 3v12m-5-5 5 5 5-5M5 21h14'/>",
        "upload" => "<path d='M12 21V9m-5 5 5-5 5 5M5 3h14'/>",
        // The kit's own inventory (design-kit/icon-manifest.md), drawn with the
        // same 1.7 stroke as the rest rather than as text symbols.
        "search" => "<circle cx='11' cy='11' r='7'/><path d='m20 20-3.6-3.6'/>",
        "power" => "<path d='M12 3v9'/><path d='M18.4 6.6a9 9 0 1 1-12.8 0'/>",
        "database" => {
            "<ellipse cx='12' cy='5.5' rx='8' ry='3'/><path d='M4 5.5v13c0 1.7 3.6 3 8 3s8-1.3 8-3v-13'/><path d='M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3'/>"
        }
        "stack" => "<path d='m12 3 9 5-9 5-9-5 9-5Z'/><path d='m3 13 9 5 9-5'/>",
        "connections" => {
            "<circle cx='7' cy='7' r='3.4'/><circle cx='17' cy='7' r='3.4'/><circle cx='7' cy='17' r='3.4'/><path d='M17 14.2v5.6m-2.8-2.8h5.6'/>"
        }
        "refresh" => "<path d='M20 12a8 8 0 1 1-2.4-5.7'/><path d='M20 4v4.5h-4.5'/>",
        "plus" => "<path d='M12 5v14M5 12h14'/>",
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

/// The core's health as the kit draws it: a 22 px dot wearing a 7 px ring of
/// its own colour, so "running" is legible before any text is read.
pub(crate) fn health_dot(on: bool) -> impl IntoElement {
    div()
        .size(px(36.0))
        .flex_shrink_0()
        .rounded(px(18.0))
        .bg(rgb(if on { BLUE_2 } else { SURFACE_2 }))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .size(px(22.0))
                .rounded(px(11.0))
                .bg(rgb(if on { MINT } else { 0xaeb9b6 })),
        )
}

/// The kit's switch: a 46 x 26 track with a 20 px knob, inside a hitbox that
/// keeps the whole control at the 34 px minimum a pointer target needs.
/// `command` is `None` while the setting cannot change (a running core will
/// not pick up a new traffic mode), and the track says so by staying grey.
pub(crate) fn switch(
    id: &'static str,
    on: bool,
    command: Option<ClientCommand>,
    cx: &mut Context<Sbgui>,
) -> impl IntoElement {
    let clickable = command.is_some();
    div()
        .id(id)
        .min_h(px(34.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .when(clickable, |style| style.cursor_pointer())
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
            if let Some(command) = command.clone() {
                view.send(command);
                cx.notify();
            }
        }))
        .child(
            div()
                .w(px(46.0))
                .h(px(26.0))
                .p(px(3.0))
                .rounded(px(13.0))
                .flex()
                .items_center()
                .when(on, |track| track.justify_end())
                .when(!on, |track| track.justify_start())
                .bg(rgb(if on { CYAN } else { 0xc7cbd1 }))
                .child(
                    div()
                        .w(px(20.0))
                        .h(px(20.0))
                        .rounded(px(10.0))
                        .bg(rgb(SURFACE)),
                ),
        )
}

/// One labelled disclosure: the trigger names a lower-frequency group, the
/// body carries the values. Progressive disclosure is how the kit keeps the
/// overview scannable without dropping the details.
pub(crate) fn accordion(
    id: &'static str,
    title: &'static str,
    subtitle: impl Into<String>,
    open: bool,
    cx: &mut Context<Sbgui>,
    toggle: impl Fn(&mut Sbgui) + 'static,
    body: impl IntoElement,
) -> impl IntoElement {
    let subtitle = subtitle.into();
    div()
        .mt(px(GAP_ITEM))
        .w_full()
        .rounded(px(RADIUS_CONTROL + 1.0))
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE_2))
        .overflow_hidden()
        .child(
            div()
                .id(id)
                .w_full()
                .min_h(px(42.0))
                .px(px(14.0))
                .flex()
                .items_center()
                .gap(px(9.0))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(BLUE_2)))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    toggle(view);
                    cx.notify();
                }))
                .child(icon(
                    if open { "chevron_down" } else { "chevron" },
                    MUTED,
                    15.0,
                ))
                .child(
                    div()
                        .text_size(px(LABEL))
                        .font_weight(WEIGHT_MEDIUM)
                        .text_color(rgb(TEXT))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(META))
                        .text_color(rgb(FAINT))
                        .child(subtitle),
                ),
        )
        .when(open, |panel| {
            panel.child(
                div()
                    .w_full()
                    .px(px(16.0))
                    .py(px(13.0))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .bg(rgb(SURFACE))
                    .child(body),
            )
        })
}

/// One value inside a disclosure body: the label in muted type, the value
/// after it in the page's own text colour.
pub(crate) fn detail_item(label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(px(META))
        .text_color(rgb(MUTED))
        .child(label.to_owned())
        .child(
            div()
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(TEXT))
                .child(value),
        )
}

/// One headline metric: icon above a label and a display-size value, with a
/// hairline on its left so a row of them reads as one instrument, not as four
/// floating cards.
pub(crate) fn metric_cell(
    glyph: &'static str,
    label: &'static str,
    value: String,
    detail: Option<String>,
    first: bool,
) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(132.0))
        .px(px(17.0))
        .py(px(8.0))
        .when(!first, |cell| cell.border_l_1().border_color(rgb(BORDER)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(icon(glyph, CYAN, 20.0))
                .child(
                    div()
                        .text_size(px(LABEL))
                        .text_color(rgb(MUTED))
                        .child(label),
                ),
        )
        .child(
            div()
                .mt(px(5.0))
                .text_size(px(DISPLAY))
                .font_weight(WEIGHT_SEMIBOLD)
                .text_color(rgb(TEXT))
                .truncate()
                .child(value),
        )
        .children(detail.map(|text| {
            div()
                .mt(px(3.0))
                .text_size(px(META))
                .text_color(rgb(FAINT))
                .child(text)
        }))
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

/// One working surface: the kit's white card that every section sits in — 1 px
/// border, 13 px radius, and 24/26 px of internal padding so a heading and a
/// table share the same left edge.
pub(crate) fn work_surface() -> gpui::Div {
    div()
        .w_full()
        .rounded(px(RADIUS))
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .px(px(PAD_SURFACE_X))
        .py(px(PAD_SURFACE_Y))
        .flex()
        .flex_col()
}

/// The band that opens a management page's surface: what the page is for on the
/// left, the actions that belong to it on the right, one hairline under both.
/// The page's own name is already in the band above the workspace, so this one
/// carries the sentence and the buttons rather than repeating the title.
pub(crate) fn page_head(description: &'static str) -> gpui::Div {
    div()
        .w_full()
        .pb(px(19.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(20.0))
        .child(
            div()
                .flex_1()
                .min_w(px(240.0))
                .text_size(px(BODY))
                .line_height(px(21.0))
                .text_color(rgb(MUTED))
                .child(description),
        )
}

/// The header line of a data table: 37 px of soft surface carrying 11 px muted
/// column names, which is what lets a row of values stay at 12 px and still be
/// scannable.
pub(crate) fn table_head_row() -> gpui::Div {
    div()
        .w_full()
        .min_h(px(37.0))
        .px(px(14.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .bg(rgb(SURFACE_2))
        .text_size(px(META))
        .font_weight(WEIGHT_MEDIUM)
        .text_color(rgb(MUTED))
}

/// One column of a table: a fixed track when the content is a date or a count,
/// a share of the remaining width when it is free text.
pub(crate) fn table_col(label: &'static str, width: Option<f32>) -> impl IntoElement {
    div()
        .when_some(width, |cell, width| cell.w(px(width)).flex_shrink_0())
        .when(width.is_none(), |cell| cell.flex_1().min_w(px(0.0)))
        .truncate()
        .child(label)
}

pub(crate) fn traffic_chart(samples: Vec<(u64, u64)>, peak: u64) -> impl IntoElement {
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
pub(crate) fn legend(color: u32, label: &'static str, value: u64) -> impl IntoElement {
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

/// One rule row: the kit's 39 px data row, with the matcher's own type split
/// out of the condition so a wall of rules scans down two columns instead of
/// one long sentence.
pub(crate) fn rule_row(index: usize, rule: &RouteRuleSnapshot) -> gpui::AnyElement {
    let (kind, condition) = match rule.matcher.split_once(" · ") {
        Some((kind, condition)) => (kind.to_owned(), condition.to_owned()),
        None => ("条件".to_owned(), rule.matcher.clone()),
    };
    div()
        .id(format!("rule-row-{index}"))
        .w_full()
        .min_h(px(39.0))
        .px(px(13.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .when(index > 0, |row| row.border_t_1().border_color(rgb(BORDER)))
        .hover(|style| style.bg(rgb(ROW_HOVER)))
        .text_size(px(LABEL))
        .child(
            div()
                .w(px(40.0))
                .flex_shrink_0()
                .text_size(px(META))
                .text_color(rgb(FAINT))
                .child((index + 1).to_string()),
        )
        .child(
            div()
                .w(px(96.0))
                .flex_shrink_0()
                .truncate()
                .text_color(rgb(MUTED))
                .child(kind),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(TEXT))
                .child(condition),
        )
        .child(
            div()
                .w(px(170.0))
                .flex_shrink_0()
                .truncate()
                .font_weight(WEIGHT_MEDIUM)
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

/// One read-only setting: the name on the left, the value on the right of the
/// same row, so a list of them is one vertical sweep instead of a column of
/// label/value pairs stacked apart.
pub(crate) fn setting_line(label: &str, value: &str) -> impl IntoElement {
    div()
        .w_full()
        .min_h(px(68.0))
        .py(px(14.0))
        .flex()
        .items_center()
        .gap(px(16.0))
        .border_b_1()
        .border_color(rgb(BORDER))
        .child(
            div()
                .w(px(180.0))
                .flex_shrink_0()
                .text_size(px(BODY))
                .font_weight(WEIGHT_MEDIUM)
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

/// A short explanation that belongs to the rows under it rather than to one
/// row: the label carries the weight, the detail says what changes.
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
        .min_h(px(68.0))
        .py(px(14.0))
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
                .font_weight(WEIGHT_MEDIUM)
                .text_color(rgb(TEXT))
                .child(label),
        )
        .child(switch(
            id,
            on,
            Some(ClientCommand::UpdateSettings(patch)),
            cx,
        ))
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
