//! sing-box core lifecycle: discovery, download, verification, and the child
//! process the TUI manages.
//!
//! Cores come from the sing-box GitHub Release: a versioned archive
//! (`...-windows-<arch>.zip` on Windows, `...-linux-<arch>.tar.gz` /
//! `...-darwin-<arch>.tar.gz` elsewhere) fetched directly or through a
//! configured mirror prefix (settings).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};

pub struct CoreHandle {
    pub child: Child,
    /// Whether the operating system will reap this child if the client dies
    /// without tearing it down cooperatively. See [`attach_orphan_guard`].
    pub orphan_guard: bool,
}

pub struct StartedCore {
    pub handle: CoreHandle,
    pub api: crate::clash_api::ClashApi,
    pub version: String,
    pub config: String,
}

/// How long a freshly spawned core may take to expose its control API before
/// startup is treated as failed. A first run may need to download remote
/// rule-sets, so this is deliberately generous; the reason for any failure is
/// reported from the core log instead of a bare timeout.
const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

/// Each launch gets a fresh local endpoint and secret. A port race can make
/// startup fail, but cannot cause us to control an existing proxy instance.
pub async fn start_managed(
    dir: &Path,
    raw: &str,
    mode: crate::system_proxy::TrafficMode,
    mixed_port: u16,
) -> Result<StartedCore> {
    let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = reservation.local_addr()?;
    // Fail before rewriting the runtime configuration when the data-plane port
    // is already taken: the core would exit on its own bind and the readiness
    // wait would only report it seconds later, after the config was clobbered.
    if mode == crate::system_proxy::TrafficMode::SystemProxy
        && std::net::TcpListener::bind(("127.0.0.1", mixed_port)).is_err()
    {
        bail!(
            "mixed 端口 {mixed_port} 已被占用：可能是上一次未正常退出时残留的 sing-box 内核，\
             或其他代理软件；结束残留进程或在设置中改用其他端口"
        );
    }
    let mut entropy = [0u8; 32];
    getrandom::fill(&mut entropy)
        .map_err(|error| anyhow::anyhow!("controller secret generation failed: {error}"))?;
    let secret = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let config = runtime_config(raw, mode, mixed_port, &address.to_string(), &secret)?;
    let api = crate::clash_api::ClashApi::authenticated(&format!("http://{address}"), &secret);
    let core = crate::settings::core_path(dir);
    let active = dir.join("cache/active-config.json");
    use tokio::io::AsyncWriteExt;
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&active).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .await?;
    }
    file.write_all(config.as_bytes()).await?;
    file.flush().await?;
    drop(file);
    let check = Command::new(&core)
        .args(["check", "-c"])
        .arg(&active)
        .kill_on_drop(true)
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(30), check)
        .await
        .context("sing-box check timed out")??;
    if !output.status.success() {
        bail!(
            "sing-box check rejected the configuration: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    drop(reservation);
    let core_log = dir.join("cache/core.log");
    let mut handle = start(&core, &active, &core_log).await?;
    let ready = tokio::time::timeout(READY_TIMEOUT, wait_ready(&mut handle.child, &api)).await;
    match ready {
        Ok(Ok(version)) => Ok(StartedCore {
            handle,
            api,
            version,
            config,
        }),
        Ok(Err(error)) => {
            let _ = handle.child.kill().await;
            Err(error.context(format!(
                "sing-box 启动失败；核心日志末尾：\n{}",
                core_log_tail(&core_log)
            )))
        }
        Err(_) => {
            let _ = handle.child.kill().await;
            bail!(
                "内核控制通道在 {} 秒内未就绪（常见原因是远端规则集下载缓慢或被阻断）；核心日志末尾：\n{}",
                READY_TIMEOUT.as_secs(),
                core_log_tail(&core_log)
            )
        }
    }
}

/// The tail of the core log, appended to a startup failure so the reason
/// (a rejected configuration, a failed remote rule-set download, a port
/// conflict) is visible instead of a bare timeout or channel error.
fn core_log_tail(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return "（核心日志不可读）".to_owned();
    };
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return "（核心日志为空）".to_owned();
    }
    let start = lines.len().saturating_sub(10);
    lines[start..].join("\n")
}

