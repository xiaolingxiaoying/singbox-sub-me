#!/bin/bash
# Probe which sniffing keys mihomo v1.19.30 actually recognises, and where.
#
# mihomo ignores unknown YAML keys, so "config accepted" proves nothing on its
# own: the control below must pass (an invented key is silently accepted), and
# a candidate is only *recognised* when a bad value makes it fail to parse.
# That is what distinguishes a key mihomo honours from one it throws away.
set -u
bin="$HOME/bin/mihomo"
work=$(mktemp -d)
out=/mnt/c/Users/ranly/Documents/singbox-sub-me/.scratch/mihomo-sniff-probe.txt
: >"$out"

base() {
  cat >"$work/config.yaml" <<'YAML'
mixed-port: 7890
proxies:
  - name: placeholder
    type: socks5
    server: 127.0.0.1
    port: 1080
proxy-groups:
  - name: G
    type: select
    proxies:
      - placeholder
rules:
  - MATCH,G
YAML
}

probe() {
  local label="$1" extra="$2"
  base
  printf '%b' "$extra" >>"$work/config.yaml"
  if "$bin" -t -d "$work" -f "$work/config.yaml" >"$work/last.log" 2>&1; then
    echo "$label: ACCEPTED" >>"$out"
  else
    echo "$label: REJECTED -- $(grep -io 'error.*' "$work/last.log" | head -1)" >>"$out"
  fi
}

echo "=== control ===" >>"$out"
probe "base config" ""
probe "invented key bogus-key-xyz: [nope]" "\nbogus-key-xyz: [nope]\n"
echo "=== candidates (a REJECTED bad value proves the key is real) ===" >>"$out"
probe "top-level sniff: true" "\nsniff: true\n"
probe "top-level sniffers: [bogus]" "\nsniffers:\n  - bogus\n"
probe "top-level sniffers: [domain,http,tls,quic]" "\nsniffers:\n  - domain\n  - http\n  - tls\n  - quic\n"
probe "top-level sniffers: [dns,http,tls,quic]" "\nsniffers:\n  - dns\n  - http\n  - tls\n  - quic\n"
probe "sniffing.enable+sniffers:[bogus]" "\nsniffing:\n  enable: true\n  sniffers:\n    - bogus\n"
probe "sniffing.enable+sniffers:[domain,http,tls,quic]" "\nsniffing:\n  enable: true\n  sniffers:\n    - domain\n    - http\n    - tls\n    - quic\n"
probe "top-level dns-hijack: [bogus]" "\ndns-hijack:\n  - bogus\n"
probe "tun.dns-hijack: [bogus]" "\ntun:\n  dns-hijack:\n    - bogus\n"
probe "tun.dns-hijack: [any:53]" "\ntun:\n  dns-hijack:\n    - any:53\n"
probe "top-level sniff-port: [bogus]" "\nsniff-port: bogus-not-a-list\n"
echo "=== correct paths (v1.19.30: sniffer.sniffing list) ===" >>"$out"
probe "type-error control mixed-port: not-a-number" "
mixed-port: not-a-number
"
probe "sniffer.enable+sniffing:[bogus]" "
sniffer:
  enable: true
  sniffing:
    - bogus
"
probe "sniffer.enable+sniffing:[domain,http,tls,quic]" "
sniffer:
  enable: true
  sniffing:
    - domain
    - http
    - tls
    - quic
"
probe "sniffer.enable+sniffing:[dns,http,tls,quic]" "
sniffer:
  enable: true
  sniffing:
    - dns
    - http
    - tls
    - quic
"
probe "sniffer.override-destination: true" "
sniffer:
  enable: true
  sniffing:
    - domain
  override-destination: true
"
echo "=== enumerate sniffer names, one per run ===" >>"$out"
probe "sniffer.sniffing:[http]" "\nsniffer:\n  enable: true\n  sniffing:\n    - http\n"
probe "sniffer.sniffing:[tls]" "\nsniffer:\n  enable: true\n  sniffing:\n    - tls\n"
probe "sniffer.sniffing:[quic]" "\nsniffer:\n  enable: true\n  sniffing:\n    - quic\n"
probe "sniffer.sniffing:[domain]" "\nsniffer:\n  enable: true\n  sniffing:\n    - domain\n"
probe "sniffer.sniffing:[dns]" "\nsniffer:\n  enable: true\n  sniffing:\n    - dns\n"
probe "sniffer.sniffing:[rdp]" "\nsniffer:\n  enable: true\n  sniffing:\n    - rdp\n"
probe "sniffer.sniffing:[sniff]" "\nsniffer:\n  enable: true\n  sniffing:\n    - sniff\n"
probe "sniffer.sniffing:[mongo]" "\nsniffer:\n  enable: true\n  sniffing:\n    - mongo\n"
probe "sniffer.sniffing:[pop3]" "\nsniffer:\n  enable: true\n  sniffing:\n    - pop3\n"
probe "sniffer.sniffing:[smtp]" "\nsniffer:\n  enable: true\n  sniffing:\n    - smtp\n"
probe "sniffer.sniffing:[imap]" "\nsniffer:\n  enable: true\n  sniffing:\n    - imap\n"
echo "=== done ===" >>"$out"
rm -rf "$work"
