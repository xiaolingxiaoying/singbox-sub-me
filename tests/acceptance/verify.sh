#!/usr/bin/env sh
# This script deliberately uses only the public sbctl CLI and HTTP endpoints.
set -eu

sbctl=${SBCTL_BIN:-/usr/local/bin/sbctl}
work=$(mktemp -d)
trap 'jobs -p | xargs -r kill 2>/dev/null || true; rm -rf "$work"' EXIT
. /etc/os-release
platform=$ID

fail() { echo "acceptance failure: $*" >&2; exit 1; }
contains() { printf '%s' "$1" | grep -F -- "$2" >/dev/null || fail "expected output to contain: $2"; }
root_for() {
  root="$work/$1"
  mkdir -p "$root/etc" "$root/run/systemd/system" "$root/sys/class/net/ens3/statistics" "$root/proc/sys/kernel/random"
  printf 'ID=%s\nVERSION_ID=1\n' "$2" > "$root/etc/os-release"
  printf '100\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
  printf '200\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
  printf 'acceptance-boot\n' > "$root/proc/sys/kernel/random/boot_id"
}
seed_uninstall_fixture() {
  mkdir -p "$root/etc/systemd/system" "$root/etc/nginx" "$root/etc/ufw" "$root/usr/bin"
  printf 'Description=sbctl private subscription service\n' > "$root/etc/systemd/system/sbctl.service"
  printf 'Description=sing-box data plane managed by sbctl\n' > "$root/etc/systemd/system/sing-box.service"
  printf 'sbctl-managed-v1\n' > "$root/var/lib/sbctl/ownership"
  printf 'proxy' > "$root/etc/nginx/nginx.conf"
  printf 'firewall' > "$root/etc/ufw/user.rules"
  printf '#!/bin/sh\nexit 0\n' > "$root/usr/bin/systemctl"
  chmod 0755 "$root/usr/bin/systemctl"
}
fake_sing_box="$work/sing-box"
printf '#!/bin/sh\nexit 0\n' > "$fake_sing_box"
chmod 0755 "$fake_sing_box"

# Fresh direct-mode installation exercises the default five-protocol release artifact.
root_for direct "$platform"
install_output=$("$sbctl" --root "$root" install --subscription-host sub.example.test --interface ens3 --reality-decoy-sni www.cloudflare.com --sing-box-bin "$fake_sing_box" --no-start)
contains "$install_output" 'enabled protocols: vless-reality, vmess-websocket, hysteria2, tuic, anytls'
credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
test -n "$credential" || fail 'direct subscription credential was not persisted'
for protocol in vless-reality vmess-websocket hysteria2 tuic anytls; do
  grep -F -- "$protocol" "$root/etc/sbctl/config.toml" >/dev/null || fail "missing $protocol"
done
python3 - "$root/var/lib/sbctl/artifacts" "$credential" <<'PY'
import json
import pathlib
import sys
import yaml

artifacts = pathlib.Path(sys.argv[1])
credential = sys.argv[2]
sing_box = json.loads((artifacts / "subscription-sing-box.json").read_text())
clash = yaml.safe_load((artifacts / "subscription-clash.yaml").read_text())
uris = (artifacts / "subscription-uri.txt").read_text()
expected = {"vless", "vmess", "hysteria2", "tuic", "anytls"}
actual_json = {node["type"] for node in sing_box["outbounds"]}
actual_yaml = {node["type"] for node in clash["proxies"]}
assert actual_json == expected, actual_json
assert actual_yaml == expected, actual_yaml
for scheme in expected:
    assert f"{scheme}://" in uris, scheme
assert credential not in uris
PY
test ! -e "$root/etc/ufw/user.rules" || fail 'installation changed firewall rules'
certificate_directory="$root/etc/letsencrypt/live/sub.example.test"
mkdir -p "$certificate_directory"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj /CN=sub.example.test -keyout "$certificate_directory/privkey.pem" -out "$certificate_directory/fullchain.pem" >/dev/null 2>&1
"$sbctl" --root "$root" serve &
sleep 1
curl --silent --show-error --insecure --resolve sub.example.test:443:127.0.0.1 "https://sub.example.test/sub/$credential/uri" | grep -F 'vless://' >/dev/null || fail 'direct HTTPS endpoint did not serve the URI subscription'

# Existing deployment rejection must not modify the administrator's data.
root_for existing "$platform"
mkdir -p "$root/etc/sing-box"
printf 'keep me' > "$root/etc/sing-box/config.json"
if "$sbctl" --root "$root" install >"$work/existing.out" 2>"$work/existing.err"; then
  fail 'Existing deployment was accepted'
