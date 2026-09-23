#!/usr/bin/env sh
# This script deliberately uses only the public sbctl CLI and HTTP endpoints.
set -eu

sbctl=${SBCTL_BIN:-/usr/local/bin/sbctl}
work=$(mktemp -d)
trap 'jobs -p | xargs -r kill 2>/dev/null || true; rm -rf "$work"' EXIT
. /etc/os-release
platform=$ID
acceptance_lib=${SBCTL_ACCEPTANCE_LIB:-/usr/local/lib/sbctl-acceptance/fixture.sh}
. "$acceptance_lib"

# The acceptance image installs nginx for the reverse-proxy chain, and the
# distro service auto-starts on TCP 80, which would starve the Direct mode
# socket activation below. Stop it; the reverse-proxy section runs its own
# private instance on 127.0.0.1:8080 instead.
systemctl stop nginx >/dev/null 2>&1 || service nginx stop >/dev/null 2>&1 || true

fail() { echo "acceptance failure: $*" >&2; exit 1; }
contains() {
  printf '%s' "$1" | grep -F -- "$2" >/dev/null && return 0
  echo "acceptance output did not contain: $2" >&2
  printf '%s\n' "$1" | grep -i 'subscription-userinfo' >&2 || true
  fail "expected output to contain: $2"
}
fake_sing_box="$work/sing-box"
printf '#!/bin/sh\nexit 0\n' > "$fake_sing_box"
chmod 0755 "$fake_sing_box"

# Fresh direct-mode installation exercises the default five-protocol release artifact.
fixture_root_for direct "$platform"
install_output=$("$sbctl" --root "$root" install --subscription-host sub.example.test --interface ens3 --reality-decoy-sni www.cloudflare.com --sing-box-bin "$fake_sing_box" --no-start)
contains "$install_output" '启用协议: vless-reality, vmess-websocket, hysteria2, tuic, anytls'
credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
test -n "$credential" || fail 'direct subscription credential was not persisted'
for protocol in vless-reality vmess-websocket hysteria2 tuic anytls; do
  grep -F -- "$protocol" "$root/etc/sbctl/config.toml" >/dev/null || fail "missing $protocol"
done
test ! -e "$root/etc/ufw/user.rules" || fail 'installation changed firewall rules'
grep -F 'ListenStream=80' "$root/etc/systemd/system/sbctl-http.socket" >/dev/null || fail 'Direct socket unit lacks ListenStream=80'
grep -F 'ListenStream=443' "$root/etc/systemd/system/sbctl-http.socket" >/dev/null || fail 'Direct socket unit lacks ListenStream=443'
grep -F 'Requires=sbctl-http.socket' "$root/etc/systemd/system/sbctl.service" >/dev/null || fail 'sbctl.service does not depend on the Direct socket'
grep -F 'User=sbctl' "$root/etc/systemd/system/sbctl.service" >/dev/null || fail 'sbctl.service does not run as sbctl'
grep -F 'User=sing-box' "$root/etc/systemd/system/sing-box.service" >/dev/null || fail 'sing-box.service does not run as sing-box'
fixture_seed_certificate sub.example.test
"$sbctl" --root "$root" certificate verify >/dev/null
"$sbctl" --root "$root" accounting-reset >/dev/null
# systemd-socket-activate plays the role of the sbctl-http.socket unit: it
# binds TCP 80 and 443 and passes both listeners to a non-root-required
# equivalent of the sbctl.service handoff through LISTEN_FDS.
systemd-socket-activate -l 0.0.0.0:80 -l 0.0.0.0:443 -- "$sbctl" --root "$root" serve >"$work/direct.out" 2>"$work/direct.err" &
direct_pid=$!
sleep 1
curl --silent --show-error --retry 5 --retry-connrefused --retry-delay 1 --insecure --resolve sub.example.test:443:127.0.0.1 "https://sub.example.test/sub/$credential/uri" | grep -F 'vless://' >/dev/null || fail 'direct HTTPS endpoint did not serve the URI subscription'
token="acceptance-token"
mkdir -p "$root/var/lib/sbctl/acme-webroot/.well-known/acme-challenge"
printf 'challenge-body' > "$root/var/lib/sbctl/acme-webroot/.well-known/acme-challenge/$token"
challenge=$(curl --silent --show-error --retry 5 --retry-connrefused --retry-delay 1 "http://127.0.0.1:80/.well-known/acme-challenge/$token")
test "$challenge" = 'challenge-body' || fail 'Direct HTTP-01 challenge endpoint did not serve the token'
kill "$direct_pid" 2>/dev/null || true
wait "$direct_pid" 2>/dev/null || true
sleep 1

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
"$sbctl" --root "$root" accounting-reset >/dev/null
printf '130\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '260\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
"$sbctl" --root "$root" accounting-reset >/dev/null
traffic=$("$sbctl" --root "$root" traffic)
contains "$traffic" 'total: 90 bytes'
contains "$traffic" 'accounting period:'
contains "$("$sbctl" --root "$root" status)" 'total: 90 bytes'
printf '4\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '9\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
printf 'recovered-boot\n' > "$root/proc/sys/kernel/random/boot_id"
"$sbctl" --root "$root" accounting-reset >/dev/null
contains "$("$sbctl" --root "$root" traffic)" 'total: 90 bytes'
printf '10\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '20\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
contains "$("$sbctl" --root "$root" traffic)" 'total: 107 bytes'
state_before=$(stat -c '%Y %s' "$root/var/lib/sbctl/state.json")
"$sbctl" --root "$root" serve --max-requests 24 &
serve_pid=$!
sleep 1
# Every artifact-backed format in the subscription matrix shares the traffic
# metadata; the QR and index routes serve scannable/HTML bodies instead.
for path in sing-box.json sing-box-full.json sing-box-1.12.json sing-box-1.13.json sing-box-1.14.json clash.yaml clash-1.18.yaml uri uri.txt shadowrocket.txt; do
  response=$(curl --silent --show-error --include "http://127.0.0.1:2080/sub/$credential/$path")
  contains "$response" 'HTTP/1.1 200 OK'
  contains "$response" 'subscription-userinfo: upload=71; download=36; total=999; expire='
  contains "$response" '; profile-update-interval=24'
