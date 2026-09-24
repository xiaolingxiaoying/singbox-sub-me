#!/usr/bin/env bash
# G11: prove that a forced exit still reaches sbtui's teardown.
#
# The bug was silent: `q` restored the machine-wide proxy because it is a key the
# loop handles, while `kill`, a closed terminal or a session logout were never
# watched for, so the proxy stayed pointed at a mixed port with no listener.
#
# A unit test cannot see this wiring (an async loop with a terminal attached), so
# this gate runs the real binary under a pty in WSL and signals it.
#
#   Usage (Git Bash on the Windows host):
#     wsl.exe -d Ubuntu-22.04 -- bash -lc 'bash /mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me/scripts/dev/wsl-signal-exit.sh'
#
set -euo pipefail

SRC=${SRC:-/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me}
DST=${DST:-$HOME/ws/singbox-sub-me}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/ws/target}

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

# Announce the branch that was chosen, so a run that silently took a fallback is
# visible in the log rather than looking like a pass.
echo "== signal-exit gate =="
echo "source: $SRC"
echo "build tree: $DST"
echo "target dir: $CARGO_TARGET_DIR"

mkdir -p "$DST"
tar -C "$SRC" \
  --exclude=./target --exclude='./target-*' --exclude=./.git \
  --exclude=./.reference-* --exclude=./.scratch --exclude=./.tmp-sing-box-yg-research \
  -cf - . | tar -C "$DST" -xf -
cd "$DST"
cargo build -p sbtui
BIN=$CARGO_TARGET_DIR/debug/sbtui
[ -x "$BIN" ] || { echo "FAIL: $BIN was not built"; exit 1; }

# A private data directory: the client writes its lock file and settings there,
# and nothing about the run can touch the distro's own configuration.
#
# The signal has to go to sbtui itself. An earlier draft matched `pgrep -f`
# against the wrapper's own command line and so signalled `script`, which catches
# SIGTERM and exits 0 on its own — a pass that measured the wrong process. The
# control case below is what caught it: KILL gave 137 while TERM gave 0 for the
# same, wrong target. Match the child's exact command line, and refuse to run a
# case whose target pid is the wrapper's.
run_case() { # <label> <signal> <expected exit code>
  local label=$1 signal=$2 expected=$3
  local home out code pid wrapper tries
  home=$(mktemp -d)
  out=$(mktemp)
  # `script -e` propagates the child's exit status, including 128+signum when it
  # dies from a signal we did not catch.
  HOME="$home" XDG_CONFIG_HOME="$home/.config" \
    setsid script -qeefc "$BIN" /dev/null >"$out" 2>&1 &
  wrapper=$!
  pid=""
  tries=0
  while [ "$tries" -lt 60 ]; do
    pid=$(pgrep -x "$(basename "$BIN")" | head -1 || true)
    [ -n "$pid" ] && break
    tries=$((tries + 1))
    sleep 0.25
  done
  if [ -z "$pid" ]; then
    echo "FAIL [$label]: sbtui never started under the pty; output was:"
    tail -20 "$out"
    kill -9 "$wrapper" 2>/dev/null || true
    rm -rf "$home" "$out"
    return 1
  fi
  if [ "$pid" = "$wrapper" ]; then
    echo "FAIL [$label]: the target pid is the wrapper, so this case would measure the wrong process"
    kill -9 "$pid" 2>/dev/null || true
    rm -rf "$home" "$out"
    return 1
  fi
  echo "case $label: wrapper=$wrapper sbtui=$pid, signalling sbtui with -$signal"
  # Give the loop a tick to install its handler and paint a frame.
  sleep 2
  kill -"$signal" "$pid" 2>/dev/null || true
  # Poll for the child to go; a handler that never fires would hang this gate
  # forever, and a hung gate reads like a slow pass.
  tries=0
  while [ "$tries" -lt 40 ] && kill -0 "$pid" 2>/dev/null; do
    tries=$((tries + 1))
    sleep 0.5
  done
  if kill -0 "$pid" 2>/dev/null; then
    echo "case $label: sbtui survived kill -$signal for 20s -> treated as a failure"
    kill -9 "$pid" 2>/dev/null || true
    code=255
  else
    wait "$wrapper" && code=0 || code=$?
  fi
  echo "case $label: kill -$signal -> exit $code (expected $expected)"
  rm -rf "$home" "$out"
  [ "$code" = "$expected" ]
}

failures=0

# The control first: SIGKILL cannot be caught, so the harness must be able to see
# a signal death at all. If this reports 0, the pass below would mean nothing.
if run_case "control SIGKILL (uncatchable, must look like a signal death)" KILL 137; then
  echo "PASS control: the harness can see an uncaught signal"
else
  echo "FAIL control: cannot trust the other cases until this one passes"
  failures=$((failures + 1))
fi

if run_case "SIGTERM (service stop, kill, logout)" TERM 0; then
  echo "PASS SIGTERM: sbtui exited on its own, so the teardown ran"
else
  echo "FAIL SIGTERM: the signal was not caught, or the process died in it"
  failures=$((failures + 1))
fi

if run_case "SIGHUP (closed terminal)" HUP 0; then
  echo "PASS SIGHUP: a closed terminal leaves through the same door as q"
else
  echo "FAIL SIGHUP: the process died without running the teardown"
  failures=$((failures + 1))
fi

echo "== signal-exit gate: $failures failing case(s) =="
[ "$failures" = 0 ]
