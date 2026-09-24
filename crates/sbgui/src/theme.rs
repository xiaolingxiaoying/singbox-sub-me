//! The design tokens: one palette, one type scale, one spacing scale.
//!
//! Every page and widget takes its colours, sizes and gaps from here, so
//! the visual system stays a single point of change.

use gpui::FontWeight;

use crate::state::Tone;

// Serein's desktop palette: a cool-gray canvas, white working surfaces, and a
// teal reserved for selected, enabled and primary actions. Values follow
// prototypes/sbgui-progressive-workspace/design-kit/colors.css.
pub(crate) const BG: u32 = 0xf3f6f5;

pub(crate) const SURFACE: u32 = 0xffffff;

pub(crate) const SURFACE_2: u32 = 0xf7f9f8;

pub(crate) const BORDER: u32 = 0xdce4e1;

/// The kit's second border step: the one a surface uses when it wants to be
/// touched (`--serein-border-strong`), not merely enclosed.
pub(crate) const BORDER_STRONG: u32 = 0xcbd8d4;

pub(crate) const TEXT: u32 = 0x13201e;

/// Secondary text. The kit's own `#687774` clears AA on pure white (4.69:1) and
/// fails on every background the app actually paints — panel `#f3f6f5`,
/// `#f7f9f8`, the selected nav item (4.09:1) — so a screenshot of one surface
/// could never tell whether the rest were readable. Darkened to clear 4.5:1 on
/// all six; see the tests at the bottom of this file.
pub(crate) const MUTED: u32 = 0x5d6b68;

/// Decoration only: separators, disabled glyphs, unit suffixes. At 2.5–2.8:1 it
/// is below even the 3:1 non-text floor for some pairings, which is why the test
/// below refuses to let it become a label color.
pub(crate) const FAINT: u32 = 0x8f9c99;

/// Accent for borders, focus bars and indicator dots — not for text. Its text
/// sibling is [`CYAN_DARK`], which clears 4.79:1 on the lightest nav background.
pub(crate) const CYAN: u32 = 0x0d9488;

pub(crate) const CYAN_DARK: u32 = 0x08766d;

pub(crate) const BLUE_2: u32 = 0xdef4f1;

/// The upload series only. Teal is download and anything actionable; green is
/// health, so a rate chart needs a third hue rather than reusing one.
pub(crate) const BLUE: u32 = 0x3b82f6;

pub(crate) const NAV_ACTIVE: u32 = 0xdef4f1;

/// A table row under the pointer, and one that carries the current selection.
/// Both come from the kit's own data-table rules, which keep a selected row a
/// shade quieter than a selected navigation item.
pub(crate) const ROW_HOVER: u32 = 0xf2faf8;

pub(crate) const ROW_SELECTED: u32 = 0xedf9f7;

/// The kit keeps two teal steps only, so the accent's edge shares the darker
/// one rather than inventing a third.
const EDGE: u32 = 0x08766d;

pub(crate) const MINT: u32 = 0x16a06d;

/// Warning text. `#b45309` sat at 4.38:1 on the nav tint and 4.62:1 on the panel,
/// i.e. right on the AA line where a background change pushes it under.
pub(crate) const AMBER: u32 = 0x9a4708;

pub(crate) const DANGER: u32 = 0xa33c3c;

// Type scale. Seven sizes, each with one job, and nothing below 11px: the old
// mix of 10/11/12/13/15/28px runs is what made every page read as crowded.
// The two display steps exist because the kit gives a working surface a 19px
// heading and its headline numbers a size of their own; a 14px title over a
// 13px body cannot carry that hierarchy.
pub(crate) const DISPLAY: f32 = 24.0;

pub(crate) const TITLE: f32 = 20.0;

pub(crate) const SECTION_LG: f32 = 19.0;

pub(crate) const SECTION: f32 = 14.0;

pub(crate) const BODY: f32 = 13.0;

pub(crate) const LABEL: f32 = 12.0;

pub(crate) const META: f32 = 11.0;

pub(crate) const WEIGHT_NORMAL: FontWeight = FontWeight(400.0);

pub(crate) const WEIGHT_MEDIUM: FontWeight = FontWeight(500.0);

pub(crate) const WEIGHT_SEMIBOLD: FontWeight = FontWeight(600.0);

