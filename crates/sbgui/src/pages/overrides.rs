//! The override page (覆写配置文件内容): a read-only view of the configuration the
//! core starts from, plus the one edit this client offers — switching a rule
//! fragment on or off.
//!
//! There is deliberately no JSON editor here, in either client:
//! `docs/adr/0023-client-config-overrides-share-the-merge-engine.md` and
//! `PRODUCT.md` lock the surface to "只读展示合并结果 + 规则片段开关". A hand-written
//! document is supported — a bare object reads as one implicit fragment — but it
//! is edited outside the client, the way the server's administrator templates are
//! edited outside the daemon.
//!
//! The page is two layers: [`build`] turns the engine snapshot into
//! [`OverridePage`], plain strings plus what each one means, and the render half
//! below only paints that. So every state the engine can publish is assertable
//! without a window, and this page and the terminal client's
//! `view/config_override.rs` read the same three snapshot fields
//! (`override_summary`, `override_error`, `effective_outline`) and cannot disagree
//! about them. Two rules the TUI version already learned apply here:
//!
//! * A malformed file is the loudest thing on the page, in the engine's own words.
//!   A paraphrase is how a broken override becomes an invisible one.
//! * Reserved fields the override reached for are listed per fragment and again in
//!   total, because the merge does apply them and the start writes them back.
//!   Reported, never silently ignored.

use client_core::ClientCommand;
use client_core::state::{ClientSnapshot, OverrideFragmentSummary, OverrideSummary};
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px, rgb,
};

use crate::components::{page_head, switch, work_surface};
use crate::lang::{Locale, reserved_reason};
use crate::state::{Sbgui, Tone};
use crate::theme::{
    AMBER, BODY, BORDER, BORDER_STRONG, CYAN, DANGER, FAINT, GAP_ITEM, LABEL, META, MUTED, RADIUS,
    RADIUS_CONTROL, ROW_HOVER, SURFACE, SURFACE_2, TEXT, WEIGHT_MEDIUM, WEIGHT_SEMIBOLD,
};
use crate::tr;

/// What a line means, decided in one place so the colour and the text can never
/// be two different judgements. The text stays a plain value a test can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shade {
    Muted,
    Text,
    Alert,
    Danger,
}

impl Shade {
    fn color(self) -> u32 {
        match self {
            Self::Muted => MUTED,
            Self::Text => TEXT,
            Self::Alert => AMBER,
            Self::Danger => DANGER,
        }
    }
}

/// One text line of the page: what it says, how loudly, and whether it is data
/// rather than prose.
#[derive(Debug)]
pub(crate) struct Row {
    pub text: String,
    pub shade: Shade,
    /// Set for a value that has to be read exactly — the override file's own
    /// 64-character name — and must therefore keep a monospace face.
    pub mono: bool,
}

impl Row {
    fn new(shade: Shade, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shade,
            mono: false,
        }
    }

    fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
}

/// One fragment of the override file: a card, with a switch whenever the file
/// has a fragment list to switch.
pub(crate) struct Fragment {
    /// The file's own label, with the id it is addressed by beside it.
    pub title: String,
    /// 启用 / 停用 in words, so the switch is not the only thing carrying state.
    pub state: String,
    pub enabled: bool,
    /// `None` for a hand-written bare object, whose single implicit fragment the
    /// engine refuses to rewrite. No command means no switch is drawn at all: a
    /// control that answers to nothing is the bug, and the page already says in
    /// the same words why there is nothing to switch.
    pub command: Option<ClientCommand>,
    /// The changed pointers and the reserved ones, one row each.
    pub details: Vec<Row>,
}

/// Everything the page draws, in the order it draws it.
pub(crate) struct OverridePage {
    /// An unusable file owns the page: nothing below its error was applied.
    pub broken: bool,
    pub status: Vec<Row>,
    pub fragments: Vec<Fragment>,
    /// The reserved fields the *enabled* fragments reached for, in total.
    pub reserved: Vec<Row>,
    /// The redaction label and the source line, above the outline proper.
    pub outline_head: Vec<Row>,
    pub outline_lines: Vec<String>,
    /// The profile whose file `清除覆写` would delete, when there is one.
    pub clear_profile: Option<String>,
    /// The armed confirmation's sentence: clearing is the one action here that
    /// pressing it again cannot undo.
    pub clear_warning: Option<String>,
}

impl OverridePage {
    /// Every string the page can put on screen. The redaction assertion walks
    /// this, the way the terminal client asserts it of every drawn frame: the
    /// outline, the fragment list, the status block and the confirmation bar are
    /// four different paths from the snapshot to the screen, and a secret
    /// escaping down any one of them is the same bug.
    #[cfg(test)]
    fn all_text(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .status
            .iter()
            .chain(self.reserved.iter())
            .chain(self.outline_head.iter())
            .map(|row| row.text.clone())
            .collect();
        lines.extend(self.outline_lines.clone());
        for fragment in &self.fragments {
            lines.push(fragment.title.clone());
            lines.push(fragment.state.clone());
            lines.extend(fragment.details.iter().map(|row| row.text.clone()));
            if let Some(ClientCommand::ToggleOverrideFragment { profile, id, .. }) =
                &fragment.command
            {
                lines.push(profile.clone());
                lines.push(id.clone());
            }
        }
        lines.extend(self.clear_profile.clone());
        lines.extend(self.clear_warning.clone());
        lines
    }
}