fi
grep -F 'Existing deployment detected' "$work/existing.err" >/dev/null || fail 'missing Existing deployment diagnostic'
test "$(cat "$root/etc/sing-box/config.json")" = 'keep me' || fail 'Existing deployment was modified'

# IP fallback is a real HTTP endpoint: all formats share its credential and query credentials fail.
root_for fallback "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2080 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com --monthly-traffic-limit 999
credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
test -n "$credential" || fail 'subscription credential was not persisted'
"$sbctl" --root "$root" traffic >/dev/null
printf '130\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '260\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
traffic=$("$sbctl" --root "$root" traffic)
contains "$traffic" 'total: 90 bytes'
contains "$traffic" 'accounting period:'
contains "$("$sbctl" --root "$root" status)" 'total: 90 bytes'
printf '4\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '9\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
printf 'recovered-boot\n' > "$root/proc/sys/kernel/random/boot_id"
contains "$("$sbctl" --root "$root" traffic)" 'total: 90 bytes'
printf '10\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '20\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
contains "$("$sbctl" --root "$root" traffic)" 'total: 107 bytes'
"$sbctl" --root "$root" serve --max-requests 4 &
sleep 1
for path in sing-box.json clash.yaml uri; do
  response=$(curl --silent --show-error --include "http://127.0.0.1:2080/sub/$credential/$path")
  contains "$response" 'HTTP/1.1 200 OK'
  contains "$response" 'subscription-userinfo: upload=36; download=71; total=999; expire='
done
query_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2080/sub/$credential/uri?credential=$credential")
test "$query_status" = 404 || fail 'query-string credential was accepted'

# Reverse-proxy mode must bind loopback, retain the five generated protocols, and serve a cache.
root_for reverse "$platform"
"$sbctl" --root "$root" config init --mode external-proxy --subscription-host sub.example.test --listen-port 2081 --interface ens3 --protocol vless-reality --protocol vmess-websocket --protocol hysteria2 --protocol tuic --protocol anytls --reality-decoy-sni www.cloudflare.com --sing-box-bin "$fake_sing_box"
reverse_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
"$sbctl" --root "$root" serve --max-requests 1 &
sleep 1
curl --silent --show-error --include "http://127.0.0.1:2081/sub/$reverse_credential/uri" | grep -F 'HTTP/1.1 200 OK' >/dev/null || fail 'reverse-proxy endpoint did not respond'

# Update check is read-only; a failed update retains its known-good binaries and rollback point.
root_for update "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2082 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com
mkdir -p "$root/usr/local/bin"
printf 'known-good sbctl' > "$root/usr/local/bin/sbctl"
printf 'known-good sing-box' > "$root/usr/local/bin/sing-box"
printf '{"sbctl":{"version":"0.1.1","sha256":"0000000000000000000000000000000000000000000000000000000000000000"},"sing_box":{"version":"1.12.0","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}' > "$work/manifest.json"
before=$(find "$root" -type f -exec sha256sum {} \; | sort)
"$sbctl" --root "$root" update --check --manifest "$work/manifest.json" >/dev/null
after=$(find "$root" -type f -exec sha256sum {} \; | sort)
test "$before" = "$after" || fail 'update --check changed the host'
if "$sbctl" --root "$root" update --manifest "$work/manifest.json" --sbctl-artifact "$fake_sing_box" --sing-box-artifact "$fake_sing_box" >/dev/null 2>&1; then
  fail 'invalid update artifact was accepted'
fi
test "$(cat "$root/usr/local/bin/sbctl")" = 'known-good sbctl' || fail 'failed update changed sbctl'
test "$(cat "$root/usr/local/bin/sing-box")" = 'known-good sing-box' || fail 'failed update changed sing-box'

# Uninstall preserves unrelated proxy/firewall files by default; --purge only removes sbctl data.
seed_uninstall_fixture
"$sbctl" --root "$root" uninstall >/dev/null
test -f "$root/etc/sbctl/config.toml" || fail 'default uninstall removed persistent data'
test -d "$root/var/backups/sbctl" || fail 'default uninstall did not preserve a backup'
test "$(cat "$root/etc/nginx/nginx.conf")" = proxy || fail 'default uninstall changed proxy configuration'
test "$(cat "$root/etc/ufw/user.rules")" = firewall || fail 'default uninstall changed firewall rules'
seed_uninstall_fixture
"$sbctl" --root "$root" uninstall --purge >/dev/null
test ! -e "$root/var/lib/sbctl" || fail '--purge retained sbctl data'
test "$(cat "$root/etc/nginx/nginx.conf")" = proxy || fail '--purge changed proxy configuration'
test "$(cat "$root/etc/ufw/user.rules")" = firewall || fail '--purge changed firewall rules'

echo "sbctl acceptance passed on $(. /etc/os-release; printf '%s %s' "$ID" "$VERSION_ID")"
