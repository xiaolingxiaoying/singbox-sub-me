//! End-to-end proof that a killed client takes its core with it (issue: the
//! non-Windows `attach_orphan_guard` used to return `true` having armed nothing).
//!
//! A unit test cannot cover this: the guarantee is about what happens *after*
//! the process that made the promise stops. So the test runs itself twice —
//! once as a parent that starts a stub core and then gets `kill -9`'d, and once
//! as the observer that checks whether the core survived its parent.
//!
//! Linux-only, because `PR_SET_PDEATHSIG` is what provides the guarantee there;
//! macOS has no equivalent and `core::attach_orphan_guard` says so.

#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The probe parent: starts a stub core, prints its pid, then waits to be
/// killed. `exec sleep` in the stub means the reported pid *is* the long-lived
/// process, so checking it afterwards checks the core and not a shell wrapper.
const PROBE_ENV: &str = "SBCTL_ORPHAN_PROBE_PARENT";

fn stub_core(dir: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("stub-core");
    // Keep the stub to one process, like the sing-box executable. A shell
    // wrapper that traps SIGTERM can intentionally stay alive after the kernel
    // sends the parent-death signal, testing shell trap semantics instead of
    // the core's orphan guard.
    std::fs::write(&path, "#!/bin/sh\nexec sleep 120\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Whether the pid names a *live* process.
///
/// A zombie still has a `/proc/<pid>` entry and its `PPid` still points at the
/// parent, so the naive existence check is satisfied by a child that already
/// exited — and then it disappears the instant the parent dies and init reaps
/// it, which looks exactly like the orphan guard having worked. That is how the
/// first two versions of this test produced a green they had not earned.
fn alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !proc_state(&stat).starts_with('Z'),
        Err(_) => false,
    }
}

/// The state letter from `/proc/<pid>/stat`, after the comm field (which itself
/// contains spaces and parentheses). Field order past `)` is state, ppid, pgrp…
/// so the state is the *first* token, not the second.
fn proc_state(stat: &str) -> String {
    match stat.rsplit_once(')') {
        Some((_, rest)) => rest
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        None => String::new(),
    }
}

/// Whether the stub script has replaced itself with the long-lived process.
/// Sampling the wrapper before `exec` would only prove that the shell dies.
fn is_stub_process(pid: u32) -> bool {
    let cmdline = command_line(pid);
    cmdline
        .split_whitespace()
        .next()
        .and_then(|arg| Path::new(arg).file_name())
        .is_some_and(|name| name == "sleep")
}

fn command_line(pid: u32) -> String {
    String::from_utf8_lossy(&std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default())
        .replace('\0', " ")
        .trim()
        .to_owned()
}

/// A one-line description of the pid for diagnostics: state, parent, command.
fn describe(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    format!(
        "pid {pid} state={} ppid={:?} cmd={:?}",
        proc_state(&stat),
        parent_of(pid),
        command_line(pid)
    )
}

/// The parent recorded in `/proc/<pid>/status`, or `None` once the pid is gone.
/// The observer uses this instead of trusting the pid it is handed: a misread
/// number makes the whole test pass without the guard doing anything.
fn parent_of(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("PPid:")
            .and_then(|rest| rest.trim().parse::<u32>().ok())
    })
}

async fn run_probe_parent() {
    let dir = tempfile::tempdir().unwrap();
    let handle = client_core::core::start(
        &stub_core(dir.path()),
        &dir.path().join("config.json"),
        &dir.path().join("core.log"),
    )
    .await
    .expect("the stub core starts");
    let pid = handle.child.id().expect("the stub core has a pid");
    // The guard must be claimed for the claim to be testable at all.
    assert!(handle.orphan_guard, "the probe expects an armed guard");
    println!("PROBE {} {}", std::process::id(), pid);
    std::io::stdout().flush().unwrap();
    // Keep the handle alive: dropping it would `kill_on_drop` the child and
    // prove nothing about what the kernel does when the parent dies instead.
    tokio::time::sleep(Duration::from_secs(120)).await;
    std::process::exit(0);
}

