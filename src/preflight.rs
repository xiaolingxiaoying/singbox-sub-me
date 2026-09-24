use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Debug, PartialEq, Eq)]
pub struct HostPlatform {
    pub id: String,
    pub version_id: String,
}

pub fn parse_os_release(contents: &str) -> Option<HostPlatform> {
    let mut id = None;
    let mut version_id = None;

    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        match key.trim() {
            "ID" => id = Some(value),
            "VERSION_ID" => version_id = Some(value),
            _ => {}
        }
    }

    Some(HostPlatform {
        id: id?,
        version_id: version_id?,
    })
}

pub fn is_supported_platform(platform: &HostPlatform) -> bool {
    matches!(platform.id.as_str(), "debian" | "ubuntu")
}

pub fn is_supported_architecture(architecture: &str) -> bool {
    matches!(architecture, "x86_64" | "aarch64")
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreflightError {
    #[error(
        "could not read operating system metadata; sbctl requires Debian or Ubuntu with systemd"
    )]
    MissingOsRelease,
    #[error("unsupported operating system; sbctl requires Debian or Ubuntu with systemd")]
    UnsupportedPlatform,
    #[error("systemd is not available; sbctl requires a systemd host")]
    MissingSystemd,
    #[error("unsupported CPU architecture; sbctl requires amd64 or arm64")]
    UnsupportedArchitecture,
    #[error("sbctl install requires root; rerun as root (for example: sudo sbctl install)")]
    RequiresRoot,
    #[error(
        "Direct mode requires TCP ports 80 and 443, but these are occupied: {0}. Stop the listed service or choose External proxy mode; sbctl will not change existing services."
    )]
    DirectPortsOccupied(String),
    #[error(
        "could not inspect TCP 80/443 listeners with `ss`; install iproute2 and retry. sbctl will not assume the ports are free"
    )]
    PortProbeUnavailable,
    #[error("Existing deployment detected ({0}); sbctl will not modify it")]
    ExistingDeployment(ExistingDeployment),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExistingDeployment {
    artifacts: Vec<String>,
}

impl ExistingDeployment {
    fn from_artifacts(artifacts: Vec<String>) -> Self {
        Self { artifacts }
    }
}

impl fmt::Display for ExistingDeployment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.artifacts.join(", "))
    }
}

/// Performs only filesystem and environment reads. It never installs, stops, or
/// changes a detected Existing deployment.
pub fn preflight(root: &Path) -> Result<(), PreflightError> {
    let os_release = fs::read_to_string(root.join("etc/os-release"))
        .ok()
        .and_then(|contents| parse_os_release(&contents))
        .ok_or(PreflightError::MissingOsRelease)?;

    if !is_supported_platform(&os_release) {
        return Err(PreflightError::UnsupportedPlatform);
    }

    if !root.join("run/systemd/system").is_dir() {
        return Err(PreflightError::MissingSystemd);
    }

    if !is_supported_architecture(env::consts::ARCH) {
        return Err(PreflightError::UnsupportedArchitecture);
    }

    let existing = existing_deployment_paths(root);
    if existing.is_empty() {
        Ok(())
    } else {
        Err(PreflightError::ExistingDeployment(
            ExistingDeployment::from_artifacts(existing),
        ))
    }
}

/// Checks privileges and Direct's fixed public listeners before installation
/// downloads artifacts or writes persistent state. Fixture roots are read-only
/// test environments and intentionally skip host privilege/port probes.
pub fn preflight_install(root: &Path, direct: bool) -> Result<(), PreflightError> {
    require_install_privileges(root)?;
    preflight(root)?;
    if direct && root == Path::new("/") {
        let output = std::process::Command::new("ss")
            .args(["-H", "-ltnp", "( sport = :80 or sport = :443 )"])
            .output();
        let output = output.map_err(|_| PreflightError::PortProbeUnavailable)?;
        if !output.status.success() {
            return Err(PreflightError::PortProbeUnavailable);
        }
        let listeners = String::from_utf8_lossy(&output.stdout).into_owned();
        if !listeners.trim().is_empty() {
            return Err(PreflightError::DirectPortsOccupied(
                listeners.trim().to_owned(),
            ));
        }
    }
    Ok(())
}

pub fn require_install_privileges(root: &Path) -> Result<(), PreflightError> {
    if root == Path::new("/") {
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 {
            return Err(PreflightError::RequiresRoot);
        }
    }
    Ok(())
}

