//! Display text and severity bands shared by both clients.
//!
//! The terminal and desktop clients are meant to be two renderings of one
//! control plane, so anything that decides *what a value reads as* belongs
//! here: bytes, relative ages, the subscription usage line, the latency band a
//! node falls in. Colours and widgets stay in each UI on purpose — a ratatui
//! `Color` and a GPUI `u32` cannot be shared, and the palettes are deliberately
//! different.

use crate::subscription::SubscriptionUserinfo;
use std::time::{SystemTime, UNIX_EPOCH};

/// Whole seconds since the Unix epoch; a clock before the epoch reads as zero.
pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A byte count in binary units, one decimal place above the byte threshold.
pub fn human_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// How long ago an epoch timestamp was, phrased for a label that must not grow
/// wider as it ages.
pub fn age_label(epoch_seconds: u64) -> String {
    if epoch_seconds == 0 {
        return "从未更新".to_owned();
    }
    let age = now_epoch().saturating_sub(epoch_seconds);
    if age < 3600 {
        format!("{} 分钟前", age / 60)
    } else if age < 86_400 {
        format!("{} 小时前", age / 3600)
    } else {
        format!("{} 天前", age / 86_400)
    }
}

/// Whole seconds elapsed since an RFC3339 timestamp such as the core's
/// connection `start` field; `None` when it is empty or unparseable.
///
/// An absolute timestamp is the wrong thing for a table column: at the width
/// the column gets it renders as a truncated `2026-09-23T0…`, and every
/// connection opened inside the same second looks identical. Both clients
/// therefore show an age. The wording deliberately stays in the UIs (`tr!`) so
/// this module keeps its language-free contract.
pub fn seconds_since(start: &str) -> Option<u64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(start).ok()?;
    Some(now_epoch().saturating_sub(parsed.timestamp().max(0) as u64))
}

/// A one-line rendering of the subscription's traffic metadata, including the
/// reset window when the provider advertises an expiry.
pub fn usage_label(usage: Option<&SubscriptionUserinfo>) -> String {
    let Some(usage) = usage else {
        return "用量信息待更新".to_owned();
    };
    let mut text = match usage.remaining() {
        Some(remaining) => format!(
            "已用 {} / {} · 剩余 {}",
            human_bytes(usage.used()),
            human_bytes(usage.total),
            human_bytes(remaining)
        ),
        None => format!("已用 {} · 未设置配额", human_bytes(usage.used())),
    };
    if let Some(expire) = usage.expire {
        let now = now_epoch();
        if expire > now {
            text.push_str(&format!(" · {} 天后重置", (expire - now) / 86_400));
        } else {
            text.push_str(" · 已到期");
        }
    }
    text
}

/// The latency band a measurement falls in. The thresholds are a product
/// decision shared by both clients; each client maps the band to its own
/// palette, including what an unmeasured node looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayLevel {
    Fast,
    Slow,
    Timeout,
    Unknown,
}

/// Delays under 200 ms are snappy, under 500 ms usable, and anything else is
/// reported as a timeout band rather than a number to compare.
pub fn delay_level(delay: Option<u64>) -> DelayLevel {
    match delay {
        Some(delay) if delay < 200 => DelayLevel::Fast,
        Some(delay) if delay < 500 => DelayLevel::Slow,
        Some(_) => DelayLevel::Timeout,
        None => DelayLevel::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_progresses_through_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MiB");
    }

    #[test]
    fn seconds_since_reads_the_core_timestamp_and_rejects_junk() {
        let two_minutes_ago = chrono::DateTime::from_timestamp(now_epoch() as i64 - 120, 0)
            .expect("a recent epoch")
            .to_rfc3339();
        let elapsed = seconds_since(&two_minutes_ago).expect("an RFC3339 value parses");
        assert!(
            (119..=122).contains(&elapsed),
            "a timestamp 120 seconds old must read as about 120, got {elapsed}"
        );
        assert_eq!(seconds_since(""), None, "an absent start stays absent");
        assert_eq!(seconds_since("2026-09-23 01:00"), None);
        // Clock skew that puts a connection in the future must read as new
        // rather than wrapping a u64 subtraction around.
        let future = chrono::DateTime::from_timestamp(now_epoch() as i64 + 3_600, 0)
            .expect("a near-future epoch")
            .to_rfc3339();
        assert_eq!(seconds_since(&future), Some(0));
    }

    #[test]
    fn age_label_distinguishes_never_from_recent() {
        assert_eq!(age_label(0), "从未更新");
        let now = now_epoch();
        assert_eq!(age_label(now - 90), "1 分钟前");
    }

    #[test]
    fn usage_label_reports_the_reset_window() {
        let usage = SubscriptionUserinfo {
            upload: 0,
            download: 0,
            total: 1024,
            expire: Some(now_epoch() + 3 * 86_400),
        };
        let label = usage_label(Some(&usage));
        assert!(label.contains("3 天后重置"), "unexpected label: {label}");
        let expired = SubscriptionUserinfo {
            expire: Some(now_epoch().saturating_sub(10)),
            ..usage
        };
        assert!(usage_label(Some(&expired)).contains("已到期"));
    }

    #[test]
    fn usage_label_names_the_missing_metadata_and_the_uncapped_quota() {
        assert_eq!(usage_label(None), "用量信息待更新");
        let uncapped = SubscriptionUserinfo {
            upload: 1024,
            download: 0,
            total: 0,
            expire: None,
        };
        assert_eq!(usage_label(Some(&uncapped)), "已用 1.0 KiB · 未设置配额");
    }

    #[test]
    fn delay_level_bands_match_what_both_clients_drew() {
        assert_eq!(delay_level(None), DelayLevel::Unknown);
        assert_eq!(delay_level(Some(199)), DelayLevel::Fast);
        assert_eq!(delay_level(Some(200)), DelayLevel::Slow);
        assert_eq!(delay_level(Some(499)), DelayLevel::Slow);
        assert_eq!(delay_level(Some(500)), DelayLevel::Timeout);
    }
}
