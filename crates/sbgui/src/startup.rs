//! The launch path's own reporting: one trace file in the client's data
//! directory, plus the sentence a window can still carry.
//!
//! A release build of a GUI binary has no console — `windows_subsystem =
//! "windows"` in `main.rs` — so the `eprintln!` that used to be the only word
//! about a refused start went nowhere, and a second instance simply vanished
//! with no trace of why it had (issue 02 item 5). Every outcome the launch path
//! can take is now recorded in `serein-startup.log`, beside the log export, and
//! every outcome that still gets a window says so inside it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use client_core::format::now_epoch;

use crate::lang::Locale;

/// The trace file's name inside the data directory. `serein-` is the prefix the
/// log export already uses, so the client's own files stay together.
pub(crate) const TRACE_FILE: &str = "serein-startup.log";

/// Where [`record`] last wrote, so the panic hook — which gets no arguments of
/// its own — has somewhere to put its line. Empty until the data directory is
/// resolved, which is the one window of time the client genuinely cannot report
/// into a file.
static TRACE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// One trace line: the epoch stamp, the process, and what happened. The stamp
/// stays numeric on purpose — a formatted date would need a calendar library,
/// and what this file has to answer is which start said what, in which order.
pub(crate) fn trace_line(now: u64, pid: u32, message: &str) -> String {
    format!("[{now}] pid={pid} {message}\n")
}

/// Appends one line, creating the file on first use. The error carries the path,
/// because a trace that cannot be written must not go quiet either.
pub(crate) fn append_trace(dir: &Path, now: u64, pid: u32, message: &str) -> Result<(), String> {
    let path = dir.join(TRACE_FILE);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(trace_line(now, pid, message).as_bytes()))
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Records one launch event in the file, then mirrors it on stderr for the runs
/// that do have a console. A failed write is reported, never swallowed.
pub(crate) fn record(dir: &Path, message: &str) {
    if let Err(error) = append_trace(dir, now_epoch(), std::process::id(), message) {
        eprintln!("sbgui: 无法写入启动日志 / could not write the startup trace: {error}");
    }
    eprintln!("sbgui: {message}");
}

/// The line a panic leaves behind: what unwound, and from where.
pub(crate) fn panic_line(payload: &str, file: &str, line: u32) -> String {
    format!("panic: {payload} at {file}:{line}")
}

/// Chains a hook that puts panics in the trace file, then keeps the default one
/// so a debug run still sees them on the console. A release GUI build has no
/// console at all, which is why an `expect` on the way up used to be the silent
/// exit this item is about. Best effort by construction: it can only report once
/// the data directory is resolved, and a toolkit that installs its own hook
/// afterwards can still take the baton.
pub(crate) fn install_panic_hook(dir: &Path) {
    let _ = TRACE_DIR.set(dir.to_path_buf());
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "无法解析的恐慌负载 / unparseable panic payload".to_owned());
        if let (Some(location), Some(dir)) = (info.location(), TRACE_DIR.get()) {
            let line = panic_line(&payload, location.file(), location.line());
            record(dir, &line);
        }
        default(info);
    }));
}

/// What the launch path decided before it had a window to say it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    /// The instance lock is ours; nothing to report.
    Proceed,
    /// Another instance holds the data directory. Leaving is correct — two
    /// clients would fight over the mixed port, the operating-system proxy and
    /// the runtime configuration — but it no longer leaves silently.
    AlreadyRunning,
    /// The lock file could not be taken at all: a permissions problem, not a
    /// conflict. Still fatal, because starting unlocked is what the lock exists
    /// to prevent.
    LockFailed(String),
    /// The data directory itself could not be resolved, so there is nowhere to
    /// write. The one case the console and the exit code have to carry alone.
    NoDataDir(String),
    /// A persisted file could not be read and the client starts on defaults.
    /// Not fatal, and precisely the case that must not be silent: the user is
    /// about to see settings they never chose.
    DefaultsInstead(String),
}

impl Launch {
    /// The outcome in both languages, or `None` when the start was clean. The
    /// trace file carries both halves: whoever reads it is not necessarily the
    /// person the window was talking to.
    fn reason(&self) -> Option<(String, String)> {
        Some(match self {
            Self::Proceed => return None,
            Self::AlreadyRunning => (
                "已有另一个实例在运行，本进程退出".to_owned(),
                "another instance is already running, this process is exiting".to_owned(),
            ),
            Self::LockFailed(error) => (
                format!("无法锁定数据目录，本进程退出: {error}"),
                format!("could not lock the data directory, this process is exiting: {error}"),
            ),
            Self::NoDataDir(error) => (
                format!("无法打开数据目录，本进程退出: {error}"),
                format!("could not open the data directory, this process is exiting: {error}"),
            ),
            Self::DefaultsInstead(what) => (
                format!("未能读取 {what}，已按默认值启动"),
                format!("could not read {what}, starting with the defaults"),
            ),
        })
    }

    /// The line for the trace file.
    pub(crate) fn trace(&self) -> Option<String> {
        self.reason().map(|(zh, en)| format!("{zh} / {en}"))
    }