/// Turns the snapshot into the page. `armed` is the view's own "the clear
/// confirmation is showing" flag; everything else is a function of the engine.
pub(crate) fn build(snapshot: &ClientSnapshot, locale: Locale, armed: bool) -> OverridePage {
    let summary = snapshot.override_summary.as_ref();
    let active = snapshot
        .active_profile
        .as_ref()
        .map(|profile| profile.name.clone());
    let broken = snapshot.override_error.is_some();
    // Nothing to delete when there is neither a file nor a refused one; the
    // button itself would then offer an action with no object.
    let clear_profile = active.filter(|_| summary.is_some() || broken);
    let clear_warning = if armed && clear_profile.is_some() {
        let name = snapshot
            .active_profile
            .as_ref()
            .map(|profile| profile.name.as_str())
            .unwrap_or("?");
        Some(tr!(
            locale,
            format!("确认删除档案 {name} 的覆写文件？不可撤销。"),
            format!("Delete the override file of profile {name}? This cannot be undone.")
        ))
    } else {
        None
    };
    OverridePage {
        broken,
        status: status_rows(snapshot, locale),
        fragments: summary
            .map(|summary| fragments(summary, locale))
            .unwrap_or_default(),
        reserved: reserved_rows(snapshot, locale),
        outline_head: outline_head(snapshot, locale),
        outline_lines: outline_lines(snapshot),
        clear_profile,
        clear_warning,
    }
}

/// The block above the fragment list: whose file is in force, how big it is, and
/// why it might not be. The error branch returns first, so an unusable override
/// cannot be buried under an empty list.
fn status_rows(snapshot: &ClientSnapshot, locale: Locale) -> Vec<Row> {
    if let Some(error) = snapshot.override_error.as_deref() {
        return vec![
            Row::new(
                Shade::Danger,
                tr!(
                    locale,
                    "覆写无效 · 内核不会启动",
                    "Override refused · the core will not start"
                ),
            ),
            // Verbatim: the engine's message already names the file, the field
            // path and the line and column it gave up on.
            Row::new(Shade::Danger, error.to_owned()),
            Row::new(
                Shade::Muted,
                tr!(
                    locale,
                    "文件原样留在磁盘上，没有被改写，也没有被部分应用。",
                    "The file is still on disk untouched: not rewritten, and not partly applied."
                ),
            ),
        ];
    }
    let Some(summary) = snapshot.override_summary.as_ref() else {
        let profile = snapshot
            .active_profile
            .as_ref()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| {
                tr!(
                    locale,
                    "（尚无档案：先导入订阅）",
                    "(no profile yet: import a subscription first)"
                )
                .to_owned()
            });
        return vec![
            Row::new(
                Shade::Text,
                format!("{}  {profile}", tr!(locale, "档案", "Profile")),
            ),
            Row::new(
                Shade::Muted,
                tr!(
                    locale,
                    "覆写  无覆写文件（数据目录的 overrides/ 下没有本档案的 JSON）",
                    "Override  no file for this profile under the data directory's overrides/"
                ),
            ),
            Row::new(
                Shade::Muted,
                tr!(
                    locale,
                    "内核只用订阅原文，加上客户端自己写回的保留字段。",
                    "The core starts from the subscription alone, plus the reserved fields the client writes back."
                ),
            ),
        ];
    };
    // The rounded form only says something once the file is past a kilobyte;
    // below that it just repeats the byte count.
    let size = if summary.bytes < 1024 {
        tr!(
            locale,
            format!("{} 字节", summary.bytes),
            format!("{} bytes", summary.bytes)
        )
    } else {
        tr!(
            locale,
            format!(
                "{}（{} 字节）",
                client_core::format::human_bytes(summary.bytes),
                summary.bytes
            ),
            format!(
                "{} ({} bytes)",
                client_core::format::human_bytes(summary.bytes),
                summary.bytes
            )
        )
    };
    let mut rows = vec![
        Row::new(
            Shade::Text,
            format!("{}  {}", tr!(locale, "档案", "Profile"), summary.profile),
        ),
        Row::new(
            Shade::Text,
            tr!(
                locale,
                format!(
                    "覆写  {}/{} 片段启用 · {size}",
                    summary.enabled_count(),
                    summary.fragments.len()
                ),
                format!(
                    "Override  {}/{} fragments on · {size}",
                    summary.enabled_count(),
                    summary.fragments.len()
                )
            ),
        ),
        Row::new(
            Shade::Muted,
            tr!(
                locale,
                "生效  开关只改这个文件，下次启动内核时才生效",
                "When it applies  a switch only rewrites this file; the next core start reads it"
            ),
        ),
        // The file's name is a 64-digit sha256 nobody can derive by hand, so it
        // gets a line of its own with no label competing for the same width.
        Row::new(
            Shade::Muted,
            tr!(
                locale,
                "文件  数据目录/overrides/ 下本档案名的 sha256：",
                "File  the sha256 of the profile's name, under the data directory's overrides/:"
            ),
        ),
        Row::new(Shade::Muted, summary.file_name.clone()).mono(),
    ];
    if summary.implicit {
        rows.push(Row::new(
            Shade::Alert,
            tr!(
                locale,
                "手写整份覆写：没有片段列表，无片段可开关。",
                "A hand-written whole document: there is no fragment list, so nothing to switch."
            ),
        ));
    }
    rows
}

/// One card per fragment, in the order the file lists them (highest precedence
/// first, the same order the merged rules end up in).
fn fragments(summary: &OverrideSummary, locale: Locale) -> Vec<Fragment> {
    summary
        .fragments
        .iter()
        .map(|fragment| Fragment {
            title: if fragment.label == fragment.id {
                fragment.id.clone()
            } else {
                format!("{}（{}）", fragment.label, fragment.id)
            },
            state: tr!(
                locale,
                if fragment.enabled { "启用" } else { "停用" },
                if fragment.enabled { "On" } else { "Off" }
            )
            .to_owned(),
            enabled: fragment.enabled,
            command: (!summary.implicit).then(|| {
                ClientCommand::ToggleOverrideFragment {
                    profile: summary.profile.clone(),
                    id: fragment.id.clone(),
                    // `None` asks the engine for the flip, which keeps this UI
                    // from tracking a flag it does not own.
                    enabled: None,
                }
            }),
            details: fragment_rows(fragment, locale),
        })
        .collect()
}

