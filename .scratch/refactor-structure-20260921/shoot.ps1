# Shoot every sbgui page at one or two window sizes for before/after comparison.
# Usage: powershell -File shoot.ps1 -OutDir <dir> [-Pages a,b,c] [-Size 1440x900]
param(
  [Parameter(Mandatory=$true)][string]$OutDir,
  [string]$Pages = "dashboard,subscriptions,proxies,rules,connections,logs,settings",
  [string]$Size = "1440x900"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$exe = Join-Path $PSScriptRoot "..\..\target\debug\sbgui.exe"
$capture = Join-Path $PSScriptRoot "..\gui-screenshots\capture.ps1"

foreach ($page in $Pages.Split(",")) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = (Resolve-Path $exe).Path
  $psi.UseShellExecute = $false
  $psi.EnvironmentVariables["SBGUI_PAGE"] = $page
  $psi.EnvironmentVariables["SBGUI_SIZE"] = $Size
  $p = [System.Diagnostics.Process]::Start($psi)
  Start-Sleep -Milliseconds 3000
  $out = Join-Path $OutDir "$Size-$page.png"
  try {
    & $capture $out | Out-Null
    Write-Host "shot $out"
  } finally {
    if (!$p.HasExited) { $p.Kill() }
    Start-Sleep -Milliseconds 500
  }
}
