//! The parsers and the one validator the fields commit their values through.

use crate::lang::Locale;
use crate::tr;

/// Parses the mixed-inbound port. 0 would point the system proxy nowhere, so
/// it is rejected; the engine rejects the change anyway while the core runs.
pub(crate) fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

/// Parses the auto-update interval in minutes; 0 ("off") is a valid value.
pub(crate) fn parse_count(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

/// Why the 「添加」 click did not add anything. A code rather than a sentence so
/// the panel renders it in whichever language is showing at that moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportReject {
    /// Nothing to import. The terminal client answers the same input with
    /// 「档案名与链接都不能为空」; here only the link is asked for.
    EmptyUrl,
}

impl ImportReject {
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::EmptyUrl => tr!(
                locale,
                "订阅链接不能为空。",
                "The subscription link cannot be empty."
            ),
        }
    }
}

/// What the 「添加」 button (or Enter in the link field) makes of the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImportAttempt {
    /// Ask the engine for this link. `keep_in_field` says whether the text stays
    /// where it is: a line that is not an HTTP(S) URL is the engine's to refuse,
    /// and the half-typed value has to survive the refusal so it can be fixed.
    Submit { url: String, keep_in_field: bool },
    /// Refuse the click here and show the reason, keeping the panel open. Closing
    /// it instead — what this path used to do — swallowed the user's intent with
    /// no word about it (issue 02 item 4).
    Reject(ImportReject),
}

/// The whole of the validation the panel can do on its own, as a pure function
/// so both the button and the Enter key answer through it.
pub(crate) fn classify_import(text: &str) -> ImportAttempt {
    let url = text.trim();
    if url.is_empty() {
        return ImportAttempt::Reject(ImportReject::EmptyUrl);
    }
    ImportAttempt::Submit {
        keep_in_field: !(url.starts_with("http://") || url.starts_with("https://")),
        url: url.to_owned(),
    }
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

    /// The bug this pins: an empty link used to be a silent `return`, and the
    /// click handler closed the panel regardless, so the user's entry vanished
    /// without a word.
    #[test]
    fn an_empty_link_is_refused_with_a_reason_instead_of_vanishing() {
        for blank in ["", "   ", "\t\n"] {
            assert_eq!(
                classify_import(blank),
                ImportAttempt::Reject(ImportReject::EmptyUrl),
                "whitespace is as empty as nothing: {blank:?}"
            );
        }
        assert_eq!(
            ImportReject::EmptyUrl.label(Locale::Zh),
            "订阅链接不能为空。",
            "Chinese keeps the terminal client's shape: it names what is missing"
        );
        assert!(
            !ImportReject::EmptyUrl.label(Locale::En).is_empty(),
            "an English window must not fall back to Chinese for its errors"
        );
        assert_ne!(
            ImportReject::EmptyUrl.label(Locale::Zh),
            ImportReject::EmptyUrl.label(Locale::En)
        );
    }

    #[test]
    fn a_real_link_goes_to_the_engine_and_only_a_valid_one_clears_the_field() {
        assert_eq!(
            classify_import("  https://sub.example.test/sb.json  "),
            ImportAttempt::Submit {
                url: "https://sub.example.test/sb.json".to_owned(),
                keep_in_field: false,
            }
        );
        assert!(matches!(
            classify_import("http://example.test/a"),
            ImportAttempt::Submit {
                keep_in_field: false,
                ..
            }
        ));
        // Not an HTTP(S) link: sent, because the engine owns the detailed
        // complaint, but left in the field so the user can fix it.
        assert_eq!(
            classify_import("ftp://example.test/a"),
            ImportAttempt::Submit {
                url: "ftp://example.test/a".to_owned(),
                keep_in_field: true,
            }
        );
    }
}
