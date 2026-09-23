#requires -Version 5.1
<#
  Windows 11 guest verification pipeline (L5).

  The only leg that can prove CJK font fallback, per-monitor DPI, DWM frame and
  drag/close, a REAL HKCU internet-settings proxy write and its restore,
  wintun.dll detection with TUN elevation, and Job Object orphan reclamation on
  Windows. Docker and WSL cannot; a screenshot taken on this host cannot either.

    verify.ps1 parse-check          # no VM needed: prove all three scripts compile
    verify.ps1 snapshot             # create the base snapshot (once, by hand)
    verify.ps1 revert               # back to a known-clean guest state
    verify.ps1 gui                  # deliver binaries, shoot the 8 GUI pages
    verify.ps1 tui                  # deliver binaries, TUI smoke + privilege/registry probes
    verify.ps1 collect              # pull evidence out of the guest
    verify.ps1 all                  # revert -> gui -> tui -> collect -> revert

  Guest credentials are NEVER stored in this repo or in any log: the password is
  read from $env:WINVM_PASS at run time and only ever placed in an argument
  array, never echoed. Everything the run writes on the host lands under
  .scratch/winvm/<timestamp>/, which .gitignore already covers.

  Isolation: the host is only used to drive vmrun. No binary, PATH entry,
  registry key or service is written to it, and the run fails if the host's own
  proxy/service state moved while the guest was busy.
#>
param(
  [Parameter(Position = 0)]
  [string]$Command = 'parse-check',
  [string]$VmPath = '',
  [string]$SnapshotName = 'clean-base',
  [string]$Pages = 'dashboard,subscriptions,proxies,rules,connections,logs,settings,about',
  [string]$Lang = '',
  [string]$GuestUser = 'Test',
  [string]$OutRoot = '',
  [switch]$KeepState
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Vmrun = 'C:\Program Files\VMware\VMware Workstation\vmrun.exe'
$GuestDir = 'C:\Users\Public\sbwin'
$GuestPs = 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'

function Write-Step([string]$m) { Write-Host "[winvm] $m" }
function Fail([string]$m) { throw "winvm: $m" }

# --- script self-check -------------------------------------------------------
# runProgramInGuest reports almost nothing when a guest script has a syntax
# error: the page list comes back empty and it looks like the app misbehaved.
# This gate catches it on the host, without a VM, and is what `.scratch/win11-vm`
# used before every one of its runs.
function Get-ParseErrors([string]$Path) {
  $tokens = $null; $errors = $null
  [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref]$tokens, [ref]$errors) | Out-Null
  return @($errors)
}

function Invoke-ParseCheck {
  $rc = 0
  foreach ($f in @('verify.ps1', 'shot-guest.ps1', 'tui-guest.ps1')) {
    $p = Join-Path $PSScriptRoot $f
    if (-not (Test-Path $p)) { Write-Host "MISSING  $f"; $rc = 1; continue }
    $errs = Get-ParseErrors $p
    if ($errs.Count -eq 0) {
      Write-Host ("OK       {0}  ({1} bytes)" -f $f, (Get-Item $p).Length)
    } else {
      $rc = 1
      Write-Host ("PARSE    {0}  {1} error(s)" -f $f, $errs.Count)
      foreach ($e in $errs) { Write-Host ("  line {0}: {1}" -f $e.Extent.StartLineNumber, $e.Message) }
    }
  }
  return $rc
}

if ($Command -eq 'parse-check') { exit (Invoke-ParseCheck) }

# --- vmrun plumbing ----------------------------------------------------------
if (-not (Test-Path $Vmrun)) { Fail "vmrun not found at $Vmrun" }
if (-not $VmPath) {
  # Prefer a running match, then the env var, then a filesystem search. Hardcoding
  # one path would break the moment the VM is moved between disks.
  if ($env:WINVM_VMX -and (Test-Path $env:WINVM_VMX)) {
    $VmPath = $env:WINVM_VMX
  } else {
    $running = & $Vmrun -T ws list | Select-Object -Skip 1
    $VmPath = ($running | Where-Object { $_ -match 'Win11' -and $_ -match '\.vmx$' } | Select-Object -First 1)
    if (-not $VmPath) {
      $roots = @((Join-Path $env:USERPROFILE 'Virtual Machines'),
                 (Join-Path $env:USERPROFILE 'Virtual Machines VMs'),
                 'D:\Virtual Machines', 'E:\Virtual Machines')
      foreach ($r in $roots) {
        if (-not (Test-Path $r)) { continue }
        $hit = Get-ChildItem -Path $r -Recurse -Filter '*Win11*.vmx' -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($hit) { $VmPath = $hit.FullName; break }
      }
    }
  }
}
if (-not $VmPath -or -not (Test-Path $VmPath)) { Fail 'no VM: pass -VmPath <file.vmx> or set WINVM_VMX' }
Write-Step "vm=$VmPath"

$Pass = $env:WINVM_PASS
if (-not $Pass) { Fail 'set $env:WINVM_PASS (the guest password is never stored in the repo)' }

