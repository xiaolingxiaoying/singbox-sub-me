#!/usr/bin/env bash
set -euo pipefail

if [[ "${EUID}" -ne 0 ]]; then
  echo "请使用 root 运行此安装脚本" >&2
  exit 2
fi

if [[ ! -r /etc/os-release ]]; then
  echo "无法识别操作系统" >&2
  exit 2
fi
. /etc/os-release
case "${ID}" in
  debian|ubuntu) ;;
  *) echo "仅支持 Debian 或 Ubuntu" >&2; exit 2 ;;
esac

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl jq

manifest_url=${SBCTL_MANIFEST_URL:?请设置 SBCTL_MANIFEST_URL}
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

curl --fail --location --silent --show-error "$manifest_url" >"$work_dir/manifest.json"
artifact_url=$(jq -er '.sbctl.url' "$work_dir/manifest.json")
expected_sha=$(jq -er '.sbctl.sha256' "$work_dir/manifest.json")
curl --fail --location --silent --show-error "$artifact_url" >"$work_dir/sbctl"
printf '%s  %s\n' "$expected_sha" "$work_dir/sbctl" | sha256sum --check --status
install -m 0755 "$work_dir/sbctl" /usr/local/bin/sbctl

sing_box_url=$(jq -er '.sing_box.url' "$work_dir/manifest.json")
sing_box_sha=$(jq -er '.sing_box.sha256' "$work_dir/manifest.json")
curl --fail --location --silent --show-error "$sing_box_url" >"$work_dir/sing-box"
printf '%s  %s\n' "$sing_box_sha" "$work_dir/sing-box" | sha256sum --check --status
chmod 0755 "$work_dir/sing-box"

exec /usr/local/bin/sbctl install --sing-box-bin "$work_dir/sing-box" "$@"
