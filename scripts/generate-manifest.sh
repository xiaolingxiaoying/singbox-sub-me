#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 7 ]]; then
  echo "usage: generate-manifest.sh OWNER/REPO TAG SING_BOX_VERSION ARCH SBCTL_FILE SING_BOX_FILE OUTPUT" >&2
  exit 2
fi

repo=$1
tag=$2
sing_box_version=$3
arch=$4
sbctl_file=$5
sing_box_file=$6
output=$7

sbctl_sha=$(sha256sum "$sbctl_file" | awk '{print $1}')
sing_box_sha=$(sha256sum "$sing_box_file" | awk '{print $1}')
base="https://github.com/${repo}/releases/download/${tag}"
jq -n \
  --arg sbctl_version "${tag#v}" \
  --arg sbctl_url "$base/sbctl-linux-$arch" \
  --arg sbctl_sha "$sbctl_sha" \
  --arg sing_box_version "$sing_box_version" \
  --arg sing_box_url "$base/sing-box-linux-$arch" \
  --arg sing_box_sha "$sing_box_sha" \
  '{sbctl:{version:$sbctl_version,url:$sbctl_url,sha256:$sbctl_sha},sing_box:{version:$sing_box_version,url:$sing_box_url,sha256:$sing_box_sha}}' \
  > "$output"
