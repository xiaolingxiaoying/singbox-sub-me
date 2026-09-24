#!/usr/bin/env bash
# Fetch the pinned sing-box cores the L2 real-core matrix needs into ~/bin.
#
# `tests/version_profiles.rs` skips nothing since Phase 0.5: it asserts
# `checked == SING_BOX_VERSION_PROFILES.len()`, so a missing binary is a hard
# failure - and it surfaces as "subscription artifact is unavailable: No such
# file or directory", which reads like a product bug and is really a missing
# prerequisite. This script is that prerequisite.
#
# Versions match the CI pins in .github/workflows/ci.yml. Run inside WSL:
#   wsl -d Ubuntu-22.04 -- bash <repo>/scripts/dev/fetch-sing-box-cores.sh
# then:
#   wsl -d Ubuntu-22.04 -- bash -c 'source ~/bin/sb-cores.env && \
#     bash <repo>/scripts/dev/wsl-gate.sh'
set -eu

# The five minors the subscription version band currently covers, newest first.
CORE_VERSIONS="1.14.1 1.13.21 1.12.25 1.11.15 1.10.7"
BIN_DIR=${BIN_DIR:-$HOME/bin}

mkdir -p "$BIN_DIR"
for version in $CORE_VERSIONS; do
  target="$BIN_DIR/sing-box-$version"
  if [ -x "$target" ]; then
    echo "have sing-box-$version"
    continue
  fi
  archive="sing-box-$version-linux-amd64.tar.gz"
  echo "fetching $version"
  curl --fail --location --silent --show-error --retry 3 --connect-timeout 20 \
    "https://github.com/SagerNet/sing-box/releases/download/v$version/$archive" \
    --output "$BIN_DIR/$archive"
  tar -xzf "$BIN_DIR/$archive" -C "$BIN_DIR"
  mv "$BIN_DIR/sing-box-$version-linux-amd64/sing-box" "$target"
  chmod +x "$target"
  rm -rf "$BIN_DIR/$archive" "$BIN_DIR/sing-box-$version-linux-amd64"
  echo "got sing-box-$version"
done

# Written separately so the gate can source one file instead of re-listing the
# versions in three places.
env_file="$BIN_DIR/sb-cores.env"
: > "$env_file"
for version in $CORE_VERSIONS; do
  major_minor=$(printf '%s' "$version" | cut -d. -f1,2 | tr '.' '_')
  printf 'export SING_BOX_BIN_%s=%s/sing-box-%s\n' "$major_minor" "$BIN_DIR" "$version" >> "$env_file"
done
{
  echo "export SBCTL_UPSTREAM_LATEST=v$(printf '%s\n' $CORE_VERSIONS | head -1)"
  echo "export MIHOMO_BIN=$BIN_DIR/mihomo"
} >> "$env_file"

echo "env file: $env_file"
"$BIN_DIR/sing-box-$(printf '%s\n' $CORE_VERSIONS | head -1) version" | head -1
