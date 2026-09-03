#!/usr/bin/env sh
set -eu

installer=/usr/local/lib/sbctl-acceptance/install.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/bin"
printf '#!/bin/sh\nexit 0\n' > "$work/bin/apt-get"
chmod 0755 "$work/bin/apt-get"

printf '#!/bin/sh\nprintf "%%s\\n" "$@" > /tmp/sbctl-bootstrap-arguments\n' > "$work/sbctl"
printf '#!/bin/sh\nexit 0\n' > "$work/sing-box"
chmod 0755 "$work/sbctl" "$work/sing-box"

sbctl_sha=$(sha256sum "$work/sbctl" | awk '{print $1}')
sing_box_sha=$(sha256sum "$work/sing-box" | awk '{print $1}')
cat > "$work/manifest-amd64.json" <<EOF
{"sbctl":{"url":"file://$work/sbctl","sha256":"$sbctl_sha"},"sing_box":{"url":"file://$work/sing-box","sha256":"$sing_box_sha"}}
EOF

PATH="$work/bin:$PATH" SBCTL_MANIFEST_URL="file://$work/manifest-{arch}.json" "$installer" \
  --mode ip-fallback \
  --subscription-host 127.0.0.1 \
  --proxy-host 127.0.0.1 \
  --http-port 2081 \
  --reality-decoy-sni www.cloudflare.com \
  --disable-protocol vmess-websocket \
  --disable-protocol hysteria2 \
  --disable-protocol tuic \
  --disable-protocol anytls

grep -Fx -- '--http-port' /tmp/sbctl-bootstrap-arguments >/dev/null
grep -Fx -- '2081' /tmp/sbctl-bootstrap-arguments >/dev/null

echo 'bootstrap acceptance passed'
