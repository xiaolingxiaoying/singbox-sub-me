#!/usr/bin/env bash
# Sync the Windows working tree into the WSL filesystem, then run the same
# gates as the Linux CI job. Everything is compiled and executed inside WSL;
# the Windows host only serves the source mount.
#
# Usage (from PowerShell):
#   wsl -d Ubuntu-22.04 -- bash /mnt/c/.../scripts/dev/wsl-gate.sh
#
# Narrow a run while iterating:
#   TEST_ARGS='--test cli update' SKIP_CLIPPY=1 bash scripts/dev/wsl-gate.sh
#
# The GUI needs the X11/fontconfig/vulkan development packages. They are not
# installed by default here, so sbgui is excluded unless INCLUDE_GUI=1.
set -euo pipefail

SRC=${SRC:-/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me}
DST=${DST:-$HOME/ws/singbox-sub-me}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/ws/target}
TEST_ARGS=${TEST_ARGS:-}
SKIP_CLIPPY=${SKIP_CLIPPY:-0}
SKIP_TESTS=${SKIP_TESTS:-0}

exclude=()
if [ "${INCLUDE_GUI:-0}" != "1" ]; then
  exclude=(--exclude sbgui)
fi

# `wsl -- bash script.sh` is a non-login shell, so it never reads ~/.cargo/env
# the way `wsl -e bash -lc` does. Without this the gate dies at its first cargo
# line with "cargo: command not found", which reads like a broken toolchain.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

mkdir -p "$DST"
synced_at=$(date +%s)
tar -C "$SRC" \
  --exclude=./target --exclude='./target-*' --exclude=./.git \
  --exclude=./.reference-* --exclude=./.tmp-sing-box-yg-research \
  --exclude=./.scratch --exclude=./dist --exclude=./node_modules \
  --exclude=./.zcode --exclude=./.kilo --exclude=./.qoder \
  -cf - . | tar -C "$DST" -xf -
echo "synced to $DST in $(( $(date +%s) - synced_at ))s (target: $CARGO_TARGET_DIR)"
cd "$DST"

echo "=== fmt ==="
cargo fmt --all -- --check

if [ "$SKIP_CLIPPY" != "1" ]; then
  echo "=== clippy ==="
  cargo clippy --workspace "${exclude[@]}" --all-targets --all-features -- -D warnings
fi

if [ "$SKIP_TESTS" != "1" ]; then
  echo "=== tests ==="
  # shellcheck disable=SC2086
  cargo test --workspace "${exclude[@]}" --features sbctl/test-signing $TEST_ARGS
fi

echo "wsl gate passed"
