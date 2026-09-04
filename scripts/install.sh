#!/usr/bin/env bash
set -euo pipefail

# sbctl bootstrap installer.
#
# One-line usage:  bash <(wget -qO- https://raw.githubusercontent.com/xiaolingxiaoying/singbox-sub-me/main/scripts/install.sh)
#
# The only trust decisions this script makes are the sbctl binary download,
# which it protects by verifying the Ed25519 signature over the canonical JSON
# of the release manifest BEFORE trusting any URL or digest. Every later trust
# decision (sing-box download, digest, compatibility matrix, candidate checks)
# is made by the installed sbctl binary with the same built-in public key, so
# the script cannot bypass the Rust verification rules.

red()   { echo -e "\033[31m\033[01m$*\033[0m"; }
green() { echo -e "\033[32m\033[01m$*\033[0m"; }
yellow(){ echo -e "\033[33m\033[01m$*\033[0m"; }
blue()  { echo -e "\033[36m\033[01m$*\033[0m"; }
white() { echo -e "\033[37m\033[01m$*\033[0m"; }

clear
white "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~"
blue  "   sbctl  ·  私有 sing-box 订阅控制面"
white "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~"
blue  " 项目  : github.com/xiaolingxiaoying/singbox-sub-me"
blue  "快捷方式: ly"
white "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~"
echo

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
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates curl jq openssl

# The first-release Ed25519 verification key, identical to the one embedded in
# src/release.rs. The signature in the manifest covers the canonical JSON of
# every field except `signature`, produced exactly like `jq -S -c 'del(.signature)'`.
read -r -d '' SBCTL_PUBLIC_KEY_PEM <<'PEM' || true
-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAJH+I4WMkKYa3EH63BKmD4SGG0ml6OSe35rQuwrNkJys=
-----END PUBLIC KEY-----
PEM

default_manifest_url='https://github.com/xiaolingxiaoying/singbox-sub-me/releases/latest/download/manifest-{arch}.json'
manifest_url_template=${SBCTL_MANIFEST_URL:-$default_manifest_url}
case "$(dpkg --print-architecture)" in
  amd64|arm64) arch=$(dpkg --print-architecture) ;;
  *) echo "仅支持 amd64 和 arm64" >&2; exit 2 ;;
esac
manifest_url=$(printf '%s' "$manifest_url_template" | sed "s/{arch}/$arch/g")
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

curl --fail --globoff --location --silent --show-error "$manifest_url" >"$work_dir/manifest.json"

# Verify the manifest signature BEFORE trusting any URL or digest in it. A
# signature failure stops the install with no artifact accessed.
# jq appends a trailing newline, while Rust's canonical JSON signer covers the
# exact compact JSON bytes without one. Remove only jq's record terminator so
# both verification implementations sign and verify the same payload.
jq -S -c 'del(.signature)' "$work_dir/manifest.json" | tr -d '\r\n' >"$work_dir/canonical.json"
jq -r '.signature' "$work_dir/manifest.json" | base64 -d >"$work_dir/signature.bin"
printf '%s\n' "$SBCTL_PUBLIC_KEY_PEM" >"$work_dir/public-key.pem"
if ! openssl pkeyutl -verify -pubin -inkey "$work_dir/public-key.pem" -rawin \
  -in "$work_dir/canonical.json" -sigfile "$work_dir/signature.bin" >/dev/null 2>&1; then
  echo "release manifest 签名校验失败，已中止安装（未访问其中任何下载地址）。" >&2
  exit 2
fi

# The manifest is now trusted: fetch the pinned sbctl and check its digest.
artifact_url=$(jq -er '.sbctl.url' "$work_dir/manifest.json")
expected_sha=$(jq -er '.sbctl.sha256' "$work_dir/manifest.json")
# Floating, unsignable references are rejected exactly like the Rust verifier
# rejects "latest"/"main"/"master" version fields: the artifact path must name
# a fixed version, never a moving branch or the GitHub "latest" redirect.
case "$artifact_url" in
  *"/releases/latest/"* | *"/latest/download/"* | */latest | */main | */master)
    echo "release manifest 使用了不受支持的 latest/main 引用，已中止安装。" >&2
    exit 2 ;;
esac
curl --fail --location --silent --show-error "$artifact_url" >"$work_dir/sbctl"
printf '%s  %s\n' "$expected_sha" "$work_dir/sbctl" | sha256sum --check --status
install -m 0755 "$work_dir/sbctl" /usr/local/bin/sbctl
ln -sf /usr/local/bin/sbctl /usr/local/bin/ly
green "sbctl 已安装；快捷方式：ly"

if [[ "$#" -eq 0 ]]; then
  if [[ -t 0 ]]; then
    input=/dev/stdin
  elif [[ -r /dev/tty ]]; then
    input=/dev/tty
  else
    echo "未提供安装参数且无法打开终端。请在交互式终端运行，或传入 sbctl install 参数。" >&2
    exit 2
  fi

  read_required() {
    local label=$1 default=${2-} value
    while :; do
      if [[ -n "$default" ]]; then
        read -r -p "$label [$default]: " value <"$input"
        value=${value:-$default}
      else
        read -r -p "$label: " value <"$input"
      fi
      if [[ -n "${value//[[:space:]]/}" ]]; then
        printf '%s' "$value"
        return
      fi
      echo "此项不能为空。" >&2
    done
  }

  echo ""
  echo "sbctl 交互式安装"
  echo "1) Direct：sbctl 使用公网 80/443 提供 HTTPS 订阅"
  echo "2) External proxy：使用现有 Nginx/Caddy 反代本机 2080 端口"
  echo "3) IP fallback：使用 IP + 高位 HTTP 端口（仅 VLESS Reality，安全性较低）"
  while :; do
    read -r -p "请选择订阅模式 [1]: " mode_choice <"$input"
    mode_choice=${mode_choice:-1}
    case "$mode_choice" in
      1) mode=direct; break ;;
      2) mode=external-proxy; break ;;
      3) mode=ip-fallback; break ;;
      *) echo "请输入 1、2 或 3。" >&2 ;;
    esac
  done

  if [[ "$mode" == ip-fallback ]]; then
    subscription_host=$(read_required "VPS 公网 IP")
    http_port=$(read_required "HTTP 订阅端口" "2080")
  else
    subscription_host=$(read_required "订阅域名（请先解析到此 VPS）")
  fi
  proxy_host=$(read_required "代理连接主机（直接回车则使用订阅主机）" "$subscription_host")
  interface=$(read_required "流量统计网卡（直接回车自动识别）" "auto")
  reality_decoy_sni=$(read_required "Reality 伪装 SNI" "www.cloudflare.com")

  install_args=(--mode "$mode" --subscription-host "$subscription_host" --proxy-host "$proxy_host" --reality-decoy-sni "$reality_decoy_sni")
  if [[ "$interface" != auto ]]; then
    install_args+=(--interface "$interface")
  fi
  if [[ "$mode" == ip-fallback ]]; then
    install_args+=(--http-port "$http_port" --disable-protocol vmess-websocket --disable-protocol hysteria2 --disable-protocol tuic --disable-protocol anytls)
  fi

  echo ""
  echo "接下来可逐项选择要启用的协议；直接回车即启用。"
  # sing-box 下载、摘要、兼容矩阵和配置检查全部由 sbctl 依据同一签名 manifest 完成。
  exec /usr/local/bin/sbctl install --manifest "$work_dir/manifest.json" "${install_args[@]}" <"$input"
fi

exec /usr/local/bin/sbctl install --manifest "$work_dir/manifest.json" "$@"
