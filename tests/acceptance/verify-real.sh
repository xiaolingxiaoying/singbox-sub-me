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
# Production bootstrap installs the management binary before invoking `install`.
# The acceptance artifact lives outside that managed path so it survives purge.
install -m 0755 "$sbctl" /usr/local/bin/sbctl
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

# Direct mode: systemd itself owns TCP 80/443 through sbctl-http.socket and
# passes both listeners to the non-root sbctl service via LISTEN_FDS.
$sbctl uninstall --purge >/dev/null
install -m 0755 "$sbctl" /usr/local/bin/sbctl
certificate_directory=/etc/letsencrypt/live/sub.example.test
mkdir -p "$certificate_directory"
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=sub.example.test" \
  -keyout "$certificate_directory/privkey.pem" \
  -out "$certificate_directory/fullchain.pem" >/dev/null 2>&1
# The Direct TLS listener runs as the non-root sbctl account. A Certbot deploy
# hook grants the service account certificate access after renewal (ticket 09);
# seed the equivalent grant here so this test exercises the socket-activated
# TLS path instead of failing on a root-only private key.
chown -R sbctl:sbctl "$certificate_directory"
chmod 0640 "$certificate_directory/privkey.pem"
direct_install_output=$(
  "$sbctl" install \
    --mode direct \
    --subscription-host sub.example.test \
    --interface "$interface" \
    --reality-decoy-sni www.cloudflare.com \
    --sing-box-bin "$fake_sing_box"
)
contains "$direct_install_output" 'enabled protocols: vless-reality, vmess-websocket, hysteria2, tuic, anytls'
systemctl is-active --quiet sbctl-http.socket || fail 'sbctl-http.socket is not active'
systemctl is-active --quiet sbctl.service || fail 'Direct sbctl.service is not active'
systemctl is-active --quiet sing-box.service || fail 'Direct sing-box.service is not active'
service_user=$(systemctl show -p User --value sing-box.service)
[ "$service_user" = sing-box ] || fail 'sing-box.service does not run as sing-box'
id sing-box >/dev/null 2>&1 || fail 'dedicated sing-box account was not created'

direct_credential=$(sed -n 's/^subscription_credential = "\([^"]*\)"/\1/p' /etc/sbctl/config.toml)
direct_response=$(mktemp)
if ! curl --silent --show-error --retry 5 --retry-connrefused --retry-delay 1 --insecure \
  --resolve sub.example.test:443:127.0.0.1 \
  "https://sub.example.test/sub/$direct_credential/uri" >"$direct_response"; then
  systemctl --no-pager status sbctl.service >&2 || true
  journalctl --no-pager -u sbctl.service -n 30 >&2 || true
  fail 'Direct HTTPS did not serve the subscription through the systemd socket'
fi
grep -F 'vless://' "$direct_response" >/dev/null \
  || fail 'Direct HTTPS did not serve the subscription through the systemd socket'
rm -f "$direct_response"
direct_token="real-acceptance-token"
mkdir -p /var/lib/sbctl/acme-webroot/.well-known/acme-challenge
printf 'real-challenge-body' > "/var/lib/sbctl/acme-webroot/.well-known/acme-challenge/$direct_token"
challenge=$(curl --silent --show-error --retry 5 --retry-connrefused --retry-delay 1 \
  "http://127.0.0.1:80/.well-known/acme-challenge/$direct_token")
[ "$challenge" = 'real-challenge-body' ] || fail 'Direct HTTP-01 challenge did not serve through the systemd socket'
$sbctl uninstall --purge >/dev/null

echo "real sbctl acceptance passed on $ID $VERSION_ID"
