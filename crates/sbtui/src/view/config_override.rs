//! The Override tab (覆写配置文件内容): a read-only view of the configuration
//! the core actually starts from, plus the two edits this UI offers — switching
//! a rule fragment on or off, and installing a JSON document the user prepared
//! outside the client (`f`, which sends `SetOverride` for the active profile).
//!
//! There is deliberately no JSON editor here. `docs/target-spec-gap-and-
//! verification-plan.md` §2 locks the terminal client to "只读查看生效配置 +
//! 规则片段开关", and ADR-0023 keeps authoring the document outside the app; the
//! page answers the three questions an operator can ask without an editor: is
//! there an override for this profile, what does it change, and did the client
//! refuse part of it.
//!
//! Everything drawn comes from the engine snapshot (`override_summary`,
//! `override_error`, `effective_outline`), so the page is honest before the
//! first start and cannot disagree with the desktop client. Two rules shape the
//! copy:
//!
//! * A malformed file is the loudest thing on the page, in its own words. The
//!   engine refuses to start the core under it, and a truncated or reworded
//!   error is how a broken override becomes an invisible one.
//! * Reserved fields the override reached for are listed per fragment and again
//!   in total, because the merge *does* apply them and the start writes them
//!   back afterwards. Reported, never silently ignored.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use crate::app::App;
use crate::state::OverrideFragmentSummary;
use crate::style::{AMBER, DANGER, MINT, MUTED, TEXT, panel};
use client_core::config_override::reserved_reason;

/// What a row means on this page, so the colour is decided where the frame is
/// drawn and the text stays a plain value a test can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Muted,
    Text,
    On,
    Alert,
    Danger,
}

impl Tone {
    fn style(self) -> Style {
        match self {
            Self::Muted => Style::default().fg(MUTED),
            Self::Text => Style::default().fg(TEXT),
            Self::On => Style::default().fg(MINT),
            Self::Alert => Style::default().fg(AMBER),
            Self::Danger => Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
        }
    }
}

/// One display row: text plus what it means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    text: String,
    tone: Tone,
}

impl Row {
    fn new(tone: Tone, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }

    fn line(self) -> Line<'static> {
        Line::from(Span::styled(self.text, self.tone.style()))
    }
}

/// Rows that have to fit one line each (the fragment list and the outline).
/// Clipping is drawn as `…` instead of happening silently at the border.
fn clipped(text: impl Into<String>, tone: Tone, width: usize) -> Row {
    Row::new(tone, fit(&text.into(), width))
}

/// Truncates to a *display* width. `short_label` counts characters and a CJK
/// glyph is two columns wide, so a character cap lets the row run past the
/// border and ratatui cuts the tail without any marker — the one way a rule
/// count or a field path could go missing from a page whose whole job is to
/// name what is missing.
fn fit(text: &str, width: usize) -> String {
    let target = width.max(4);
    if Line::from(text).width() <= target {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 0;
    for character in text.chars() {
        let wide = Line::from(character.to_string()).width();
        if used + wide + 1 > target {
            break;
        }
        kept.push(character);
        used += wide;
    }
    kept.push('…');
    kept
}

/// The panel title. It says what the page can do *for this file*, because a
/// bare object has no fragments and an unusable file has nothing at all.
pub(crate) fn override_title(app: &App) -> String {
    if app.snapshot.override_error.is_some() {
        return "配置覆写 · 覆写无效".to_owned();
    }
    match app.snapshot.override_summary.as_ref() {
        None => "配置覆写 · 无覆写文件".to_owned(),
        Some(summary) if summary.implicit => "配置覆写 · 手写整份（无片段可开关）".to_owned(),
        Some(summary) => format!(
            "配置覆写 · {}/{} 启用 · ↑↓ 选择 · Enter 开关",
            summary.enabled_count(),
            summary.fragments.len()
        ),
    }
}

/// The outline panel's title: the redaction has to be stated before the first
/// line is read, not after the user assumes they are seeing the whole file.
pub(crate) fn outline_title() -> &'static str {
    "生效配置 · 已脱敏 / redacted · 只读"
}

