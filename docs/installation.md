# 安装与 sing-box 生命周期

## 首次安装 sbctl

发布 manifest 必须同时包含 `sbctl.url`、`sbctl.sha256`、`sing_box.url` 和
`sing_box.sha256`。GitHub Release 会生成 `manifest-amd64.json` 和
`manifest-arm64.json`。在 Debian 12 或 Ubuntu 22.04+ VPS 上，`{arch}` 会自动替换为系统架构：

公开 GitHub Release 可直接由 VPS 匿名下载；如果使用其他下载站，请确保 manifest 和
对应工件均可公开访问。

```bash
curl -fL -o /tmp/sbctl-install.sh \
  https://github.com/xiaolingxiaoying/singbox-sub-me/releases/latest/download/install.sh
bash /tmp/sbctl-install.sh
```

取**发布工件**里的 `install.sh`，不要从 `raw.githubusercontent.com` 拿仓库中的那一份：
仓库里的是模板，`SBCTL_PUBLIC_KEY_PEM` 还是 `@SBCTL_RELEASE_PUBLIC_KEY_PEM@` 占位符，
由 `scripts/prepare-installer.py` 在打包时才替换成生产公钥。用模板安装会得到
「此安装脚本尚未配置生产公钥」并退出 2 —— 这是有意的，签好名的脚本才是信任锚。

首次无参数运行会进入中文引导菜单：选择 Direct、External proxy 或 IP fallback，再填写订阅
域名/IP、可选代理连接主机、网卡和 Reality 伪装 SNI；随后逐项确认需要启用的协议。协议端口
默认自动分配。IP fallback 会自动只启用 VLESS Reality，并询问 HTTP 订阅端口。

安装脚本下载并校验 sbctl 后，会在询问首次部署配置前做只读预检。如果发现已有部署，会给出选项：
保留并退出，或输入确认词后先备份旧配置、状态、证书、二进制和相关 systemd 单元，再停止服务并继续全新安装。
备份保存在 `/var/backups/sbctl/reinstall/`；如果新安装失败，安装器会尝试恢复旧部署和原服务状态。
如果只想管理现有部署，请运行 `ly` 或 `sbctl menu`，查看状态可运行 `sbctl status`。
对于不属于 sbctl 的 sing-box 服务，选择清理也会先将检测到的路径存入备份，之后才移除。

脚本默认使用 GitHub Release 的 `latest/download/manifest-{arch}.json`。如需固定版本或使用
镜像，请设置 `SBCTL_MANIFEST_URL`，例如：

```bash
SBCTL_MANIFEST_URL=https://发布地址/manifest-{arch}.json \
  bash <(wget -qO- https://发布地址/install.sh)
```

bootstrap 脚本只安装并校验 sbctl；它不会接管已有的 sing-box 部署，也不会修改防火墙。

安装完成后，运行 `sbctl menu`（或 `sbctl m`）可重新打开管理菜单，无需再次下载安装脚本。
菜单可查看状态、VPS 流量、节点端口和订阅地址，也可在确认后重启服务或卸载 sbctl（默认保留备份和配置）。

首次部署可在菜单中选择“引导式安装”，一次完成订阅模式、域名/IP、协议、端口和流量账期设置；安装前会显示脱敏摘要并要求确认，取消不会写入部署配置。命令行可用 `sbctl install --guided` 打开同一向导。菜单中的“快速安装”保留较短的默认配置路径。

安装时可为五个协议分别指定监听端口；端口必须大于 1024，且五个协议之间不能重复：

```bash
sbctl install \
  --subscription-host sub.example.com \
  --reality-decoy-sni www.cloudflare.com \
  --vless-port 12001 \
  --vmess-port 12002 \
  --hysteria2-port 12003 \
  --tuic-port 12004 \
  --anytls-port 12005
```

