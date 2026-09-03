#!/usr/bin/env sh
# This script deliberately uses only the public sbctl CLI and HTTP endpoints.
set -eu

sbctl=${SBCTL_BIN:-/usr/local/bin/sbctl}
work=$(mktemp -d)
trap 'jobs -p | xargs -r kill 2>/dev/null || true; rm -rf "$work"' EXIT
. /etc/os-release
platform=$ID
. /usr/local/lib/sbctl-acceptance/fixture.sh

fail() { echo "acceptance failure: $*" >&2; exit 1; }
contains() { printf '%s' "$1" | grep -F -- "$2" >/dev/null || fail "expected output to contain: $2"; }
fake_sing_box="$work/sing-box"
printf '#!/bin/sh\nexit 0\n' > "$fake_sing_box"
chmod 0755 "$fake_sing_box"

# Fresh direct-mode installation exercises the default five-protocol release artifact.
fixture_root_for direct "$platform"
install_output=$("$sbctl" --root "$root" install --subscription-host sub.example.test --interface ens3 --reality-decoy-sni www.cloudflare.com --sing-box-bin "$fake_sing_box" --no-start)
contains "$install_output" 'enabled protocols: vless-reality, vmess-websocket, hysteria2, tuic, anytls'
credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
test -n "$credential" || fail 'direct subscription credential was not persisted'
for protocol in vless-reality vmess-websocket hysteria2 tuic anytls; do
  grep -F -- "$protocol" "$root/etc/sbctl/config.toml" >/dev/null || fail "missing $protocol"
done
test ! -e "$root/etc/ufw/user.rules" || fail 'installation changed firewall rules'
fixture_seed_certificate sub.example.test
"$sbctl" --root "$root" serve &
sleep 1
curl --silent --show-error --insecure --resolve sub.example.test:443:127.0.0.1 "https://sub.example.test/sub/$credential/uri" | grep -F 'vless://' >/dev/null || fail 'direct HTTPS endpoint did not serve the URI subscription'

# Existing deployment rejection must not modify the administrator's data.
fixture_root_for existing "$platform"
mkdir -p "$root/etc/sing-box"
printf 'keep me' > "$root/etc/sing-box/config.json"
if "$sbctl" --root "$root" install >"$work/existing.out" 2>"$work/existing.err"; then
  fail 'Existing deployment was accepted'
fi
grep -F 'Existing deployment detected' "$work/existing.err" >/dev/null || fail 'missing Existing deployment diagnostic'
test "$(cat "$root/etc/sing-box/config.json")" = 'keep me' || fail 'Existing deployment was modified'

# IP fallback is a real HTTP endpoint: all formats share its credential and query credentials fail.
fixture_root_for fallback "$platform"
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

# Reverse-proxy mode must bind loopback and return all formats from the five-node set.
fixture_root_for reverse "$platform"
"$sbctl" --root "$root" config init --mode external-proxy --subscription-host sub.example.test --listen-port 2081 --interface ens3 --protocol vless-reality --protocol vmess-websocket --protocol hysteria2 --protocol tuic --protocol anytls --reality-decoy-sni www.cloudflare.com --sing-box-bin "$fake_sing_box"
reverse_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
"$sbctl" --root "$root" serve --max-requests 4 &
sleep 1
for format in sing-box.json clash.yaml uri; do
  curl --silent --show-error --dump-header "$work/$format.headers" --output "$work/$format.body" "http://127.0.0.1:2081/sub/$reverse_credential/$format"
  grep -F 'HTTP/1.1 200 OK' "$work/$format.headers" >/dev/null || fail "reverse-proxy $format did not respond"
  grep -Fi 'subscription-userinfo:' "$work/$format.headers" >/dev/null || fail "reverse-proxy $format lacks traffic metadata"
done
python3 - "$work/sing-box.json.body" "$work/clash.yaml.body" "$work/uri.body" <<'PY'
import json
import pathlib
import sys
import yaml

sing_box = json.loads(pathlib.Path(sys.argv[1]).read_text())
clash = yaml.safe_load(pathlib.Path(sys.argv[2]).read_text())
uris = pathlib.Path(sys.argv[3]).read_text()
expected = {"vless", "vmess", "hysteria2", "tuic", "anytls"}
assert {node["type"] for node in sing_box["outbounds"]} == expected
assert {node["type"] for node in clash["proxies"]} == expected
for scheme in expected:
    assert f"{scheme}://" in uris, scheme
PY
proxy_credential=$(sed -n 's|^vless://\([^@]*\)@.*|\1|p' "$work/uri.body")
test -n "$proxy_credential" || fail 'VLESS proxy credential was not emitted'
proxy_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2081/sub/$proxy_credential/uri")
test "$proxy_status" = 404 || fail 'proxy credential authorized a subscription'

# Update check is read-only; a failed health check restores the known-good binaries and keeps a rollback point.
fixture_root_for update "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2082 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com
mkdir -p "$root/usr/local/bin"
printf 'known-good sbctl' > "$root/usr/local/bin/sbctl"
printf 'known-good sing-box' > "$root/usr/local/bin/sing-box"
candidate_digest=$(sha256sum "$fake_sing_box" | awk '{print $1}')
printf '{"sbctl":{"version":"0.1.1","sha256":"%s"},"sing_box":{"version":"1.12.0","sha256":"%s"}}' "$candidate_digest" "$candidate_digest" > "$work/manifest.json"
before=$(find "$root" -type f -exec sha256sum {} \; | sort)
"$sbctl" --root "$root" update --check --manifest "$work/manifest.json" >/dev/null
after=$(find "$root" -type f -exec sha256sum {} \; | sort)
test "$before" = "$after" || fail 'update --check changed the host'
mkdir -p "$root/usr/bin"
printf '#!/bin/sh\nexit 1\n' > "$root/usr/bin/systemctl"
chmod 0755 "$root/usr/bin/systemctl"
if "$sbctl" --root "$root" update --manifest "$work/manifest.json" --sbctl-artifact "$fake_sing_box" --sing-box-artifact "$fake_sing_box" >/dev/null 2>&1; then
  fail 'update with a failed service health check was accepted'
fi
test "$(cat "$root/usr/local/bin/sbctl")" = 'known-good sbctl' || fail 'failed update changed sbctl'
test "$(cat "$root/usr/local/bin/sing-box")" = 'known-good sing-box' || fail 'failed update changed sing-box'
test -d "$root/var/lib/sbctl/rollback" || fail 'failed update did not keep a rollback point'

# Uninstall preserves unrelated proxy/firewall files by default; --purge only removes sbctl data.
fixture_seed_uninstall
"$sbctl" --root "$root" uninstall >/dev/null
test -f "$root/etc/sbctl/config.toml" || fail 'default uninstall removed persistent data'
test -d "$root/var/backups/sbctl" || fail 'default uninstall did not preserve a backup'
test "$(cat "$root/etc/nginx/nginx.conf")" = proxy || fail 'default uninstall changed proxy configuration'
test "$(cat "$root/etc/ufw/user.rules")" = firewall || fail 'default uninstall changed firewall rules'
fixture_seed_uninstall
"$sbctl" --root "$root" uninstall --purge >/dev/null
test ! -e "$root/var/lib/sbctl" || fail '--purge retained sbctl data'
test "$(cat "$root/etc/nginx/nginx.conf")" = proxy || fail '--purge changed proxy configuration'
test "$(cat "$root/etc/ufw/user.rules")" = firewall || fail '--purge changed firewall rules'

echo "sbctl acceptance passed on $(. /etc/os-release; printf '%s %s' "$ID" "$VERSION_ID")"