done
for path in "qr/uri" index; do
  response=$(curl --silent --show-error --include "http://127.0.0.1:2080/sub/$credential/$path")
  contains "$response" 'HTTP/1.1 200 OK'
done
curl --silent "http://127.0.0.1:2080/sub/$credential/qr/clash.yaml" | grep -F '<svg' >/dev/null || fail 'QR route did not serve an SVG document'
curl --silent "http://127.0.0.1:2080/sub/$credential/index" | grep -F 'sbctl 订阅中心' >/dev/null || fail 'index page did not render the overview title'
curl --silent "http://127.0.0.1:2080/sub/$credential/sing-box-full.json" | python3 -c 'import json,sys; c=json.load(sys.stdin); assert "tun" in str(c["inbounds"][0]["type"]), c["inbounds"]; assert c["experimental"]["clash_api"]["external_controller"], c' || fail 'full sing-box profile lacks tun/clash_api'
shadowrocket_body=$(curl --silent "http://127.0.0.1:2080/sub/$credential/shadowrocket.txt")
printf '%s' "$shadowrocket_body" | python3 -c 'import base64,sys; base64.b64decode(sys.stdin.read().strip())' || fail 'shadowrocket artifact is not valid base64'
query_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2080/sub/$credential/uri?credential=$credential")
test "$query_status" = 404 || fail 'query-string credential was accepted'
wrong_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2080/sub/wrong-credential/uri")
test "$wrong_status" = 404 || fail 'invalid credential was not a uniform 404'
bogus_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2080/sub/$credential/bogus")
test "$bogus_status" = 404 || fail 'unknown subscription format path was not a uniform 404'
unknown_version_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2080/sub/$credential/sing-box-1.09.json")
test "$unknown_version_status" = 404 || fail 'unknown sing-box version was not a uniform 404'
trailing_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2080/sub/$credential/uri/extra")
test "$trailing_status" = 404 || fail 'trailing path segment was not a uniform 404'
# The server only exits at its request cap, which exceeds the requests above;
# stop it explicitly before continuing.
kill "$serve_pid" 2>/dev/null || true
wait "$serve_pid" 2>/dev/null || true
test "$(stat -c '%Y %s' "$root/var/lib/sbctl/state.json")" = "$state_before" || fail 'subscription reads changed accounting state'