async fn wait_ready(child: &mut Child, api: &crate::clash_api::ClashApi) -> Result<String> {
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("sing-box exited during startup: {status}");
        }
        if let Ok(version) = api.version().await {
            if let Some(status) = child.try_wait()? {
                bail!("sing-box exited during startup: {status}");
            }
            return Ok(version);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn runtime_config(
    raw: &str,
    mode: crate::system_proxy::TrafficMode,
    mixed_port: u16,
    address: &str,
    secret: &str,
) -> Result<String> {
    let mut config: serde_json::Value =
        serde_json::from_str(&adapt_inbounds(raw, mode, mixed_port)?)?;
    if !config
        .get("experimental")
        .is_some_and(serde_json::Value::is_object)
    {
        config["experimental"] = serde_json::json!({});
    }
    if !config["experimental"]
        .get("clash_api")
        .is_some_and(serde_json::Value::is_object)
    {
        config["experimental"]["clash_api"] = serde_json::json!({});
    }
    config["experimental"]["clash_api"]["external_controller"] = address.into();
    config["experimental"]["clash_api"]["secret"] = secret.into();
    Ok(serde_json::to_string_pretty(&config)?)
}

/// The result of a core download: where the binary was installed and the
/// SHA-256 of the release archive it came from.
pub struct CoreDownload {
    pub path: PathBuf,
    pub sha256: String,
}

/// The core's reported version (`sing-box version`), or an error when the
/// binary is missing or unreadable.
pub fn detect_version(core: &Path) -> Result<String> {
    let output = std::process::Command::new(core)
        .arg("version")
        .output()
        .context("running `sing-box version` failed")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or("").trim();
    // The binary reports "sing-box version 1.14.1"; the "sing-box" half is
    // already every caller's label, so only the version remains.
    Ok(first
        .strip_prefix("sing-box version ")
        .unwrap_or(first)
        .to_owned())
}

/// Validates a configuration with `sing-box check`.
pub fn check_config(core: &Path, config: &Path) -> Result<()> {
    let output = std::process::Command::new(core)
        .args(["check", "-c"])
        .arg(config)
        .output()
        .context("running `sing-box check` failed")?;
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "sing-box check rejected the configuration: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Starts the core with the activated configuration, logging to a file the
/// TUI can tail.
pub async fn start(core: &Path, config: &Path, log_path: &Path) -> Result<CoreHandle> {
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let log_file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)
        .await
        .context("opening the core log file")?;
    let mut command = Command::new(core);
    command
        .args(["run", "-c"])
        .arg(config)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file.into_std().await))
        .kill_on_drop(true);
    prearm_orphan_guard(&mut command);
    let child = command.spawn().context("spawning the sing-box core")?;
    let orphan_guard = attach_orphan_guard(child.id());
    Ok(CoreHandle {
        child,
        orphan_guard,
    })
}

/// Arms the operating system to terminate the core when this process goes
/// away. `kill_on_drop` is the crate's only cooperative teardown and it never
/// runs when the process exits first — which is the normal GUI exit path, since
/// the engine's runtime is deliberately leaked. Without a guard the client
/// leaves an orphaned core holding the mixed port and, in TUN mode, the routes
/// it took over.
///
/// A core cannot be asked to shut itself down over its control API: sing-box
/// registers no shutdown route and only honours SIGINT/SIGTERM
/// (`cmd/sing-box/cmd_run.go`), which a console-less Windows GUI parent cannot
/// deliver. Hence the job object. A failure here is not fatal: it only means
/// the guarantee is missing, which the caller surfaces.
#[cfg(windows)]
fn attach_orphan_guard(pid: Option<u32>) -> bool {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

    /// `HANDLE` wraps a raw pointer, so it is neither `Send` nor `Sync`, while
    /// the job is process-wide and only ever crosses these syscalls.
    struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    // One job per client process. Its handle is deliberately never closed,
    // because closing the last handle is what terminates the members.
    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    let Some(pid) = pid else { return false };
    let job = JOB.get_or_init(|| {
        let created = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.ok()?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                created,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured.is_err() {
            let _ = unsafe { CloseHandle(created) };
            return None;
        }
        Some(Job(created))
    });
    let Some(job) = job else { return false };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) })
    else {
        return false;
    };
    let assigned = unsafe { AssignProcessToJobObject(job.0, process) };
    let _ = unsafe { CloseHandle(process) };
    assigned.is_ok()
}

