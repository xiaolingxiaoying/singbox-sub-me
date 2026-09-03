//! Runtime adapters shared by production code and isolated acceptance fixtures.
//!
//! The application deliberately talks to the host through this small boundary:
//! production uses the real clock and filesystem root, while tests can supply a
//! fixed instant and a temporary root containing synthetic host files.

use chrono::{DateTime, Utc};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus};

pub trait Clock {
    fn now_utc(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now_utc(&self) -> DateTime<Utc> {
        self.0
    }
}

#[derive(Clone, Debug)]
pub struct Runtime<C> {
    root: PathBuf,
    clock: C,
    allow_live_commands: bool,
}

impl Runtime<SystemClock> {
    pub fn live(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            clock: SystemClock,
            allow_live_commands: true,
        }
    }
}

impl Runtime<FixedClock> {
    pub fn fixture(root: &Path, now: DateTime<Utc>) -> Self {
        Self {
            root: root.to_owned(),
            clock: FixedClock(now),
            allow_live_commands: false,
        }
    }
}

impl<C: Clock> Runtime<C> {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn now_utc(&self) -> DateTime<Utc> {
        self.clock.now_utc()
    }

    /// Read a host-relative file without allowing an absolute path to escape
    /// the fixture root.
    pub fn read_to_string(&self, relative: impl AsRef<Path>) -> io::Result<String> {
        fs::read_to_string(self.host_path(relative)?)
    }

    /// Run a host command. A fixture can provide `usr/bin/<program>` under its
    /// root; live operation falls back to the system command when absent.
    pub fn run_command(&self, program: &str, args: &[&str]) -> io::Result<ExitStatus> {
        if program.is_empty()
            || Path::new(program).is_absolute()
            || Path::new(program)
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "runtime command must be a simple program name",
            ));
        }
        let rooted = self.root.join("usr/bin").join(program);
        let executable = if rooted.is_file() {
            rooted
        } else if self.allow_live_commands {
            PathBuf::from(program)
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("fixture command is missing: {program}"),
            ));
        };
        Command::new(executable).args(args).status()
    }

    fn host_path(&self, relative: impl AsRef<Path>) -> io::Result<PathBuf> {
        let relative = relative.as_ref();
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "host path must be relative to the runtime root",
            ));
        }
        Ok(self.root.join(relative))
    }
}

#[cfg(test)]
mod tests {
    use super::{Clock, FixedClock, Runtime};
    use chrono::{TimeZone, Utc};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn fixture_runtime_reads_host_files_and_returns_a_fixed_instant() {
        let fixture = TempDir::new().unwrap();
        fs::create_dir_all(fixture.path().join("sys/class/net/ens3/statistics")).unwrap();
        fs::write(
            fixture
                .path()
                .join("sys/class/net/ens3/statistics/rx_bytes"),
            "123\n",
        )
        .unwrap();
        let now = Utc.with_ymd_and_hms(2025, 2, 3, 4, 5, 6).unwrap();
        let runtime = Runtime::fixture(fixture.path(), now);

        assert_eq!(runtime.now_utc(), now);
        assert_eq!(
            runtime
                .read_to_string("sys/class/net/ens3/statistics/rx_bytes")
                .unwrap(),
            "123\n"
        );
    }

    #[test]
    fn fixture_runtime_executes_a_rooted_command_without_touching_the_live_host() {
        let fixture = TempDir::new().unwrap();
        let command = fixture.path().join("usr/bin/systemctl");
        fs::create_dir_all(command.parent().unwrap()).unwrap();
        fs::write(&command, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&command, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = Runtime::fixture(fixture.path(), Utc::now());

        assert!(
            runtime
                .run_command("systemctl", &["is-active"])
                .unwrap()
                .success()
        );
        assert!(!fixture.path().join("command-output").exists());
    }

    #[test]
    fn fixture_runtime_rejects_host_escape_paths_and_missing_commands() {
        let fixture = TempDir::new().unwrap();
        let runtime = Runtime::fixture(fixture.path(), Utc::now());

        assert!(runtime.read_to_string("../outside").is_err());
        assert!(runtime.read_to_string("/etc/passwd").is_err());
        assert!(runtime.run_command("missing-systemctl", &[]).is_err());
    }

    #[test]
    fn fixed_clock_is_a_reusable_clock_adapter() {
        let instant = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        assert_eq!(FixedClock(instant).now_utc(), instant);
    }
}
