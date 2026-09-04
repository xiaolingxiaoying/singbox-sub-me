//! Minimal ANSI terminal helpers shared by the interactive menu.

pub const RESET: &str = "\x1b[0m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[36m";
pub const WHITE: &str = "\x1b[37m";
pub const BOLD: &str = "\x1b[1m";

pub fn color(code: &str, text: impl AsRef<str>) -> String {
    format!("{code}{}{RESET}", text.as_ref())
}

pub fn red(text: impl AsRef<str>) -> String {
    color(RED, text)
}

pub fn green(text: impl AsRef<str>) -> String {
    color(GREEN, text)
}

pub fn yellow(text: impl AsRef<str>) -> String {
    color(YELLOW, text)
}

pub fn blue(text: impl AsRef<str>) -> String {
    color(BLUE, text)
}

pub fn white(text: impl AsRef<str>) -> String {
    color(WHITE, text)
}

/// A full-width separator line using the two given ANSI colors for its halves.
pub fn rule(open: &str, close: &str) -> String {
    format!("{open}{}{close}", "-".repeat(70))
}
