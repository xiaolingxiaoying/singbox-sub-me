# Windows 11 GUI packaging

The WiX v4 source in `sbtui.wxs` produces an MSI containing `sbgui.exe`,
`sbtui.exe`, and `ly.exe`. Build the release binaries first, copy them into
`packaging/windows/bin`, then run WiX v4 `wix build sbtui.wxs` from this
directory.

The installer does not include or remove `%APPDATA%\sbtui`; subscriptions,
cached configurations, the downloaded sing-box core, and proxy backups remain
available across upgrades and uninstall.
