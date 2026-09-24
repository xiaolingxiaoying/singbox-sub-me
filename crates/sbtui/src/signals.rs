//! The signals that mean "this process is being taken away from us".
//!
//! Every exit has to reach the teardown at the bottom of [`crate::run_app`],
//! because the OS proxy is a machine-wide setting: leaving it pointed at a mixed
//! port that no longer has a listener breaks the user's network until they find
//! the registry key or the GNOME setting themselves. `q` and Ctrl+C reach it
//! because they arrive as *keys*; a closed terminal (`SIGHUP`), `kill`, a service
//! stop or a session logout (`SIGTERM`), and on Windows a console close or break
//! are not keys, and until now nothing was watching for them.
//!
//! `SIGINT` is deliberately not caught. While the terminal is in raw mode
//! crossterm delivers Ctrl+C as a key event and the loop's own quit path handles
//! it, so catching the signal would only remove the one way to stop a wedged UI.

/// Resolves once the operating system asks this process to terminate.
///
/// Panics if a handler cannot be installed: a client that cannot hear the signal
/// is the bug this module exists to fix, and failing loudly at start is better
/// than failing quietly at exit.
pub(crate) async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM can be caught");
        let mut hangup = signal(SignalKind::hangup()).expect("SIGHUP can be caught");
        tokio::select! {
            _ = terminate.recv() => {}
            _ = hangup.recv() => {}
        }
    }
    #[cfg(windows)]
    {
        let mut close = tokio::signal::windows::ctrl_close().expect("the console close can be caught");
        let mut break_event =
            tokio::signal::windows::ctrl_break().expect("the console break can be caught");
        tokio::select! {
            _ = close.recv() => {}
            _ = break_event.recv() => {}
        }
    }
}

/// Whether this exit should clear the machine-wide proxy.
///
/// `kept_proxy_on_purpose` is the user's explicit choice, made by pressing `q`
/// twice while the proxy was on: the first press asks, the second leaves with the
/// proxy as it is. Every other exit — a failed read, a failed draw, or one the
/// operating system forced — clears it, because nobody chose to keep it.
pub(crate) fn should_clear_proxy(system_proxy_enabled: bool, kept_proxy_on_purpose: bool) -> bool {
    system_proxy_enabled && !kept_proxy_on_purpose
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The choice is the user's, and only the user's: an exit the OS forced is
    /// not consent to leave the proxy pointing at a dead port.
    #[test]
    fn only_an_explicit_choice_leaves_the_proxy_on() {
        assert!(
            should_clear_proxy(true, false),
            "a proxy that is on must come off unless the user said otherwise"
        );
        assert!(!should_clear_proxy(true, true), "the user chose to keep it");
        assert!(
            !should_clear_proxy(false, false),
            "nothing to clear when the proxy was never enabled"
        );
    }

    /// What this does *not* prove: that the signal branch reaches the teardown.
    /// That is a wiring fact about an async loop with a terminal attached, and a
    /// unit test cannot see it, so the gate for it is the L2 shell script
    /// `scripts/dev/wsl-signal-exit.sh`, which starts a real sbtui under a pty,
    /// sends it `SIGTERM`, and checks the proxy backup was consumed. A scan of
    /// source lines would have "passed" for the wrong reasons here, so there is
    /// none.
    #[test]
    fn the_proxy_decision_has_three_cases_and_no_other() {
        // on, not chosen  -> clear: the common case, and every forced exit
        assert!(should_clear_proxy(true, false));
        // on, chosen to keep -> leave it: the user answered the prompt
        assert!(!should_clear_proxy(true, true));
        // off             -> nothing to clear, whichever way we exited
        assert!(!should_clear_proxy(false, false));
        assert!(!should_clear_proxy(false, true));
    }
}
