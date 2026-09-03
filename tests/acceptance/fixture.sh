#!/usr/bin/env sh
# Reusable host fixture primitives for black-box acceptance tests.
# Every path is rooted below $work; these helpers never touch the host runtime.

fixture_root_for() {
  root="$work/$1"
  mkdir -p "$root/etc" "$root/run/systemd/system" \
    "$root/sys/class/net/ens3/statistics" "$root/proc/sys/kernel/random"
  printf 'ID=%s\nVERSION_ID=1\n' "$2" > "$root/etc/os-release"
  fixture_set_counters 100 200
  fixture_set_boot_id acceptance-boot
}

fixture_set_counters() {
  printf '%s\n' "$1" > "$root/sys/class/net/ens3/statistics/rx_bytes"
  printf '%s\n' "$2" > "$root/sys/class/net/ens3/statistics/tx_bytes"
}

fixture_set_boot_id() {
  printf '%s\n' "$1" > "$root/proc/sys/kernel/random/boot_id"
}

fixture_seed_certificate() {
  host=${1:?certificate host is required}
  certificate_directory="$root/etc/letsencrypt/live/$host"
  mkdir -p "$certificate_directory"
  openssl req -x509 -newkey rsa:2048 -nodes -days 1 -subj "/CN=$host" \
    -keyout "$certificate_directory/privkey.pem" \
    -out "$certificate_directory/fullchain.pem" >/dev/null 2>&1
}

fixture_seed_systemctl() {
  mkdir -p "$root/usr/bin"
  printf '#!/bin/sh\nexit %s\n' "${1:-0}" > "$root/usr/bin/systemctl"
  chmod 0755 "$root/usr/bin/systemctl"
}

fixture_seed_uninstall() {
  mkdir -p "$root/etc/systemd/system" "$root/etc/nginx" "$root/etc/ufw" "$root/usr/bin"
  printf 'Description=sbctl private subscription service\n' > "$root/etc/systemd/system/sbctl.service"
  printf 'Description=sing-box data plane managed by sbctl\n' > "$root/etc/systemd/system/sing-box.service"
  printf 'sbctl-managed-v1\n' > "$root/var/lib/sbctl/ownership"
  printf 'proxy' > "$root/etc/nginx/nginx.conf"
  printf 'firewall' > "$root/etc/ufw/user.rules"
  fixture_seed_systemctl 0
}
