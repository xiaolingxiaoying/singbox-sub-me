#!/usr/bin/env bash
# Seed sbgui's data directory so the client has something to show.
#
# The engine parses profiles.toml and cache/active-config.json at startup, so
# the subscription, rules and overview pages fill in without a core. The core
# binary is optional: pass its path as $1 and the node, connection and log
# pages fill in too, because sing-box then answers on the clash_api port.
#
# Nothing here is product code and nothing writes outside the data directory.
set -euo pipefail

DIR=${SBGUI_DIR:-$HOME/.config/sbgui}
# Callers that copy this script elsewhere (the container does) must say where
# the fixture lives; deriving it from $0 silently pointed at the wrong place.
FIXTURE=${FIXTURE_DIR:-$(dirname "$0")/fixture}
[ -f "$FIXTURE/active-config.json" ] || { echo "fixture not found: $FIXTURE/active-config.json"; exit 1; }
NOW=$(date +%s)

mkdir -p "$DIR/cache/profiles" "$DIR/core"
cp "$FIXTURE/active-config.json" "$DIR/cache/active-config.json"

# Two profiles: the active one carries traffic metadata headers on purpose so
# the usage line, the reset countdown and the "never updated" case are all on
# screen at once.
cat >"$DIR/profiles.toml" <<EOF
active = "家庭实验室"

[[profiles]]
name = "家庭实验室"
url = "http://127.0.0.1:8099/sub/demo/sing-box.json"
source = "http://127.0.0.1:8099/sub/demo"
last_updated = $NOW

[[profiles]]
name = "备用出口"
url = "http://127.0.0.1:8099/sub/backup/sing-box.json"
source = "http://127.0.0.1:8099/sub/backup"
last_updated = 0
EOF

for name in "家庭实验室" "备用出口"; do
  id=$(printf '%s' "$name" | sha256sum | cut -d' ' -f1)
  cp "$FIXTURE/active-config.json" "$DIR/cache/profiles/$id.json"
done

cat >"$DIR/settings.toml" <<EOF
mirror = ""
core_version = ""
auto_update_minutes = 0
mixed_port = 2080
test_url = "http://www.gstatic.com/generate_204"
auto_start = $( [ -n "${1:-}" ] && echo true || echo false )
auto_system_proxy = false
traffic_mode = "SystemProxy"
EOF

if [ -n "${1:-}" ]; then
  cp "$1" "$DIR/core/sing-box"
  chmod +x "$DIR/core/sing-box"
  "$DIR/core/sing-box" version | head -1
fi

echo "seeded $DIR (core: ${1:-none})"
find "$DIR" -maxdepth 3 -type f | sed "s|$DIR/|  |"
