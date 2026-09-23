//! Machine-readable client events, so a UI can render its own language.
//!
//! The engine phrases its own log lines in Chinese and hands both clients a
//! `VecDeque<String>` — text as data. That is why an English interface still
//! shows Chinese status, event and error lines, and why both UIs decide an
//! event's *colour* by substring-matching Chinese words. The fix is a code
//! table here plus structured arguments, with the wording chosen at render
//! time; `docs/../.scratch/sbgui-progressive-workspace/issues/02-engine-text-english.md`
//! keeps the inventory of what still says `note(...)` with a literal.
//!
//! `RuleKind` in [`crate::state`] is the shape this copies: an enum with `zh()`
//! and `en()` accessors and a structured payload, so a missing translation is
//! a compile error rather than a silent Chinese fallback.

use std::fmt;

/// How prominent an event is. Derived from the code rather than guessed from
/// the rendered text, which is what [`crate::state::log_level_of`] does today.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventLevel {
    #[default]
    Info,
    Warn,
    Error,
}

/// One engine event. `args` fills the `{0}`, `{1}` … placeholders in the
/// templates below; keeping them positional means the vocabulary can grow
/// without a per-code argument struct for every entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    pub code: EventCode,
    pub args: Vec<String>,
}

impl EventRecord {
    pub fn new(code: EventCode, args: Vec<String>) -> Self {
        Self { code, args }
    }

    pub fn level(&self) -> EventLevel {
        self.code.level()
    }

    pub fn render_zh(&self) -> String {
        expand(self.code.zh(), &self.args)
    }

    pub fn render_en(&self) -> String {
        expand(self.code.en(), &self.args)
    }
}

impl fmt::Display for EventRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render_zh())
    }
}

fn expand(template: &str, args: &[String]) -> String {
    let mut rendered = template.to_owned();
    for (index, value) in args.iter().enumerate() {
        rendered = rendered.replace(&format!("{{{index}}}"), value);
    }
    rendered
}

/// The engine's own events. Every variant's Chinese template must reproduce the
/// `note(format!(…))` string it replaced byte-for-byte, or the frames pinned by
/// the client render goldens move for no reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventCode {
    /// `{0}` is the error. The core was running but the proxy-group read failed.
    ProxyGroupRefreshFailed,
    /// `{0}` is the error from a scheduled subscription refresh.
    SubscriptionAutoUpdateFailed,
    /// `{0}` is the error from an automatic restart after an unexpected exit.
    CoreAutoRestartFailed,
    /// `{0}` is the error. The core was running but the connection list failed.
    ConnectionRefreshFailed,
    /// `{0}` is the error. Starting the core on the automatic path failed.
    CoreAutoStartFailed,
    /// A command arrived while another operation held the engine.
    OperationAlreadyRunning,
    /// `{0}` is the profile name.
    ProfileActivated,
    /// Settings were written to disk.
    SettingsSaved,
    /// The core stopped on request.
    CoreStopped,
    /// The active profile has no URL, so there is nothing to pull.
    LocalProfileNeedsNoUpdate,
    /// `{0}` is the profile name whose subscription URL was edited.
    ProfileUrlUpdated,
    /// `{0}` is the profile name that was deleted.
    ProfileDeleted,
    /// `{0}` is the child's exit status.
    CoreExitedUnexpectedly,
    /// `{0}` is the error from `try_wait` on the core child.
    CoreStatusCheckFailed,
    /// `{0}` is why the store could not be read; saving is refused until it is
    /// fixed, so this is the initial status a UI shows when persistence is off.
    StoreUnreadable,
    /// The store loaded and the core is installed, so starting it is the next
    /// step. This is the engine's initial status in that case.
    CoreReadyToStart,
    /// The store loaded but no core is installed yet: download it first.
    CoreNotInstalledHint,
    /// Nothing has been imported yet: a subscription comes before starting.
    ReadyToImportFirstProfile,
}

