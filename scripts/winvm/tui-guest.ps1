# Runs INSIDE the Windows guest: TUI smoke, privilege probe, and the registry
# system-proxy round trip that only a real Windows box can prove.
param(
  [string]$Dir = 'C:\Users\Public\sbwin',
  [string]$OutFile = 'C:\Users\Public\sbwin\tui.txt'
)

$ErrorActionPreference = 'SilentlyContinue'
$o = @()
$o += "started=" + (Get-Date -Format o)
$o += "host=" + $env:COMPUTERNAME
$o += "user=" + [System.Environment]::UserName
$o += "ps=" + $PSVersionTable.PSVersion.ToString()
$o += "arch=" + $env:PROCESSOR_ARCHITECTURE
$o += "build=" + ((Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').DisplayVersion)

function Invoke-Captured([string]$exe, [string[]]$args_) {
  if (-not (Test-Path $exe)) { return @("MISSING $exe") }
  $lines = @()
  try {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = ($args_ -join ' ')
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    if (-not $p.WaitForExit(15000)) { $p.Kill(); $lines += 'TIMEOUT-15s' }
    $lines += "exit=" + $p.ExitCode
    foreach ($l in ($so -split "`r?`n")) { if ($l) { $lines += "OUT: $l" } }
    foreach ($l in ($se -split "`r?`n")) { if ($l) { $lines += "ERR: $l" } }
  } catch {
    $lines += 'EXCEPTION ' + $_.Exception.Message
  }
  return $lines
}

$o += '--- sbtui --version ---'
$o += Invoke-Captured (Join-Path $Dir 'sbtui.exe') @('--version')
$o += '--- sbtui --help ---'
$o += Invoke-Captured (Join-Path $Dir 'sbtui.exe') @('--help')

# A TUI cannot be screenshotted while it owns the terminal from runProgramInGuest,
# so the interactive leg is a separate, human-run step; what IS provable here is
# that the binary resolves its data directory without a terminal attached.
$o += '--- sbtui data dir ---'
$o += Invoke-Captured (Join-Path $Dir 'sbtui.exe') @('--print-dir')

$o += '--- privilege probe ---'
$admin = $false
try {
  & net session 2>&1 | Out-Null
  $admin = ($LASTEXITCODE -eq 0)
} catch { $admin = $false }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$role = New-Object Security.Principal.WindowsPrincipal($identity)
$o += "net-session-ok=" + $admin
$o += "is-administrator=" + $role.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$o += "wintun-present=" + (Test-Path (Join-Path $Dir 'wintun.dll'))
$o += "wintun-system32=" + (Test-Path (Join-Path $env:SystemRoot 'System32\wintun.dll'))

# Writing the real proxy keys is the part no container can prove. Snapshot first,
# set, verify the value landed, then restore - a leaked proxy would outlive the
# snapshot revert because HKCU is not part of the disk snapshot's rolling state.
$o += '--- registry system proxy ---'
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$before = Get-ItemProperty $key
$stateBefore = @{ ProxyEnable = $before.ProxyEnable; ProxyServer = $before.ProxyServer; AutoConfigURL = $before.AutoConfigURL }
$o += "before: Enable=$($stateBefore.ProxyEnable) Server=$($stateBefore.ProxyServer)"
try {
  New-ItemProperty -Path $key -Name ProxyEnable -PropertyType DWord -Value 1 -Force | Out-Null
  New-ItemProperty -Path $key -Name ProxyServer -PropertyType String -Value '127.0.0.1:2080' -Force | Out-Null
  $after = Get-ItemProperty $key
  $written = ($after.ProxyEnable -eq 1 -and $after.ProxyServer -eq '127.0.0.1:2080')
  # Re-read through the API the kernel itself uses, not just the file we wrote.
  $viaNetsh = (& netsh winhttp show proxy) -join ' '
  $o += "write-round-trip=" + $written
  $o += "netsh-view=$viaNetsh"
} finally {
  Set-ItemProperty -Path $key -Name ProxyEnable -Value $stateBefore.ProxyEnable
  if ($null -eq $stateBefore.ProxyServer) {
    Remove-ItemProperty -Path $key -Name ProxyServer -ErrorAction SilentlyContinue
  } else {
    Set-ItemProperty -Path $key -Name ProxyServer -Value $stateBefore.ProxyServer
  }
  $restored = Get-ItemProperty $key
  $o += "restored-Enable=" + ($restored.ProxyEnable -eq $stateBefore.ProxyEnable)
  $o += "restored-Server=" + ($restored.ProxyServer -eq $stateBefore.ProxyServer)
}

$o += '--- dpi ---'
Add-Type -AssemblyName System.Windows.Forms
$b = [System.Windows.Forms.Screen]::PrimaryScreen
$o += "primary=" + $b.Bounds.Width + "x" + $b.Bounds.Height + " bpp=" + $b.BitsPerPixel
$dpi = (Get-ItemProperty 'HKCU:\Control Panel\Desktop\WindowMetrics').AppliedDPI
$o += "applied-dpi=$dpi"

$o += '--- residue check (must be none before revert) ---'
$strays = (Get-Process sbgui, sbtui, sing-box -ErrorAction SilentlyContinue | ForEach-Object { $_.ProcessName }) -join ','
$o += "stray-processes=" + ($(if ($strays) { $strays } else { 'none' }))
$o += '--- listening ports we may have left ---'
$o += ((Get-NetTCPConnection -State Listen -LocalPort 2080, 2081 -ErrorAction SilentlyContinue | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) -join ' ')
$o += "finished=" + (Get-Date -Format o)
$o | Out-File -Encoding utf8 $OutFile
Write-Output ($o -join "`n")