/// Asks the kernel to signal the core when this process goes away.
///
/// `PR_SET_PDEATHSIG` is set in the child between fork and exec, so it is armed
/// before the core exists and cannot be missed. Failing inside the hook fails
/// the spawn rather than leaving an unguarded core running, which is why
/// [`attach_orphan_guard`] can report success unconditionally on Linux.
#[cfg(target_os = "linux")]
fn prearm_orphan_guard(command: &mut Command) {
    let parent = std::process::id();
    // SAFETY: the closure runs between fork and exec and calls only `prctl`
    // and `getppid`, both async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The flag is only honoured while the parent is alive at the moment
            // of death: if the parent already exited, the child was reparented
            // and no signal will ever arrive. Closing that window here turns a
            // silent orphan into a spawn failure the caller reports.
            if libc::getppid() != parent as i32 {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn prearm_orphan_guard(_command: &mut Command) {}

/// Linux arms its guard before the spawn and cannot fail there without the
/// spawn failing too, so reaching this point means the guarantee holds.
#[cfg(target_os = "linux")]
fn attach_orphan_guard(_pid: Option<u32>) -> bool {
    true
}

/// macOS has no `PR_SET_PDEATHSIG` and no job object, so there is genuinely no
/// guarantee to report: returning `false` here lets the caller say so instead of
/// claiming protection it does not have. Closing the gap needs a launchd
/// KeepAlive job or a supervisor process, which is a separate piece of work.
#[cfg(all(unix, not(target_os = "linux")))]
fn attach_orphan_guard(_pid: Option<u32>) -> bool {
    false
}

/// Exponential backoff for automatic core restarts after a crash: 2s, 4s,
/// 8s, 16s, then capped at 30s so a persistently failing core never spins.
pub fn restart_backoff(attempt: u32) -> std::time::Duration {
    let seconds = 2u64.saturating_pow((attempt + 1).min(5)).min(30);
    std::time::Duration::from_secs(seconds)
}

/// Downloads the sing-box release for the current platform into the data
/// directory. The release is resolved through the GitHub API (or the pinned
/// version) and the versioned asset is fetched: `...-windows-<arch>.zip` on
/// Windows, `...-linux-<arch>.tar.gz` / `...-darwin-<arch>.tar.gz` elsewhere.
pub async fn download_core(target_dir: &Path, version: &str, mirror: &str) -> Result<CoreDownload> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let tag = resolve_release_tag(&client, version).await?;
    let release = tag.trim_start_matches('v').to_owned();
    let (os, arch) = core_platform();
    let (archive_name, is_zip) = release_asset_name(&release, os, arch);
    let url = crate::subscription::apply_mirror(
        &format!("https://github.com/SagerNet/sing-box/releases/download/{tag}/{archive_name}"),
        mirror,
    );
    let bytes = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("downloading {url}"))?
        .error_for_status()
        .with_context(|| format!("core download failed for {url}"))?
        .bytes()
        .await
        .context("reading the core archive")?
        .to_vec();
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    tokio::fs::create_dir_all(target_dir).await?;
    verify_or_record_hash(target_dir, &tag, &sha256)?;
    let binary_name = core_binary_name();
    if is_zip {
        extract_zip_core(&bytes, target_dir, binary_name)?;
    } else {
        extract_tar_gz_core(&bytes, target_dir, binary_name)?;
    }
    Ok(CoreDownload {
        path: target_dir.join(binary_name),
        sha256,
    })
}