fn fragment_rows(fragment: &OverrideFragmentSummary, locale: Locale) -> Vec<Row> {
    let mut rows = Vec::new();
    let changes = if fragment.changes.is_empty() {
        tr!(
            locale,
            "（空片段，不改动任何字段）",
            "(an empty fragment: it changes no field)"
        )
        .to_owned()
    } else {
        // Sorted here rather than taken in the file's own order: `serde_json`'s
        // map order depends on whether the `preserve_order` feature is unified
        // into the build, so walk order would make this page disagree with the
        // terminal client's over the same file.
        let mut paths = fragment.changes.clone();
        paths.sort();
        paths.join("、")
    };
    let rules = if fragment.rules > 0 {
        tr!(
            locale,
            format!(" · +{} 条规则", fragment.rules),
            format!(" · +{} rules", fragment.rules)
        )
        .to_owned()
    } else {
        String::new()
    };
    rows.push(Row::new(
        Shade::Text,
        format!("{}  {changes}{rules}", tr!(locale, "改动", "Changes")),
    ));
    // The summary carries a fragment's reserved fields whether or not it is on,
    // because that is what the file asks for. Only an enabled one can actually
    // be refused, so the warning colour stays with the fragments that will be
    // written back at the next start; the rest read as a condition.
    for path in &fragment.reserved {
        let (wording, shade) = if fragment.enabled {
            (
                tr!(
                    locale,
                    format!("⚠ 保留  {path}（客户端写回，不生效）"),
                    format!("⚠ Reserved  {path} (the client writes the original back; refused)")
                ),
                Shade::Danger,
            )
        } else {
            (
                tr!(
                    locale,
                    format!("保留字段  {path}（启用后会被客户端写回）"),
                    format!("Reserved  {path} (enabling this fragment would be refused)")
                ),
                Shade::Muted,
            )
        };
        rows.push(Row::new(shade, wording.to_owned()));
    }
    rows
}

/// The block under the fragment list: reserved-field refusals, repeated in total
/// so a per-fragment warning cannot be the only place they are written.
fn reserved_rows(snapshot: &ClientSnapshot, locale: Locale) -> Vec<Row> {
    let Some(summary) = snapshot.override_summary.as_ref() else {
        return vec![readonly_note(locale)];
    };
    let reserved = summary.reserved();
    let mut rows = Vec::new();
    if reserved.is_empty() {
        rows.push(Row::new(
            Shade::Muted,
            tr!(
                locale,
                "保留字段  没有启用中的片段改动它们",
                "Reserved fields  no enabled fragment changes them"
            ),
        ));
    } else {
        rows.push(Row::new(
            Shade::Danger,
            tr!(
                locale,
                "⚠ 保留字段被覆写改动 · 启动内核时客户端写回原值，并在事件行再次报告",
                "⚠ Reserved fields were overridden · the start writes the originals back and the event list reports them again"
            ),
        ));
        for path in reserved {
            rows.push(Row::new(
                Shade::Alert,
                format!("{path} — {}", reserved_reason(&path, locale)),
            ));
        }
    }
    rows.push(readonly_note(locale));
    rows
}

/// Where an override comes from at all, said on every state of the page: this UI
/// reads the configuration and switches fragments, and it never opens an editor.
fn readonly_note(locale: Locale) -> Row {
    Row::new(
        Shade::Muted,
        tr!(
            locale,
            "本页只读，不编辑 JSON：要改覆写就在数据目录的 overrides/ 下改这个文件，下次启动内核时生效。",
            "This page is read-only and edits no JSON: to change an override, edit that file under the data directory's overrides/ and start the core again."
        ),
    )
}

/// The outline panel's head: the redaction has to be stated before the first line
/// is read, not after the user assumes they are looking at the whole file.
fn outline_head(snapshot: &ClientSnapshot, locale: Locale) -> Vec<Row> {
    let mut rows = vec![Row::new(
        Shade::Alert,
        tr!(
            locale,
            "已脱敏 / redacted：密钥与订阅凭据不在此显示",
            "Redacted: secrets and subscription credentials never appear here"
        ),
    )];
    if snapshot.effective_outline.is_empty() {
        rows.push(Row::new(
            Shade::Muted,
            tr!(
                locale,
                "（尚无生效配置；启动一次内核，这里会显示订阅 + 覆写 + 保留字段的合并结果）",
                "(no effective configuration yet: start the core once and the merge of subscription, override and reserved fields appears here)"
            ),
        ));
        return rows;
    }
    rows.push(Row::new(
        Shade::Muted,
        if snapshot.core_running {
            tr!(
                locale,
                "来源  运行中内核的配置（active-config.json）",
                "Source  the configuration the running core started from (active-config.json)"
            )
        } else {
            tr!(
                locale,
                "来源  上次启动写入的配置（active-config.json）",
                "Source  the configuration the last start wrote (active-config.json)"
            )
        }
        .to_owned(),
    ));
    rows
}

/// The engine's own lines, with nothing added, removed or re-serialised: this is
/// `config_outline`'s redacted text, and the redaction lives there.
fn outline_lines(snapshot: &ClientSnapshot) -> Vec<String> {
    // Sorted for the same reason as the fragment paths: the outline is a walk of
    // the configuration's maps, and that walk's order is a property of the
    // build's `serde_json` features, not of the configuration.
    let mut outline = snapshot.effective_outline.clone();
    outline.sort();
    outline
}

/// One read-only text line.
fn row_element(item: &Row, key: impl Into<String>) -> impl IntoElement {
    div()
        .id(key.into())
        .w_full()
        .text_size(px(if item.mono { META } else { BODY }))
        .line_height(px(if item.mono { 18.0 } else { 20.0 }))
        .text_color(rgb(item.shade.color()))
        .when(item.mono, |style| {
            style.font_family("Cascadia Mono, Consolas")
        })
        .child(item.text.clone())
}

impl Sbgui {
    // ----------------------------------------------------------- overrides

