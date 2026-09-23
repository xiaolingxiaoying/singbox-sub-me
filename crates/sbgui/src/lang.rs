//! Interface language. Chinese is the authored language; English is supplied
//! alongside it at each site rather than looked up from a table.
//!
//! A dictionary keyed on the Chinese text would silently fall back to Chinese
//! whenever someone reworded a label, which is the failure mode this exists to
//! avoid, and it cannot carry interpolated strings. `tr!` takes both languages
//! at the point of use, so a missing translation is a compile error, and each
//! branch is its own expression, so `format!` works on both sides.

use client_core::clash_api::OutboundMode;
use client_core::format::{
    age_label as core_age_label, human_bytes, now_epoch, usage_label as core_usage_label,
};
use client_core::system_proxy::TrafficMode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Locale {
    #[default]
    Zh,
    En,
}

impl Locale {
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Zh => Self::En,
            Self::En => Self::Zh,
        }
    }

    /// The label of the button that switches away from this language: it names
    /// the language it will give you, not the one you are in.
    pub(crate) fn next_label(self) -> &'static str {
        match self {
            Self::Zh => "English",
            Self::En => "中文",
        }
    }
}

#[macro_export]
macro_rules! tr {
    ($locale:expr, $zh:expr, $en:expr $(,)?) => {
        if $locale == $crate::lang::Locale::En {
            $en
        } else {
            $zh
        }
    };
}

/// The engine's own enums carry a Chinese `label` only, and the interface has
/// to name them in either language. Writing the pair here — one arm per
/// variant, exactly as `Page::title` does it — keeps a new engine state a
/// compile error until someone says what it is called in English.
pub(crate) fn outbound_mode(mode: OutboundMode, locale: Locale) -> &'static str {
    match mode {
        OutboundMode::Rule => tr!(locale, "规则", "Rule"),
        OutboundMode::Global => tr!(locale, "全局", "Global"),
        OutboundMode::Direct => tr!(locale, "直连", "Direct"),
    }
}

pub(crate) fn traffic_mode(mode: TrafficMode, locale: Locale) -> &'static str {
    match mode {
        TrafficMode::SystemProxy => tr!(locale, "系统代理", "System proxy"),
        TrafficMode::Tun => "TUN",
    }
}

/// The two relative-time strings the engine formats for itself. They live in
/// `client-core` because the terminal client renders them too, so the Chinese
/// half stays the shared helper's output — delegating keeps the two clients
/// word-for-word identical — and only the English half is written here.
pub(crate) fn age_label(epoch_seconds: u64, locale: Locale) -> String {
    if locale != Locale::En {
        return core_age_label(epoch_seconds);
    }
    if epoch_seconds == 0 {
        return "Never updated".to_owned();
    }
    let age = now_epoch().saturating_sub(epoch_seconds);
    if age < 3_600 {
        format!("{} min ago", age / 60)
    } else if age < 86_400 {
        format!("{} h ago", age / 3_600)
    } else {
        format!("{} d ago", age / 86_400)
    }
}

/// The connection's start time as a relative age. The wire value is RFC3339,
/// which both overflowed the narrow column and said nothing at a glance;
/// client-core owns parsing and the Chinese half, this file owns the English
/// wording.
pub(crate) fn connection_age_label(start: &str, locale: Locale) -> String {
    let epoch = client_core::format::connection_start_epoch(start);
    if locale != Locale::En {
        return core_age_label(epoch);
    }
    let age = now_epoch().saturating_sub(epoch);
    if age < 60 {
        "Just now".to_owned()
    } else if age < 3_600 {
        format!("{} min ago", age / 60)
    } else if age < 86_400 {
        format!("{} h ago", age / 3_600)
    } else {
        format!("{} d ago", age / 86_400)
    }
}

/// The subscription quota line, same arrangement as [`age_label`].
pub(crate) fn usage_label(
    usage: Option<&client_core::subscription::SubscriptionUserinfo>,
    locale: Locale,
) -> String {
    if locale != Locale::En {
        return core_usage_label(usage);
    }
    let Some(usage) = usage else {
        return "Usage not reported yet".to_owned();
    };
    let mut text = match usage.remaining() {
        Some(remaining) => format!(
            "Used {} / {} · {} left",
            human_bytes(usage.used()),
            human_bytes(usage.total),
            human_bytes(remaining)
        ),
        None => format!("Used {} · no quota set", human_bytes(usage.used())),
    };
    if let Some(expire) = usage.expire {
        let now = now_epoch();
        if expire > now {
            text.push_str(&format!(" · resets in {} days", (expire - now) / 86_400));
        } else {
            text.push_str(" · expired");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{Locale, age_label, core_age_label, core_usage_label, usage_label};

    #[test]
    fn the_button_names_the_language_it_switches_to() {
        assert_eq!(Locale::Zh.next_label(), "English");
        assert_eq!(Locale::En.next_label(), "中文");
    }

    #[test]
    fn toggling_round_trips() {
        assert_eq!(Locale::default(), Locale::Zh);
        assert_eq!(Locale::Zh.toggle().toggle(), Locale::Zh);
        assert_eq!(Locale::En.toggle(), Locale::Zh);
    }

    #[test]
    fn tr_selects_by_locale_and_keeps_interpolation_on_both_sides() {
        for locale in [Locale::Zh, Locale::En] {
            let plain = tr!(locale, "概览", "Overview");
            let counted = tr!(
                locale,
                format!("{n} 个代理组", n = 2),
                format!("{n} groups", n = 2)
            );
            if locale == Locale::Zh {
                assert_eq!((plain, counted), ("概览", "2 个代理组".to_owned()));
            } else {
                assert_eq!((plain, counted), ("Overview", "2 groups".to_owned()));
            }
        }
    }

    /// The engine's own helpers stay the single source of the Chinese wording,
    /// so the two clients can never drift apart by one character.
    #[test]
    fn the_engine_helpers_keep_their_chinese_wording() {
        let now = client_core::format::now_epoch();
        for epoch in [0, now - 180, now - 7_200, now - 172_800] {
            assert_eq!(age_label(epoch, Locale::Zh), core_age_label(epoch));
        }
        assert_eq!(usage_label(None, Locale::Zh), core_usage_label(None));
    }

    #[test]
    fn the_engine_helpers_have_an_english_half() {
        let now = client_core::format::now_epoch();
        assert_eq!(age_label(0, Locale::En), "Never updated");
        assert_eq!(age_label(now - 180, Locale::En), "3 min ago");
        assert_eq!(age_label(now - 7_200, Locale::En), "2 h ago");
        assert_eq!(age_label(now - 172_800, Locale::En), "2 d ago");
        assert_eq!(usage_label(None, Locale::En), "Usage not reported yet");
        let quota = client_core::subscription::SubscriptionUserinfo {
            upload: 0,
            download: 1024,
            total: 2048,
            expire: None,
        };
        assert_eq!(
            usage_label(Some(&quota), Locale::En),
            "Used 1.0 KiB / 2.0 KiB · 1.0 KiB left"
        );
        let uncapped = client_core::subscription::SubscriptionUserinfo { total: 0, ..quota };
        assert_eq!(
            usage_label(Some(&uncapped), Locale::En),
            "Used 1.0 KiB · no quota set"
        );
    }
}
