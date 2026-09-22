#!/usr/bin/env bash
# Sync the Windows working tree into WSL and run the server test suites there.
# Nothing is executed on the Windows host beyond cargo's own compile steps.
set -euo pipefail
SRC=/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me
DST=$HOME/sbctl-refactor
export CARGO_TARGET_DIR=$HOME/refactor-target

cd "$SRC"
mkdir -p "$DST"
find . -name '*.rs' -newer "$DST/.synced" -print -quit | grep -q . || {
  echo "no newer sources; reusing $DST"
}
tar --exclude=./target --exclude='./target-*' --exclude=./.git --exclude=./.qoder \
    --exclude=./.kilo --exclude=./node_modules --exclude=./dist \
    --exclude=./.tmp-sing-box-yg-research --exclude=./.reference-* \
    --exclude=./sbctl-linux-amd64 --exclude=./.scratch \
    -cf - . | tar -xf - -C "$DST"
date > "$DST/.synced"
echo "synced to $DST (target: $CARGO_TARGET_DIR)"
cd "$DST"
# sbgui is excluded: GPUI needs Linux desktop build deps (libfontconfig1-dev et al.)
# that this rig does not install, and the GUI is verified in the Windows VM anyway.
echo "=== clippy (Linux, excluding the GUI) ==="
cargo clippy --workspace --exclude sbgui --all-targets --all-features -- -D warnings
echo "=== tests (Linux, excluding the GUI) ==="
cargo test --workspace --exclude sbgui --features sbctl/test-signing