/// Trust-on-first-use integrity check over the downloaded archive. sing-box
/// publishes no official checksum file, so the first download of a release
/// tag can only be recorded; every later download of the same tag must
/// reproduce the recorded hash, which turns a mirror silently swapping the
/// archive into a loud failure instead of an unnoticed binary replacement.
/// The ledger (`verified-hashes.json`) stays human-readable so the recorded
/// hash can be compared against a copy of the release the user trusts.
fn verify_or_record_hash(target_dir: &Path, tag: &str, sha256: &str) -> Result<()> {
    let ledger_path = target_dir.join("verified-hashes.json");
    let mut ledger: BTreeMap<String, String> = std::fs::read_to_string(&ledger_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    match ledger.get(tag) {
        Some(recorded) if recorded != sha256 => bail!(
            "下载的 {tag} 内核 SHA-256 与首次记录不一致（{sha256} ≠ {recorded}）；\
             镜像返回的文件可能已被篡改，请更换镜像，或核对发布页后删除 {} 重试",
            ledger_path.display()
        ),
        Some(_) => Ok(()),
        None => {
            ledger.insert(tag.to_owned(), sha256.to_owned());
            std::fs::write(&ledger_path, serde_json::to_string_pretty(&ledger)?)?;
            Ok(())
        }
    }
}

/// The sing-box release tag to use: the pinned version normalized to `vX.Y.Z`,
/// or the latest release's tag fetched from the GitHub API.
async fn resolve_release_tag(client: &reqwest::Client, version: &str) -> Result<String> {
    let version = version.trim();
    if !version.is_empty() {
        return Ok(if version.starts_with('v') {
            version.to_owned()
        } else {
            format!("v{version}")
        });
    }
    let value: serde_json::Value = client
        .get("https://api.github.com/repos/SagerNet/sing-box/releases/latest")
        .header("User-Agent", "sbtui")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("requesting the latest sing-box release")?
        .error_for_status()
        .context("the latest sing-box release request failed")?
        .json()
        .await
        .context("the sing-box release response is not JSON")?;
    value
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .map(str::to_owned)
        .context("the latest sing-box release has no tag_name")
}

fn core_binary_name() -> &'static str {
    if cfg!(windows) {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

/// The sing-box release asset filename for a platform. Windows ships a
/// versioned `.zip`; Linux and macOS ship a versioned `.tar.gz`. The second
/// value reports whether the archive is a zip.
fn release_asset_name(release: &str, os: &str, arch: &str) -> (String, bool) {
    if os == "windows" {
        (format!("sing-box-{release}-windows-{arch}.zip"), true)
    } else {
        (format!("sing-box-{release}-{os}-{arch}.tar.gz"), false)
    }
}

fn extract_zip_core(bytes: &[u8], target_dir: &Path, binary_name: &str) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).context("the core archive is not a readable zip")?;
    let mut extracted = false;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_owned();
        let file_name = Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let is_binary = file_name == binary_name;
        let is_wintun = cfg!(windows) && file_name.eq_ignore_ascii_case("wintun.dll");
        if !is_binary && !is_wintun {
            continue;
        }
        let destination = target_dir.join(file_name);
        let mut buffered = Vec::with_capacity(file.size() as usize);
        std::io::Read::read_to_end(&mut file, &mut buffered)?;
        std::fs::write(&destination, buffered)?;
        if is_binary {
            mark_executable(&destination);
            extracted = true;
        }
    }
    if !extracted {
        bail!("the core archive does not contain {binary_name}");
    }
    Ok(())
}

fn extract_tar_gz_core(bytes: &[u8], target_dir: &Path, binary_name: &str) -> Result<()> {
    // The whole extraction happens synchronously and the archive is dropped
    // before the caller awaits again: `tar::Archive`/`Entry` are not `Send`,
    // and holding them across an await would make the engine future non-Send.
    let extracted = {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        let mut found: Option<(PathBuf, Vec<u8>)> = None;
        for entry in archive
            .entries()
            .context("the core archive is not a readable tar.gz")?
        {
            let mut entry = entry?;
            let path = entry
                .path()
                .context("the core archive has an invalid path")?
                .into_owned();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name != binary_name {
                continue;
            }
            let mut buffered = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut buffered)
                .context("reading the core binary")?;
            found = Some((target_dir.join(binary_name), buffered));
            break;
        }
        found
    };
    let Some((destination, buffered)) = extracted else {
        bail!("the core archive does not contain {binary_name}");
    };
    std::fs::write(&destination, buffered)?;
    mark_executable(&destination);
    Ok(())
}

fn mark_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

fn core_platform() -> (&'static str, &'static str) {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let os = match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "darwin",
        "linux" => "linux",
        other => other,
    };
    (os, arch)
}

