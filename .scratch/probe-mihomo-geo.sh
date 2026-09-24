#!/usr/bin/env bash
# Throwaway probe, run under WSL. Two questions:
#   1. Which geo-database mirrors does this box actually reach? `curl https://github.com/`
#      is not the instrument — mihomo pulls geodata from a CDN mirror.
#   2. Does `mihomo -t` block on a GEOIP rule, and does the compiled-in list form
#      avoid that? Timed, because "it passed in 0.4s" and "it hung for 60s" cannot
#      both be true about the same rule type.
set -uo pipefail
MIHOMO=${MIHOMO:-$HOME/bin/mihomo}

echo "=== egress probes (8s cap each) ==="
for url in \
  https://github.com/ \
  https://raw.githubusercontent.com/metacubeX/meta-rules-dat/sing/geo/geosite/geolocation-cn.json \
  https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/country-lite.dat \
  https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@meta/country-lite.dat \
  https://github.com/Loyalsoldier/geoip/releases/download/latest/country.mmdb
do
  code=$(timeout 8 curl -sS -o /dev/null -L -w '%{http_code}' "$url" 2>/dev/null) || code=fail
  printf '%-8s %s\n' "$code" "$url"
done

run_probe() { # label, yaml body
  local d
  d=$(mktemp -d)
  printf '%s' "$2" > "$d/c.yaml"
  local start now status
  start=$(date +%s)
  ( cd "$d" && timeout 45 "$MIHOMO" -t -d "$d" -f "$d/c.yaml" > "$d/out.txt" 2>&1 )
  status=$?
  now=$(date +%s)
  echo "=== $1 ==="
  echo "exit=$status seconds=$((now-start))"
  tail -3 "$d/out.txt" | sed 's/^/  /'
  find "$d" -maxdepth 1 -mindepth 1 -printf '  left: %s bytes  %f\n' | grep -v ' c.yaml$' || echo '  left: nothing but the config'
  rm -rf "$d"
}

run_probe "GEOIP rules (what the standard clash artifact emits)" \
'port: 7890
rules:
  - GEOIP,LAN,DIRECT
  - GEOIP,CN,DIRECT
  - MATCH,DIRECT
'

run_probe "compiled-in lists (what minimal emits now)" \
'port: 7890
rules:
  - DOMAIN-SUFFIX,lan,DIRECT
  - IP-CIDR,10.0.0.0/8,DIRECT,no-resolve
  - IP-CIDR,27.192.0.0/11,DIRECT,no-resolve
  - IP-CIDR6,fc00::/7,DIRECT,no-resolve
  - MATCH,DIRECT
'