impl EventCode {
    /// Every code, so the vocabulary tests cannot silently stop covering a
    /// variant added later.
    pub const ALL: &'static [EventCode] = &[
        EventCode::ProxyGroupRefreshFailed,
        EventCode::SubscriptionAutoUpdateFailed,
        EventCode::CoreAutoRestartFailed,
        EventCode::ConnectionRefreshFailed,
        EventCode::CoreExitedUnexpectedly,
        EventCode::CoreStatusCheckFailed,
        EventCode::CoreAutoStartFailed,
        EventCode::OperationAlreadyRunning,
        EventCode::ProfileActivated,
        EventCode::SettingsSaved,
        EventCode::CoreStopped,
        EventCode::LocalProfileNeedsNoUpdate,
        EventCode::ProfileUrlUpdated,
        EventCode::ProfileDeleted,
        EventCode::StoreUnreadable,
        EventCode::CoreReadyToStart,
        EventCode::CoreNotInstalledHint,
        EventCode::ReadyToImportFirstProfile,
    ];

    pub fn zh(self) -> &'static str {
        match self {
            Self::ProxyGroupRefreshFailed => "刷新代理组失败: {0}",
            Self::SubscriptionAutoUpdateFailed => "自动更新订阅失败: {0}",
            Self::CoreAutoRestartFailed => "自动重启失败: {0}",
            Self::ConnectionRefreshFailed => "刷新连接失败: {0}",
            Self::CoreExitedUnexpectedly => "内核意外退出（{0}），准备自动重启",
            Self::CoreStatusCheckFailed => "检测内核状态失败: {0}",
            Self::CoreAutoStartFailed => "自动启动失败: {0}",
            Self::OperationAlreadyRunning => "操作正在进行，请等待完成或停止内核以取消",
            Self::ProfileActivated => "已激活档案 {0}",
            Self::SettingsSaved => "设置已保存",
            Self::CoreStopped => "内核已停止",
            Self::LocalProfileNeedsNoUpdate => "本地档案无需更新",
            Self::ProfileUrlUpdated => "已更新档案 {0} 的订阅链接",
            Self::ProfileDeleted => "已删除订阅 {0}",
            Self::StoreUnreadable => "本地存储无法读取，改动不会被保存：{0}",
            Self::CoreReadyToStart => "就绪。按“启动内核”开始。",
            Self::CoreNotInstalledHint => "就绪。先下载 sing-box 内核，再导入订阅。",
            Self::ReadyToImportFirstProfile => "就绪。先导入订阅，再启动内核。",
        }
    }

    pub fn en(self) -> &'static str {
        match self {
            Self::ProxyGroupRefreshFailed => "Proxy group refresh failed: {0}",
            Self::SubscriptionAutoUpdateFailed => "Automatic subscription update failed: {0}",
            Self::CoreAutoRestartFailed => "Automatic restart failed: {0}",
            Self::ConnectionRefreshFailed => "Connection refresh failed: {0}",
            Self::CoreExitedUnexpectedly => "The core exited unexpectedly ({0}); restarting it",
            Self::CoreStatusCheckFailed => "Could not read the core's status: {0}",
            Self::CoreAutoStartFailed => "Automatic start failed: {0}",
            Self::OperationAlreadyRunning => {
                "An operation is already running; wait for it or stop the core to cancel"
            }
            Self::ProfileActivated => "Activated profile {0}",
            Self::SettingsSaved => "Settings saved",
            Self::CoreStopped => "Core stopped",
            Self::LocalProfileNeedsNoUpdate => "The local profile has no link to update",
            Self::ProfileUrlUpdated => "Updated the subscription link of profile {0}",
            Self::ProfileDeleted => "Deleted subscription {0}",
            Self::StoreUnreadable => {
                "Local storage could not be read; changes will not be saved: {0}"
            }
            Self::CoreReadyToStart => "Ready. Press “Start core” to begin.",
            Self::CoreNotInstalledHint => {
                "Ready. Download the sing-box core first, then import a subscription."
            }
            Self::ReadyToImportFirstProfile => {
                "Ready. Import a subscription first, then start the core."
            }
        }
    }

    pub fn level(self) -> EventLevel {
        match self {
            Self::CoreExitedUnexpectedly | Self::CoreStatusCheckFailed => EventLevel::Warn,
            Self::OperationAlreadyRunning => EventLevel::Warn,
            Self::ProxyGroupRefreshFailed
            | Self::SubscriptionAutoUpdateFailed
            | Self::CoreAutoRestartFailed
            | Self::CoreAutoStartFailed
            | Self::ConnectionRefreshFailed => EventLevel::Error,
            Self::ProfileActivated
            | Self::SettingsSaved
            | Self::CoreStopped
            | Self::LocalProfileNeedsNoUpdate
            | Self::ProfileUrlUpdated
            | Self::ProfileDeleted
            | Self::StoreUnreadable
            | Self::CoreReadyToStart
            | Self::CoreNotInstalledHint
            | Self::ReadyToImportFirstProfile => EventLevel::Info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EventCode, EventLevel, EventRecord};

    #[test]
    fn arguments_fill_the_placeholders_in_both_languages() {
        let record = EventRecord::new(EventCode::CoreExitedUnexpectedly, vec!["signal 9".into()]);
        assert_eq!(record.render_zh(), "内核意外退出（signal 9），准备自动重启");
        assert_eq!(
            record.render_en(),
            "The core exited unexpectedly (signal 9); restarting it"
        );
        assert_eq!(record.level(), EventLevel::Warn);
    }

    #[test]
    fn every_code_declares_both_languages_and_fills_every_placeholder() {
        for code in EventCode::ALL {
            assert!(!code.zh().is_empty() && !code.en().is_empty());
            for (language, template) in [("zh", code.zh()), ("en", code.en())] {
                let record = EventRecord::new(*code, vec!["ARG".to_owned()]);
                let rendered = if language == "zh" {
                    record.render_zh()
                } else {
                    record.render_en()
                };
                if template.contains("{0}") {
                    // A template that still shows `{0}` afterwards would silently
                    // drop a real error message on the floor.
                    assert!(
                        !rendered.contains("{0}"),
                        "{code:?} ({language}) left a hole: {rendered}"
                    );
                    assert!(
                        rendered.contains("ARG"),
                        "{code:?} ({language}) dropped its argument"
                    );
                } else {
                    assert_eq!(
                        rendered, template,
                        "{code:?} ({language}) has no placeholder"
                    );
                }
            }
            // Both languages must agree about whether an argument is expected.
            assert_eq!(
                code.zh().contains("{0}"),
                code.en().contains("{0}"),
                "{code:?} disagrees between languages about its argument"
            );
        }
    }

    /// Both clients colour the status line, and today they do it by matching
    /// Chinese words. A code that fell through to a guessed level would send
    /// them back to guessing, so every one has to classify deliberately.
    #[test]
    fn every_code_classifies_its_severity() {
        for code in EventCode::ALL {
            let level = code.level();
            assert!(
                matches!(
                    level,
                    EventLevel::Info | EventLevel::Warn | EventLevel::Error
                ),
                "{code:?} has no usable level"
            );
        }
    }
}