/// The block above the fragment list: whose file is in force, and why it might
/// not be. The error branch comes first and returns, so an unusable override
/// cannot be buried under an empty list.
pub(crate) fn head_rows(app: &App) -> Vec<Row> {
    let snapshot = &app.snapshot;
    if let Some(error) = snapshot.override_error.as_deref() {
        return vec![
            Row::new(Tone::Danger, "覆写无效 · 内核不会启动"),
            // Verbatim: the engine's message already names the file, the field
            // path and the line/column it gave up on.
            Row::new(Tone::Danger, error),
            Row::new(
                Tone::Muted,
                "文件原样留在磁盘上，没有被改写，也没有被部分应用。".to_owned(),
            ),
        ];
    }
    let Some(summary) = snapshot.override_summary.as_ref() else {
        return vec![
            Row::new(Tone::Text, format!("档案  {}", profile_label(app))),
            Row::new(
                Tone::Muted,
                "覆写  无覆写文件（数据目录的 overrides/ 下没有本档案的 JSON）".to_owned(),
            ),
            Row::new(
                Tone::Muted,
                "内核只用订阅原文，加上客户端自己写回的保留字段。".to_owned(),
            ),
        ];
    };
    // The file's name is a 64-digit sha256, wider than this column: it gets a
    // row of its own with no label in front of it, so the wrap breaks it at the
    // border instead of leaving a row with nothing but a label on it.
    let mut rows = vec![
        Row::new(Tone::Text, format!("档案  {}", summary.profile)),
        Row::new(
            Tone::Text,
            format!(
                "覆写  {}/{} 片段启用 · {} 字节",
                summary.enabled_count(),
                summary.fragments.len(),
                summary.bytes
            ),
        ),
        Row::new(
            Tone::Muted,
            "生效  开关只改这个文件，下次启动内核时才生效".to_owned(),
        ),
        Row::new(
            Tone::Muted,
            "文件  数据目录/overrides/ 下本档案名的 sha256：".to_owned(),
        ),
        Row::new(Tone::Muted, summary.file_name.clone()),
    ];
    if summary.implicit {
        rows.push(Row::new(
            Tone::Alert,
            "手写整份覆写：没有片段列表，无片段可开关。".to_owned(),
        ));
    }
    rows
}

/// The block under the fragment list: reserved-field refusals, repeated in
/// total so a per-fragment warning cannot be the only place they are written.
/// `width` is the panel's inner width — these rows are fitted to one line each
/// rather than wrapped, because every row this block saves is a fragment the
/// list above can show instead.
pub(crate) fn note_rows(app: &App, width: usize) -> Vec<Row> {
    let Some(summary) = app.snapshot.override_summary.as_ref() else {
        return vec![readonly_note()];
    };
    let reserved = summary.reserved();
    let mut rows = Vec::new();
    if reserved.is_empty() {
        rows.push(Row::new(
            Tone::Muted,
            "保留字段  没有启用中的片段改动它们".to_owned(),
        ));
    } else {
        rows.push(clipped(
            "⚠ 保留字段被覆写改动 · 启动内核时客户端写回原值，并在事件行再次报告",
            Tone::Danger,
            width,
        ));
        for path in reserved {
            rows.push(clipped(
                format!("  {path} — {}", reserved_reason(&path)),
                Tone::Alert,
                width,
            ));
        }
    }
    rows.push(readonly_note());
    rows
}

/// Where an override comes from at all, said on every state of the page: this UI
/// reads the configuration, switches fragments, and installs a document the user
/// prepared elsewhere — it never opens an editor.
fn readonly_note() -> Row {
    Row::new(
        Tone::Muted,
        "本页只读、不编辑 JSON：外部写好的文件按 f 装载，也可直接改数据目录 overrides/ 下的文件。"
            .to_owned(),
    )
}

/// One group of rows per fragment, in the order the file lists them (highest
/// precedence first, the same order the merged rules end up in).
pub(crate) fn fragment_groups(app: &App, width: usize) -> Vec<Vec<Row>> {
    let Some(summary) = app.snapshot.override_summary.as_ref() else {
        return Vec::new();
    };
    summary
        .fragments
        .iter()
        .map(|fragment| fragment_rows(fragment, width))
        .collect()
}

