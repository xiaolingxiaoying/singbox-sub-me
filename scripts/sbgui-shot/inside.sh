#!/usr/bin/env bash
# Runs inside the container. Two modes:
#   default          loop over PAGES x SIZES, spawning one throwaway X server each
#   --capture ...    one page under an existing Xvfb: start sbgui, screenshot, stop it
# The second mode is what xvfb-run invokes, so the capture happens on the same
# display the window is mapped to.
set -uo pipefail

if [[ "${1:-}" == "--capture" ]]; then
  page=$2 size=$3 out=$4 bin=$5
  w=${size%x*}
  # Every page runs in the same container, and killing sbgui does not kill the
  # sing-box it spawned: the survivor holds the mixed port and the next page
  # then auto-starts into "端口已被占用" and shows an empty table.
  pkill -f '/.config/sbgui/core/sing-box' 2>/dev/null
  pkill sbgui 2>/dev/null
  sleep 1
  "$bin" >/tmp/sbgui.log 2>&1 &
  pid=$!
  # GPUI needs a mapped window plus a few snapshot ticks (the engine publishes
  # at ~4/s) before the frame is worth looking at.
  sleep 6
  sleep 9
  # Known gap: the connections table body is still never captured. clash_api
  # lists only live connections, and every request the fixture can make either
  # fails (the demo nodes point at closed local ports) or finishes before the
  # frame is shot; a held-open CONNECT did not show up either. Verifying that
  # table needs a reachable outbound, not more screenshot plumbing.
  if ! import -window root "$out/$size-$page.png" 2>>/tmp/import.log; then
    echo "CAPTURE FAILED for $size-$page"
    tail -5 /tmp/import.log 2>/dev/null | sed 's/^/    import: /'
    kill "$pid" 2>/dev/null
    exit 1
  fi
  # Keep the client's own output: an empty page with no log behind it is a
  # guess, not a diagnosis.
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
  pkill -f '/.config/sbgui/core/sing-box' 2>/dev/null
  cp /tmp/sbgui.log "$out/$size-$page.sbgui.log" 2>/dev/null
  echo "shot $size-$page.png"
  tail -3 /tmp/sbgui.log 2>/dev/null | sed 's/^/    sbgui: /'
  exit 0
fi

export CARGO_TARGET_DIR=/target
cd /src
cargo build -p sbgui || exit 1
BIN=$CARGO_TARGET_DIR/debug/sbgui
HERE=$(dirname "$0")
mkdir -p "$OUT"

# With a core path present, seed a subscription, a running config and the core
# itself so the dense pages have something to draw. Without it, every surface
# renders its empty state and the layout work cannot be reviewed.
if [ -n "${DEMO_CORE:-}" ] && [ -f "$DEMO_CORE" ]; then
  tr -d '\r' < /src/scripts/sbgui-shot/seed-demo.sh > /tmp/seed.sh
  SBGUI_DIR=${SBGUI_DIR:-$HOME/.config/sbgui} FIXTURE_DIR=/src/scripts/sbgui-shot/fixture \
    bash /tmp/seed.sh "$DEMO_CORE" 2>&1 | tee "$OUT/seed.log"
else
  echo "no DEMO_CORE: dense pages will show their empty states"
fi
# Prove the seed survived into the run instead of trusting it: an empty profile
# list here means the harness is shooting empty states again.
if [ -n "${DEMO_CORE:-}" ]; then
  {
    echo "SBGUI_DIR=${SBGUI_DIR:-$HOME/.config/sbgui}"
    ls -la "${SBGUI_DIR:-$HOME/.config/sbgui}" 2>&1
    echo "--- profiles.toml ---"
    cat "${SBGUI_DIR:-$HOME/.config/sbgui}/profiles.toml" 2>&1
    echo "--- settings.toml ---"
    cat "${SBGUI_DIR:-$HOME/.config/sbgui}/settings.toml" 2>&1
  } >"$OUT/seed-verify.log" 2>&1
  # A demo run that silently fell back to empty states is worse than a failed
  # one: the pictures look plausible.
  if [ ! -s "${SBGUI_DIR:-$HOME/.config/sbgui}/profiles.toml" ]; then
    echo "SEED FAILED: profiles.toml absent; screenshots would show empty states"
    cat "$OUT/seed.log" 2>/dev/null
    exit 1
  fi
fi

for size in $SIZES; do
  for page in $PAGES; do
    SBGUI_PAGE=$page SBGUI_SIZE=$size SBGUI_SETTINGS_SECTION="${SBGUI_SETTINGS_SECTION:-}" \
      xvfb-run -a -s "-screen 0 ${size}x24" \
      bash "$HERE/inside.sh" --capture "$page" "$size" "$OUT" "$BIN"
  done
done

# One extra frame with the exit-confirmation overlay, which is otherwise only
# reachable by closing the window.
SBGUI_SHOW_EXIT_CONFIRM=1 SBGUI_PAGE=dashboard \
  xvfb-run -a -s "-screen 0 1440x900x24" \
  bash "$HERE/inside.sh" --capture exit-confirm 1440x900 "$OUT" "$BIN"

echo "=== $OUT ==="
ls -la "$OUT"
# A zero-exit run that produced no pictures is a broken harness, not a pass.
missing=0
for size in $SIZES; do
  for page in $PAGES; do
    f="$OUT/$size-$page.png"
    if [ ! -s "$f" ]; then
      echo "MISSING: $f"
      missing=$((missing + 1))
    fi
  done
done
if [ "$missing" -ne 0 ]; then
  echo "HARNESS FAILED: $missing screenshot(s) missing"
  exit 1
fi
echo "HARNESS OK"
