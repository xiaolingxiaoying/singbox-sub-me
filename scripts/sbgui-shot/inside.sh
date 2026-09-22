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
  "$bin" >/tmp/sbgui.log 2>&1 &
  pid=$!
  # GPUI needs a mapped window plus a few snapshot ticks (the engine publishes
  # at ~4/s) before the frame is worth looking at.
  sleep 9
  if ! import -window root "$out/$size-$page.png" 2>>/tmp/import.log; then
    echo "CAPTURE FAILED for $size-$page"
    tail -5 /tmp/import.log 2>/dev/null | sed 's/^/    import: /'
    kill "$pid" 2>/dev/null
    exit 1
  fi
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
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

for size in $SIZES; do
  for page in $PAGES; do
    SBGUI_PAGE=$page SBGUI_SIZE=$size \
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
