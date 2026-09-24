//! The parsers and the one validator the fields commit their values through.

use client_core::ClientCommand;

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
    /// The path field's own reason, because the sentence has to name the field
    /// that is blank: one message for both would send the user back to the link.
    EmptyPath,
}

impl ImportReject {
    pub(crate) fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::EmptyUrl => tr!(
                locale,
                "订阅链接不能为空。",
                "The subscription link cannot be empty."
            ),
            Self::EmptyPath => tr!(
                locale,
                "本地 JSON 文件路径不能为空。",
                "The local JSON file path cannot be empty."
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

/// What one of the subscriptions page's three commits sends, or why it sends
/// nothing. [`classify_import`] reads a link; this says which command the read
/// value buys, which is where the profile name and the local path live — the
/// two things a field holds that a handler could otherwise drop on the floor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImportRequest {
    Send {
        command: ClientCommand,
        /// See `ImportAttempt::Submit`: text only the engine can rule on has to
        /// survive the click so it can be fixed.
        keep_in_field: bool,
    },
    Reject(ImportReject),
}

/// 「添加」 and Enter in either import field: the link decides the shape, the name
/// field only labels the profile.
pub(crate) fn subscription_request(name: &str, url: &str) -> ImportRequest {
    match classify_import(url) {
        ImportAttempt::Submit { url, keep_in_field } => ImportRequest::Send {
            command: ClientCommand::ImportSubscription {
                name: import_name(name),
                url,
            },
            keep_in_field,
        },
        ImportAttempt::Reject(reason) => ImportRequest::Reject(reason),
    }
}

/// A blank name is not a name: the engine keeps generating its `订阅 N` labels
/// only for a profile it heard no name for, so this must be `None` rather than
/// an empty string.
pub(crate) fn import_name(text: &str) -> Option<String> {
    let name = text.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// 「导入本地 JSON」 and Enter in the path field: the other command the panel
/// offers, and the only one that never touches a URL. Whether the file exists
/// and parses is the engine's answer, so the path always stays in the field —
/// a refusal has to leave the text that caused it.
pub(crate) fn file_import_request(path: &str) -> ImportRequest {
    let path = path.trim();
    if path.is_empty() {
        return ImportRequest::Reject(ImportReject::EmptyPath);
    }
    ImportRequest::Send {
        command: ClientCommand::ImportProfileFile(path.to_owned()),
        keep_in_field: true,
    }
}

/// 「保存链接」 on one profile row: the import panel's link rules again, but the
/// command rewrites an existing profile instead of adding one. An empty field is
/// refused here rather than passed down, because `SetProfileUrl` has no
/// file-only meaning — clearing a profile's link has to stay impossible.
pub(crate) fn link_edit_request(profile: &str, url: &str) -> ImportRequest {
    match classify_import(url) {
        ImportAttempt::Submit { url, keep_in_field } => ImportRequest::Send {
            command: ClientCommand::SetProfileUrl {
                name: profile.to_owned(),
                url,
            },
            keep_in_field,
        },
        ImportAttempt::Reject(reason) => ImportRequest::Reject(reason),
    }
}

/// Which row's link editor is open. Arming the open row again closes it, and any
/// other row moves the editor there, so two rows never edit at once.
pub(crate) fn url_editor_target(open: Option<&str>, clicked: &str) -> Option<String> {
    (open != Some(clicked)).then(|| clicked.to_owned())
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

    /// The claim of item 1: a path typed in the panel has to reach the engine as
    /// a *file*, because sending it as a subscription link would answer with
    /// "订阅地址必须是有效的 HTTP/HTTPS 链接" and never look at the file.
    #[test]
    fn a_local_path_is_imported_as_a_file_and_stays_in_the_field() {
        assert_eq!(
            file_import_request("  C:\\sbctl\\home 本地.json  "),
            ImportRequest::Send {
                command: ClientCommand::ImportProfileFile("C:\\sbctl\\home 本地.json".to_owned()),
                keep_in_field: true,
            }
        );
        for path in ["/home/u/.config/sb/local.json", "D:\\sb.json", "local.json"] {
            assert!(
                !matches!(
                    file_import_request(path),
                    ImportRequest::Send {
                        command: ClientCommand::ImportSubscription { .. },
                        ..
                    }
                ),
                "a path must never be fetched as a link: {path}"
            );
        }
    }

    #[test]
    fn an_empty_path_is_refused_with_a_reason_of_its_own() {
        for blank in ["", "   ", "\t\n"] {
            assert_eq!(
                file_import_request(blank),
                ImportRequest::Reject(ImportReject::EmptyPath),
                "a blank path is no file to read: {blank:?}"
            );
        }
        assert_eq!(
            ImportReject::EmptyPath.label(Locale::Zh),
            "本地 JSON 文件路径不能为空。",
            "Chinese names the field the way the other refusal names its field"
        );
        assert_ne!(
            ImportReject::EmptyPath.label(Locale::Zh),
            ImportReject::EmptyUrl.label(Locale::Zh),
            "the two blanks must not blame the same field"
        );
        assert!(
            !ImportReject::EmptyPath.label(Locale::En).contains("本地"),
            "an English window must not fall back to Chinese for its errors"
        );
    }

    /// The claim of item 2: the name field is not decoration. A typed name has
    /// to survive trimming and reach the command, and a blank one has to reach
    /// it as `None` so the engine's own `订阅 N` labelling stays in place.
    #[test]
    fn a_typed_profile_name_reaches_the_engine_and_a_blank_one_does_not() {
        assert_eq!(
            subscription_request(" 家宽 ", "https://sub.example.test/sb.json"),
            ImportRequest::Send {
                command: ClientCommand::ImportSubscription {
                    name: Some("家宽".to_owned()),
                    url: "https://sub.example.test/sb.json".to_owned(),
                },
                keep_in_field: false,
            }
        );
        for blank in ["", "   ", "\n"] {
            assert_eq!(
                import_name(blank),
                None,
                "whitespace is not a name: {blank:?}"
            );
        }
        // The link still owns the refusal, however good the name is: the old
        // silent no-op (issue 02 item 4) must not come back through the name.
        assert_eq!(
            subscription_request("家宽", "   "),
            ImportRequest::Reject(ImportReject::EmptyUrl)
        );
    }

    /// The claim of item 3: the row editor rewrites the profile it was opened
    /// on, adds nothing, and refuses to blank a profile's link out.
    #[test]
    fn the_link_editor_rewrites_its_own_profile_and_refuses_an_empty_link() {
        assert_eq!(
            link_edit_request("内网订阅", "https://new.example.test/sb.json"),
            ImportRequest::Send {
                command: ClientCommand::SetProfileUrl {
                    name: "内网订阅".to_owned(),
                    url: "https://new.example.test/sb.json".to_owned(),
                },
                keep_in_field: false,
            }
        );
        assert_eq!(
            link_edit_request("内网订阅", "   "),
            ImportRequest::Reject(ImportReject::EmptyUrl),
            "a file-only profile's editor opens empty; saving that must not clear the link"
        );
        // Same rule as the import field for text the engine has to rule on.
        assert_eq!(
            link_edit_request("内网订阅", "ftp://example.test/a"),
            ImportRequest::Send {
                command: ClientCommand::SetProfileUrl {
                    name: "内网订阅".to_owned(),
                    url: "ftp://example.test/a".to_owned(),
                },
                keep_in_field: true,
            }
        );
        let edited = link_edit_request("内网订阅", "https://x.test/a");
        assert!(
            !matches!(
                edited,
                ImportRequest::Send {
                    command: ClientCommand::ImportSubscription { .. },
                    ..
                }
            ),
            "editing a link must never import a second profile: {edited:?}"
        );
    }

    #[test]
    fn one_link_editor_is_open_at_a_time_and_arming_it_again_closes_it() {
        assert_eq!(
            url_editor_target(None, "内网"),
            Some("内网".to_owned()),
            "a closed row opens on its first click"
        );
        assert_eq!(url_editor_target(Some("内网"), "内网"), None);
        assert_eq!(
            url_editor_target(Some("内网"), "机场"),
            Some("机场".to_owned()),
            "another row moves the editor rather than stacking a second one"
        );
    }
}