# A broken accounting state degrades the response instead of taking the
# subscription offline: the real artifact is served (200) with no fabricated
# `subscription-userinfo`, and the diagnostic stays redacted. Invalid
# credentials remain a uniform 404.
fixture_root_for unavailable "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2087 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com
unavailable_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
"$sbctl" --root "$root" serve --max-requests 2 >"$work/unavailable.out" 2>"$work/unavailable.err" &
sleep 1
unavailable_response=$(curl --silent --show-error --include "http://127.0.0.1:2087/sub/$unavailable_credential/uri")
contains "$unavailable_response" 'HTTP/1.1 200 OK'
contains "$unavailable_response" 'vless://'
if printf '%s' "$unavailable_response" | grep -Fi 'subscription-userinfo:' >/dev/null; then
  fail 'missing state fabricated subscription-userinfo metadata'
fi
test ! -s "$work/unavailable.out" || fail 'subscription wrote diagnostics to stdout'
unavailable_404=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2087/sub/wrong-credential/uri")
test "$unavailable_404" = 404 || fail 'invalid credential was not 404 even with missing state'
grep -F -- "$unavailable_credential" "$work/unavailable.err" >/dev/null && fail 'degraded-state diagnostic leaked the Subscription credential'
wait
test -s "$work/unavailable.err" || fail 'degraded-state response did not write a redacted diagnostic'

# Corrupted accounting state degrades the same way: artifact-only 200.
fixture_root_for corrupt "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2088 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com
corrupt_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
mkdir -p "$root/var/lib/sbctl"
printf 'not json\n' > "$root/var/lib/sbctl/state.json"
"$sbctl" --root "$root" serve --max-requests 2 >"$work/corrupt.out" 2>"$work/corrupt.err" &
sleep 1
corrupt_response=$(curl --silent --show-error --include "http://127.0.0.1:2088/sub/$corrupt_credential/uri")
contains "$corrupt_response" 'HTTP/1.1 200 OK'
contains "$corrupt_response" 'vless://'
if printf '%s' "$corrupt_response" | grep -Fi 'subscription-userinfo:' >/dev/null; then
  fail 'corrupt state fabricated subscription-userinfo metadata'
fi
curl --silent --output /dev/null "http://127.0.0.1:2088/sub/$corrupt_credential/uri"
grep -F -- "$corrupt_credential" "$work/corrupt.err" >/dev/null && fail 'corrupt-state diagnostic leaked the Subscription credential'
wait

# A future first anchor keeps traffic metering live and uses that anchor as the next reset.
fixture_root_for pending "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2089 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com --accounting-policy anchored-month --accounting-timezone UTC --anchored-reset-at "$(date -d '+2 months' +%Y-%m-%dT%H:%M)"
pending_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
"$sbctl" --root "$root" accounting-reset >/dev/null
"$sbctl" --root "$root" serve --max-requests 2 &
sleep 1
pending_response=$(curl --silent --show-error --include "http://127.0.0.1:2089/sub/$pending_credential/uri")
contains "$pending_response" 'HTTP/1.1 200 OK'
contains "$pending_response" 'subscription-userinfo: upload=0; download=0; total=0; expire='
contains "$pending_response" '; profile-update-interval=24'
curl --silent --output /dev/null "http://127.0.0.1:2089/sub/wrong-credential/uri"

# status --json reports the current period without exposing the credential.
status_json=$("$sbctl" --root "$root" status --json)
contains "$status_json" '"configured": true'
if printf '%s' "$status_json" | grep -F -- '"accounting_period": "pending-first-reset"' >/dev/null; then
  fail 'future first anchor suppressed the active accounting period'
fi
contains "$status_json" '"total": 0'
if printf '%s' "$status_json" | grep -F -- "$pending_credential" >/dev/null; then
  fail 'status --json exposed the Subscription credential'
fi

# Explicit traffic corrections are administrator-authorized writers: they show a
# summary, never touch the sysfs counters, and reject invalid targets.
fixture_root_for correction "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2086 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com
"$sbctl" --root "$root" accounting-reset >/dev/null
printf '130\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '260\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
"$sbctl" --root "$root" accounting-reset >/dev/null
summary=$("$sbctl" --root "$root" traffic set-used --bytes 5000)
contains "$summary" 'accounting period:'
contains "$summary" 'current total: 90 bytes'
contains "$summary" 'target received: 30 bytes'
contains "$summary" 'target transmitted: 60 bytes'
contains "$summary" 'target total: 5000 bytes'
contains "$("$sbctl" --root "$root" traffic)" 'total: 5000 bytes'
printf '134\n' > "$root/sys/class/net/ens3/statistics/rx_bytes"
printf '265\n' > "$root/sys/class/net/ens3/statistics/tx_bytes"
contains "$("$sbctl" --root "$root" traffic)" 'total: 5009 bytes'
"$sbctl" --root "$root" traffic set-used --rx 500 --tx 300 >/dev/null
contains "$("$sbctl" --root "$root" traffic)" 'received: 500 bytes'
contains "$("$sbctl" --root "$root" traffic)" 'transmitted: 300 bytes'
test "$(cat "$root/sys/class/net/ens3/statistics/rx_bytes")" = 134 || fail 'direction correction modified the sysfs counter'
test "$(cat "$root/sys/class/net/ens3/statistics/tx_bytes")" = 265 || fail 'direction correction modified the sysfs counter'
if "$sbctl" --root "$root" traffic set-used --bytes 100 >/dev/null 2>&1; then
  fail 'total correction below the current total was accepted'
