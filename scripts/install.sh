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

manifest_url_template=${SBCTL_MANIFEST_URL:-https://github.com/xiaolingxiaoying/singbox-sub-me/releases/latest/download/manifest-{arch}.json}
case "$(dpkg --print-architecture)" in
  amd64|arm64) arch=$(dpkg --print-architecture) ;;
  *) echo "仅支持 amd64 和 arm64" >&2; exit 2 ;;
esac
placeholder='{arch}'
manifest_url=${manifest_url_template//$placeholder/$arch}
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
  exec /usr/local/bin/sbctl install --sing-box-bin "$work_dir/sing-box" "${install_args[@]}" <"$input"
fi

exec /usr/local/bin/sbctl install --sing-box-bin "$work_dir/sing-box" "$@"