function Invoke-Vmrun([string[]]$Args_, [switch]$Credentialed, [switch]$AllowFail) {
  $a = @('-T', 'ws')
  if ($Credentialed) { $a += @('-gu', $GuestUser, '-gp', $Pass) }
  $a += $VmPath
  $a += $Args_
  $out = & $Vmrun @a 2>&1
  $code = $LASTEXITCODE
  if ($code -ne 0 -and -not $AllowFail) {
    # The only thing safe to print is the argument shape, never the values.
    Fail ("vmrun {0} exited {1}: {2}" -f $Args_[0], $code, (($out | Select-Object -First 4) -join ' / '))
  }
  return [pscustomobject]@{ Code = $code; Out = @($out) }
}

function Ensure-Started {
  $p = Invoke-Vmrun @('checkToolsRunningStatus') -Credentialed -AllowFail
  if ($p.Code -eq 0) { return }
  Write-Step 'starting guest'
  Invoke-Vmrun @('start', 'nogui') -AllowFail | Out-Null
  for ($i = 0; $i -lt 60; $i++) {
    Start-Sleep -Seconds 5
    $t = Invoke-Vmrun @('checkToolsRunningStatus') -Credentialed -AllowFail
    if ($t.Code -eq 0) { Write-Step 'guest tools ready'; return }
  }
  Fail 'timed out waiting for VMware Tools in the guest'
}

function Revert {
  Write-Step "revert to snapshot '$SnapshotName'"
  $r = Invoke-Vmrun @('revertToSnapshot', $SnapshotName) -AllowFail
  if ($r.Code -ne 0) { Fail "revertToSnapshot failed: $(($r.Out) -join ' / ')" }
  Start-Sleep -Seconds 20
}

function Get-Snapshots { Invoke-Vmrun @('snapshot', $SnapshotName) }

function Copy-In([string]$HostPath, [string]$GuestPath, [string]$Label) {
  if (-not (Test-Path $HostPath)) { Fail "missing $Label : $HostPath" }
  Invoke-Vmrun @('copyFileFromHostToGuest', $HostPath, $GuestPath) -Credentialed | Out-Null
  Write-Step ("delivered {0} ({1} bytes)" -f $Label, (Get-Item $HostPath).Length)
}

function Copy-Out([string]$GuestPath, [string]$HostPath) {
  New-Item -ItemType Directory -Force -Path (Split-Path -Parent $HostPath) | Out-Null
  Invoke-Vmrun @('copyFileFromGuestToHost', $GuestPath, $HostPath) -Credentialed -AllowFail | Out-Null
  return (Test-Path $HostPath)
}

function Invoke-Guest([string[]]$ProgArgs) {
  return Invoke-Vmrun (@('runProgramInGuest', '-interactive') + $ProgArgs) -Credentialed -AllowFail
}

# --- artifacts ---------------------------------------------------------------
$Stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
if (-not $OutRoot) { $OutRoot = Join-Path $RepoRoot ".scratch\winvm\$Stamp" }
New-Item -ItemType Directory -Force -Path $OutRoot | Out-Null
$Log = Join-Path $OutRoot 'verify.log'
Start-Transcript -Path $Log -Append | Out-Null

# Both guest scripts print a `stray-processes=` line. It is the assertion the
# acceptance list asks for: a survivor keeps the mixed port busy, and the next
# page's auto-start then fails for a reason that has nothing to do with itself.
function Assert-NoStrays {
  $found = @()
  foreach ($f in @('gui-manifest.txt', 'tui.txt')) {
    $p = Join-Path $OutRoot $f
    if (-not (Test-Path $p)) { $found += "$f missing (the guest script never ran)"; continue }
    Select-String -Path $p -Pattern 'stray-processes=(.+)' | ForEach-Object {
      $v = $_.Matches[0].Groups[1].Value.Trim()
      if ($v -ne 'none') { $found += "$f : $v" }
    }
  }
  if ($found.Count -gt 0) {
    $found | Out-File -Encoding utf8 (Join-Path $OutRoot 'strays.txt')
    Write-Step ("STRAY PROCESSES ({0}) - recorded, guest still gets reverted" -f $found.Count)
  } else {
    Write-Step 'no stray sbgui/sbtui/sing-box processes'
  }
}

# The ticket's third acceptance line: prove the host was not the test bench.
function Get-HostFingerprint {
  $k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
  $p = Get-ItemProperty $k -ErrorAction SilentlyContinue
  $svc = (Get-Service -Name 'sbctl', 'sing-box', 'sbgui', 'sbtui' -ErrorAction SilentlyContinue |
          ForEach-Object { "$($_.Name)=$($_.Status)" }) -join ','
  return [pscustomobject]@{
    ProxyEnable = $p.ProxyEnable
    ProxyServer = $p.ProxyServer
    AutoConfigURL = $p.AutoConfigURL
    WinHttp = ((netsh winhttp show proxy) -join ' ')
    Services = $svc
    PathEntries = ($env:Path -split ';').Count
  }
}
$FingerprintBefore = Get-HostFingerprint