fi
if "$sbctl" --root "$root" traffic set-used --bytes 100 --rx 5 >/dev/null 2>&1; then
  fail 'conflicting traffic correction arguments were accepted'
fi

# Anchored-month starts metering before a future first reset; DST collisions are rejected.
fixture_root_for anchored "$platform"
if "$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2084 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com --accounting-policy anchored-month --accounting-timezone America/New_York --anchored-reset-at 2024-03-10T02:30 >"$work/dst.out" 2>&1; then
  fail 'nonexistent DST anchored reset was accepted'
fi
grep -F 'does not exist in the accounting timezone' "$work/dst.out" >/dev/null || fail 'missing nonexistent DST diagnostic'
if "$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2085 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com --accounting-policy anchored-month --accounting-timezone America/New_York --anchored-reset-at 2024-11-03T01:30 >"$work/dst2.out" 2>&1; then
  fail 'ambiguous DST anchored reset was accepted'
fi
grep -F 'ambiguous in the accounting timezone' "$work/dst2.out" >/dev/null || fail 'missing ambiguous DST diagnostic'
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2083 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com --accounting-policy anchored-month --accounting-timezone UTC --anchored-reset-at "$(date -d '+2 months' +%Y-%m-%dT%H:%M)"
"$sbctl" --root "$root" accounting-reset >/dev/null
if "$sbctl" --root "$root" traffic | grep -F -- 'accounting period: pending-first-reset' >/dev/null; then
  fail 'future first anchor suppressed traffic metering'
fi
contains "$("$sbctl" --root "$root" traffic)" 'total: 0 bytes'

# Reverse-proxy mode must bind loopback and return all formats from the five-node set.
fixture_root_for reverse "$platform"
"$sbctl" --root "$root" config init --mode external-proxy --subscription-host sub.example.test --listen-port 2081 --interface ens3 --protocol vless-reality --protocol vmess-websocket --protocol hysteria2 --protocol tuic --protocol anytls --reality-decoy-sni www.cloudflare.com --sing-box-bin "$fake_sing_box"
reverse_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' "$root/etc/sbctl/config.toml")
"$sbctl" --root "$root" accounting-reset >/dev/null
"$sbctl" --root "$root" serve --max-requests 16 &
reverse_serve_pid=$!
sleep 1
for format in sing-box.json sing-box-full.json sing-box-1.14.json clash.yaml clash-1.18.yaml uri uri.txt shadowrocket.txt; do
  curl --silent --show-error --dump-header "$work/$format.headers" --output "$work/$format.body" "http://127.0.0.1:2081/sub/$reverse_credential/$format"
  grep -F 'HTTP/1.1 200 OK' "$work/$format.headers" >/dev/null || fail "reverse-proxy $format did not respond"
  grep -Fi 'subscription-userinfo:' "$work/$format.headers" >/dev/null || fail "reverse-proxy $format lacks traffic metadata"
done
for path in "qr/uri" index; do
  status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2081/sub/$reverse_credential/$path")
  test "$status" = 200 || fail "reverse-proxy $path did not respond"
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
# The full client profile must carry the tun inbound and the clash API that
# dashboards (and sbtui) rely on.
python3 - "$work/sing-box-full.json.body" <<'PY'
import json, pathlib, sys
full = json.loads(pathlib.Path(sys.argv[1]).read_text())
assert full["inbounds"] and full["inbounds"][0]["type"] == "tun"
assert full["experimental"]["clash_api"]["external_controller"]
PY
# The reverse-proxy chain must also work through a real reverse proxy: nginx
# fronts the loopback endpoint the way an administrator's public entry would.
if command -v nginx >/dev/null 2>&1; then
  mkdir -p "$work/nginx/logs" "$work/nginx/temp" "$work/nginx/conf.d"
  cat > "$work/nginx/proxy.conf" <<NGINX
