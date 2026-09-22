# Capture the VMware console window of a running VM to a PNG, so the guest
# screen can be reviewed from the host without guest credentials.
# Usage: powershell -File capture-vm.ps1 [-OutFile shot.png] [-TitleMatch 'Windows 11 x64']
param(
  [string]$OutFile = "vm-shot.png",
  [string]$TitleMatch = "Windows 11 x64"
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class VmCap {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lp);
  public delegate bool EnumProc(IntPtr h, IntPtr lp);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
"@
[VmCap]::SetProcessDPIAware() | Out-Null

$script:hwnd = [IntPtr]::Zero
$cb = [VmCap+EnumProc]{
  param($h, $lp)
  if (-not [VmCap]::IsWindowVisible($h)) { return $true }
  $sb = New-Object System.Text.StringBuilder 512
  [VmCap]::GetWindowTextW($h, $sb, $sb.Capacity) | Out-Null
  if ($sb.ToString() -like "*$TitleMatch*VMware*") { $script:hwnd = $h; return $false }
  return $true
}
[VmCap]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$h = $script:hwnd
if ($h -eq [IntPtr]::Zero) { Write-Error "no VMware console window matching '$TitleMatch'"; exit 1 }
if ([VmCap]::IsIconic($h)) { [VmCap]::ShowWindow($h, 9) | Out-Null; Start-Sleep -Milliseconds 500 }

$r = New-Object VmCap+RECT
[VmCap]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $ht = $r.B - $r.T
if ($w -le 0 -or $ht -le 0) { Write-Error "bad rect"; exit 1 }

$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [VmCap]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)

$rect = New-Object System.Drawing.Rectangle(0, 0, $w, $ht)
$samples = @()
foreach ($pt in @([System.Drawing.Point]::new(5,5), [System.Drawing.Point]::new($w-5,5), [System.Drawing.Point]::new(5,$ht-5), [System.Drawing.Point]::new($w-5,$ht-5), [System.Drawing.Point]::new($w/2,$ht/2))) {
  $samples += $bmp.GetPixel($pt.X, $pt.Y)
}
$blank = ($samples | Where-Object { $_.R -eq $samples[0].R -and $_.G -eq $samples[0].G -and $_.B -eq $samples[0].B }).Count -eq 5
if (-not $ok -or $blank) {
  [VmCap]::SetForegroundWindow($h) | Out-Null
  Start-Sleep -Milliseconds 400
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $blank = $false
}
$g.Dispose()
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "saved $OutFile title_match=$TitleMatch rect=$($r.L),$($r.T),$($r.R),$($r.B) w=$w h=$ht printwindow=$ok wasblank=$blank"
