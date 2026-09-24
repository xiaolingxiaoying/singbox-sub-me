#!/usr/bin/env bash
# Time the mihomo gate on its own, with the environment cargo test gives it.
# `probe-mihomo-geo.sh` showed a GEOIP rule hangs 45s+ with no route, so a gate
# that accepts GEOIP artifacts in under a second is not measuring what its name
# says. Either the DB exists somewhere mihomo looks, or the runs differ somehow.
set -eu
REPO=${REPO:-/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me}
DST=${DST:-$HOME/ws/singbox-sub-me}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/ws/target
# shellcheck disable=SC1090
source "$HOME/bin/sb-cores.env"
mkdir -p "$DST"
tar -C "$REPO" \
  --exclude=./target --exclude='./target-*' --exclude=./.git \
  --exclude='./.reference-*' --exclude=./.tmp-sing-box-yg-research \
  --exclude=./.scratch --exclude=./dist --exclude=./node_modules \
  -cf - . | tar -C "$DST" -xf -
cd "$DST"

echo "=== any geodata reachable from the crate root or the temp dirs? ==="
find "$HOME/ws" -maxdepth 3 \( -name 'geoip*' -o -name '*.mmdb' -o -name 'geosite*' \) -printf '%s %p\n' 2>/dev/null | head
echo "mihomo binary: $MIHOMO_BIN"
"$MIHOMO_BIN" -v | head -1

echo "=== gate, timed ==="
start=$(date +%s)
cargo test --test clash_mihomo -- --ignored --nocapture 2>&1 | grep -v Warning | tail -20
echo "seconds=$(( $(date +%s) - start ))"
