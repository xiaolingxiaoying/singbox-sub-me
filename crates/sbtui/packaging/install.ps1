# Install sbtui for the current user and expose a global `ly` shortcut.
#
# sbtui.exe is copied to %LOCALAPPDATA%\Programs\sbtui\, and a `ly.cmd` wrapper
# is written into %LOCALAPPDATA%\Microsoft\WindowsApps (already on PATH), so
# typing `ly` in any terminal opens the client. No administrator rights needed.
$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $here 'sbtui.exe'
if (-not (Test-Path $exe)) {
    throw "sbtui.exe not found next to install.ps1 ($here)"
}

$target = Join-Path $env:LOCALAPPDATA 'Programs\sbtui'
New-Item -ItemType Directory -Force -Path $target | Out-Null
Copy-Item $exe (Join-Path $target 'sbtui.exe') -Force

$bin = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps'
New-Item -ItemType Directory -Force -Path $bin | Out-Null
$launcher = Join-Path $bin 'ly.cmd'
$content = "@echo off`r`n`"$target\sbtui.exe`" %*`r`n"
Set-Content -Path $launcher -Value $content -Encoding Ascii

Write-Host "installed: $target\sbtui.exe"
Write-Host "shortcut:  $launcher (run: ly)"
& (Join-Path $target 'sbtui.exe') --version
