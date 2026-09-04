//! Host kernel tuning helpers for the `system` subcommands.
//!
//! BBR (Bottleneck Bandwidth and Round-trip propagation time) is Google's TCP
//! congestion control algorithm. It improves throughput and latency on lossy or
//! high-latency paths compared to loss-based algorithms such as CUBIC. sing-box-yg
//! enables it with the `fq` queueing discipline at the kernel level (see
//! `docs/sing-box-yg-port-plan.md`), which also accelerates the TCP-based Managed
//! protocols (VLESS Reality, VMess WebSocket, AnyTLS).
//!
//! This module only touches kernel sysctls and a `sysctl.d` drop-in. It never
//! modifies the sing-box configuration, and the settings are idempotent: a
//! setting that is already correct is left untouched. The change is system-wide
//! and requires root; the sbctl daemon itself stays unprivileged.

use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::runtime::Runtime;

/// Host-relative path of the `sysctl.d` drop-in that survives reboots.
const BBR_CONF_RELATIVE: &str = "etc/sysctl.d/99-sbctl-bbr.conf";

#[derive(Debug, Error)]
pub enum SystemError {
    #[error("could not read kernel setting {0}: {1}")]
    Read(&'static str, String),
    #[error("could not apply sysctl {0}: {1}")]
    Apply(String, String),
    #[error("could not persist BBR settings: {0}")]
    Persist(String),
}

/// The current kernel congestion control and default queueing discipline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BbrStatus {
    pub congestion_control: String,
    pub qdisc: String,
}

/// Reads the current TCP congestion control and default queueing discipline.
/// The congestion control is essential and must be readable; the queueing
/// discipline is best-effort (some minimal kernels omit it) and falls back to
/// `unknown`.
pub fn read_current<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
) -> Result<BbrStatus, SystemError> {
    let congestion_control = runtime
        .read_to_string("proc/sys/net/ipv4/tcp_congestion_control")
        .map_err(|error| SystemError::Read("tcp_congestion_control", error.to_string()))?
        .trim()
        .to_owned();
    let qdisc = runtime
        .read_to_string("proc/sys/net/core/default_qdisc")
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|_| "unknown".to_owned());
    Ok(BbrStatus {
        congestion_control,
        qdisc,
    })
}

/// Enables BBR + FQ, applying only the setting that is not already correct, then
/// persists both via a `sysctl.d` drop-in so they survive a reboot. Returns the
/// resulting kernel settings.
pub fn enable_bbr<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
) -> Result<BbrStatus, SystemError> {
    let before = read_current(runtime)?;
    let mut congestion_control = before.congestion_control.clone();
    let mut qdisc = before.qdisc.clone();
    if congestion_control != "bbr" {
        apply_sysctl(runtime, "net.ipv4.tcp_congestion_control=bbr")?;
        congestion_control = "bbr".to_owned();
    }
    if qdisc != "fq" {
        apply_sysctl(runtime, "net.core.default_qdisc=fq")?;
        qdisc = "fq".to_owned();
    }
    persist(runtime)?;
    Ok(BbrStatus {
        congestion_control,
        qdisc,
    })
}

fn apply_sysctl<C: crate::runtime::Clock>(
    runtime: &Runtime<C>,
    setting: &str,
) -> Result<(), SystemError> {
    let (status, output) = runtime
        .run_command_output("sysctl", &["-w", setting])
        .map_err(|error| SystemError::Apply(setting.to_owned(), error.to_string()))?;
    if !status.success() {
        return Err(SystemError::Apply(setting.to_owned(), output));
    }
    Ok(())
}