fn fragment_rows(fragment: &OverrideFragmentSummary, width: usize) -> Vec<Row> {
    let label = if fragment.label == fragment.id {
        fragment.id.clone()
    } else {
        format!("{}（{}）", fragment.label, fragment.id)
    };
    let rules = if fragment.rules > 0 {
        format!(" · +{} 条规则", fragment.rules)
    } else {
        String::new()
    };
    let mut rows = vec![clipped(
        format!(
            "[{}] {label}{rules}",
            if fragment.enabled { "启用" } else { "停用" }
        ),
        if fragment.enabled {
            Tone::On
        } else {
            Tone::Muted
        },
        width,
    )];
    // Sorted here rather than taken in the file's own order: `serde_json`'s map
    // order depends on whether the `preserve_order` feature is unified into the
    // build (the server's graph turns it on, `cargo test -p sbtui` does not), so
    // walk order would make one golden two frames.
    let changes = if fragment.changes.is_empty() {
        "（空片段，不改动任何字段）".to_owned()
    } else {
        let mut paths = fragment.changes.clone();
        paths.sort();
        paths.join("、")
    };
    rows.push(clipped(format!("改动  {changes}"), Tone::Text, width));
    // The summary lists a fragment's reserved fields whether or not it is on,
    // because that is what the file asks for. Only an enabled one can actually
    // be refused, so the warning glyph is reserved for the fragments that will
    // be written back at the next start; the rest read as a condition.
    for path in &fragment.reserved {
        let (wording, tone) = if fragment.enabled {
            ("⚠ 保留  {path}（客户端写回，不生效）", Tone::Danger)
        } else {
            ("保留字段  {path}（启用后会被客户端写回）", Tone::Muted)
        };
        rows.push(clipped(wording.replace("{path}", path), tone, width));
    }
    rows
}

/// The read-only outline rows: the redaction label, which config it came from,
/// then the engine's own lines with nothing added or removed.
pub(crate) fn outline_rows(app: &App, width: usize) -> Vec<Row> {
    let mut rows = vec![Row::new(
        Tone::Alert,
        "已脱敏 / redacted：密钥与订阅凭据不在此显示".to_owned(),
    )];
    if app.snapshot.effective_outline.is_empty() {
        rows.push(Row::new(
            Tone::Muted,
            "（尚无生效配置；启动一次内核，这里会显示订阅 + 覆写 + 保留字段的合并结果）".to_owned(),
        ));
        return rows;
    }
    rows.push(Row::new(
        Tone::Muted,
        if app.snapshot.core_running {
            "来源  运行中内核的配置（active-config.json）"
        } else {
            "来源  上次启动写入的配置（active-config.json）"
        }
        .to_owned(),
    ));
    rows.push(Row::new(Tone::Muted, String::new()));
    // Sorted for the same reason as the fragment paths above: the outline is a
    // walk of the configuration's maps, and that walk's order is a property of
    // the build's `serde_json` features, not of the configuration.
    let mut outline = app.snapshot.effective_outline.clone();
    outline.sort();
    rows.extend(outline.iter().map(|line| clipped(line, Tone::Text, width)));
    rows
}

fn profile_label(app: &App) -> String {
    app.snapshot
        .active_profile
        .as_ref()
        .map(|profile| profile.name.clone())
        .unwrap_or_else(|| "（尚无档案：先导入订阅）".to_owned())
}

/// How many display rows a list of single-line rows takes in a column. Rows
/// that have to wrap are counted as the wrapped height so the layout below them
/// cannot be squeezed out of existence.
fn block_height(lines: &[Line<'_>], width: usize) -> u16 {
    if width == 0 {
        return lines.len() as u16;
    }
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width) as u16)
        .sum::<u16>()
        .min(8)
}

pub(crate) fn draw_override(frame: &mut Frame, area: Rect, app: &mut App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)]).split(area);
    draw_override_panel(frame, columns[0], app);
    draw_outline_panel(frame, columns[1], app);
}

