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

pub(crate) const TEXT: u32 = 0x13201e;

pub(crate) const MUTED: u32 = 0x687774;

pub(crate) const FAINT: u32 = 0x8f9c99;

pub(crate) const CYAN: u32 = 0x0d9488;

pub(crate) const CYAN_DARK: u32 = 0x08766d;

pub(crate) const BLUE_2: u32 = 0xdef4f1;

pub(crate) const NAV_ACTIVE: u32 = 0xdef4f1;

/// The kit keeps two teal steps only, so the accent's edge shares the darker
/// one rather than inventing a third.
const EDGE: u32 = 0x08766d;

pub(crate) const MINT: u32 = 0x16a06d;

pub(crate) const AMBER: u32 = 0xb45309;

pub(crate) const DANGER: u32 = 0xa33c3c;

// Type scale. Five sizes, each with one job, and nothing below 11px: the old
// mix of 10/11/12/13/15/28px runs is what made every page read as crowded.
pub(crate) const TITLE: f32 = 20.0;

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

pub(crate) const WINDOW_RADIUS: f32 = 16.0;

pub(crate) const CONTENT_PAD: f32 = 20.0;

pub(crate) const GAP_SECTION: f32 = 12.0;

pub(crate) const GAP_ITEM: f32 = 12.0;

pub(crate) const PAD_CARD: f32 = 20.0;

pub(crate) const ROW_X: f32 = 20.0;

pub(crate) const ROW_Y: f32 = 13.0;

pub(crate) const TITLEBAR_H: f32 = 64.0;

pub(crate) const SIDEBAR_W: f32 = 188.0;

pub(crate) const CONTENT_MAX: f32 = 1440.0;

/// How many rows the rules and connections lists draw before asking.
pub(crate) const LIST_PAGE: usize = 120;

pub(crate) const DATA_DIR: &str = "sbgui";

/// The window's own artwork, served to GPUI by [`SereinAssets`] and drawn in
/// the titlebar. The same file is compiled into the exe as the Win32 icon.
pub(crate) const BRAND_ICON_PATH: &str = "serein.ico";

pub(crate) fn tone_colors(tone: Tone) -> (u32, u32, u32) {
    match tone {
        Tone::Accent => (0xffffff, CYAN, EDGE),
        Tone::Neutral => (TEXT, SURFACE_2, BORDER),
        Tone::Warning => (0x985c08, 0xfff5e5, 0xf2dfbf),
    }
}