#[test]
fn a_killed_client_leaves_no_core_behind() {
    // Falsifiability was the hard part here, not the fix. Two ways this file
    // produced a green it had not earned, both now closed:
    //
    //  1. Liveness by `/proc/<pid>` existence. A zombie keeps that entry and its
    //     `PPid` still points at the parent, so an already-exited core passed
    //     "it came up", then vanished when init reaped it after the kill —
    //     indistinguishable from the guard working. `alive` now reads the state
    //     letter and treats `Z` as dead.
    //  2. The cleanup ran before the assertion. Reaching `assert!(!alive(..))`
    //     *after* killing the surviving core made an orphan pass by destroying
    //     the evidence. Liveness is sampled once and the assert uses the sample.
    //
    // Verified by mutation: with `prearm_orphan_guard` reduced to a no-op that
    // still reports success, this test fails (`state=S`, reparented, still
    // running); with the guard armed it passes because the core is gone.
    if std::env::var_os(PROBE_ENV).is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a probe runtime")
            .block_on(run_probe_parent());
        std::process::exit(0);
    }

    let exe = std::env::current_exe().unwrap();
    let mut parent = Command::new(exe)
        .arg("--exact")
        .arg("a_killed_client_leaves_no_core_behind")
        .arg("--nocapture")
        .env(PROBE_ENV, "1")
        // The probe parent must die with SIGKILL from the observer below, so it
        // cannot be its own process-group leader; keep it in this group and
        // signal it directly.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the probe parent starts");

    let mut lines = BufReader::new(parent.stdout.take().expect("stdout is piped")).lines();
    let (parent_pid, core_pid) = loop {
        let line = lines
            .next()
            .and_then(|result| result.ok())
            .expect("the probe parent prints its pids");
        let mut fields = line.split_whitespace();
        if fields.next() == Some("PROBE") {
            let parsed = (
                fields.next().and_then(|value| value.parse::<u32>().ok()),
                fields.next().and_then(|value| value.parse::<u32>().ok()),
            );
            if let (Some(p), Some(c)) = parsed {
                break (p, c);
            }
        }
    };
    assert_ne!(
        parent_pid, core_pid,
        "the probe reported the same pid for itself and the core"
    );

    // Refuse to run the experiment unless the core really is a child of the
    // process about to be killed; otherwise a green result would mean nothing.
    let started = Instant::now();
    while (parent_of(core_pid) != Some(parent_pid)
        || !alive(core_pid)
        || !is_stub_process(core_pid))
        && started.elapsed() < Duration::from_secs(10)
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        parent_of(core_pid),
        Some(parent_pid),
        "the core is not a child of the process we are about to kill, so this \
         test would prove nothing (core PPid={:?}, expected {parent_pid})",
        parent_of(core_pid)
    );
    assert!(alive(core_pid), "the stub core never came up");
    assert!(
        is_stub_process(core_pid),
        "the stub core never execed its long-lived process: {}",
        describe(core_pid)
    );
    eprintln!("probe: before killing the client — {}", describe(core_pid));

    parent.kill().expect("the probe parent is killable");
    let _ = parent.wait();

    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(core_pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    // Sample once and assert on the sample. The cleanup below kills the core, so
    // re-reading liveness after it would let an orphaned core pass its own test
    // — which is exactly how the mutant slipped through the first two versions.
    let survived = alive(core_pid);
    eprintln!("probe: after killing the client — {}", describe(core_pid));
    if survived {
        let _ = Command::new("kill")
            .arg("-9")
            .arg(core_pid.to_string())
            .status();
    }
    assert!(
        !survived,
        "core {core_pid} survived the client that started it: the orphan guard \
         does not hold, and a killed TUI would leave sing-box holding the mixed \
         port and any TUN routes it took over"
    );
}
