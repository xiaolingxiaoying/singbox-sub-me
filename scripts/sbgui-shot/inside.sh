#!/usr/bin/env bash
# Runs inside the container. Two modes:
#   default          loop over PAGES x SIZES, spawning one throwaway X server each
#   --capture ...    one page under an existing Xvfb: start sbgui, screenshot, stop it
# The second mode is what xvfb-run invokes, so the capture happens on the same
# display the window is mapped to.
set -uo pipefail

if [[ "${1:-}" == "--capture" ]]; then
  page=$2 size=$3 out=$4 bin=$5
  # The controller endpoint and secret are regenerated on every launch, so read
  # them back out of the runtime config the client just wrote rather than
  # assuming them. `count` feeds the wait loop; `detail` is what gets reported.
  api_probe() {
    python3 - "$1" <<'PY'
import json, os, sys, urllib.request
cfg = json.load(open(os.path.expanduser("~/.config/sbgui/cache/active-config.json")))
api = cfg["experimental"]["clash_api"]
req = urllib.request.Request("http://%s/connections" % api["external_controller"],
                             headers={"Authorization": "Bearer " + api["secret"]})
try:
    rows = json.load(urllib.request.urlopen(req, timeout=5))["connections"]
except Exception as error:
    print("probe failed: %s" % error)
    sys.exit(0)
if sys.argv[1] == "count":
    print(len(rows))
else:
    print(len(rows), [(c["metadata"]["host"], c["metadata"]["destinationPort"]) for c in rows])
PY
  }
  w=${size%x*}
  # Every page runs in the same container, and killing sbgui does not kill the
  # sing-box it spawned: the survivor holds the mixed port and the next page
  # then auto-starts into "端口已被占用" and shows an empty table.
  pkill -f '/.config/sbgui/core/sing-box' 2>/dev/null
  pkill -f slow_origin.py 2>/dev/null
  pkill sbgui 2>/dev/null
  sleep 1
  "$bin" >/tmp/sbgui.log 2>&1 &
  pid=$!
  # GPUI needs a mapped window plus a few snapshot ticks (the engine publishes
  # at ~4/s) before the frame is worth looking at.
  sleep 6
  if [ "$page" = "connections" ]; then
    # clash_api lists live connections only, so the table body could never be
    # photographed: a request that finished before the frame was already gone,
    # and a tunnel to one of the demo's closed node ports is not a connection.
    # The slow origin keeps requests open while bytes flow. The URL host must be
    # an IP literal: `localhost` is sniffed as a domain, ip_is_private does not
    # match domains, and the request then falls through to the final urltest
    # group and dies on a dial to a node that is not running.
    for port in 8098 8099; do
      python3 /src/scripts/sbgui-shot/fixture/slow_origin.py $port 30 >/tmp/origin-$port.log 2>&1 &
    done
    # Wait for every port this depends on, not just a fixed sleep: python needs
    # a moment to import http.server and bind, and a request fired at a port
    # that is not listening yet still exits 0 — the proxy answers it with an
    # error, so the harness reports success while nothing is in flight.
    for want in 2080 8098 8099; do
      for _ in $(seq 1 30); do
        (exec 3<>/dev/tcp/127.0.0.1/$want) 2>/dev/null && break
        sleep 1
      done
    done
    rm -f /tmp/curl.status
    for url in http://127.0.0.1:8098/ http://127.0.0.1:8099/ http://127.0.0.1:8099/img; do
      (curl -s -x http://127.0.0.1:2080 --max-time 40 "$url" -o /dev/null
       echo "$url exit=$?" >>/tmp/curl.status) &
    done
    # When does the core start reporting the in-flight requests at all? The
    # table only ever shows the last successful poll (CONNECTIONS_EVERY is 2 s),
    # so a frame captured inside that window can legitimately be empty while
    # the api already has rows. Wait for the rows to appear, then sit out two
    # full poll cycles: whatever the frame shows afterwards is the refresh
    # path's answer, not a race between two timers.
    first=none
    for i in $(seq 1 30); do
      n=$(api_probe count)
      if [ "$n" -gt 0 ] 2>/dev/null; then first=$i; break; fi
      sleep 1
    done
    echo "    api first nonzero row at +${first}s after the requests were fired"
    sleep 5
    # Three numbers say which half of the chain is broken: whether the request
    # reached the proxy at all, what the core logged about it, and what the
    # controller answers. Without them an empty table is only a guess. The
    # controller endpoint and secret are regenerated on every launch, so they
    # have to be read back out of the runtime config rather than assumed.
    echo "    curl: $(tr '\n' ' ' </tmp/curl.status 2>/dev/null)"
    echo "    core log: $(tail -4 "$HOME/.config/sbgui/cache/core.log" 2>/dev/null | tr '\n' '|')"
    echo "    api rows: $(api_probe detail)"
  else
    sleep 3
  fi
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
# Windows bind mounts and the container can disagree about file mtimes, and a
# skipped rebuild makes the harness shoot stale pixels that still look
# plausible. Touch the workspace so the pictures always match the source.
touch /src/Cargo.toml /src/Cargo.lock
find /src/crates -name '*.rs' -exec touch {} +
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
    # The import-panel seam opens the panel *and* presses 「添加」, so it must not
    # leak into the ordinary page frames: a subscriptions frame with the panel
    # pushed down is no longer a frame of the subscriptions table.
    SBGUI_PAGE=$page SBGUI_SIZE=$size SBGUI_SETTINGS_SECTION="${SBGUI_SETTINGS_SECTION:-}" \
    SBGUI_SHOW_IMPORT_PANEL= SBGUI_SHOW_URL_EDITOR= \
      xvfb-run -a -s "-screen 0 ${size}x24" \
      bash "$HERE/inside.sh" --capture "$page" "$size" "$OUT" "$BIN"
  done