    pub(crate) fn overrides(&self, cx: &mut Context<Self>) -> gpui::Div {
        let locale = self.locale;
        let page = build(&self.snapshot, locale, self.confirm_clear_override);
        let clear = page.clear_profile.clone();

        work_surface()
            .child(
                page_head(tr!(
                    locale,
                    "本档案的覆写文件与它改动了什么。生效配置只读展示，规则片段可以开关。",
                    "This profile's override file and what it changes. The merged configuration is shown read-only; rule fragments switch on and off."
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .children(page.clear_warning.clone().map(|warning| {
                            // The second confirmation is a bar that names the
                            // profile whose file it is about to remove, not a
                            // label that quietly changes meaning between one
                            // click and the next.
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.0))
                                .px(px(12.0))
                                .py(px(8.0))
                                .rounded(px(RADIUS_CONTROL))
                                .border_1()
                                .border_color(rgb(DANGER))
                                .bg(rgb(SURFACE))
                                .child(
                                    div()
                                        .text_size(px(LABEL))
                                        .line_height(px(19.0))
                                        .font_weight(WEIGHT_MEDIUM)
                                        .text_color(rgb(DANGER))
                                        .child(warning),
                                )
                                .child(self.button(
                                    "override-clear-cancel",
                                    tr!(locale, "取消", "Cancel"),
                                    Tone::Neutral,
                                    None,
                                    cx,
                                    |view, cx| {
                                        view.confirm_clear_override = false;
                                        cx.notify();
                                    },
                                ))
                                .child(self.button(
                                    "override-clear-yes",
                                    tr!(locale, "确认删除覆写", "Delete override"),
                                    Tone::Danger,
                                    None,
                                    cx,
                                    move |view, cx| {
                                        if let Some(profile) = clear.clone() {
                                            view.confirm_clear_override = false;
                                            view.send(ClientCommand::ClearOverride(profile));
                                        }
                                        cx.notify();
                                    },
                                ))
                        }))
                        .children(page.clear_profile.is_some().then(|| {
                            self.button(
                                "override-clear",
                                tr!(locale, "清除覆写", "Clear override"),
                                Tone::Danger,
                                None,
                                cx,
                                move |view, cx| {
                                    // One click arms the bar; it never deletes on
                                    // the click that asks.
                                    view.confirm_clear_override = !view.confirm_clear_override;
                                    cx.notify();
                                },
                            )
                        }))
                        .child(self.action(
                            "override-refresh",
                            tr!(locale, "刷新状态", "Refresh"),
                            Tone::Neutral,
                            cx,
                            ClientCommand::Refresh,
                        )),
                ),
            )
            // The state block: with an unusable file this is the whole argument,
            // so it wears the danger edge and nothing is listed above it.
            .child(
                div()
                    .id("override-status")
                    .mt(px(18.0))
                    .w_full()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(rgb(if page.broken {
                        DANGER
                    } else {
                        BORDER
                    }))
                    .bg(rgb(if page.broken {
                        SURFACE
                    } else {
                        SURFACE_2
                    }))
                    .px(px(16.0))
                    .py(px(14.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(
                        page.status
                            .iter()
                            .enumerate()
                            .map(|(index, item)| row_element(item, format!("override-status-{index}"))),
                    ),
            )
            .children(
                (!page.fragments.is_empty()).then(|| {
                    div()
                        .mt(px(GAP_ITEM))
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap(px(GAP_ITEM))
                        .children(
                            page.fragments
                                .iter()
                                .enumerate()
                                .map(|(index, fragment)| fragment_card(index, fragment, cx)),
                        )
                }),
            )
            .child(
                div()
                    .mt(px(18.0))
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(
                        page.reserved
                            .iter()
                            .enumerate()
                            .map(|(index, item)| row_element(item, format!("override-note-{index}"))),
                    ),
            )
            // The outline is the longest thing here and the least frequently
            // read, so it scrolls inside a panel of its own instead of pushing
            // the switches off screen at the minimum window size.
            .child(
                div()
                    .mt(px(18.0))
                    .w_full()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .min_h(px(37.0))
                            .px(px(14.0))
                            .flex()
                            .items_center()
                            .bg(rgb(SURFACE_2))
                            .text_size(px(META))
                            .font_weight(WEIGHT_MEDIUM)
                            .text_color(rgb(MUTED))
                            .child(tr!(
                                locale,
                                "生效配置 · 已脱敏 · 只读",
                                "Effective configuration · redacted · read-only"
                            )),
                    )
                    .child(
                        div()
                            .id("override-outline")
                            .h(px(240.0))
                            .px(px(14.0))
                            .py(px(12.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .children(
                                page.outline_head
                                    .iter()
                                    .enumerate()
                                    .map(|(index, item)| {
                                        row_element(item, format!("override-outline-{index}"))
                                    }),
                            )
                            .children(page.outline_lines.iter().enumerate().map(
                                |(index, line)| {
                                    div()
                                        .id(format!("override-outline-line-{index}"))
                                        .w_full()
                                        .font_family("Cascadia Mono, Consolas")
                                        .text_size(px(META))
                                        .line_height(px(18.0))
                                        .text_color(rgb(TEXT))
                                        .child(line.clone())
                                },
                            )),
                    )
                    .child(
                        div()
                            .w_full()
                            .px(px(14.0))
                            .py(px(8.0))
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(SURFACE))
                            .text_size(px(META))
                            .text_color(rgb(FAINT))
                            .child(tr!(
                                locale,
                                format!(
                                    "{} 行结构，不含配置正文",
                                    page.outline_lines.len()
                                ),
                                format!(
                                    "{} structural lines, never the configuration text",
                                    page.outline_lines.len()
                                )
                            )),
                    ),
            )
    }
}

/// One fragment card: the switch and its state in words on the head line, the
/// changed and refused pointers under it. `AnyElement` rather than
/// `impl IntoElement` because the caller builds the cards in a map over the
/// page's fragments, and an opaque return type would borrow the context it was
/// built with.
fn fragment_card(index: usize, fragment: &Fragment, cx: &mut Context<Sbgui>) -> gpui::AnyElement {
    div()
        .id(format!("override-fragment-{index}"))
        .w_full()
        .rounded(px(RADIUS_CONTROL + 2.0))
        .border_1()
        .border_color(rgb(if fragment.enabled {
            BORDER_STRONG
        } else {
            BORDER
        }))
        .bg(rgb(SURFACE))
        .px(px(14.0))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .hover(|style| style.bg(rgb(ROW_HOVER)))
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(160.0))
                        .text_size(px(BODY))
                        .font_weight(WEIGHT_SEMIBOLD)
                        .text_color(rgb(TEXT))
                        .child(fragment.title.clone()),
                )
                .child(
                    div()
                        .text_size(px(LABEL))
                        .font_weight(WEIGHT_MEDIUM)
                        .text_color(rgb(if fragment.enabled { CYAN } else { FAINT }))
                        .child(fragment.state.clone()),
                )
                .children(fragment.command.clone().map(|command| {
                    switch(
                        format!("override-switch-{index}"),
                        fragment.enabled,
                        Some(command),
                        cx,
                    )
                })),
        )
        .children(
            fragment
                .details
                .iter()
                .enumerate()
                .map(|(row, item)| row_element(item, format!("override-detail-{index}-{row}")))
                .collect::<Vec<_>>(),
        )
        .into_any_element()
}

