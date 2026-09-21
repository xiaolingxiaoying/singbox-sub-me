//! The two parsers the settings fields commit their values through.

/// Parses the mixed-inbound port. 0 would point the system proxy nowhere, so
/// it is rejected; the engine rejects the change anyway while the core runs.
pub(crate) fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

/// Parses the auto-update interval in minutes; 0 ("off") is a valid value.
pub(crate) fn parse_count(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_port_accepts_real_ports_and_rejects_zero_or_garbage() {
        assert_eq!(parse_port("2080"), Some(2080));
        assert_eq!(parse_port(" 7890 "), Some(7890));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port(""), None);
        assert_eq!(parse_port("abc"), None);
        assert_eq!(parse_port("99999"), None);
    }

    #[test]
    fn parse_count_accepts_zero_for_off() {
        assert_eq!(parse_count("0"), Some(0));
        assert_eq!(parse_count("30"), Some(30));
        assert_eq!(parse_count("-1"), None);
        assert_eq!(parse_count(""), None);
    }
}