error_log logs/error.log;
pid logs/nginx.pid;
events { }
http {
  access_log logs/access.log;
  client_body_temp_path temp/client_body;
  proxy_temp_path temp/proxy;
  server {
    listen 127.0.0.1:8080;
    location /sub/ {
      proxy_pass http://127.0.0.1:2081;
      proxy_set_header Host \$host;
    }
  }
}
NGINX
  nginx -c "$work/nginx/proxy.conf" -p "$work/nginx" -g 'daemon off;' &
  nginx_pid=$!
  sleep 1
  for format in sing-box-full.json clash.yaml shadowrocket.txt; do
    status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:8080/sub/$reverse_credential/$format")
    test "$status" = 200 || fail "nginx reverse proxy did not serve $format"
  done
  index_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:8080/sub/$reverse_credential/index")
  test "$index_status" = 200 || fail 'nginx reverse proxy did not serve the index page'
  kill "$nginx_pid" 2>/dev/null || true
fi
proxy_credential=$(sed -n 's|^vless://\([^@]*\)@.*|\1|p' "$work/uri.body")
test -n "$proxy_credential" || fail 'VLESS proxy credential was not emitted'
proxy_status=$(curl --silent --output /dev/null --write-out '%{http_code}' "http://127.0.0.1:2081/sub/$proxy_credential/uri")
test "$proxy_status" = 404 || fail 'proxy credential authorized a subscription'
# Stop the reverse-proxy fixture listener so verify-real's installs can bind
# their own subscription ports afterwards.
kill "$reverse_serve_pid" 2>/dev/null || true
wait "$reverse_serve_pid" 2>/dev/null || true

# Update check is read-only; a failed health check restores the known-good binaries and keeps a rollback point.
fixture_root_for update "$platform"
"$sbctl" --root "$root" config init --mode ip-fallback --subscription-host 127.0.0.1 --http-port 2082 --interface ens3 --protocol vless-reality --reality-decoy-sni www.cloudflare.com
mkdir -p "$root/usr/local/bin"
printf 'known-good sbctl' > "$root/usr/local/bin/sbctl"
printf 'known-good sing-box' > "$root/usr/local/bin/sing-box"
candidate_digest=$(sha256sum "$fake_sing_box" | awk '{print $1}')
printf '{"schema":1,"sbctl":{"version":"0.1.1","sha256":"%s"},"sing_box":{"version":"1.12.0","sha256":"%s"},"sing_box_compatibility":[{"min":"1.12.0","max":"1.12.0"}]}' "$candidate_digest" "$candidate_digest" > "$work/manifest.unsigned.json"
"$sbctl" release sign \
  --manifest "$work/manifest.unsigned.json" \
  --private-key /usr/local/lib/sbctl-acceptance/dev-signing-key.hex \
  --output "$work/manifest.json"
before=$(find "$root" -type f -exec sha256sum {} \; | sort)
"$sbctl" --root "$root" update --check --manifest "$work/manifest.json" >/dev/null
after=$(find "$root" -type f -exec sha256sum {} \; | sort)
test "$before" = "$after" || fail 'update --check changed the host'
mkdir -p "$root/usr/bin"
printf '#!/bin/sh\nexit 1\n' > "$root/usr/bin/systemctl"
chmod 0755 "$root/usr/bin/systemctl"
# The failure output is the diagnosis: without it a rollback that was never
# created and an update that aborted before it got that far look identical.
update_stderr=$("$sbctl" --root "$root" update --manifest "$work/manifest.json" --sbctl-artifact "$fake_sing_box" --sing-box-artifact "$fake_sing_box" 2>&1 >/dev/null) && {
  fail 'update with a failed service health check was accepted'
}
echo "failed-update stderr: $update_stderr"
test "$(cat "$root/usr/local/bin/sbctl")" = 'known-good sbctl' || fail 'failed update changed sbctl'
test "$(cat "$root/usr/local/bin/sing-box")" = 'known-good sing-box' || fail 'failed update changed sing-box'
# The rollback point lives where backups live (`ROLLBACK_ROOT` in src/update.rs),
# and it has to hold something: an empty directory would prove only that the
# code reached the mkdir, not that a known-good state was saved.
test -d "$root/var/backups/sbctl/rollback" || fail 'failed update did not keep a rollback point'
test -n "$(find "$root/var/backups/sbctl/rollback" -type f)" || fail 'the rollback point holds no files'

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
