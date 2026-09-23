#!/usr/bin/env bash
# Build the two Linux binaries the systemd acceptance suite consumes:
#   SBCTL_ARTIFACT        release binary            -> .scratch/acceptance-bin/sbctl-linux-amd64
#   SBCTL_TEST_ARTIFACT   test-signing fixture     -> .scratch/acceptance-bin/sbctl-test-signing
# The fixture build is never published (see docs/release-signing.md).
#
# Run inside WSL. The Windows tree is synced into the Linux filesystem first so
# the compiler never touches /mnt/c, and the outputs are copied back only at the
# end because Docker Desktop cannot bind a WSL path from the Windows CLI.
set -euo pipefail

SRC=${SRC:-/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me}
DST=${DST:-$HOME/ws/singbox-sub-me}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/ws/target}

# Non-login shells do not read ~/.cargo/env, and this script is often invoked
# straight through `wsl -- bash …`.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

mkdir -p "$DST"
tar -C "$SRC" \
  --exclude=./target --exclude='./target-*' --exclude=./.git \
  --exclude='./.reference-*' --exclude=./.scratch --exclude=./dist \
  --exclude=./.tmp-sing-box-yg-research --exclude=./node_modules \
  -cf - . | tar -C "$DST" -xf -

cd "$DST"
echo "=== release build (no test-signing) ==="
cargo build --release --locked -p sbctl --no-default-features
echo "=== fixture build (test-signing) ==="
cargo build --release --locked -p sbctl --features test-signing --target-dir target-fixtures

out="$SRC/.scratch/acceptance-bin"
mkdir -p "$out"
cp "$CARGO_TARGET_DIR/release/sbctl" "$out/sbctl-linux-amd64"
cp target-fixtures/release/sbctl "$out/sbctl-test-signing"
echo "=== artifacts ==="
ls -l "$out"