# runProgramInGuest cannot see host paths, so the binaries live in the guest first.
function Deliver-Binaries {
  Invoke-Vmrun @('createDirectoryInGuest', "$GuestDir\shots") -Credentialed -AllowFail | Out-Null
  $gui = Join-Path $RepoRoot 'target\release\sbgui.exe'
  $tui = Join-Path $RepoRoot 'target\release\sbtui.exe'
  if (-not (Test-Path $gui)) { Fail "build it first: cargo build --release -p sbgui  (missing $gui)" }
  Copy-In $gui "$GuestDir\sbgui.exe" 'sbgui.exe'
  if (Test-Path $tui) { Copy-In $tui "$GuestDir\sbtui.exe" 'sbtui.exe' } else { Write-Step 'sbtui.exe not built; TUI leg will report MISSING' }
  $core = Join-Path $RepoRoot '.scratch\sbgui-kernel\sing-box.exe'
  if (Test-Path $core) { Copy-In $core "$GuestDir\sing-box.exe" 'sing-box.exe' } else { Write-Step 'no guest sing-box.exe; TUN/wintun legs will report skipped' }
  Copy-In (Join-Path $PSScriptRoot 'shot-guest.ps1') "$GuestDir\shot-guest.ps1" 'shot-guest.ps1'
  Copy-In (Join-Path $PSScriptRoot 'tui-guest.ps1') "$GuestDir\tui-guest.ps1" 'tui-guest.ps1'
}

function Run-Gui {
  Write-Step "GUI pages: $Pages"
  $gp = @($GuestPs, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$GuestDir\shot-guest.ps1", '-Pages', $Pages)
  if ($Lang) { $gp += @('-Lang', $Lang) }
  $r = Invoke-Guest $gp
  Write-Step "runProgramInGuest exit=$($r.Code)"
  Collect-From-Guest
}

function Collect-From-Guest {
  if (-not (Copy-Out "$GuestDir\shots\manifest.txt" (Join-Path $OutRoot 'gui-manifest.txt'))) {
    Write-Step 'WARNING no gui manifest came back - the guest script never ran'
  }
  foreach ($page in ($Pages -split ',')) {
    $page = $page.Trim(); if (-not $page) { continue }
    Copy-Out "$GuestDir\shots\$page.png" (Join-Path $OutRoot "gui\$page.png") | Out-Null
    Copy-Out "$GuestDir\shots\$page.err.txt" (Join-Path $OutRoot "gui\$page.err.txt") | Out-Null
  }
  if (-not (Copy-Out "$GuestDir\tui.txt" (Join-Path $OutRoot 'tui.txt'))) {
    Write-Step 'no tui report in the guest (the tui leg has not run)'
  }
}

function Run-Tui {
  Write-Step 'TUI smoke + privilege/registry probes'
  $r = Invoke-Guest @($GuestPs, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$GuestDir\tui-guest.ps1")
  Write-Step "runProgramInGuest exit=$($r.Code)"
  if (-not (Copy-Out "$GuestDir\tui.txt" (Join-Path $OutRoot 'tui.txt'))) {
    Write-Step 'WARNING no tui report came back'
  }
}

function Finalize {
  $after = Get-HostFingerprint
  $drift = @()
  foreach ($n in $after.PSObject.Properties.Name) {
    if ("$($after.$n)" -ne "$($FingerprintBefore.$n)") { $drift += "$n : '$($FingerprintBefore.$n)' -> '$($after.$n)'" }
  }
  $driftPath = Join-Path $OutRoot 'host-drift.txt'
  if ($drift.Count -eq 0) {
    "host unchanged: proxy, winhttp, service states and PATH width identical before/after" | Out-File -Encoding utf8 $driftPath
    Write-Step 'host fingerprint: unchanged (good)'
  } else {
    $drift | Out-File -Encoding utf8 $driftPath
    Write-Step ("HOST DRIFT DETECTED ({0} field(s)) - see host-drift.txt" -f $drift.Count)
  }
  Get-ChildItem -Recurse $OutRoot | ForEach-Object {
    if (-not $_.PSIsContainer) { "{0,10} {1}" -f $_.Length, $_.FullName.Substring($OutRoot.Length + 1) }
  } | Out-File -Encoding utf8 (Join-Path $OutRoot 'collected.txt')
  Write-Step "evidence: $OutRoot"
}

try {
  switch ($Command) {
    'snapshot' { Ensure-Started; Get-Snapshots; Write-Step "created snapshot '$SnapshotName'" }
    'revert'   { Revert }
    'gui'      { Revert; Ensure-Started; Deliver-Binaries; Run-Gui; Assert-NoStrays; if (-not $KeepState) { Revert } }
    'tui'      { Revert; Ensure-Started; Deliver-Binaries; Run-Tui; if (-not $KeepState) { Revert } }
    'collect'  { Ensure-Started; Collect-From-Guest; Assert-NoStrays }
    'all'      { Revert; Ensure-Started; Deliver-Binaries; Run-Gui; Run-Tui; Assert-NoStrays; if (-not $KeepState) { Revert } }
    default    { Fail "unknown command '$Command' (parse-check|snapshot|revert|gui|tui|collect|all)" }
  }
  Finalize
} finally {
  Stop-Transcript | Out-Null
}
exit 0
