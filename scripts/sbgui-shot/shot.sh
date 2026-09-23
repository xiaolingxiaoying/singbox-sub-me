#!/usr/bin/env bash
# Capture every sbgui page headlessly in a Linux Docker container, so no GUI
# window ever opens on a real desktop. Run it from Git Bash (where Docker
# Desktop's CLI works); the WSL distro has no docker on its PATH.
#
#   REPO='C:\path\to\repo' bash scripts/sbgui-shot/shot.sh
#
# Screenshots land in $OUT_REL inside the repository so they can be diffed.
# Override PAGES / SIZES / OUT_REL to narrow a run. Set DEMO_CORE to a Linux
# sing-box path inside the container (e.g. /src/.scratch/sbgui-demo-bin/sing-box)
# to seed a subscription, a running core and traffic before shooting.
set -euo pipefail

IMAGE=sbgui-shot
DOCKER=${DOCKER:-docker}
REPO_WIN=${REPO_WIN:-$(pwd -W 2>/dev/null || pwd)}
OUT_REL=${OUT_REL:-.scratch/sbgui-shots}
PAGES=${PAGES:-dashboard subscriptions proxies rules connections logs settings about}
SIZES=${SIZES:-1440x900}

mkdir -p "$OUT_REL"
CTX=$(mktemp -d)
echo "building $IMAGE ..."
"$DOCKER" build -q -t "$IMAGE" -f "$REPO_WIN/scripts/sbgui-shot/Dockerfile" "$CTX" >/dev/null
rmdir "$CTX"

echo "capturing ${PAGES} at ${SIZES} ..."
# Git Bash rewrites arguments that look like Unix paths into Windows paths,
# which turns `-w /src` into a directory inside Git's own install folder.
MSYS_NO_PATHCONV=1 "$DOCKER" run --rm \
  -v "$REPO_WIN:/src" \
  -v sbgui-shot-target:/target \
  -w /src \
  -e OUT="/src/$OUT_REL" \
  -e PAGES="$PAGES" \
  -e SIZES="$SIZES" \
  -e DEMO_CORE="${DEMO_CORE:-}" \
  -e SBGUI_SHOW_STOP_CONFIRM="${SBGUI_SHOW_STOP_CONFIRM:-}" \
  -e SBGUI_SHOW_EXIT_CONFIRM="${SBGUI_SHOW_EXIT_CONFIRM:-}" \
  -e SBGUI_LANG="${SBGUI_LANG:-}" \
  -e SBGUI_SETTINGS_SECTION="${SBGUI_SETTINGS_SECTION:-}" \
  -e CLIENT_POLL_TRACE="${CLIENT_POLL_TRACE:-}" \
  "$IMAGE" bash -c 'tr -d "\r" < /src/scripts/sbgui-shot/inside.sh > /tmp/inside.sh; exec bash /tmp/inside.sh'

echo "=== $OUT_REL ==="
ls -la "$OUT_REL"
