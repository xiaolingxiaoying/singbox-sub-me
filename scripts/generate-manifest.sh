#!/usr/bin/env bash
set -euo pipefail

# usage: generate-manifest.sh OWNER/REPO TAG SING_BOX_VERSION ARCH SBCTL_FILE SING_BOX_FILE OUTPUT [MIN:MAX]
#
# Builds a versioned, Ed25519-signed release manifest. The signature covers the
# canonical JSON (every field except `signature`, compact, keys sorted) exactly
# as the Rust update logic verifies it. The manifest is signed by the sbctl
# binary via `sbctl release sign`; the signing key seed is read from
# SBCTL_SIGNING_KEY (default: scripts/dev-signing-key.hex) and the signer from
# SBCTL_SIGNER (default: sbctl on PATH).

if [[ "$#" -lt 7 || "$#" -gt 8 ]]; then
  echo "usage: generate-manifest.sh OWNER/REPO TAG SING_BOX_VERSION ARCH SBCTL_FILE SING_BOX_FILE OUTPUT [MIN:MAX]" >&2
  exit 2
fi

repo=$1
tag=$2
sing_box_version=$3
arch=$4
sbctl_file=$5
sing_box_file=$6
output=$7
compat=${8:-"${sing_box_version}:${sing_box_version}"}
min_version=${compat%%:*}
max_version=${compat##*:}

script_dir=$(cd "$(dirname "$0")" && pwd)
signing_key=${SBCTL_SIGNING_KEY:-"$script_dir/dev-signing-key.hex"}
signer=${SBCTL_SIGNER:-$(command -v sbctl || true)}
if [[ -z "$signer" ]]; then
  echo "no sbctl binary available for signing; build one or set SBCTL_SIGNER" >&2
  exit 2
fi

sbctl_sha=$(sha256sum "$sbctl_file" | awk '{print $1}')
sing_box_sha=$(sha256sum "$sing_box_file" | awk '{print $1}')
base="https://github.com/${repo}/releases/download/${tag}"
unsigned="$output.unsigned.tmp"
trap 'rm -f "$unsigned"' EXIT

jq -n \
  --argjson schema 1 \
  --arg sbctl_version "${tag#v}" \
  --arg sbctl_url "$base/sbctl-linux-$arch" \
  --arg sbctl_sha "$sbctl_sha" \
  --arg sing_box_version "$sing_box_version" \
  --arg sing_box_url "$base/sing-box-linux-$arch" \
  --arg sing_box_sha "$sing_box_sha" \
  --arg min "$min_version" \
  --arg max "$max_version" \
  '{schema:$schema,sbctl:{version:$sbctl_version,url:$sbctl_url,sha256:$sbctl_sha},sing_box:{version:$sing_box_version,url:$sing_box_url,sha256:$sing_box_sha},sing_box_compatibility:[{min:$min,max:$max}]}' \
  > "$unsigned"

"$signer" release sign --manifest "$unsigned" --private-key "$signing_key" --output "$output"