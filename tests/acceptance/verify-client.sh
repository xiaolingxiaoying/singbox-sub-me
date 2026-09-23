#!/usr/bin/env sh
# The client-side L3 assertions. They drive the terminal client (sbtui) against
# a stand-in core, so the two guarantees that only exist once the real process
# tree is up can be checked without a real sing-box binary:
#
#   1. Orphan reaping. `kill -9` the client and no core it started survives
#      holding the mixed port. That is the Linux `PR_SET_PDEATHSIG` guard in
#      crates/client-core/src/core.rs; its unit test (orphan_guard.rs) proves
#      the mechanism, this proves it end to end inside the acceptance image.
#   2. TUN wiring. With `traffic_mode = "Tun"` the client starts the core with a
#      tun inbound; the interface and the policy routing the client asked for
#      are actually present (`ip a` / `ip rule`).
#
# The rate-limit flood assertion lives in verify.sh and is deliberately not
# repeated here.
set -eu

sbtui_mount=${SBTUI_BIN:-/opt/sbtui-client/sbtui}
work=$(mktemp -d)
tun_if=tun0
script_pid=""
client_pid=""
cleanup() {
  [ -n "$client_pid" ] && kill -9 "$client_pid" 2>/dev/null || true
  [ -n "$script_pid" ] && kill "$script_pid" 2>/dev/null || true
  pkill -f "$work" 2>/dev/null || true
  pkill -f '/opt/sbtui-client/sbtui' 2>/dev/null || true
  ip rule del from 172.19.0.1 table 2022 2>/dev/null || true
  ip link del "$tun_if" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

fail() { echo "client acceptance failure: $*" >&2; exit 1; }

# The artifact is mounted read-only from the host. A bind mount from a Windows
# filesystem can drop the executable bit, so copy it into the container and set
# the mode rather than trusting the mount's.
test -f "$sbtui_mount" || fail "sbtui artifact not found at $sbtui_mount"
sbtui="$work/sbtui"
cp "$sbtui_mount" "$sbtui"
chmod 0755 "$sbtui"

# The stand-in core. It reads the *client-generated* active-config.json, so what
# it sets up is what the client asked for: the mixed listen port, or the tun
# address plus the auto_route policy rule. It answers the three calls the engine
# makes (`version`, `check -c`, `run -c`) and serves the clash_api the readiness
# wait polls, so the client reaches "core running" the same way it would with
# the real binary.
fake_core="$work/fake-core"
cat >"$fake_core" <<'FAKE_CORE'
#!/usr/bin/env python3
"""Stand-in for sing-box used by tests/acceptance/verify-client.sh."""
import json
import socket
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def die(message):
    sys.stderr.write("fake-core: " + message + "\n")
    sys.exit(1)


def config_path(argv):
    try:
        return argv[argv.index("-c") + 1]
    except (ValueError, IndexError):
        die("expected -c <config> in: " + " ".join(argv))


def load(path):
    try:
        with open(path) as handle:
            return json.load(handle)
    except Exception as error:  # noqa: BLE001 - a stub reports and dies
        die("cannot read %s: %s" % (path, error))


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def _json(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _stream(self, body):
        raw = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        # Two complete samples: the client reads the second as the measurement.
        self.wfile.write(raw + raw)
        self.wfile.flush()

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path == "/version":
            self._json({"version": "1.14.1", "meta": True})
        elif path == "/proxies":
            self._json({"proxies": {}})
        elif path == "/connections":
            self._json({"uploadTotal": 0, "downloadTotal": 0, "connections": []})
        elif path == "/configs":
            self._json({"mode": "rule", "log-level": "info"})
        elif path == "/traffic":
            self._stream('{"up":0,"down":0}\n')
        elif path == "/memory":
            self._stream('{"inuse":0}\n')
        else:
            self._json({"message": "not found"}, 404)

    def do_PATCH(self):
        self._json({})

    def do_PUT(self):
        self._json({})

    def do_DELETE(self):
        self._json({})


def run_ip(args):
    result = subprocess.run(["ip"] + args, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(
            "fake-core: ip %s failed: %s%s\n"
            % (" ".join(args), result.stdout, result.stderr)
        )
    return result.returncode == 0


def bring_up_tun(inbound):
    name = inbound.get("interface_name") or "tun0"
    if not run_ip(["tuntap", "add", "dev", name, "mode", "tun"]):
        die("cannot create tun %s (is /dev/net/tun available?)" % name)
    run_ip(["link", "set", name, "up"])
    addresses = [a for a in inbound.get("address", []) if ":" not in a]
    for address in addresses:
        run_ip(["addr", "add", address, "dev", name])
    if inbound.get("auto_route"):
        # auto_route is what makes sing-box install a policy rule sending the
        # tun's own traffic back through it; reproduce that as a real `ip rule`.
        for address in addresses:
            run_ip(["rule", "add", "from", address.split("/")[0], "table", "2022"])
            run_ip(["route", "add", "default", "dev", name, "table", "2022"])
    sys.stderr.write("fake-core: tun %s up, addresses %s\n" % (name, addresses))
    return name


def bind_mixed(inbound):
    host = inbound.get("listen") or "127.0.0.1"
    port = int(inbound["listen_port"])
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((host, port))
    listener.listen(64)
    sys.stderr.write("fake-core: mixed inbound bound on %s:%d\n" % (host, port))
    return listener


def main():
    argv = sys.argv[1:]
    if not argv:
        die("no subcommand")
    command = argv[0]
    if command == "version":
        print("sing-box version 1.14.1")
        return
    if command == "check":
        load(config_path(argv))
        return
    if command != "run":
        die("unknown subcommand %s" % command)

    config = load(config_path(argv))
    tun = next(
        (i for i in config.get("inbounds", []) if i.get("type") == "tun"), None
    )
    # Keep a reference to the mixed listener alive for the life of the process.
    mixed = None
    if tun is not None:
        bring_up_tun(tun)
    else:
        for inbound in config.get("inbounds", []):
            if inbound.get("type") == "mixed":
                mixed = bind_mixed(inbound)

    controller = (
        config.get("experimental", {}).get("clash_api", {}).get("external_controller", "")
    )
    if controller:
        host, _, port = controller.rpartition(":")
        server = ThreadingHTTPServer((host or "127.0.0.1", int(port)), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
    sys.stderr.write("fake-core: ready (mixed=%s)\n" % bool(mixed))
    sys.stderr.flush()
    while True:
        time.sleep(3600)


if __name__ == "__main__":
    main()
FAKE_CORE
chmod 0755 "$fake_core"

# Seed one client data directory. `settings::data_dir()` is `$XDG_CONFIG_HOME/sbtui`
# (or `~/.config/sbtui`); the core path is fixed at `<dir>/core/sing-box`. The
# active local profile needs a cached sing-box config, because that cache is what
# the engine starts the core from.
seed_client() {
  dir=$1
  mode=$2
  port=$3
  mkdir -p "$dir/core" "$dir/cache/profiles"
  cp "$fake_core" "$dir/core/sing-box"
  chmod 0755 "$dir/core/sing-box"
  cat >"$dir/settings.toml" <<EOF
mirror = ""
core_version = ""
auto_update_minutes = 0
mixed_port = $port
test_url = "http://aliyun.com/generate_204"
auto_start = true
auto_system_proxy = false
traffic_mode = "$mode"
EOF
  cat >"$dir/profiles.toml" <<'EOF'
active = "local"

[[profiles]]
name = "local"
url = ""
source = ""
last_updated = 0
EOF
  id=$(printf 'local' | sha256sum | cut -d' ' -f1)
  cat >"$dir/cache/profiles/$id.json" <<'EOF'
{"log":{"level":"warn"},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}
EOF
}

# Launch the TUI under a pty (`script`) and wait for the process to exist. The
# `exec` keeps the shell from wrapping the client, so the pid we find is sbtui's
# and `kill -9` lands on the process that armed the orphan guard.
start_client() {
  cfg=$1
  log=$2
  XDG_CONFIG_HOME="$cfg" script -qec "exec $sbtui" /dev/null >"$log" 2>&1 &
  script_pid=$!
  client_pid=""
  for _ in $(seq 1 50); do
    client_pid=$(pgrep -f "^$sbtui" | head -n 1 || true)
    [ -n "$client_pid" ] && break
    sleep 0.2
  done
  [ -n "$client_pid" ] || fail "the terminal client did not start (log: $log)"
}

mixed_port=2180

# --- assertion 1: orphan reaping ---------------------------------------------
cfg="$work/orphan"
seed_client "$cfg/sbtui" SystemProxy "$mixed_port"
start_client "$cfg" "$work/orphan.out"

# The core must be holding the port first, or a later "port free" would pass
# without the guard having done anything.
held=false
for _ in $(seq 1 100); do
  if ss -ltnp 2>/dev/null | grep -E ":${mixed_port}([^0-9]|$)" >/dev/null; then
    held=true
    break
  fi
  sleep 0.2
done
if [ "$held" != true ]; then
  cat "$cfg/sbtui/cache/core.log" >&2 2>/dev/null || true
  fail "the core never bound the mixed port $mixed_port; the orphan assertion would be vacuous"
fi
echo "orphan: before kill -9, the core holds the mixed port:"
ss -ltnp | grep -E ":${mixed_port}([^0-9]|$)" || true

kill -9 "$client_pid"
client_pid=""

reaped=false
for _ in $(seq 1 50); do
  if ! ss -ltnp 2>/dev/null | grep -E ":${mixed_port}([^0-9]|$)" >/dev/null; then
    reaped=true
    break
  fi
  sleep 0.2
done
if [ "$reaped" != true ]; then
  echo "orphan: after kill -9, the mixed port is still held:" >&2
  ss -ltnp >&2 || true
  fail "a core survived kill -9 of the client and still holds the mixed port"
fi
pgrep -f "$work/orphan" >/dev/null 2>&1 \
  && fail 'an orphaned core process survived the client that started it'
echo "orphan: after kill -9, no process holds the mixed port and no core survived"

# --- assertion 2: TUN wiring -------------------------------------------------
cfg="$work/tun"
seed_client "$cfg/sbtui" Tun 2181
start_client "$cfg" "$work/tun.out"

# The core creates the link first and assigns the address and the auto_route
# rule moments later, so waiting on the link alone races the setup: the link
# can exist with no address yet. Poll for the address and the rule the client
# asked for, then assert on them.
wired=false
for _ in $(seq 1 100); do
  if ip a show "$tun_if" 2>/dev/null | grep -F '172.19.0.1/30' >/dev/null \
    && ip rule 2>/dev/null | grep -F '172.19.0.1' >/dev/null; then
    wired=true
    break
  fi
  sleep 0.2
done
if [ "$wired" != true ]; then
  echo "tun: core log tail:" >&2
  cat "$cfg/sbtui/cache/core.log" >&2 2>/dev/null || true
  echo "tun: ip a show $tun_if:" >&2
  ip a show "$tun_if" >&2 2>/dev/null || true
  echo "tun: ip rule:" >&2
  ip rule >&2 || true
  fail "the client's tun wiring never came up (interface, address and auto_route rule)"
fi

echo "tun: ip a show $tun_if"
ip a show "$tun_if" || true
echo "tun: ip rule"
ip rule || true

ip a show "$tun_if" | grep -F '172.19.0.1/30' >/dev/null \
  || fail 'the tun interface lacks the address the client asked for'
ip rule | grep -F '172.19.0.1' >/dev/null \
  || fail 'the policy routing the client asked for (auto_route) is missing from ip rule'

# The client's own runtime configuration is the request that produced the wiring
# above; assert it directly so the evidence is not only the stand-in's word.
python3 - "$cfg/sbtui/cache/active-config.json" <<'PY'
import json
import sys

config = json.load(open(sys.argv[1]))
inbounds = config.get("inbounds", [])
assert inbounds and inbounds[0].get("type") == "tun", inbounds
assert inbounds[0].get("auto_route") is True, inbounds[0]
assert "172.19.0.1/30" in inbounds[0].get("address", []), inbounds[0]
PY

kill -9 "$client_pid" 2>/dev/null || true
client_pid=""

echo "client acceptance passed"
