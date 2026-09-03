#!/usr/bin/env sh
set -eu

sbctl=${SBCTL_BIN:-/usr/local/bin/sbctl}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fail() { echo "real acceptance failure: $*" >&2; exit 1; }
contains() { printf '%s' "$1" | grep -F -- "$2" >/dev/null || fail "expected output to contain: $2"; }

. /etc/os-release
case "$ID" in
  debian|ubuntu) ;;
  *) fail "unexpected distribution: $ID" ;;
esac

interface=$(ip -o route show default 2>/dev/null | awk 'NR == 1 { print $5 }')
[ -n "$interface" ] || interface=eth0

# The stub accepts generated configurations and stays alive when supervised by
# systemd. It is deliberately local so this test does not depend on a sing-box
# release or on external network access.
fake_sing_box="$work/sing-box"
cat >"$fake_sing_box" <<'EOF'
#!/bin/sh
case "${1:-}" in
  check) exit 0 ;;
  run) while :; do sleep 3600; done ;;
  *) exit 0 ;;
esac
EOF
chmod 0755 "$fake_sing_box"

install_output=$(
  "$sbctl" install \
    --mode external-proxy \
    --subscription-host sub.example.test \
    --interface "$interface" \
    --reality-decoy-sni www.cloudflare.com \
    --sing-box-bin "$fake_sing_box"
)
contains "$install_output" 'enabled protocols: vless-reality, vmess-websocket, hysteria2, tuic, anytls'

systemctl is-active --quiet sbctl.service || fail 'sbctl.service is not active'
systemctl is-active --quiet sing-box.service || fail 'sing-box.service is not active'
id sbctl >/dev/null 2>&1 || fail 'dedicated sbctl account was not created'
service_user=$(systemctl show -p User --value sbctl.service)
[ "$service_user" = sbctl ] || fail 'sbctl.service does not run as sbctl'

credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' /etc/sbctl/config.toml)
[ -n "$credential" ] || fail 'subscription credential was not persisted'
for format in sing-box.json clash.yaml uri; do
  response=$(curl --silent --show-error --include "http://127.0.0.1:2080/sub/$credential/$format")
  contains "$response" 'HTTP/1.1 200 OK'
  contains "$response" 'subscription-userinfo:'
done

status=$($sbctl status)
contains "$status" 'sbctl.service: active'
contains "$status" 'sing-box.service: active'

$sbctl restart >/dev/null
systemctl is-active --quiet sbctl.service || fail 'sbctl.service did not recover after restart'
systemctl is-active --quiet sing-box.service || fail 'sing-box.service did not recover after restart'

test -f /etc/ufw/user.rules || {
  mkdir -p /etc/ufw
  printf 'firewall\n' >/etc/ufw/user.rules
}
printf 'proxy\n' >/etc/nginx.conf
$sbctl uninstall >/dev/null
test -f /etc/sbctl/config.toml || fail 'default uninstall removed persistent data'
test -d /var/backups/sbctl || fail 'default uninstall did not preserve a backup'
test "$(cat /etc/ufw/user.rules)" = firewall || fail 'uninstall changed firewall data'
test "$(cat /etc/nginx.conf)" = proxy || fail 'uninstall changed unrelated data'

# A release artifact must accept the public IP fallback port option advertised by
# the bootstrap installer and expose its lower-security HTTP subscription.
$sbctl uninstall --purge >/dev/null
ip_install_output=$(
  "$sbctl" install \
    --mode ip-fallback \
    --subscription-host 127.0.0.1 \
    --proxy-host 127.0.0.1 \
    --http-port 2081 \
    --interface "$interface" \
    --reality-decoy-sni www.cloudflare.com \
    --disable-protocol vmess-websocket \
    --disable-protocol hysteria2 \
    --disable-protocol tuic \
    --disable-protocol anytls \
    --sing-box-bin "$fake_sing_box"
)
contains "$ip_install_output" 'enabled protocols: vless-reality'
systemctl is-active --quiet sbctl.service || fail 'IP fallback sbctl.service is not active'
systemctl is-active --quiet sing-box.service || fail 'IP fallback sing-box.service is not active'

credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' /etc/sbctl/config.toml)
response=$(curl --silent --show-error --include "http://127.0.0.1:2081/sub/$credential/uri")
contains "$response" 'HTTP/1.1 200 OK'
contains "$response" 'vless://'

echo "real sbctl acceptance passed on $ID $VERSION_ID"
