#!/usr/bin/env bash
# Build the Linux binaries the systemd acceptance suite consumes:
#   SBCTL_ARTIFACT        release binary            -> .scratch/acceptance-bin/sbctl-linux-amd64
#   SBCTL_TEST_ARTIFACT   test-signing fixture     -> .scratch/acceptance-bin/sbctl-test-signing
#   SBCTUI_ARTIFACT       release client binary    -> .scratch/acceptance-bin/sbtui-linux-amd64
# The fixture build is never published (see docs/release-signing.md).
#
# All three are mandatory: `tests/acceptance/run.sh` runs under `set -eu` and
# `verify-client.sh` needs a real client to prove orphan reclamation and the TUN
# wiring, so a build script that stops at two artifacts hands the suite a
# missing-variable failure that looks like a broken container.
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

# SRC_REV=HEAD builds exactly what is committed instead of the working tree.
# Worth having: with another process editing a crate, a dirty tree can fail the
# build for reasons unrelated to the artifacts under test, and an acceptance run
# should describe a commit rather than a moment.
if [ -n "${SRC_REV:-}" ]; then
  DST="$HOME/ws/acceptance-src"
  export CARGO_TARGET_DIR="$HOME/ws/acceptance-target"
  rm -rf "$DST"
  mkdir -p "$DST"
  git -C "$SRC" archive "$SRC_REV" | tar -x -C "$DST"
  echo "exported revision $SRC_REV into $DST"
else
  mkdir -p "$DST"
  tar -C "$SRC" \
    --exclude=./target --exclude='./target-*' --exclude=./.git \
    --exclude='./.reference-*' --exclude=./.scratch --exclude=./dist \
    --exclude=./.tmp-sing-box-yg-research --exclude=./node_modules \
    -cf - . | tar -C "$DST" -xf -
fi

cd "$DST"
echo "=== release build (no test-signing) ==="
cargo build --release --locked -p sbctl --no-default-features
echo "=== fixture build (test-signing) ==="
cargo build --release --locked -p sbctl --features test-signing --target-dir target-fixtures
echo "=== client build (verify-client.sh) ==="
cargo build --release --locked -p sbtui

out="$SRC/.scratch/acceptance-bin"
mkdir -p "$out"
cp "$CARGO_TARGET_DIR/release/sbctl" "$out/sbctl-linux-amd64"
cp target-fixtures/release/sbctl "$out/sbctl-test-signing"
cp "$CARGO_TARGET_DIR/release/sbtui" "$out/sbtui-linux-amd64"
echo "=== artifacts ==="
ls -l "$out"
for binary in sbctl-linux-amd64 sbctl-test-signing sbtui-linux-amd64; do
  file "$out/$binary" | sed 's/^/  /'
done
