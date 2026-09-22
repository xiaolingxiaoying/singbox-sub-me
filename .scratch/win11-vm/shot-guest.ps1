param(
  [string]$Exe = 'C:\Users\Public\sbgui\sbgui.exe',
  [string]$OutDir = 'C:\Users\Public\sbgui\shots',
  [string]$Size = '1180x820',
  [int]$SettleMs = 6000,
  [int]$Retries = 2,
  [string]$Pages = 'dashboard,subscriptions,proxies,rules,connections,logs,settings'
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }

$manifest = @()

# The UI strings are Chinese but this VM was installed from an English ISO, so
# tofu boxes are a real possibility; check before trusting any screenshot.
$fontNames = (New-Object System.Drawing.Text.InstalledFontCollection).Families | ForEach-Object { $_.Name }
$cjk = $fontNames | Where-Object { $_ -match 'YaHei|JhengHei|SimSun|SimHei|KaiTi|Noto.*CJK|Source Han|MS Gothic|PMingLiU' }
$manifest += "cjk-fonts=" + ($(if ($cjk) { $cjk -join ',' } else { 'NONE' }))
$mem = Get-CimInstance Win32_OperatingSystem
$manifest += "guest-freeMB=" + [math]::Round($mem.FreePhysicalMemory / 1KB)

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class G {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT val, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@
[G]::SetProcessDPIAware() | Out-Null

# GetWindowRect includes the invisible resize border, so cropping with it puts a
# strip of desktop (icons, wallpaper) into the shot. DWM's frame bounds do not.
function Get-ClientRect($h) {
  $r = New-Object G+RECT
  $hr = [G]::DwmGetWindowAttribute($h, 9, [ref]$r, [System.Runtime.InteropServices.Marshal]::SizeOf($r))
  if ($hr -eq 0 -and ($r.R - $r.L) -gt 0) { return $r }
  [G]::GetWindowRect($h, [ref]$r) | Out-Null
  return $r
}

function Find-Window($pid_) {
  $script:found = [IntPtr]::Zero
  $cbw = [G+EnumProc]{
    param($w, $lp)
    [uint32]$wpid = 0
    [G]::GetWindowThreadProcessId($w, [ref]$wpid) | Out-Null
    if ($wpid -eq $pid_ -and [G]::IsWindowVisible($w)) { $script:found = $w; return $false }
    return $true
  }
  [G]::EnumWindows($cbw, [IntPtr]::Zero) | Out-Null
  return $script:found
}

# One launch attempt: 'OK ...' on success, a reason string on failure.
function Invoke-Page([string]$page, [int]$attempt) {
  $env:SBGUI_PAGE = $page
  $env:SBGUI_SIZE = $Size
  $errFile = Join-Path $OutDir "$page.err.txt"
  $outFile = Join-Path $OutDir "$page.out.txt"

  try {
    $p = Start-Process -FilePath $Exe -PassThru -RedirectStandardError $errFile -RedirectStandardOutput $outFile
  } catch {
    return "LAUNCH-FAIL " + $_.Exception.Message
  }
  $targetPid = $p.Id

  $h = [IntPtr]::Zero
  $deadline = (Get-Date).AddMilliseconds([double]$SettleMs)
  while ((Get-Date) -lt $deadline -and $h -eq [IntPtr]::Zero) {
    if ($p.HasExited) { break }
    $h = Find-Window $targetPid
    if ($h -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 250 }
  }

  if ($p.HasExited) {
    $code = 'unknown'
    try { $code = $p.ExitCode } catch { }
    $tail = ''
    if ((Test-Path $errFile) -and ((Get-Item $errFile).Length -gt 0)) {
      $tail = ((Get-Content $errFile | Select-Object -Last 3) -join ' / ')
    }
    return "attempt$attempt EXITED code=$code stderr=[$tail]"
  }
  if ($h -eq [IntPtr]::Zero) {
    Stop-Process -Id $targetPid -Force -ErrorAction SilentlyContinue
    return "attempt$attempt NO-WINDOW pid=$targetPid"
  }

  Start-Sleep -Milliseconds 700
  [G]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 1200

  $r = Get-ClientRect $h
  $w = $r.R - $r.L; $ht = $r.B - $r.T
  if ($w -le 0 -or $ht -le 0) {
    Stop-Process -Id $targetPid -Force -ErrorAction SilentlyContinue
    return "attempt$attempt BAD-RECT $($r.L),$($r.T),$($r.R),$($r.B)"
  }

  $bmp = New-Object System.Drawing.Bitmap($w, $ht)
  $g2 = [System.Drawing.Graphics]::FromImage($bmp)
  $g2.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $png = Join-Path $OutDir "$page.png"
  $bmp.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
  $g2.Dispose(); $bmp.Dispose()

  $title = New-Object System.Text.StringBuilder 512
  [G]::GetWindowTextW($h, $title, $title.Capacity) | Out-Null
  Stop-Process -Id $targetPid -Force -ErrorAction SilentlyContinue
  return "OK ${w}x${ht} title=[$($title.ToString())]"
}

foreach ($page in $Pages.Split(',')) {
  $page = $page.Trim()
  $result = ''
  for ($attempt = 1; $attempt -le ($Retries + 1); $attempt++) {
    $result = Invoke-Page $page $attempt
    if ($result -like 'OK *') { break }
    Start-Sleep -Milliseconds 3000
  }
  $manifest += "$page : $result"
  Start-Sleep -Milliseconds 1500
}

$manifest | Out-File -Encoding utf8 (Join-Path $OutDir 'manifest.txt')
Write-Output ($manifest -join "`n")
