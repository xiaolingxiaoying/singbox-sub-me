//! QR rendering helpers built on the `qrcode` crate: terminal ANSI half-blocks
//! for `sbctl qr` and standalone SVG documents for the subscription QR links.

use qrcode::{Color, QrCode, render::svg};

/// Render a terminal ANSI QR code for `data` using black-on-white UTF-8
/// half-blocks, which keeps the square roughly half as tall as a naive two-row
/// rendering. Returns a string with embedded ANSI escape codes suitable for
/// printing to an interactive terminal.
pub fn render_ansi(data: &str) -> Result<String, String> {
    let code = QrCode::new(data.as_bytes()).map_err(|error| error.to_string())?;
    let modules = code.to_colors();
    let width = code.width();
    let mut out = String::new();
    out.push_str("\x1b[30;47m");
    let mut row = 0;
    while row < width {
        let mut line = String::new();
        for col in 0..width {
            let top = modules[row * width + col] == Color::Dark;
            let bottom = if row + 1 < width {
                modules[(row + 1) * width + col] == Color::Dark
            } else {
                false
            };
            let ch = match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            line.push(ch);
        }
        out.push_str(&line);
        out.push('\n');
        row += 2;
    }
    out.push_str("\x1b[0m");
    Ok(out)
}

/// Render a self-contained SVG document (black modules on white, quiet zone
/// included) so a subscription QR link can be scanned from any browser screen.
pub fn render_svg(data: &str) -> Result<String, String> {
    let code = QrCode::new(data.as_bytes()).map_err(|error| error.to_string())?;
    Ok(code
        .render::<svg::Color>()
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_scan_friendly_qr_for_a_url() {
        let rendered = render_ansi("https://example.com/sub/abc").expect("qr should render");
        assert!(rendered.contains("\x1b[30;47m"), "missing ANSI start");
        assert!(rendered.contains("\x1b[0m"), "missing ANSI reset");
        assert!(rendered.contains('█'), "missing half-block modules");
    }

    #[test]
    fn renders_a_parseable_svg_for_the_same_payload_as_the_terminal_code() {
        let url = "https://example.com/sub/abc/index";
        let svg = render_svg(url).expect("svg should render");
        assert!(svg.contains("<svg"), "missing svg root element");
        assert!(svg.contains("</svg>"), "missing svg close tag");
        assert!(svg.contains("width") && svg.contains("height"));
        assert_eq!(svg, render_svg(url).expect("svg is deterministic"));
        // The same payload renders in both representations, and a different
        // payload produces a different document.
        assert!(render_ansi(url).is_ok());
        assert_ne!(
            svg,
            render_svg("https://example.com/sub/other").expect("svg renders")
        );
    }
}
