# Install the sbtui client and its `ly` shortcut binary for the current user.
#
# Both binaries are copied into %LOCALAPPDATA%\Programs\sbtui, and a copy of
# `ly.exe` (plus `sbtui.exe`) is placed in %LOCALAPPDATA%\Microsoft\WindowsApps
# which is already on PATH, so typing `ly` (or `sbtui`) in any terminal opens
# the client. No administrator rights needed.
$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$target = Join-Path $env:LOCALAPPDATA 'Programs\sbtui'
$bin = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps'
New-Item -ItemType Directory -Force -Path $target, $bin | Out-Null

$found = $false
foreach ($name in 'sbtui.exe', 'ly.exe') {
    $src = Join-Path $here $name
    if (-not (Test-Path $src)) { continue }
    Copy-Item $src (Join-Path $target $name) -Force
    Copy-Item $src (Join-Path $bin $name) -Force
    $found = $true
}

if (-not $found) {
    throw "no sbtui.exe / ly.exe next to install.ps1 ($here)"
}

Write-Host "installed: $target"
Write-Host "on PATH:   $bin\ly.exe (run: ly)"
& (Join-Path $bin 'ly.exe') --version