fn draw_override_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = panel(override_title(app));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 4 || inner.height < 4 {
        return;
    }
    let width = usize::from(inner.width);
    let head: Vec<Line<'_>> = head_rows(app).into_iter().map(Row::line).collect();
    let notes: Vec<Line<'_>> = note_rows(app, width).into_iter().map(Row::line).collect();
    let groups = fragment_groups(app, width);
    let head_height = block_height(&head, width).min(inner.height.saturating_sub(3));
    let note_height = block_height(&notes, width).min(inner.height.saturating_sub(head_height + 2));
    // With nothing to list, the notes follow the head straight away: a `Min`
    // gap for an empty fragment list would push the lines that matter — the
    // reason a file was refused, or "本页只读" — to the bottom of a panel that
    // is otherwise blank.
    let listing = !groups.is_empty();
    let rows = Layout::vertical(if listing {
        [
            Constraint::Length(head_height),
            Constraint::Min(1),
            Constraint::Length(note_height),
        ]
    } else {
        [
            Constraint::Length(head_height),
            Constraint::Length(note_height),
            Constraint::Min(1),
        ]
    })
    .split(inner);
    frame.render_widget(Paragraph::new(head).wrap(Wrap { trim: false }), rows[0]);
    let items: Vec<ListItem<'_>> = groups
        .iter()
        .map(|group| {
            ListItem::new(
                group
                    .clone()
                    .into_iter()
                    .map(Row::line)
                    .collect::<Vec<Line<'_>>>(),
            )
        })
        .collect();
    let selected = match items.len() {
        0 => None,
        len => Some(app.selected_fragment.min(len - 1)),
    };
    // The list is empty exactly when `listing` is false, so in that case this
    // paints nothing and the notes paragraph, drawn after it, owns the row.
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("▸ ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        rows[1],
        &mut ListState::default().with_selected(selected),
    );
    frame.render_widget(
        Paragraph::new(notes).wrap(Wrap { trim: false }),
        if listing { rows[2] } else { rows[1] },
    );
}

