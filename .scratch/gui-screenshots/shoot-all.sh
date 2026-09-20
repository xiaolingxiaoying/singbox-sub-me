#!/bin/bash
# Capture every sbgui page. Each page: relaunch with SBGUI_PAGE, wait for the
# engine + core auto-start, shoot. The traffic loops run independently.
set -e
cd "C:\Users\ranly\Documents\ChatGPT\singbox-sub-me\.scratch\gui-screenshots"
ROOT="C:\Users\ranly\Documents\ChatGPT\singbox-sub-me"

shoot() {
  local page="$1" size="$2" out="$3" wait_extra="$4"
  taskkill //IM sbgui.exe //F >/dev/null 2>&1 || true
  taskkill //IM sing-box.exe //F >/dev/null 2>&1 || true
  sleep 1
  ( export SBGUI_PAGE="$page"; export SBGUI_SIZE="$size"; export SBGUI_SHOW_EXIT_CONFIRM="${SBGUI_EXIT:-0}"
    nohup "$ROOT/target/release/sbgui.exe" >/dev/null 2>&1 & )
  sleep 7
  sleep "${wait_extra:-0}"
  powershell -NoProfile -ExecutionPolicy Bypass -File capture.ps1 -OutFile "$out"
}

shoot dashboard    1080x760  01-dashboard.png          8
shoot dashboard    1080x1120 02-dashboard-tall.png     0
shoot subscriptions 1080x760 03-subscriptions.png      0
shoot proxies      1080x900  04-proxies.png            0
shoot rules        1080x900  05-rules.png              0
shoot connections  1080x760  06-connections.png        0
shoot logs         1080x900  07-logs.png               0
shoot settings     1080x1180 08-settings.png           0
SBGUI_EXIT=1 shoot dashboard 1080x760 09-exit-confirm.png 0
echo ALL_DONE
