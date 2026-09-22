//! Interface language. Chinese is the authored language; English is supplied
//! alongside it at each site rather than looked up from a table.
//!
//! A dictionary keyed on the Chinese text would silently fall back to Chinese
//! whenever someone reworded a label, which is the failure mode this exists to
//! avoid, and it cannot carry interpolated strings. `tr!` takes both languages
//! at the point of use, so a missing translation is a compile error, and each
//! branch is its own expression, so `format!` works on both sides.

use client_core::clash_api::OutboundMode;
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

#[cfg(test)]
mod tests {
    use super::Locale;

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
}