fn existing_deployment_paths(root: &Path) -> Vec<String> {
    let mut paths = vec![
        "usr/bin/sing-box",
        "usr/local/bin/sing-box",
        "opt/sing-box/sing-box",
        "etc/sing-box",
        "usr/local/etc/sing-box",
        "opt/sing-box",
        "etc/systemd/system/sing-box.service",
        "lib/systemd/system/sing-box.service",
        // sbctl's own persistent state, e.g. left behind on purpose by a
        // non-purge uninstall. A fresh install regenerates the subscription
        // credential, every protocol credential and the accounting state, and a
        // failed install deletes everything under these paths, so leftover state
        // has to stop the install before it touches anything. The data directory
        // itself is not on the list: a rolled-back install leaves only the
        // operation lock behind, and that is not a deployment.
        "etc/sbctl/config.toml",
        "var/lib/sbctl/state.json",
        "var/lib/sbctl/ownership",
        "var/lib/sbctl/artifacts",
        "var/lib/sbctl/certificates",
    ]
    .into_iter()
    .filter(|path| root.join(path).exists())
    .map(str::to_owned)
    .collect::<Vec<_>>();

    paths.extend(systemd_unit_matches(root));

    paths.extend(path_binary_matches(root));
    paths.sort();
    paths.dedup();
    paths
}

fn systemd_unit_matches(root: &Path) -> Vec<String> {
    [
        "etc/systemd/system",
        "lib/systemd/system",
        "usr/lib/systemd/system",
        "run/systemd/system",
    ]
    .into_iter()
    .flat_map(|directory| files_below(&root.join(directory)))
    .filter_map(|path| {
        fs::read_to_string(&path)
            .ok()
            .filter(|contents| contents.contains("sing-box"))
            .and_then(|_| {
                path.strip_prefix(root)
                    .ok()
                    .map(|path| path.to_string_lossy().replace('\\', "/"))
            })
    })
    .collect()
}

fn files_below(directory: &Path) -> Vec<PathBuf> {
    let canonical_root = fs::canonicalize(directory).ok();
    let mut visited = HashSet::new();
    files_below_within(directory, canonical_root.as_deref(), &mut visited)
}

