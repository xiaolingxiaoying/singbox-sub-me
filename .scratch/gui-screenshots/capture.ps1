# Capture the Serein (sbgui) window to a PNG.
# Usage: powershell -File capture.ps1 <output.png>
# Tries PrintWindow(PW_RENDERFULLCONTENT) first; falls back to
# foreground + CopyFromScreen if the result is blank.
param([string]$OutFile = "shot.png")

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinCap {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[WinCap]::SetProcessDPIAware() | Out-Null

# Find the visible top-level window of a process by id.
$targetPid = (Get-Process sbgui -ErrorAction Stop).Id
$script:hwnd = [IntPtr]::Zero
$cb = [WinCap+EnumProc]{
  param($h, $lp)
  [uint32]$wpid = 0
  [WinCap]::GetWindowThreadProcessId($h, [ref]$wpid) | Out-Null
  if ($wpid -eq $targetPid -and [WinCap]::IsWindowVisible($h)) {
    $script:hwnd = $h
    return $false
  }
  return $true
}
[WinCap]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$h = $script:hwnd
if ($h -eq [IntPtr]::Zero) { Write-Error "Serein window not found"; exit 1 }
if ([WinCap]::IsIconic($h)) { [WinCap]::ShowWindow($h, 9) | Out-Null; Start-Sleep -Milliseconds 500 }

$r = New-Object WinCap+RECT
[WinCap]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $ht = $r.B - $r.T
if ($w -le 0 -or $ht -le 0) { Write-Error "bad rect"; exit 1 }

$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [WinCap]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)

# Detect a blank capture (all one color) and retry from the screen.
$rect = New-Object System.Drawing.Rectangle(0, 0, $w, $ht)
$samples = @()
foreach ($pt in @([System.Drawing.Point]::new(5,5), [System.Drawing.Point]::new($w-5,5), [System.Drawing.Point]::new(5,$ht-5), [System.Drawing.Point]::new($w-5,$ht-5), [System.Drawing.Point]::new($w/2,$ht/2))) {
  $samples += $bmp.GetPixel($pt.X, $pt.Y)
}
$blank = ($samples | Where-Object { $_.R -eq $samples[0].R -and $_.G -eq $samples[0].G -and $_.B -eq $samples[0].B }).Count -eq 5
if (-not $ok -or $blank) {
  [WinCap]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 400
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
}
$g.Dispose()
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved $OutFile rect=$($r.L),$($r.T),$($r.R),$($r.B) w=$w h=$ht printwindow=$ok blank=$blank"