Direct 模式安装前会只读检查 TCP 80/443；若被 Nginx、Caddy 或其他程序占用，安装会停止并报告 `ss` 检测到的监听信息。请自行释放端口或改用 External proxy，sbctl 不会停止或改写已有服务。`sbctl install` 需要 root 权限，建议通过 `sudo` 执行。首次执行 `sbctl certificate obtain` 前也会检查 Certbot；Debian/Ubuntu 可用 `sudo apt-get update && sudo apt-get install certbot` 安装。

`sbctl config init` 使用同样的五个参数。未指定的协议端口仍会自动分配高端口；指定了未启用协议的端口会直接报错。请同时在 VPS 安全组/防火墙中放行对应的 TCP 或 UDP 端口。

## 独立管理 sing-box

```bash
# 下载并校验 sing-box
sbctl sing-box download --manifest /path/to/manifest.json --output /tmp/sing-box

# 安装已校验的 sing-box
sbctl sing-box install --manifest /path/to/manifest.json --artifact /tmp/sing-box

# 使用本地文件更新；不提供 --artifact 时按 manifest.url 自动下载
sbctl sing-box update --manifest /path/to/manifest.json

# 仅移除 sbctl 标记的 sing-box 服务和二进制，保留配置及订阅数据
sbctl sing-box remove
```

`sing-box update` 会先用候选二进制执行 `sing-box check`，再替换二进制并检查
systemd 服务；失败时恢复 rollback 目录中的旧二进制。完整的 `sbctl update` 仍然
保留同时升级控制面和数据面的能力。

服务端发布的 sing-box 工件默认在发布时解析 SagerNet 官方最新稳定版；手动触发
`release.yml` 时也可显式填写 `sing_box_version` 固定版本。签名 manifest 与归档均使用同一个已解析版本。

## 证书状态与后续必做清单

Direct 模式下，`sbctl certificate status` 显示证书路径、SAN 覆盖、有效期与剩余天数、
deploy hook 是否在位，以及 certificate 缺失或过期时的修复命令（`sbctl certificate
obtain --email <邮箱>` / `sbctl certificate renew`）。`sbctl status` 也会在剩余天数
少于 14 天时给出续期提醒。

签发证书时邮箱只用于 ACME 到期通知。确实不需要邮箱时使用显式的免邮箱路径，它会先要求
交互式确认：

```bash
sbctl certificate obtain --email admin@example.com
# 或者：确认后用 --register-unsafely-without-email 注册
sbctl certificate obtain --no-email
```

安装或通过 `sbctl config wizard` 更改配置后，按输出清单手动核对并放行端口（sbctl 永不自动修改防火墙）。
协议端口见 `sbctl node`：VLESS Reality、VMess WebSocket、AnyTLS 使用 TCP；Hysteria2、TUIC 使用 UDP。

```bash
# Direct 模式需要 80/443
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
# 逐协议放行（将端口替换为 sbctl node 显示的当前值）
sudo ufw allow <vless端口>/tcp
sudo ufw allow <vmess端口>/tcp
sudo ufw allow <hysteria2端口>/udp
sudo ufw allow <tuic端口>/udp
sudo ufw allow <anytls端口>/tcp

# DNS 自检：应返回 VPS 公网 IP
dig +short sub.example.com

# 订阅自检（凭据见 sbctl sub 输出）
curl -fsS https://sub.example.com/sub/<凭据>/uri | head
curl -fsS -o /dev/null -w '%{http_code}\n' https://sub.example.com/sub/<凭据>/index      # 200
curl -fsS -o /dev/null -w '%{http_code}\n' https://sub.example.com/sub/<凭据>/qr/uri     # 200
```

External proxy 模式由反向代理负责 TLS，清单不含证书步骤，改为提示 Caddy/Nginx 反代
`127.0.0.1:<subscription_listen_port>`。IP fallback 模式订阅为明文 HTTP，无证书步骤，
但需放行高位订阅端口并提醒其安全边界。手动联测步骤见
`docs/subscription-modes-testing.md`。
