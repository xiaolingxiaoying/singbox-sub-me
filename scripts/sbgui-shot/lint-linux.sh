#!/usr/bin/env bash
# CI's Linux gate, run locally in Docker: the workspace is clippy-checked with
# -D warnings on Linux, which catches platform-specific breakage a Windows host
# build cannot see (cfg-gated imports are the usual one).
set -euo pipefail

IMAGE=sbgui-shot
DOCKER=${DOCKER:-docker}
REPO_WIN=${REPO_WIN:-$(pwd -W 2>/dev/null || pwd)}

CTX=$(mktemp -d)
"$DOCKER" build -q -t "$IMAGE" -f "$REPO_WIN/scripts/sbgui-shot/Dockerfile" "$CTX" >/dev/null
rmdir "$CTX"

MSYS_NO_PATHCONV=1 "$DOCKER" run --rm \
  -v "$REPO_WIN:/src" \
  -v sbgui-shot-target:/target \
  -w /src \
  -e CARGO_TARGET_DIR=/target \
  "$IMAGE" \
  bash -c 'tr -d "\r" < /src/scripts/sbgui-shot/lint-inside.sh > /tmp/l.sh; exec bash /tmp/l.sh'