// Spacing scale. Blocks are separated by whitespace first and by a hairline
// only where a surface boundary is real, so the same content needs less ink.
// The rhythm follows layout.md: 12 px between surfaces, 18-26 px inside one,
// and radii kept inside the 7-13 px band.
pub(crate) const RADIUS: f32 = 13.0;

/// Chips, buttons, fields and nav rows. The kit keeps controls visibly
/// tighter than surfaces, and nothing goes below 7 px.
pub(crate) const RADIUS_CONTROL: f32 = 7.0;

pub(crate) const WINDOW_RADIUS: f32 = 16.0;

pub(crate) const CONTENT_PAD: f32 = 20.0;

pub(crate) const GAP_SECTION: f32 = 12.0;

pub(crate) const GAP_ITEM: f32 = 12.0;

pub(crate) const PAD_CARD: f32 = 20.0;

/// A working surface's own padding: `layout.md` allows 18-26 px inside one,
/// and the kit's pages use 24 px vertically against 26 px horizontally so a
/// table row and a heading share the same left edge.
pub(crate) const PAD_SURFACE_X: f32 = 26.0;

pub(crate) const PAD_SURFACE_Y: f32 = 24.0;

pub(crate) const ROW_X: f32 = 20.0;

pub(crate) const ROW_Y: f32 = 13.0;

pub(crate) const TITLEBAR_H: f32 = 64.0;

pub(crate) const SIDEBAR_W: f32 = 188.0;

pub(crate) const CONTENT_MAX: f32 = 1440.0;

/// How many rows the rules and connections lists draw before asking.
pub(crate) const LIST_PAGE: usize = 120;

/// The horizontal inset every data table shares — header and rows alike. It was
/// `14` on the header and `13` on the rules row, which is the 1px drift issue 02
/// item 8 named: the column labels sat one pixel away from the values under them.
/// One constant because the two must agree, and the gate in `components.rs`
/// fails if either goes back to a literal.
pub(crate) const TABLE_X: f32 = 14.0;

// The subscriptions table at narrow width, as numbers that have to add up.
// They were literals until issue 01 item 8: the columns summed past what an
// 860-wide window can show, the `overflow_x_scroll` wrapper engaged, and the
// overflow was taken off the *last* button of the widest triple — 「删除」 in
// Chinese, "Delete" in English. The test at the bottom of this file is the
// reason they will not be re-guessed.

/// The name column's floor at narrow. It is a grow column, so it gives way
/// before the fixed ones do.
pub(crate) const SUB_NAME_NARROW: f32 = 120.0;

/// The status column at narrow: a pill, a dot and two or three words.
pub(crate) const SUB_STATUS_NARROW: f32 = 64.0;

/// The action track at narrow. Sized for the widest *triple* in the longer of
/// the two languages — "Use this" + "Edit link" + "Delete".
pub(crate) const SUB_ACTION_NARROW: f32 = 250.0;

pub(crate) const DATA_DIR: &str = "sbgui";

/// The window's own artwork, served to GPUI by [`SereinAssets`] and drawn in
/// the titlebar. The same file is compiled into the exe as the Win32 icon.
pub(crate) const BRAND_ICON_PATH: &str = "serein.ico";

pub(crate) fn tone_colors(tone: Tone) -> (u32, u32, u32) {
    match tone {
        Tone::Accent => (0xffffff, CYAN, EDGE),
        Tone::Neutral => (TEXT, SURFACE_2, BORDER),
        Tone::Warning => (0x985c08, 0xfff5e5, 0xf2dfbf),
        Tone::Danger => (DANGER, SURFACE, BORDER_STRONG),
    }
}

