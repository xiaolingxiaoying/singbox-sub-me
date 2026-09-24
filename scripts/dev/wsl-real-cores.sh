#!/usr/bin/env bash
# L2 leg, real-kernel half: run the per-minor `sing-box check` matrix and the
# mihomo gate against real binaries.
#
# These tests are `#[ignore]`d because they need actual kernels, and since
# Phase 0.5 they assert `checked == SING_BOX_VERSION_PROFILES.len()` - a missing
# binary is a hard failure that reads like a generator bug. Fetch the binaries
# first with scripts/dev/fetch-sing-box-cores.sh.
#
# Run from the Windows side by path (a `bash -c` string gets its $vars eaten by
# Git Bash, and /mnt/... paths get rewritten without MSYS_NO_PATHCONV):
#   MSYS_NO_PATHCONV=1 wsl -d Ubuntu-22.04 -- bash <repo>/scripts/dev/wsl-real-cores.sh
set -eu

REPO=${REPO:-/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me}
DST=${DST:-$HOME/ws/singbox-sub-me}
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/ws/target}

# shellcheck disable=SC1090
source "$HOME/bin/sb-cores.env"

# Same sync the daily gate uses, so the matrix runs the tree you are looking at.
mkdir -p "$DST"
tar -C "$REPO" \
  --exclude=./target --exclude='./target-*' --exclude=./.git \
  --exclude=./.reference-* --exclude=./.tmp-sing-box-yg-research \
  --exclude=./.scratch --exclude=./dist --exclude=./node_modules \
  --exclude=./.zcode --exclude=./.kilo --exclude=./.qoder \
  -cf - . | tar -C "$DST" -xf -
cd "$DST"

echo "=== sing-box band (five real cores, per minor) ==="
cargo test --test version_profiles -- --ignored --nocapture

echo "=== mihomo gate (pinned core) ==="
# Needs GitHub: `mihomo -t` downloads geoip.metadb to evaluate inline GEOIP
# rules, so an unreachable network shows up as a rejected artifact.
if [ -n "${MIHOMO_BIN:-}" ] && [ -x "${MIHOMO_BIN:-}" ]; then
  cargo test --test clash_mihomo -- --ignored --nocapture
else
  echo "MIHOMO_BIN missing: $MIHOMO_BIN" >&2
  exit 1
fi

echo "real-core matrix passed"