fn draw_outline_panel(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = panel(outline_title());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 4 || inner.height < 1 {
        return;
    }
    let lines: Vec<Line<'_>> = outline_rows(app, usize::from(inner.width))
        .into_iter()
        .map(Row::line)
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::fixtures;
    use crate::{ClientController, ClientSnapshot};

    fn app_with(snapshot: ClientSnapshot) -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temporary data directory");
        let mut app = App::new(
            ClientController::start(dir.path().to_path_buf()),
            dir.path().to_path_buf(),
        );
        app.snapshot = snapshot;
        // The override fixtures come from `view::fixtures`, shared with the
        // golden frames. `App::new` reads the engine's own snapshot — the
        // temporary directory behind it holds nothing — and the view's path is
        // pinned to the same placeholder the goldens use, so neither a frame nor
        // an assertion can depend on this host's temporary-directory name.
        app.dir = std::path::PathBuf::from("<DATA_DIR>");
        (app, dir)
    }

    fn texts(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|row| row.text.clone()).collect()
    }

    fn group_texts(groups: &[Vec<Row>]) -> Vec<String> {
        groups.iter().flat_map(|group| texts(group)).collect()
    }

    #[tokio::test]
    async fn a_fragment_list_shows_state_rule_count_changed_paths_and_refused_paths() {
        let snapshot = ClientSnapshot {
            override_summary: Some(fixtures::summary(fixtures::FRAGMENT_OVERRIDE, "内网订阅")),
            ..ClientSnapshot::default()
        };
        let (app, dir) = app_with(snapshot);
        let groups = fragment_groups(&app, 60);
        assert_eq!(groups.len(), 3, "one group per fragment");
        let joined = group_texts(&groups);
        assert!(
            joined
                .iter()
                .any(|line| line.contains("[启用] 内网直连（private-direct） · +2 条规则")),
            "the rule count and the switch state read on one line: {joined:?}"
        );
        assert!(
            joined.iter().any(|line| line.contains("[停用] 自建 DNS")),
            "a disabled fragment says so: {joined:?}"
        );
        assert!(
            joined
                .iter()
                .any(|line| line == "改动  /dns/final、/dns/servers(1 项)"),
            "field paths, not a JSON dump: {joined:?}"
        );
        assert!(
            joined
                .iter()
                .any(|line| line == "⚠ 保留  /experimental/clash_api/secret（客户端写回，不生效）"),
            "the refused path is named on the fragment that asked for it: {joined:?}"
        );
        assert!(
            joined
                .iter()
                .any(|line| line == "⚠ 保留  /inbounds（客户端写回，不生效）"),
            "every reserved pointer the enabled fragment touches is listed: {joined:?}"
        );
        assert!(
            !joined.iter().any(|line| line.contains("MUST-NOT-PRINT")),
            "a reserved field is reported by path only, never by value: {joined:?}"
        );
        let notes = texts(&note_rows(&app, 60));
        assert!(
            notes
                .iter()
                .any(|line| line.contains("⚠ 保留字段被覆写改动") && !line.contains("静默")),
            "{notes:?}"
        );
        assert!(
            notes
                .iter()
                .any(|line| line.contains("/route/auto_detect_interface — 本地运行")),
            "the path leads the row, so the reason can be cut and the row still              says which field: {notes:?}"
        );
        assert!(
            notes.iter().any(|line| line.contains("本页只读")),
            "{notes:?}"
        );
        app.controller.shutdown();
        drop(dir);
    }

    #[tokio::test]
    async fn a_disabled_fragment_loses_its_rule_and_its_conflict() {
        // The summary is the engine's own, so "disabled" has to reach the panel
        // as a fragment that neither counts as enabled nor reports a conflict.
        let mut snapshot = ClientSnapshot::default();
        let mut built = fixtures::summary(fixtures::FRAGMENT_OVERRIDE, "内网订阅");
        built
            .fragments
            .iter_mut()
            .for_each(|fragment| fragment.enabled = fragment.id == "custom-dns");
        snapshot.override_summary = Some(built);
        let (app, dir) = app_with(snapshot);
        let notes = texts(&note_rows(&app, 60));
        assert!(
            notes
                .iter()
                .any(|line| line.contains("没有启用中的片段改动它们")),
            "the only conflict came from the now-disabled fragment: {notes:?}"
        );
        let groups = fragment_groups(&app, 60);
        let lines = group_texts(&groups);
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
        assert_eq!(
            override_title(&app),
            "配置覆写 · 1/3 启用 · ↑↓ 选择 · Enter 开关",
            "the title counts what merges, not what exists"
        );
        app.controller.shutdown();
        drop(dir);
    }

    #[tokio::test]
    async fn a_bare_object_says_there_is_nothing_to_toggle() {
        let snapshot = ClientSnapshot {
            override_summary: Some(fixtures::summary(fixtures::BARE_OVERRIDE, "手写档案")),
            ..ClientSnapshot::default()
        };
        let (app, dir) = app_with(snapshot);
        assert!(
            app.snapshot
                .override_summary
                .as_ref()
                .is_some_and(|s| s.implicit)
        );
        let head = texts(&head_rows(&app));
        assert!(
            head.iter()
                .any(|line| line.contains("手写整份覆写：没有片段列表，无片段可开关")),
            "{head:?}"
        );
        assert_eq!(
            override_title(&app),
            "配置覆写 · 手写整份（无片段可开关）",
            "the title promises no key it cannot honour"
        );
        let groups = fragment_groups(&app, 60);
        assert_eq!(groups.len(), 1, "the whole file reads as one fragment");
        app.controller.shutdown();
        drop(dir);
    }

    #[tokio::test]
    async fn a_malformed_override_is_printed_in_the_engine_s_own_words() {
        // The engine's loader produces the message; the panel only decides how
        // loudly to say it, so nothing here can paraphrase a field path away.
        let error = fixtures::load_error(fixtures::BROKEN_OVERRIDE);
        let snapshot = ClientSnapshot {
            override_error: Some(error.clone()),
            ..ClientSnapshot::default()
        };
        let (app, dir) = app_with(snapshot);
        let head = texts(&head_rows(&app));
        assert_eq!(
            head[0], "覆写无效 · 内核不会启动",
            "the consequence comes before the cause"
        );
        assert!(
            head.iter().any(|line| line == &error),
            "verbatim, not summarized: {head:?}"
        );
        assert!(
            head.iter()
                .any(|line| line.contains(fixtures::OVERRIDE_FILE)),
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
            fragment_groups(&app, 60).is_empty(),
            "nothing was applied, so there is nothing to list"
        );
        assert_eq!(override_title(&app), "配置覆写 · 覆写无效");
        app.controller.shutdown();
        drop(dir);
    }

    #[tokio::test]
    async fn no_override_file_is_a_state_rather_than_a_problem() {
        let (app, dir) = app_with(ClientSnapshot::default());
        let head = texts(&head_rows(&app));
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
        assert!(
            !texts(&note_rows(&app, 60))
                .iter()
                .any(|line| line.contains("保留字段被覆写改动")),
            "no file, no refusal to report"
        );
        app.controller.shutdown();
        drop(dir);
    }

    #[tokio::test]
    async fn the_outline_is_labelled_redacted_and_never_carries_a_secret() {
        let snapshot = ClientSnapshot {
            core_running: true,
            effective_outline: client_core::state::config_outline(fixtures::EFFECTIVE_CONFIG),
            ..ClientSnapshot::default()
        };
        let (app, dir) = app_with(snapshot);
        assert_eq!(outline_title(), "生效配置 · 已脱敏 / redacted · 只读");
        let rows = texts(&outline_rows(&app, 60));
        assert!(
            rows[0].contains("已脱敏 / redacted"),
            "the label leads: {rows:?}"
        );
        assert!(
            rows.iter().any(|line| line.contains("运行中内核的配置")),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|line| line.contains("/experimental/clash_api/secret: [已脱敏]")),
            "the redaction marker is what the engine publishes: {rows:?}"
        );
        assert!(
            rows.iter()
                .any(|line| line.contains("https://sbctl.test/sub/[redacted]")),
            "a private rule-set URL keeps its shape and loses its credential: {rows:?}"
        );
        for marker in fixtures::SECRET_MARKERS {
            assert!(
                !rows.iter().any(|line| line.contains(marker)),
                "{marker} leaked into the outline rows: {rows:?}"
            );
        }
        app.controller.shutdown();
        drop(dir);
    }

    #[tokio::test]
    async fn an_empty_outline_explains_itself() {
        let (app, dir) = app_with(ClientSnapshot::default());
        let rows = texts(&outline_rows(&app, 60));
        assert!(
            rows.iter().any(|line| line.contains("尚无生效配置")),
            "{rows:?}"
        );
        assert!(
            !rows.iter().any(|line| line.contains("运行中内核")),
            "nothing has started, so the page must not claim it has: {rows:?}"
        );
        app.controller.shutdown();
        drop(dir);
    }

    /// `short_label` counts characters; a CJK row is twice as wide as it is
    /// long, and a row wider than its column loses its tail at the border with
    /// no marker at all. This is what keeps "+2 条规则" on screen.
    #[test]
    fn rows_are_cut_at_the_display_width_and_the_cut_is_marked() {
        assert_eq!(
            fit("[启用] 内网直连 · +2 条规则", 60),
            "[启用] 内网直连 · +2 条规则"
        );
        let cut = fit("[启用] 内网直连 · +2 条规则", 20);
        assert_eq!(Line::from(cut.clone()).width(), 20, "{cut:?}");
        assert!(cut.ends_with('…') && !cut.contains('规'), "{cut:?}");
        assert_eq!(
            fit("abcdef", 2),
            "abc…",
            "the four-column floor still marks a cut"
        );
        assert_eq!(
            fit("中文文", 1),
            "中…",
            "a two-column glyph cannot fit four with the mark"
        );
    }

    #[test]
    fn a_wrapping_error_does_not_squeeze_the_list_out_of_the_panel() {
        // `block_height` is the only thing standing between a long error line
        // and a zero-height fragment list.
        let lines: Vec<Line<'_>> = vec![
            Row::new(Tone::Danger, "x".repeat(200)).line(),
            Row::new(Tone::Text, "y".repeat(5)).line(),
        ];
        let height = block_height(&lines, 50);
        assert_eq!(
            height, 5,
            "the 200-column row wraps to 4, the short one to 1"
        );
        let one_long_row = [Row::new(Tone::Danger, "x".repeat(900)).line()];
        assert_eq!(block_height(&one_long_row, 50), 8, "capped, not tall");
        assert_eq!(block_height(&[], 50), 0);
        assert_eq!(block_height(&lines, 0), 2, "no width means one row each");
    }
}