fn files_below_within(
    directory: &Path,
    canonical_root: Option<&Path>,
    visited: &mut HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let Ok(canonical_directory) = fs::canonicalize(directory) else {
        return Vec::new();
    };
    if canonical_root.is_some_and(|root| !canonical_directory.starts_with(root))
        || !visited.insert(canonical_directory)
    {
        return Vec::new();
    }

    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .flat_map(|entry| {
            let path = entry.path();
            let Ok(metadata) = fs::metadata(&path) else {
                return Vec::new();
            };
            let stays_within_root = canonical_root.is_none_or(|root| {
                fs::canonicalize(&path).is_ok_and(|target| target.starts_with(root))
            });
            if !stays_within_root {
                Vec::new()
            } else if metadata.is_dir() {
                files_below_within(&path, canonical_root, visited)
            } else if metadata.is_file() {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}

fn path_binary_matches(root: &Path) -> Vec<String> {
    let Some(path) = env::var_os("PATH") else {
        return Vec::new();
    };

    env::split_paths(&path)
        .filter_map(|directory| rooted_path(root, &directory))
        .filter_map(|directory| directory.join("sing-box").exists().then_some(directory))
        .map(|directory| {
            format!(
                "{}{}sing-box",
                directory.display(),
                std::path::MAIN_SEPARATOR
            )
        })
        .collect()
}

fn rooted_path(root: &Path, path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }

    let mut rooted = root.to_path_buf();
    for component in path.components() {
        if let Component::Normal(segment) = component {
            rooted.push(segment);
        }
    }
    Some(rooted)
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        ExistingDeployment, HostPlatform, PreflightError, is_supported_architecture,
        is_supported_platform, parse_os_release, preflight,
    };

    #[test]
    fn accepts_debian_os_release() {
        let platform = parse_os_release("NAME=Debian GNU/Linux\nID=debian\nVERSION_ID=12\n")
            .expect("a Debian os-release file is parsed");

        assert_eq!(
            platform,
            HostPlatform {
                id: "debian".into(),
                version_id: "12".into(),
            }
        );
        assert!(is_supported_platform(&platform));
    }

    #[test]
    fn rejects_unsupported_platform() {
        let platform = parse_os_release("ID=alpine\nVERSION_ID=3.20\n")
            .expect("an Alpine os-release file is parsed");

        assert!(!is_supported_platform(&platform));
    }

    #[test]
    fn accepts_release_architectures_and_rejects_other_architectures() {
        assert!(is_supported_architecture("x86_64"));
        assert!(is_supported_architecture("aarch64"));
        assert!(!is_supported_architecture("arm"));
    }

    #[test]
    fn refuses_an_existing_deployment_without_modifying_it() {
        let fixture = supported_systemd_host();
        let existing_config = fixture.path().join("etc/sing-box/config.json");
        fs::create_dir_all(existing_config.parent().expect("config has a parent"))
            .expect("fixture directory is created");
        fs::write(&existing_config, "administrator configuration")
            .expect("fixture config is written");

        let result = preflight(fixture.path());

        assert_eq!(
            result,
            Err(PreflightError::ExistingDeployment(
                ExistingDeployment::from_artifacts(vec!["etc/sing-box".into()])
            ))
        );
        assert_eq!(
            fs::read_to_string(existing_config).expect("preflight preserves the existing config"),
            "administrator configuration"
        );
    }

    #[test]
    fn refuses_an_install_over_sbctl_s_own_leftover_state() {
        let fixture = supported_systemd_host();
        let existing_config = fixture.path().join("etc/sbctl/config.toml");
        fs::create_dir_all(existing_config.parent().expect("config has a parent"))
            .expect("fixture directory is created");
        fs::write(&existing_config, "subscription_credential = 'keep me'")
            .expect("fixture config is written");

        assert_eq!(
            preflight(fixture.path()),
            Err(PreflightError::ExistingDeployment(
                ExistingDeployment::from_artifacts(vec!["etc/sbctl/config.toml".into()])
            ))
        );
        assert_eq!(
            fs::read_to_string(existing_config).expect("preflight preserves the existing config"),
            "subscription_credential = 'keep me'"
        );
    }

    #[test]
    fn requires_systemd_on_an_otherwise_supported_host() {
        let fixture = TempDir::new().expect("temporary root is created");
        write_os_release(&fixture, "ID=ubuntu\nVERSION_ID=24.04\n");

        assert_eq!(
            preflight(fixture.path()),
            Err(PreflightError::MissingSystemd)
        );
    }

    #[test]
    fn refuses_a_systemd_service_that_uses_sing_box_under_another_name() {
        let fixture = supported_systemd_host();
        let service = fixture.path().join("etc/systemd/system/proxy.service");
        fs::create_dir_all(service.parent().expect("service has a parent"))
            .expect("unit directory is created");
        fs::write(
            &service,
            "[Service]\nExecStart=/opt/sing-box/sing-box run\n",
        )
        .expect("service unit is written");

        assert_eq!(
            preflight(fixture.path()),
            Err(PreflightError::ExistingDeployment(
                ExistingDeployment::from_artifacts(vec!["etc/systemd/system/proxy.service".into()])
            ))
        );
    }

    #[test]
    fn refuses_a_runtime_systemd_drop_in_that_uses_sing_box() {
        let fixture = supported_systemd_host();
        let drop_in = fixture
            .path()
            .join("run/systemd/system/proxy.service.d/override.conf");
        fs::create_dir_all(drop_in.parent().expect("drop-in has a parent"))
            .expect("drop-in directory is created");
        fs::write(
            &drop_in,
            "[Service]\nExecStart=/opt/sing-box/sing-box run\n",
        )
        .expect("drop-in is written");

        assert_eq!(
            preflight(fixture.path()),
            Err(PreflightError::ExistingDeployment(
                ExistingDeployment::from_artifacts(vec![
                    "run/systemd/system/proxy.service.d/override.conf".into()
                ])
            ))
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_systemd_service_that_uses_sing_box() {
        let fixture = supported_systemd_host();
        let target = fixture.path().join("etc/systemd/system/proxy-unit");
        fs::create_dir_all(target.parent().expect("service target has a parent"))
            .expect("service target directory is created");
        fs::write(&target, "[Service]\nExecStart=/opt/sing-box/sing-box run\n")
            .expect("service target is written");
        let link = fixture.path().join("etc/systemd/system/proxy.service");
        create_file_symlink(&target, &link).expect("service symlink is created");

        assert!(matches!(
            preflight(fixture.path()),
            Err(PreflightError::ExistingDeployment(_))
        ));
    }

    fn supported_systemd_host() -> TempDir {
        let fixture = TempDir::new().expect("temporary root is created");
        write_os_release(&fixture, "ID=debian\nVERSION_ID=12\n");
        fs::create_dir_all(fixture.path().join("run/systemd/system"))
            .expect("systemd runtime directory is created");
        fixture
    }

    fn write_os_release(fixture: &TempDir, contents: &str) {
        let path = fixture.path().join("etc/os-release");
        fs::create_dir_all(path.parent().expect("os-release has a parent"))
            .expect("etc directory is created");
        fs::write(path, contents).expect("os-release is written");
    }

    #[cfg(unix)]
    fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }
}
