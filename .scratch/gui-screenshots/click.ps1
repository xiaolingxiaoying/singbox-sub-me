# Click at physical screen coordinates (DPI-aware).
# Usage: powershell -File click.ps1 <x> <y> [doubleClick]
param([int]$X, [int]$Y, [switch]$Double)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Click {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  public const uint LEFTDOWN = 0x0002, LEFTUP = 0x0004;
}
"@
[Click]::SetProcessDPIAware() | Out-Null
[Click]::SetCursorPos($X, $Y) | Out-Null
Start-Sleep -Milliseconds 120
[Click]::mouse_event([Click]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 60
[Click]::mouse_event([Click]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
if ($Double) {
  Start-Sleep -Milliseconds 90
  [Click]::mouse_event([Click]::LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 60
  [Click]::mouse_event([Click]::LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
}
Write-Output "clicked $X,$Y"
