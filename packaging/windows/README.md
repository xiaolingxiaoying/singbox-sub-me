# Windows 11 GUI packaging

The WiX v4 source in `sbtui.wxs` produces an MSI containing `sbgui.exe`,
`sbtui.exe`, and `ly.exe`. The Start Menu offers one entry per launchable
binary: `sbgui` -> `sbgui.exe` (the desktop GUI) and `sbtui` -> `sbtui.exe`
(the terminal client). `ly` gets no entry because it is a short PATH launcher
for the same TUI, not a separate product.

`Package/@Version` must equal the release version in the root `Cargo.toml`
(the version the tag carries). It is the value Windows Installer and
`MajorUpgrade` compare, so a stale literal here silently breaks
upgrade-in-place; the `build-msi` job in `.github/workflows/release.yml` packs
whatever this file says, so bump it in the same commit as the version bump.

## Building the MSI locally

Requires the .NET SDK (for the WiX CLI) and a Windows host toolchain able to
build `sbgui` (the L1 gate in `docs/verification-and-build-flow.md`). From the
repository root in PowerShell:

```powershell
# 1. WiX v4 command line as a local dotnet tool.
dotnet tool install --tool-path $env:TEMP\wix --version 4.0.6 wix

# 2. The release binaries the manifest packages.
cargo build --release --locked -p sbgui -p sbtui
New-Item -ItemType Directory -Force packaging\windows\bin | Out-Null
Copy-Item target\release\sbgui.exe, target\release\sbtui.exe, target\release\ly.exe packaging\windows\bin\

# 3. Build. The manifest refers to its inputs as `bin\...`, so run from here;
#    CI does the same.
cd packaging\windows
& "$env:TEMP\wix\wix.exe" build sbtui.wxs -arch x64 -o sbtui-windows-amd64.msi
```

Release CI does the same thing in the `build-msi` job of
`.github/workflows/release.yml`, feeding `packaging/windows/bin` from the
`build-sbtui` and `build-sbgui` artifacts, and publishes
`sbtui-windows-amd64.msi` as a release asset.

## What is intentionally not in the MSI

* `%APPDATA%\sbtui` (and `%APPDATA%\sbgui`): subscriptions, cached
  configurations, the downloaded sing-box core and proxy backups stay on disk
  across upgrades and uninstall.
* `wintun.dll`. It is not a packaging artifact in this project: the client
  resolves it as `<data-dir>/core/wintun.dll`
  (`crates/client-core/src/settings.rs`, `wintun_path`) and extracts it from
  the sing-box release zip while downloading the core
  (`crates/client-core/src/core.rs`, `extract_zip_core`); when it is missing the
  controller tells the user to re-download the core
  (`crates/client-core/src/controller.rs`). Shipping a second copy in
  `Program Files` would create a stale source of truth that no code reads.
  (This withdraws the earlier "MSI 缺 `wintun.dll` 组件" recommendation in
  `docs/known-gaps-after-merge.md` G2.)
* An embedded `requestedExecutionLevel` application manifest. Forcing
  `requireAdministrator` would make every launch prompt for UAC, including the
  system-proxy users who never enable TUN — a worse product than the bug it
  fixes. The agreed fix is self-elevation on the TUN start path only, tracked
  separately as gap G11 in `docs/known-gaps-after-merge.md`; it does not belong
  in the packaging layer. (This withdraws the "无 `requestedExecutionLevel`
  清单" recommendation from G2.)
* An MSI-based install of `sbgui` on the command line: `install.ps1` in
  `crates/sbtui/packaging/` is a separate per-user (no admin) path that copies
  `sbtui.exe` and `ly.exe` into `%LOCALAPPDATA%\Programs\sbtui` and links them
  onto PATH through `%LOCALAPPDATA%\Microsoft\WindowsApps`. It installs the TUI
  only, never `sbgui.exe`, and it is published as a release asset for users who
  want it — put `install.ps1`, `sbtui.exe` and `ly.exe` in one folder and run
  the script from there.