fn persist<C: crate::runtime::Clock>(runtime: &Runtime<C>) -> Result<(), SystemError> {
    let path = runtime.root().join(Path::new(BBR_CONF_RELATIVE));
    let parent = path
        .parent()
        .expect("the sysctl drop-in has a parent directory");
    fs::create_dir_all(parent).map_err(|error| SystemError::Persist(error.to_string()))?;
    let contents = "net.ipv4.tcp_congestion_control=bbr\nnet.core.default_qdisc=fq\n";
    fs::write(&path, contents).map_err(|error| SystemError::Persist(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::TempDir;

    use super::{enable_bbr, read_current};
    use crate::runtime::Runtime;

    fn write_proc(root: &PathFixture, path: &str, contents: &str) {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().expect("proc path has a parent")).unwrap();
        fs::write(path, contents).unwrap();
    }

    struct PathFixture(TempDir);

    impl PathFixture {
        fn new() -> Self {
            Self(TempDir::new().expect("temporary root is created"))
        }
        fn path(&self) -> &std::path::Path {
            self.0.path()
        }
    }

    fn write_sysctl_fixture(root: &std::path::Path) {
        let command = root.join("usr/bin/sysctl");
        fs::create_dir_all(command.parent().expect("sysctl has a parent")).unwrap();
        fs::write(&command, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&command, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    #[test]
    fn read_current_reports_the_host_kernel_settings() {
        let fixture = PathFixture::new();
        write_proc(
            &fixture,
            "proc/sys/net/ipv4/tcp_congestion_control",
            "cubic\n",
        );
        write_proc(&fixture, "proc/sys/net/core/default_qdisc", "fq_codel\n");
        let runtime = Runtime::fixture(fixture.path(), Utc::now());

        let status = read_current(&runtime).expect("kernel settings are readable");
        assert_eq!(status.congestion_control, "cubic");
        assert_eq!(status.qdisc, "fq_codel");
    }

    #[test]
    fn read_current_treats_a_missing_qdisc_as_unknown() {
        let fixture = PathFixture::new();
        write_proc(
            &fixture,
            "proc/sys/net/ipv4/tcp_congestion_control",
            "bbr\n",
        );
        let runtime = Runtime::fixture(fixture.path(), Utc::now());

        let status = read_current(&runtime).expect("kernel settings are readable");
        assert_eq!(status.congestion_control, "bbr");
        assert_eq!(status.qdisc, "unknown");
    }

    #[test]
    fn enable_bbr_applies_missing_settings_and_persists_a_drop_in() {
        let fixture = PathFixture::new();
        write_proc(
            &fixture,
            "proc/sys/net/ipv4/tcp_congestion_control",
            "cubic\n",
        );
        write_proc(&fixture, "proc/sys/net/core/default_qdisc", "fq_codel\n");
        write_sysctl_fixture(fixture.path());
        let runtime = Runtime::fixture(fixture.path(), Utc::now());

        let status = enable_bbr(&runtime).expect("BBR is enabled");
        assert_eq!(status.congestion_control, "bbr");
        assert_eq!(status.qdisc, "fq");

        let drop_in = fs::read_to_string(fixture.path().join("etc/sysctl.d/99-sbctl-bbr.conf"))
            .expect("the sysctl drop-in is persisted");
        assert!(drop_in.contains("net.ipv4.tcp_congestion_control=bbr"));
        assert!(drop_in.contains("net.core.default_qdisc=fq"));
    }

    #[test]
    fn enable_bbr_is_idempotent_when_settings_are_already_correct() {
        let fixture = PathFixture::new();
        write_proc(
            &fixture,
            "proc/sys/net/ipv4/tcp_congestion_control",
            "bbr\n",
        );
        write_proc(&fixture, "proc/sys/net/core/default_qdisc", "fq\n");
        write_sysctl_fixture(fixture.path());
        let runtime = Runtime::fixture(fixture.path(), Utc::now());

        let status = enable_bbr(&runtime).expect("BBR is already enabled");
        assert_eq!(status.congestion_control, "bbr");
        assert_eq!(status.qdisc, "fq");

        let drop_in = fs::read_to_string(fixture.path().join("etc/sysctl.d/99-sbctl-bbr.conf"))
            .expect("the sysctl drop-in is persisted");
        assert!(drop_in.contains("net.ipv4.tcp_congestion_control=bbr"));
    }
}