    /// The sentence for the window, in the language it is showing. Only an
    /// outcome that still opens a window has one: a process that exits has
    /// nowhere to put it, and the trace file is what it leaves behind.
    pub(crate) fn notice(&self, locale: Locale) -> Option<String> {
        if self.fatal() {
            return None;
        }
        let (zh, en) = self.reason()?;
        Some(if locale == Locale::En { en } else { zh })
    }

    /// Whether the process stops before it can open a window.
    pub(crate) fn fatal(&self) -> bool {
        matches!(
            self,
            Self::AlreadyRunning | Self::LockFailed(_) | Self::NoDataDir(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line has to survive being read a week later by someone who was not
    /// there, which means the process and the reason both have to be in it.
    #[test]
    fn a_trace_line_names_its_moment_its_process_and_its_reason() {
        let line = trace_line(1_761_234_567, 4242, "已有另一个实例在运行");
        assert!(line.starts_with("[1761234567] "), "{line}");
        assert!(line.contains("pid=4242"), "{line}");
        assert!(line.ends_with("已有另一个实例在运行\n"), "{line}");
    }

    /// The hook's whole job is turning a panic into something a person can act
    /// on: the payload alone would not say which `expect` died, and a release
    /// build has no console for either of them to be printed to.
    #[test]
    fn a_panic_line_names_the_payload_and_the_place() {
        let line = panic_line("data directory", "crates/sbgui/src/main.rs", 104);
        assert!(line.contains("data directory"), "{line}");
        assert!(
            line.ends_with("crates/sbgui/src/main.rs:104"),
            "a panic line with no place in it is not actionable: {line}"
        );
    }

    #[test]
    fn the_trace_appends_one_line_per_start() {
        let dir = test_dir("append");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");
        append_trace(&dir, 1, 10, "first").expect("the first line writes");
        append_trace(&dir, 2, 10, "second").expect("the second line appends");
        let written = std::fs::read_to_string(dir.join(TRACE_FILE)).expect("the file is readable");
        assert_eq!(
            written, "[1] pid=10 first\n[2] pid=10 second\n",
            "one line per event, newest last, and the file never truncated"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A write that cannot happen (the directory is not there) has to come back
    /// as an error the caller prints: a trace tool that fails quietly is the bug
    /// this module exists to fix.
    #[test]
    fn an_unwritable_trace_says_so_instead_of_going_quiet() {
        let missing = test_dir("missing").join("no-such-directory");
        let error = append_trace(&missing, 1, 1, "启动")
            .expect_err("writing into an absent directory cannot succeed");
        assert!(
            error.contains(TRACE_FILE),
            "the complaint names the file it could not write: {error}"
        );
    }

    /// The three silent exits of the old launch path each owed a sentence, and
    /// only a clean start owes none.
    #[test]
    fn every_outcome_the_process_leaves_on_explains_itself_in_both_languages() {
        assert_eq!(
            Launch::Proceed.trace(),
            None,
            "a good start has nothing to say"
        );
        for outcome in [
            Launch::AlreadyRunning,
            Launch::LockFailed("PermissionDenied".to_owned()),
            Launch::NoDataDir("no config dir".to_owned()),
            Launch::DefaultsInstead("settings.toml".to_owned()),
        ] {
            let (zh, en) = outcome
                .reason()
                .unwrap_or_else(|| panic!("{outcome:?} leaves the user with no reason"));
            assert!(!zh.is_empty() && !en.is_empty(), "{outcome:?} is mute");
            assert_ne!(zh, en, "{outcome:?} has no English half");
            let trace = outcome.trace().expect("a trace line for a fatal outcome");
            assert!(
                trace.contains(" / "),
                "the file carries both languages: {trace}"
            );
        }
    }

    /// Which outcomes stop the process, and which ones still get to speak in the
    /// window: an exiting process has nowhere to put a notice, so `notice` and
    /// `fatal` have to agree or a message is written to nobody.
    #[test]
    fn a_notice_only_survives_outcomes_that_still_open_a_window() {
        for outcome in [
            Launch::AlreadyRunning,
            Launch::LockFailed("boom".to_owned()),
            Launch::NoDataDir("boom".to_owned()),
        ] {
            assert!(outcome.fatal(), "{outcome:?} must stop the process");
            assert_eq!(
                outcome.notice(Locale::Zh),
                None,
                "{outcome:?} exits, so nothing would render the notice"
            );
        }
        let degraded = Launch::DefaultsInstead("settings.toml".to_owned());
        assert!(
            !degraded.fatal(),
            "starting on defaults still opens a window"
        );
        assert_eq!(
            degraded.notice(Locale::Zh),
            Some("未能读取 settings.toml，已按默认值启动".to_owned())
        );
        assert_eq!(
            degraded.notice(Locale::En),
            Some("could not read settings.toml, starting with the defaults".to_owned()),
            "an English window never inherits Chinese from the record"
        );
    }

    /// The same convention `client_core::settings`' own tests use.
    fn test_dir(case: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sbgui-startup-test-{}-{case}", std::process::id()))
    }
}
