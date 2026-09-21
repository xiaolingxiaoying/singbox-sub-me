//! The self-update and release tooling: `sbctl update`, `sbctl sing-box`,
//! `sbctl release` and `sbctl uninstall`.

use crate::cli::args::{ReleaseCommand, SingBoxCommand};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

pub(crate) fn sing_box(root: &Path, command: SingBoxCommand) -> ExitCode {
    let result = match command {
        SingBoxCommand::Download { manifest, output } => sbctl::update::read_manifest(&manifest)
            .and_then(|manifest| sbctl::update::download_sing_box(&manifest, &output))
            .map(|_| format!("sing-box downloaded and verified: {}", output.display())),
        SingBoxCommand::Install { manifest, artifact } => sbctl::update::read_manifest(&manifest)
            .and_then(|manifest| sbctl::update::verify_sing_box_artifact(&manifest, &artifact))
            .and_then(|_| {
                sbctl::lifecycle::install_checked_sing_box(root, &artifact)
                    .map_err(sbctl::update::UpdateError::Storage)
            })
            .map(|_| "sing-box installed".to_owned()),
        SingBoxCommand::Update { manifest, artifact } => match manifest {
            Some(path) => {
                // 签名 manifest 流程：URL 与摘要全部固定并校验签名后才会使用。
                sbctl::update::read_manifest(&path)
                    .and_then(|manifest| {
                        let temporary = tempfile::NamedTempFile::new().map_err(|error| {
                            sbctl::update::UpdateError::DownloadFailed(
                                "sing-box",
                                error.to_string(),
                            )
                        })?;
                        // Holds the candidate path without an open write handle:
                        // Linux refuses to execute a file that is still open for
                        // writing (`Text file busy`, ETXTBSY).
                        let mut guard = None;
                        let candidate = match artifact {
                            Some(candidate) => {
                                sbctl::update::verify_sing_box_artifact(&manifest, &candidate)?;
                                candidate
                            }
                            None => {
                                let path = temporary.into_temp_path();
                                let candidate = path.to_path_buf();
                                sbctl::update::download_sing_box(&manifest, &candidate)?;
                                guard = Some(path);
                                candidate
                            }
                        };
                        let result = sbctl::update::apply_sing_box(
                            &sbctl::config::DeploymentStore::new(root),
                            &manifest,
                            &candidate,
                        );
                        drop(guard);
                        result
                    })
                    .map(|rollback| {
                        format!("sing-box updated; rollback point: {}", rollback.display())
                    })
            }
            None => update_sing_box_official(root, artifact.as_deref()),
        },
        SingBoxCommand::Remove => sbctl::lifecycle::remove_managed_sing_box(root)
            .map(|_| "sing-box removed".to_owned())
            .map_err(sbctl::update::UpdateError::Operation),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("sing-box operation failed: {error}");
            ExitCode::from(2)
        }
    }
}

/// Updates the managed sing-box kernel to the latest stable release from the
/// official SagerNet repository (github.com/SagerNet/sing-box), or installs a
/// locally supplied candidate artifact. Both paths run the same configuration
/// check and health-check rollback flow as the signed-manifest update.
fn update_sing_box_official(
    root: &Path,
    artifact: Option<&Path>,
) -> Result<String, sbctl::update::UpdateError> {
    let store = sbctl::config::DeploymentStore::new(root);
    let temporary = tempfile::NamedTempFile::new().map_err(|error| {
        sbctl::update::UpdateError::DownloadFailed("sing-box", error.to_string())
    })?;
    // Keeps the downloaded candidate on disk until the update finishes, while
    // holding no open write handle. On Linux a file that is still open for
    // writing cannot be executed (`Text file busy`, ETXTBSY), and the candidate
    // is executed for the pre-install `sing-box check`.
    let mut candidate_guard: Option<tempfile::TempPath> = None;
    let (candidate, version_note) = match artifact {
        Some(path) => (path.to_path_buf(), "本地 sing-box 候选".to_owned()),
        None => {
            let version = sbctl::update::fetch_latest_official_sing_box_version()?;
            println!("官方最新稳定版：sing-box {version}，开始下载并校验…");
            sbctl::update::download_sing_box_official(&version, temporary.path())?;
            let path = temporary.into_temp_path();
            let candidate = path.to_path_buf();
            candidate_guard = Some(path);
            (candidate, format!("sing-box {version}（官方最新稳定版）"))
        }
    };
    // The candidate is verified again here: it must run and pass a
    // `sing-box check` against the active server configuration before the
    // managed binary is replaced.
    let contents = fs::read(&candidate)?;
    let rollback = sbctl::update::install_candidate_sing_box(&store, &candidate, &contents)?;
    drop(candidate_guard);
    Ok(format!(
        "{version_note} 更新完成，已通过配置检查与服务健康检查；回滚点：{}",
        rollback.display()
    ))
}

