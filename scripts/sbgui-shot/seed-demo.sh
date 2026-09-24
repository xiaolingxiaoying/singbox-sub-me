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

# sing-box resolves a relative cache_file.path against the process's working
# directory, which under the harness is the /src bind mount. The core then dies
# on "initialize cache-file: timeout" before it ever listens, so pin the path
# inside the data directory instead of trusting the cwd.
seed_config() {
  sed 's|"path": "cache.db"|"path": "'"$DIR"'/cache.db"|' \
    "$FIXTURE/active-config.json" >"$1"
}
seed_config "$DIR/cache/active-config.json"

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
  seed_config "$DIR/cache/profiles/$id.json"
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

# The override page reads a real file, and its name is the sha256 of the profile
# name, so the harness cannot point at it by hand. Gated behind
# SBGUI_SEED_OVERRIDE so every earlier screenshot run stays byte-identical.
# Same three fragments the terminal client's fixtures use: one that wins the rule
# order, one switched off, one that reaches for the client's own fields.
case "${SBGUI_SEED_OVERRIDE:-}" in
  "") ;;
  fragments)
    oid=$(printf '%s' "家庭实验室" | sha256sum | cut -d' ' -f1)
    mkdir -p "$DIR/overrides"
    cat >"$DIR/overrides/$oid.json" <<'JSON'
{"fragments":[
  {"id":"private-direct","label":"内网直连","enabled":true,
   "overlay":{"route":{"rules":[{"action":"direct","ip_cidr":["10.0.0.0/8"]},
                                {"action":"direct","domain_suffix":["internal.example"]}]}}},
  {"id":"custom-dns","label":"自建 DNS","enabled":false,
   "overlay":{"dns":{"servers":["223.5.5.5"],"final":"local"}}},
  {"id":"take-over","label":"接管控制通道","enabled":true,
   "overlay":{"experimental":{"clash_api":{"secret":"OVERRIDE-MUST-NOT-PRINT-77aa"}},
              "inbounds":[{"type":"mixed","listen_port":1080}],
              "route":{"auto_detect_interface":false}}}
]}
JSON
    ;;
  broken)
    oid=$(printf '%s' "家庭实验室" | sha256sum | cut -d' ' -f1)
    mkdir -p "$DIR/overrides"
    printf '{ "route": {"rules": }\n' >"$DIR/overrides/$oid.json"
    ;;
  bare)
    # A bare object that `sing-box check` accepts: `dns.servers` wants objects
    # with an address, so the shape the unit fixtures use would make the demo
    # core refuse to start and the frame would show a failure nobody seeded on
    # purpose.
    oid=$(printf '%s' "家庭实验室" | sha256sum | cut -d' ' -f1)
    mkdir -p "$DIR/overrides"
    printf '{"log":{"level":"debug","timestamp":false}}\n' >"$DIR/overrides/$oid.json"
    ;;
  *)
    echo "unknown SBGUI_SEED_OVERRIDE=${SBGUI_SEED_OVERRIDE}"
    exit 1
    ;;
esac

echo "seeded $DIR (core: ${1:-none}, override: ${SBGUI_SEED_OVERRIDE:-none})"
find "$DIR" -maxdepth 3 -type f | sed "s|$DIR/|  |"