/// WCAG 2.1 relative-luminance contrast ratio between two `0xRRGGBB` colors,
/// from 1.0 (identical) to 21.0 (black on white). Kept here rather than in a dev
/// dependency so the palette tests below can run on any platform, headless, with
/// no GPU and no window — the thing being measured is a number in a constant.
#[cfg(test)]
pub(crate) fn contrast(fg: u32, bg: u32) -> f64 {
    let luminance = |color: u32| {
        let channel = |shift: u32| {
            let value = f64::from((color >> shift) & 0xff) / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    };
    let (one, other) = (luminance(fg), luminance(bg));
    (one.max(other) + 0.05) / (one.min(other) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.1 AA for body-size text.
    const AA: f64 = 4.5;

    /// The surfaces a label can end up on: the window, a card, a raised card,
    /// the selected nav item, and the two table-row states.
    const SURFACES: &[(&str, u32)] = &[
        ("BG", BG),
        ("SURFACE", SURFACE),
        ("SURFACE_2", SURFACE_2),
        ("NAV_ACTIVE", NAV_ACTIVE),
        ("ROW_HOVER", ROW_HOVER),
        ("ROW_SELECTED", ROW_SELECTED),
    ];

    /// Tokens that are painted as text somewhere in the app.
    const TEXT_TOKENS: &[(&str, u32)] = &[
        ("TEXT", TEXT),
        ("MUTED", MUTED),
        ("CYAN_DARK", CYAN_DARK),
        ("AMBER", AMBER),
        ("DANGER", DANGER),
    ];

    /// The pairing that made the old palette look fine in a screenshot: `MUTED`
    /// cleared AA on pure white and failed on the four backgrounds the app
    /// actually paints around it. Checking one surface proves nothing, so every
    /// text token is checked against every surface.
    #[test]
    fn every_text_token_clears_aa_on_every_surface_it_can_land_on() {
        let mut failures: Vec<String> = Vec::new();
        for (fg_name, fg) in TEXT_TOKENS {
            for (bg_name, bg) in SURFACES {
                let ratio = contrast(*fg, *bg);
                if ratio < AA {
                    failures.push(format!("{fg_name} on {bg_name} = {ratio:.2}:1"));
                }
            }
        }
        assert!(failures.is_empty(), "below AA: {}", failures.join(", "));
    }

    /// The warning button's own colors are literals in `tone_colors`, so they are
    /// not covered by the token table above and would drift unnoticed.
    #[test]
    fn the_warning_button_itsself_clears_aa() {
        let (text, background, _) = tone_colors(Tone::Warning);
        let ratio = contrast(text, background);
        assert!(ratio >= AA, "Tone::Warning label = {ratio:.2}:1");
    }

    /// `FAINT` and `CYAN` stay decoration: separators, focus bars, indicator
    /// dots. Neither clears AA as text on the lightest background in the app, so
    /// a single use of either as a label color is a readability bug, not a style choice
    /// — and the only way to keep it out is to look for it.
    #[test]
    fn decoration_tokens_are_never_used_as_text() {
        let needles: Vec<String> = ["FAINT", "CYAN"]
            .iter()
            .map(|token| format!("text_color(rgb({token}))"))
            .collect();
        let mut offenders: Vec<String> = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("the crate's own src is readable") {
                let path = entry.expect("a readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("a readable source file");
                for needle in &needles {
                    if text.contains(needle.as_str()) {
                        offenders.push(format!("{}: {needle}", path.display()));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "decoration-only tokens painted as text:\n{}",
            offenders.join("\n")
        );
    }

    // The gate's own arithmetic. These three are test-only on purpose: the page
    // reads the column tokens above, and only the fit check needs to know what a
    // card costs in padding.
    /// The card's own padding and border, measured off the 860×640 frame.
    const CARD_INSET: f32 = 28.0;
    /// The gap between two table columns.
    const SUB_COL_GAP: f32 = 12.0;

    /// The width a page's content actually gets: the window minus the sidebar
    /// and the content padding on both sides.
    fn content_width(window: f32) -> f32 {
        window - SIDEBAR_W - 2.0 * CONTENT_PAD
    }

    /// The arithmetic that issue 01 item 8 got wrong: a table whose columns sum
    /// past the content box does not shrink — its `overflow_x_scroll` wrapper
    /// scrolls, and what disappears off the card is whatever is painted last.
    ///
    /// The assertion is deliberately about the *narrow* window the harness
    /// shoots at, so widening a track, adding a column, or dropping one of the
    /// narrow hides fails here instead of in a screenshot someone has to notice.
    #[test]
    fn the_narrow_subscriptions_columns_fit_the_narrow_window() {
        let narrow_window = 860.0;
        let available = content_width(narrow_window) - CARD_INSET;
        let columns = SUB_NAME_NARROW + SUB_STATUS_NARROW + SUB_ACTION_NARROW + 2.0 * SUB_COL_GAP;
        assert!(
            columns <= available,
            "the narrow subscriptions columns need {columns}px but the card has {available}px \
             at {narrow_window} wide — the overflow would be taken off the last action button"
        );
        // Slack, not just fitting: the English labels are the long ones and the
        // buttons are padded, so a track that fits to the pixel does not fit.
        assert!(
            available - columns >= 60.0,
            "only {:.0}px of slack at narrow; the widest triple has nowhere to grow",
            available - columns
        );
    }
}