done

# One extra frame with the exit-confirmation overlay, which is otherwise only
# reachable by closing the window.
SBGUI_SHOW_EXIT_CONFIRM=1 SBGUI_PAGE=dashboard \
  xvfb-run -a -s "-screen 0 1440x900x24" \
  bash "$HERE/inside.sh" --capture exit-confirm 1440x900 "$OUT" "$BIN"

# The settings sections the page loop cannot reach, because picking one is a
# click: `network` carries the ports, `tun` carries the switch that has to be
# grey while the core runs. Both sizes, because the ticket asks for a narrow
# frame of every new interaction, and `SBGUI_SIZE` is what makes the window
# actually that size — the Xvfb screen alone would leave it at its default.
for size in $SIZES; do
  for section in network tun; do
    SBGUI_SETTINGS_SECTION=$section SBGUI_PAGE=settings SBGUI_SIZE=$size SBGUI_SHOW_IMPORT_PANEL= \
      xvfb-run -a -s "-screen 0 ${size}x24" \
      bash "$HERE/inside.sh" --capture "settings-$section" "$size" "$OUT" "$BIN"
  done
done

# The subscription import panel with 「添加」 pressed on an empty field, asked for
# by the operator because it opens the panel on every subscriptions frame.
if [ -n "${SBGUI_SHOW_IMPORT_PANEL:-}" ]; then
  for size in $SIZES; do
    SBGUI_PAGE=subscriptions SBGUI_SIZE=$size SBGUI_SHOW_URL_EDITOR= \
      xvfb-run -a -s "-screen 0 ${size}x24" \
      bash "$HERE/inside.sh" --capture subscriptions-import-panel "$size" "$OUT" "$BIN"
  done
fi

# One profile row's link editor, armed through its own handler so the frame shows
# the prefilled link rather than a hand-drawn mock of it. Needs a seeded profile
# to arm on, which is what DEMO_CORE provides.
if [ -n "${SBGUI_SHOW_URL_EDITOR:-}" ]; then
  for size in $SIZES; do
    SBGUI_PAGE=subscriptions SBGUI_SIZE=$size SBGUI_SHOW_IMPORT_PANEL= \
      xvfb-run -a -s "-screen 0 ${size}x24" \
      bash "$HERE/inside.sh" --capture subscriptions-url-editor "$size" "$OUT" "$BIN"
  done
fi

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