/// The same four states the terminal client's `view::fixtures` pins, on purpose:
/// one document with a disabled fragment and a fragment that reaches reserved
/// fields, a hand-written bare object, and a file that cannot be read. Both
/// clients are then tested against the states the engine can actually publish.
#[cfg(test)]
mod fixtures {
    use std::path::PathBuf;

    use client_core::config_override::{OVERRIDE_DIRECTORY, ProfileOverride};
    use client_core::state::OverrideSummary;

    /// A literal stand-in of the real length: the file is named after the
    /// sha256 of the profile name, and hashing the fixture's name would make
    /// the assertions depend on the digest.
    pub(crate) const OVERRIDE_FILE: &str =
        "9f2c4a7e1b8d35f6a0c7e2d4b9f83c5e6d0a2f47b1c93d8e0a5c7f2419db6e3";

    pub(crate) const FRAGMENT_OVERRIDE: &str = r#"{"fragments":[
        {"id":"private-direct","label":"内网直连","enabled":true,
         "overlay":{"route":{"rules":[{"action":"direct","ip_cidr":["10.0.0.0/8"]},
                                      {"action":"direct","domain_suffix":["internal.example"]}]}}},
        {"id":"custom-dns","label":"自建 DNS","enabled":false,
         "overlay":{"dns":{"servers":["223.5.5.5"],"final":"local"}}},
        {"id":"take-over","label":"接管控制通道","enabled":true,
         "overlay":{"experimental":{"clash_api":{"secret":"OVERRIDE-MUST-NOT-PRINT-77aa"}},
                    "inbounds":[{"type":"mixed","listen_port":1080}],
                    "route":{"auto_detect_interface":false}}}
    ]}"#;

    pub(crate) const BARE_OVERRIDE: &str = r#"{"dns":{"servers":["223.5.5.5"],"final":"local"}}"#;

    pub(crate) const BROKEN_OVERRIDE: &str = r#"{ "route": {"rules": }"#;

    /// The merged configuration the core starts from, with secrets planted where
    /// a subscription really puts them.
    pub(crate) const EFFECTIVE_CONFIG: &str = r#"{
      "log": {"level": "info", "timestamp": true},
      "dns": {"tag": "dns-in", "final": "dns-out", "servers": ["223.5.5.5", "https://dns.example/resolve"]},
      "inbounds": [{"type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": 2080},
                   {"type": "tun", "tag": "tun-in"}],
      "outbounds": [{"tag": "🚀节点选择", "type": "selector", "outbounds": ["东京-A"]},
                    {"tag": "东京-A", "type": "vless", "uuid": "SUBSCRIPTION-CREDENTIAL-9e2f7a1c",
                     "password": "SUBSCRIPTION-CREDENTIAL-9e2f7a1c"}],
      "route": {"auto_detect_interface": true, "final": "🚀节点选择",
                "object_cache_url": "https://sbctl.test/sub/SUBSCRIPTION-CREDENTIAL-9e2f7a1c/geoip-cn.srs",
                "rules": [{"action": "direct"}, {"action": "proxy"}],
                "rule_set": [{"tag": "geoip-cn", "type": "remote",
                              "url": "https://sbctl.test/sub/SUBSCRIPTION-CREDENTIAL-9e2f7a1c/geoip-cn.srs"}]},
      "experimental": {"clash_api": {"external_controller": "127.0.0.1:41223",
                                     "secret": "CLASH-API-SECRET-4f9b7c1d"}}
    }"#;

    /// The clash_api secret, the subscription credential in the configuration,
    /// and the secret an override fragment tries to write into `clash_api`: that
    /// last one proves a reserved field is reported by path and never by value.
    pub(crate) const SECRET_MARKERS: &[&str] = &[
        "CLASH-API-SECRET-4f9b7c1d",
        "SUBSCRIPTION-CREDENTIAL-9e2f7a1c",
        "OVERRIDE-MUST-NOT-PRINT-77aa",
    ];

    fn path() -> PathBuf {
        PathBuf::from(format!("<DATA_DIR>/{OVERRIDE_DIRECTORY}/{OVERRIDE_FILE}"))
    }

    /// The summary the engine puts in the snapshot, built by the real parser.
    pub(crate) fn summary(text: &str, profile: &str) -> OverrideSummary {
        ProfileOverride::parse(text, &path())
            .unwrap_or_else(|error| panic!("the fixture must parse: {error}"))
            .summary(profile)
    }

    /// The `override_error` the engine publishes for an unreadable file.
    pub(crate) fn load_error(text: &str) -> String {
        ProfileOverride::parse(text, &path())
            .expect_err("the fixture is invalid")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use client_core::state::{ProfileSummary, config_outline};

    fn snapshot(summary: Option<OverrideSummary>) -> ClientSnapshot {
        ClientSnapshot {
            active_profile: summary.as_ref().map(|summary| ProfileSummary {
                name: summary.profile.clone(),
                ..Default::default()
            }),
            override_summary: summary,
            ..ClientSnapshot::default()
        }
    }

    fn fragment_snapshot() -> ClientSnapshot {
        snapshot(Some(summary(FRAGMENT_OVERRIDE, "内网订阅")))
    }

    fn texts(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|row| row.text.clone()).collect()
    }

    fn card_lines(page: &OverridePage) -> Vec<String> {
        page.fragments
            .iter()
            .flat_map(|card| texts(&card.details))
            .collect()
    }

    /// The file's existence and size, the per-fragment state, rule count, changed
    /// pointers and refused fields: all of it on one page at once.
    #[test]
    fn a_fragment_list_shows_state_rule_count_changed_paths_and_refused_paths() {
        let page = build(&fragment_snapshot(), Locale::Zh, false);
        assert_eq!(page.fragments.len(), 3, "one card per fragment");
        let head = texts(&page.status);
        assert!(
            head.iter().any(|line| line.contains("2/3 片段启用")),
            "the header counts what merges, not what exists: {head:?}"
        );
        assert!(
            head.iter().any(|line| line.contains("字节")),
            "the size of the file on disk is stated: {head:?}"
        );
        assert!(
            head.iter().any(|line| line == OVERRIDE_FILE),
            "the sha256 file name is printed, or the user cannot find the file: {head:?}"
        );
        let titles: Vec<&str> = page.fragments.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "内网直连（private-direct）",
                "自建 DNS（custom-dns）",
                "接管控制通道（take-over）"
            ],
            "the readable label and the id a toggle addresses, on the same line"
        );
        let states: Vec<&str> = page.fragments.iter().map(|f| f.state.as_str()).collect();
        assert_eq!(states, ["启用", "停用", "启用"]);
        let lines = card_lines(&page);
        assert!(
            lines
                .iter()
                .any(|line| line == "改动  /route/rules(+2) · +2 条规则"),
            "the rule count rides on the changed-path line: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "改动  /dns/final、/dns/servers(1 项)"),
            "field paths, not a JSON dump: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "⚠ 保留  /experimental/clash_api/secret（客户端写回，不生效）"),
            "the refused path is named on the fragment that asked for it: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "⚠ 保留  /inbounds（客户端写回，不生效）"),
            "every reserved pointer the enabled fragment touches is listed: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("MUST-NOT-PRINT")),
            "a reserved field is reported by path only, never by value: {lines:?}"
        );
        let notes = texts(&page.reserved);
        assert!(
            notes
                .iter()
                .any(|line| line.starts_with("⚠ 保留字段被覆写改动")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|line| line.starts_with("/route/auto_detect_interface — 本地运行")),
            "the path leads the row, so a cut reason still names the field: {notes:?}"
        );
        assert!(
            notes.iter().any(|line| line.contains("本页只读")),
            "{notes:?}"
        );
    }

    /// The command, not just the picture of one: `enabled: None` is what asks the
    /// engine to flip the flag, so the UI never tracks state it does not own.
    #[test]
    fn a_switch_asks_the_engine_to_flip_one_fragment_by_id() {
        let page = build(&fragment_snapshot(), Locale::Zh, false);
        assert_eq!(
            page.fragments[0].command,
            Some(ClientCommand::ToggleOverrideFragment {
                profile: "内网订阅".to_owned(),
                id: "private-direct".to_owned(),
                enabled: None,
            })
        );
        for card in &page.fragments {
            assert!(
                matches!(
                    card.command,
                    Some(ClientCommand::ToggleOverrideFragment { enabled: None, .. })
                ),
                "a disabled fragment is switched the same way as an enabled one: {}",
                card.title
            );
        }
    }

    /// A fragment with no `label` reads as its id in the file, and the card must
    /// not print `private-direct（private-direct）`.
    #[test]
    fn a_fragment_with_no_label_of_its_own_reads_as_its_id() {
        let mut built = summary(FRAGMENT_OVERRIDE, "内网订阅");
        built.fragments[0].label = built.fragments[0].id.clone();
        let page = build(&snapshot(Some(built)), Locale::Zh, false);
        assert_eq!(page.fragments[0].title, "private-direct");
    }

    /// The size line is the page's answer to "is there a file, and how big"; a
    /// rounded form that only repeats the byte count is noise, and above a
    /// kilobyte it is the readable half.
    #[test]
    fn the_size_line_says_the_byte_count_once() {
        let page = build(&fragment_snapshot(), Locale::Zh, false);
        let line = texts(&page.status)
            .into_iter()
            .find(|line| line.contains("片段启用"))
            .expect("the size line");
        assert!(
            !line.contains(" B（"),
            "the rounded form repeated the byte count: {line}"
        );
        let mut big = summary(FRAGMENT_OVERRIDE, "内网订阅");
        big.bytes = 4096;
        let page = build(&snapshot(Some(big.clone())), Locale::Zh, false);
        let zh = texts(&page.status)
            .into_iter()
            .find(|line| line.contains("片段启用"))
            .expect("the size line");
        assert!(zh.contains("4.0 KiB（4096 字节）"), "{zh}");
        let page = build(&snapshot(Some(big)), Locale::En, false);
        let en = texts(&page.status)
            .into_iter()
            .find(|line| line.contains("fragments on"))
            .expect("the size line");
        assert!(en.contains("4.0 KiB (4096 bytes)"), "{en}");
    }

    /// The file's name is the only way to a hand-written override, and it is
    /// 64 characters of hex: at the minimum window width a proportional face
    /// breaks it mid-token, so it keeps a monospace one.
    #[test]
    fn the_sha256_file_name_keeps_one_monospace_line() {
        let page = build(&fragment_snapshot(), Locale::Zh, false);
        let row = page
            .status
            .iter()
            .find(|row| row.text == OVERRIDE_FILE)
            .expect("the file name is printed");
        assert!(row.mono, "the name would break across two lines: {row:?}");
    }

    #[test]
    fn a_disabled_fragment_loses_its_rule_and_its_conflict() {
        let mut built = summary(FRAGMENT_OVERRIDE, "内网订阅");
        built
            .fragments
            .iter_mut()
            .for_each(|fragment| fragment.enabled = fragment.id == "custom-dns");
        let page = build(&snapshot(Some(built)), Locale::Zh, false);
        let notes = texts(&page.reserved);
        assert!(
            notes
                .iter()
                .any(|line| line.contains("没有启用中的片段改动它们")),
            "the only conflict came from the now-disabled fragment: {notes:?}"
        );
        let lines = card_lines(&page);
        assert!(
            !lines.iter().any(|line| line.contains("⚠ 保留")),
            "no enabled fragment reaches for a reserved field, so nothing is refused: {lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "保留字段  /inbounds（启用后会被客户端写回）"),
            "the switch that is off still says what it would ask for: {lines:?}"
        );
        assert!(
            texts(&page.status)
                .iter()
                .any(|line| line.contains("1/3 片段启用")),
            "the header follows the switches: {:?}",
            texts(&page.status)
        );
    }

    #[test]
    fn a_bare_object_says_there_is_nothing_to_toggle() {
        let page = build(
            &snapshot(Some(summary(BARE_OVERRIDE, "手写档案"))),
            Locale::Zh,
            false,
        );
        assert!(
            page.status
                .iter()
                .any(|row| row.text == "手写整份覆写：没有片段列表，无片段可开关。"),
            "{:?}",
            texts(&page.status)
        );
        assert_eq!(page.fragments.len(), 1, "the whole file is one fragment");
        assert!(
            page.fragments[0].command.is_none(),
            "the engine refuses to rewrite a hand-written file, so the switch stays inert \
             rather than firing a command whose only outcome is a failure the page already said"
        );
        assert!(page.fragments[0].enabled);
    }

    #[test]
    fn a_malformed_override_is_printed_in_the_engine_s_own_words_and_owns_the_page() {
        // The engine's loader produces the message; the page only decides how
        // loudly to say it, so nothing here can paraphrase a field path away.
        let error = load_error(BROKEN_OVERRIDE);
        let page = build(
            &ClientSnapshot {
                active_profile: Some(ProfileSummary {
                    name: "内网订阅".to_owned(),
                    ..Default::default()
                }),
                override_error: Some(error.clone()),
                ..ClientSnapshot::default()
            },
            Locale::Zh,
            false,
        );
        assert!(page.broken);
        let head = texts(&page.status);
        assert_eq!(head[0], "覆写无效 · 内核不会启动");
        assert_eq!(head[1], error, "verbatim, not summarized");
        assert_eq!(page.status[1].shade, Shade::Danger);
        assert!(
            head.iter().any(|line| line.contains(OVERRIDE_FILE)),
            "the message names the file it could not read: {head:?}"
        );
        assert!(
            head.iter()
                .any(|line| line.contains("不是有效的 JSON") && line.contains("第 1 行")),
            "{head:?}"
        );
        assert!(
            head.iter()
                .any(|line| line.contains("没有被改写，也没有被部分应用")),
            "{head:?}"
        );
        assert!(
            page.fragments.is_empty(),
            "nothing was applied, so there is nothing to list"
        );
        assert!(
            page.clear_profile.is_some(),
            "a refused file is the state that most needs the way out"
        );
    }

    /// Clearing is the one action here with no undo, so the click that asks must
    /// not be the click that deletes, and the bar names whose file would go.
    #[test]
    fn clearing_an_override_asks_once_and_names_the_profile() {
        let with_file = fragment_snapshot();
        let page = build(&with_file, Locale::Zh, false);
        assert_eq!(page.clear_profile.as_deref(), Some("内网订阅"));
        assert!(
            page.clear_warning.is_none(),
            "one click has not armed anything yet"
        );
        let warning = build(&with_file, Locale::Zh, true)
            .clear_warning
            .expect("the armed bar");
        assert!(warning.contains("内网订阅"), "{warning}");
        assert!(warning.contains("不可撤销"), "{warning}");
        assert!(
            build(&snapshot(None), Locale::Zh, true)
                .clear_warning
                .is_none(),
            "with no file there is nothing to confirm deleting, so no bar appears"
        );
    }

    #[test]
    fn no_override_file_is_a_state_rather_than_a_problem() {
        let page = build(&snapshot(None), Locale::Zh, false);
        let head = texts(&page.status);
        assert!(
            head.iter().any(
                |line| line.contains("无覆写文件（数据目录的 overrides/ 下没有本档案的 JSON）")
            ),
            "{head:?}"
        );
        assert!(
            head.iter()
                .any(|line| line.contains("档案  （尚无档案：先导入订阅）")),
            "with no profile the page says which profile it looked for: {head:?}"
        );
        assert!(!page.broken);
        let notes = texts(&page.reserved);
        assert!(
            !notes.iter().any(|line| line.contains("保留字段被覆写改动")),
            "no file, no refusal to report: {notes:?}"
        );
        assert!(
            notes.len() == 1 && notes[0].contains("本页只读"),
            "the only note left standing is the read-one boundary: {notes:?}"
        );
        assert!(page.clear_profile.is_none());
        assert!(page.fragments.is_empty());
    }

    #[test]
    fn the_outline_is_labelled_redacted_and_never_carries_a_secret() {
        let mut snapshot = fragment_snapshot();
        snapshot.core_running = true;
        snapshot.effective_outline = config_outline(EFFECTIVE_CONFIG);
        let page = build(&snapshot, Locale::Zh, false);
        let head = texts(&page.outline_head);
        assert!(
            head[0].contains("已脱敏 / redacted"),
            "the label leads, before the first line is read: {head:?}"
        );
        assert!(
            head.iter().any(|line| line.contains("运行中内核的配置")),
            "{head:?}"
        );
        assert!(
            page.outline_lines
                .iter()
                .any(|line| line.contains("/experimental/clash_api/secret: [已脱敏]")),
            "the redaction marker is the engine's, not a copy: {:?}",
            page.outline_lines
        );
        assert!(
            page.outline_lines
                .iter()
                .any(|line| line.contains("https://sbctl.test/sub/[redacted]")),
            "a private URL keeps its shape and loses its credential: {:?}",
            page.outline_lines
        );
        let mut sorted = page.outline_lines.clone();
        sorted.sort();
        assert_eq!(
            page.outline_lines, sorted,
            "the outline shows in one order whatever order the map walk came in"
        );
        for marker in SECRET_MARKERS {
            assert!(
                !page.all_text().iter().any(|line| line.contains(marker)),
                "{marker} leaked into the page"
            );
        }
    }

    #[test]
    fn an_empty_outline_explains_itself() {
        let page = build(&snapshot(None), Locale::Zh, false);
        let head = texts(&page.outline_head);
        assert!(
            head.iter().any(|line| line.contains("尚无生效配置")),
            "{head:?}"
        );
        assert!(
            !head.iter().any(|line| line.contains("运行中内核")),
            "nothing has started, so the page must not claim it has: {head:?}"
        );
        assert!(page.outline_lines.is_empty());
    }

    /// The trap the terminal client hit first: a hint squeezed into a narrow strip
    /// loses its tail and looks broken. These two sentences are the page's only
    /// hints, so the built strings are asserted whole — the GUI half of the fix is
    /// that nothing here truncates; the rendered frame is checked in
    /// `scripts/sbgui-shot`.
    #[test]
    fn the_two_hints_survive_whole_in_both_languages() {
        for locale in [Locale::Zh, Locale::En] {
            let page = build(&fragment_snapshot(), locale, false);
            let expected_note = match locale {
                Locale::Zh => {
                    "本页只读，不编辑 JSON：要改覆写就在数据目录的 overrides/ 下改这个文件，下次启动内核时生效。"
                }
                Locale::En => {
                    "This page is read-only and edits no JSON: to change an override, edit that file under the data directory's overrides/ and start the core again."
                }
            };
            let notes = texts(&page.reserved);
            assert!(
                notes.iter().any(|line| line == expected_note),
                "the read-only hint was cut short: {notes:?}"
            );
            let expected_when = match locale {
                Locale::Zh => "生效  开关只改这个文件，下次启动内核时才生效",
                Locale::En => {
                    "When it applies  a switch only rewrites this file; the next core start reads it"
                }
            };
            let head = texts(&page.status);
            assert!(
                head.iter().any(|line| line == expected_when),
                "the when-it-applies hint was cut short: {head:?}"
            );
            assert!(
                !page.all_text().iter().any(|line| line.ends_with('…')),
                "a line lost its tail: {:?}",
                page.all_text()
            );
        }
    }

    /// One sweep of every state the engine can publish, in both languages and both
    /// confirmation states, for the three planted secrets: redaction is asserted on
    /// every page state, not only on the outline that carries the configuration.
    #[test]
    fn no_state_of_the_page_leaks_a_secret() {
        let outline = config_outline(EFFECTIVE_CONFIG);
        let mut broken = snapshot(None);
        broken.active_profile = Some(ProfileSummary {
            name: "内网订阅".to_owned(),
            ..Default::default()
        });
        broken.override_error = Some(load_error(BROKEN_OVERRIDE));
        broken.effective_outline = outline.clone();
        let mut running = fragment_snapshot();
        running.core_running = true;
        running.effective_outline = outline.clone();
        let mut bare = snapshot(Some(summary(BARE_OVERRIDE, "手写档案")));
        bare.effective_outline = outline;
        let empty = ClientSnapshot::default();
        for snapshot in [broken, running, bare, empty] {
            for locale in [Locale::Zh, Locale::En] {
                for armed in [false, true] {
                    let page = build(&snapshot, locale, armed);
                    let lines = page.all_text();
                    for marker in SECRET_MARKERS {
                        let leaked: Vec<&String> =
                            lines.iter().filter(|line| line.contains(marker)).collect();
                        assert!(
                            leaked.is_empty(),
                            "{marker} leaked into a page (broken={}): {leaked:?}",
                            page.broken
                        );
                    }
                }
            }
        }
    }

    /// Every sentence the page authors itself has an English half, so an English
    /// window never inherits Chinese from a label written once. Engine data —
    /// pointers, fragment labels, the error text — stays verbatim either way, which
    /// is why it is shown.
    #[test]
    fn the_page_has_an_english_half_for_every_state() {
        for (name, snapshot) in [
            ("no file", snapshot(None)),
            ("fragments", fragment_snapshot()),
            ("bare", snapshot(Some(summary(BARE_OVERRIDE, "手写档案")))),
        ] {
            let page = build(&snapshot, Locale::En, true);
            let lines = page.all_text().join("\n");
            assert!(
                lines.contains("Profile"),
                "{name}: the English page lost its profile line: {lines}"
            );
            assert!(
                lines.contains("overrides/"),
                "{name}: the English page stopped saying where the file lives: {lines}"
            );
        }
        let broken = build(
            &ClientSnapshot {
                active_profile: Some(ProfileSummary {
                    name: "内网订阅".to_owned(),
                    ..Default::default()
                }),
                override_error: Some("测试错误".to_owned()),
                ..ClientSnapshot::default()
            },
            Locale::En,
            false,
        );
        let lines = broken.all_text();
        assert!(
            lines
                .iter()
                .any(|line| line == "Override refused · the core will not start"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line == "测试错误"),
            "the engine's own words survive the language switch: {lines:?}"
        );

        let page = build(&fragment_snapshot(), Locale::En, false);
        let states: Vec<&str> = page.fragments.iter().map(|f| f.state.as_str()).collect();
        assert_eq!(states, ["On", "Off", "On"]);
        assert!(
            card_lines(&page)
                .iter()
                .any(|line| line.starts_with("⚠ Reserved  /inbounds")),
            "{:?}",
            card_lines(&page)
        );
        assert!(
            texts(&page.reserved).iter().any(|line| line
                == "/inbounds — the traffic mode owns the inbound list (system proxy or TUN)"),
            "the English reason belongs to the path: {:?}",
            texts(&page.reserved)
        );
        assert!(
            texts(&page.fragments[0].details)
                .iter()
                .any(|line| line == "Changes  /route/rules(+2) · +2 rules"),
            "{:?}",
            texts(&page.fragments[0].details)
        );
    }
}