pub(crate) fn release(command: ReleaseCommand) -> ExitCode {
    let result: Result<String, String> = match command {
        ReleaseCommand::Sign {
            manifest,
            private_key,
            output,
        } => sbctl::release::sign_manifest_file(&manifest, &private_key, &output)
            .map(|_| format!("signed release manifest written to {}", output.display()))
            .map_err(|error| error.to_string()),
        ReleaseCommand::Verify { manifest } => sbctl::release::verify_manifest(&manifest)
            .map(|_| "release manifest verified against the built-in public key".to_owned())
            .map_err(|error| error.to_string()),
        ReleaseCommand::Keygen { output } => (|| {
            let (public, secret) = sbctl::release::generate_keypair();
            let secret_path = output.join("sbctl-release-secret.hex");
            sbctl::release::write_secret_file(&secret_path, &secret)
                .map_err(|error| error.to_string())?;
            let hex =
                |bytes: &[u8; 32]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
            Ok(format!(
                "secret seed written to {} (0600)\npublic key hex: {}\npublic key PEM:\n{}",
                secret_path.display(),
                hex(&public),
                sbctl::release::public_key_pem(&public)
            ))
        })(),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("release operation failed: {error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn uninstall(root: &Path, purge: bool) -> ExitCode {
    match sbctl::lifecycle::uninstall(root, purge) {
        Ok(Some(backup)) => {
            println!(
                "sbctl services and binaries removed; backup preserved at {}",
                backup.display()
            );
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!("sbctl services and binaries removed; persistent sbctl data purged");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("uninstall failed: {error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn update(
    root: &Path,
    check: bool,
    manifest_path: Option<&Path>,
    sbctl_artifact: Option<&Path>,
    sing_box_artifact: Option<&Path>,
) -> ExitCode {
    let result = update_impl(
        root,
        check,
        manifest_path,
        sbctl_artifact,
        sing_box_artifact,
    );
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("update failed: {error}");
            ExitCode::from(2)
        }
    }
}

fn update_impl(
    root: &Path,
    check: bool,
    manifest_path: Option<&Path>,
    sbctl_artifact: Option<&Path>,
    sing_box_artifact: Option<&Path>,
) -> Result<String, sbctl::update::UpdateError> {
    let manifest = match manifest_path {
        Some(path) => sbctl::update::read_manifest(path)?,
        None => sbctl::update::fetch_latest_manifest()?,
    };
    if check {
        return Ok(format!(
            "update check completed without downloading or changing the host\n{}",
            sbctl::update::available_versions(&manifest)
        ));
    }
    let sbctl_download = tempfile::NamedTempFile::new()
        .map_err(|error| sbctl::update::UpdateError::DownloadFailed("sbctl", error.to_string()))?;
    let sing_box_download = tempfile::NamedTempFile::new().map_err(|error| {
        sbctl::update::UpdateError::DownloadFailed("sing-box", error.to_string())
    })?;
    // Keep both candidates on disk without open write handles: the update runs
    // them for the pre-install checks, and Linux refuses to execute a file that
    // is still open for writing (`Text file busy`, ETXTBSY).
    let mut sbctl_guard = None;
    let mut sing_box_guard = None;
    let sbctl_artifact = match sbctl_artifact {
        Some(path) => path.to_path_buf(),
        None => {
            let path = sbctl_download.into_temp_path();
            let artifact = path.to_path_buf();
            sbctl::update::download_sbctl(&manifest, &artifact)?;
            sbctl_guard = Some(path);
            artifact
        }
    };
    let sing_box_artifact = match sing_box_artifact {
        Some(path) => path.to_path_buf(),
        None => {
            let path = sing_box_download.into_temp_path();
            let artifact = path.to_path_buf();
            sbctl::update::download_sing_box(&manifest, &artifact)?;
            sing_box_guard = Some(path);
            artifact
        }
    };
    let rollback = sbctl::update::apply(
        &sbctl::config::DeploymentStore::new(root),
        &manifest,
        &sbctl_artifact,
        &sing_box_artifact,
    )?;
    drop(sbctl_guard);
    drop(sing_box_guard);
    Ok(format!(
        "update completed after verified validation and service health checks\nrollback point: {}",
        rollback.display()
    ))
}