/// Rewrites the `inbounds` section of a subscription config for the TUI's
/// local runtime: mixed proxy inbound (system proxy) or tun inbound (global).
/// Everything else the subscription carries stays untouched.
///
/// In TUN mode the profile's *own* tun inbound is reused rather than replaced.
/// Its `stack`, addresses and route options are what that profile was built
/// and tested for, and a client that substitutes its own defaults can hand the
/// kernel a stack the profile has moved away from — the reason sing-box
/// profiles pinned `stack` per minor in the first place. Keys the profile left
/// out fall back to this client's defaults, and the tag is forced so the
/// runtime always has the one it looks up.
pub fn adapt_inbounds(
    config_text: &str,
    mode: crate::system_proxy::TrafficMode,
    mixed_port: u16,
) -> Result<String> {
    let mut value: serde_json::Value =
        serde_json::from_str(config_text).context("the active configuration is not JSON")?;
    let inbounds = match mode {
        crate::system_proxy::TrafficMode::SystemProxy => serde_json::json!([
            {
                "type": "mixed",
                "tag": "mixed-in",
                "listen": "127.0.0.1",
                "listen_port": mixed_port
            }
        ]),
        crate::system_proxy::TrafficMode::Tun => {
            let mut tun = serde_json::json!({
                "type": "tun",
                "tag": "tun-in",
                "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
                "mtu": 9000,
                "auto_route": true,
                "strict_route": true,
                "stack": "mixed"
            });
            if let Some(profile_tun) = value
                .get("inbounds")
                .and_then(|inbounds| inbounds.as_array())
                .and_then(|inbounds| {
                    inbounds
                        .iter()
                        .find(|inbound| inbound.get("type").and_then(|t| t.as_str()) == Some("tun"))
                })
            {
                if let (Some(tun_object), Some(profile_object)) =
                    (tun.as_object_mut(), profile_tun.as_object())
                {
                    for (key, profile_value) in profile_object {
                        tun_object.insert(key.clone(), profile_value.clone());
                    }
                }
                // The runtime addresses the local inbound by tag, so a profile
                // that named it differently must not win that one field.
                tun["tag"] = serde_json::Value::String("tun-in".to_owned());
                tun["type"] = serde_json::Value::String("tun".to_owned());
            }
            serde_json::Value::Array(vec![tun])
        }
    };
    if let Some(route) = value.get_mut("route").and_then(|r| r.as_object_mut()) {
        // A local runtime always needs the outbound interface autodetected;
        // tun additionally relies on it to avoid routing loops.
        route.insert(
            "auto_detect_interface".to_owned(),
            serde_json::Value::Bool(true),
        );
    }
    value["inbounds"] = inbounds;
    Ok(serde_json::to_string_pretty(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finding #2 of the refactor report said the fixed-port probe was real code
    /// with no test. The claim worth pinning is not "startup fails" but "startup
    /// fails *before* the cached runtime configuration is rewritten": a refused
    /// start that still clobbered the cache would lose the last known-good
    /// profile the retry path depends on.
    #[tokio::test]
    async fn a_busy_mixed_port_is_refused_before_the_runtime_config_is_rewritten() {
        let dir = tempfile::tempdir().expect("a data directory");
        std::fs::create_dir_all(dir.path().join("cache")).expect("the cache directory");
        let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
        let port = taken.local_addr().expect("the bound address").port();

        let error = match super::start_managed(
            dir.path(),
            VALID_RAW,
            crate::system_proxy::TrafficMode::SystemProxy,
            port,
        )
        .await
        {
            Ok(_) => panic!("the mixed port is occupied, so startup must be refused"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(
            message.contains("已被占用"),
            "the refusal must name the reason rather than just fail: {message}"
        );
        assert!(
            message.contains(&port.to_string()),
            "the refusal must name the port the operator has to free: {message}"
        );
        assert!(
            !dir.path().join("cache/active-config.json").exists(),
            "a refused start must leave the cached runtime configuration alone"
        );
    }

    /// The control that makes the assertion above mean something: with the port
    /// free, startup walks past the probe and writes the cache before failing on
    /// the absent core binary. If this test went red at the `exists()` line, the
    /// refusal test would be passing for the wrong reason.
    #[tokio::test]
    async fn a_free_mixed_port_lets_startup_reach_the_config_write() {
        let dir = tempfile::tempdir().expect("a data directory");
        std::fs::create_dir_all(dir.path().join("cache")).expect("the cache directory");
        let free = std::net::TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
        let port = free.local_addr().expect("the bound address").port();
        drop(free);

        let outcome = super::start_managed(
            dir.path(),
            VALID_RAW,
            crate::system_proxy::TrafficMode::SystemProxy,
            port,
        )
        .await;
        // Whether startup then fails depends on whether a core binary sits in the
        // data directory, which this test does not provide; the point is that it
        // got past the port probe.
        assert!(
            outcome.is_err(),
            "no sing-box binary is installed in the data directory"
        );
        assert!(
            dir.path().join("cache/active-config.json").exists(),
            "startup has to pass the port probe and reach the config write"
        );
    }

    const VALID_RAW: &str = r#"{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}],"experimental":{"clash_api":{"external_controller":"127.0.0.1:9090"}}}"#;

    /// Starting a core must arm the orphan guard without breaking the spawn, and
    /// the reported guarantee has to be honest: Linux arms `PR_SET_PDEATHSIG`
    /// before the child exists, macOS has no equivalent and must say so.
    ///
    /// This covers the wiring only. The end-to-end claim — that killing the
    /// client really takes the core with it — needs a second process to die, so
    /// it belongs to the L3 (Docker) acceptance step, not to a unit test.
    #[cfg(unix)]
    #[tokio::test]
    async fn starting_a_core_arms_the_orphan_guard_and_reports_it_truthfully() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("stub-core");
        std::fs::write(&stub, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let config = dir.path().join("config.json");
        std::fs::write(&config, "{}").unwrap();

        let mut handle = start(&stub, &config, &dir.path().join("cache/core.log"))
            .await
            .expect("the stub core starts with the guard installed");
        assert_eq!(
            handle.orphan_guard,
            cfg!(target_os = "linux"),
            "the guard may only be claimed where the kernel actually provides one"
        );
        assert!(
            handle.child.try_wait().unwrap().is_none(),
            "the pre-exec hook must not break the spawn"
        );
        let _ = handle.child.kill().await;
    }

    #[tokio::test]
    async fn a_dead_child_cannot_be_made_healthy_by_another_api() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api =
            crate::clash_api::ClashApi::new(&format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0; 4096];
                let _ = socket.read(&mut buffer).await;
                let body = r#"{"version":"foreign"}"#;
                let _ = socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
            }
        });
        assert!(api.alive().await);
        #[cfg(windows)]
        let mut child = Command::new("cmd.exe")
            .args(["/C", "exit", "9"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = Command::new("sh")
            .args(["-c", "exit 9"])
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        child.wait().await.unwrap();
        assert!(wait_ready(&mut child, &api).await.is_err());
        server.abort();
    }

    #[test]
    fn each_runtime_overrides_foreign_controller_settings() {
        let raw = r#"{"outbounds":[],"experimental":{"clash_api":{"external_controller":"127.0.0.1:9090","secret":"foreign"},"cache_file":{"enabled":true}}}"#;
        let config: serde_json::Value = serde_json::from_str(
            &runtime_config(
                raw,
                crate::system_proxy::TrafficMode::SystemProxy,
                2080,
                "127.0.0.1:12345",
                "owned",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["experimental"]["clash_api"]["external_controller"],
            "127.0.0.1:12345"
        );
        assert_eq!(config["experimental"]["clash_api"]["secret"], "owned");
        assert_eq!(config["experimental"]["cache_file"]["enabled"], true);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_zip_without_wintun_still_extracts_the_core() {
        use std::io::Write;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        archive
            .start_file("sing-box.exe", zip::write::SimpleFileOptions::default())
            .expect("start core entry");
        archive.write_all(b"test core").expect("write core entry");
        let bytes = archive.finish().expect("finish archive").into_inner();
        let target_dir = tempfile::tempdir().expect("temporary core directory");

        extract_zip_core(&bytes, target_dir.path(), "sing-box.exe")
            .expect("a system-proxy core download must not require wintun.dll");

        assert!(target_dir.path().join("sing-box.exe").is_file());
        assert!(!target_dir.path().join("wintun.dll").exists());
    }

    #[test]
    fn adapt_inbounds_switches_between_mixed_and_tun() {
        let config = serde_json::json!({
            "inbounds": [{"type": "tun", "tag": "tun-in"}],
            "route": {"rules": [], "auto_detect_interface": false},
            "outbounds": []
        })
        .to_string();

        let mixed = adapt_inbounds(&config, crate::system_proxy::TrafficMode::SystemProxy, 2080)
            .expect("mixed adaptation");
        let value: serde_json::Value = serde_json::from_str(&mixed).expect("JSON");
        assert_eq!(value["inbounds"][0]["type"], "mixed");
        assert_eq!(value["inbounds"][0]["listen_port"], 2080);
        assert_eq!(value["route"]["auto_detect_interface"], true);

        let tun = adapt_inbounds(&config, crate::system_proxy::TrafficMode::Tun, 1080)
            .expect("tun adaptation");
        let value: serde_json::Value = serde_json::from_str(&tun).expect("JSON");
        assert_eq!(value["inbounds"][0]["type"], "tun");
        assert_eq!(value["inbounds"][0]["auto_route"], true);
    }

    #[test]
    fn core_platform_matches_the_host_architecture() {
        let (os, arch) = core_platform();
        if cfg!(windows) {
            assert_eq!(os, "windows");
        } else if cfg!(target_os = "macos") {
            assert_eq!(os, "darwin");
        } else {
            assert_eq!(os, "linux");
        }
        assert!(matches!(arch, "amd64" | "arm64"), "unexpected arch {arch}");
    }

    #[test]
    fn release_asset_names_match_the_sing_box_release_assets() {
        assert_eq!(
            release_asset_name("1.14.0", "linux", "amd64"),
            ("sing-box-1.14.0-linux-amd64.tar.gz".to_owned(), false)
        );
        assert_eq!(
            release_asset_name("1.14.0", "darwin", "arm64"),
            ("sing-box-1.14.0-darwin-arm64.tar.gz".to_owned(), false)
        );
        assert_eq!(
            release_asset_name("1.14.0", "windows", "amd64"),
            ("sing-box-1.14.0-windows-amd64.zip".to_owned(), true)
        );
    }

    #[test]
    fn restart_backoff_grows_then_caps() {
        use std::time::Duration;
        assert_eq!(restart_backoff(0), Duration::from_secs(2));
        assert_eq!(restart_backoff(1), Duration::from_secs(4));
        assert_eq!(restart_backoff(2), Duration::from_secs(8));
        assert_eq!(restart_backoff(3), Duration::from_secs(16));
        assert_eq!(restart_backoff(4), Duration::from_secs(30));
        assert_eq!(restart_backoff(10), Duration::from_secs(30));
    }

    /// The client used to substitute its own TUN literal for whatever the
    /// profile declared, so a profile that had moved on to a different stack,
    /// address range or mtu silently got this client defaults back. The profile
    /// declaration now wins; the client only fills gaps and keeps the one tag it
    /// looks the inbound up by.
    #[test]
    fn a_profile_that_declares_its_own_tun_inbound_keeps_those_settings() {
        let profile = r#"{"inbounds":[{"type":"tun","tag":"profile-tun","stack":"system","address":["10.0.0.1/32"],"mtu":1280,"strict_route":false}],"route":{"final":"direct"}}"#;
        let adapted = adapt_inbounds(profile, crate::system_proxy::TrafficMode::Tun, 2080)
            .expect("a profile with a tun inbound adapts");
        let value: serde_json::Value = serde_json::from_str(&adapted).expect("adapted JSON");
        let tun = &value["inbounds"][0];
        assert_eq!(tun["stack"], "system", "the profile stack must survive");
        assert_eq!(tun["address"][0], "10.0.0.1/32", "so must its addresses");
        assert_eq!(tun["mtu"], serde_json::json!(1280));
        assert_eq!(tun["strict_route"], serde_json::json!(false));
        assert_eq!(tun["tag"], "tun-in", "the runtime addresses it by tag");
        assert_eq!(tun["type"], "tun");
        assert_eq!(
            value["route"]["auto_detect_interface"],
            serde_json::json!(true),
            "the local runtime still needs the interface autodetected"
        );
    }

    /// The other half: a subscription that declares no tun inbound keeps getting
    /// exactly the defaults this client always synthesised, so the change above
    /// cannot be an excuse for a behaviour shift on the common path.
    #[test]
    fn a_profile_without_a_tun_inbound_still_gets_the_client_defaults() {
        let profile = r#"{"inbounds":[{"type":"mixed","tag":"x","listen":"127.0.0.1","listen_port":7890}],"route":{}}"#;
        let adapted = adapt_inbounds(profile, crate::system_proxy::TrafficMode::Tun, 2080)
            .expect("a subscription without a tun inbound adapts");
        let value: serde_json::Value = serde_json::from_str(&adapted).expect("adapted JSON");
        let inbounds = value["inbounds"].as_array().expect("an inbound list");
        assert_eq!(inbounds.len(), 1, "the local runtime owns the inbound list");
        assert_eq!(inbounds[0]["stack"], "mixed");
        assert_eq!(inbounds[0]["tag"], "tun-in");
        assert_eq!(inbounds[0]["mtu"], serde_json::json!(9000));
    }
}
