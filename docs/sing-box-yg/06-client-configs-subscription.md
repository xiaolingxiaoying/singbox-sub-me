# 客户端配置与订阅

安装完成后，脚本除了打印各协议分享链接（`vless://`、`vmess://`、`hysteria2://`、`tuic://`、`anytls://`）外，还生成两份完整的**客户端配置文件**，并支持两种订阅分发方式。

## 1. 客户端配置生成：`sb_client()`（`sb.sh:1206-...`）

`sbshare()`（`sb.sh:3956-3981`）在打印链接后调用 `sb_client`，内部是多个 heredoc 函数：

| 函数 | 产出文件 | 面向客户端 |
| --- | --- | --- |
| `sball()` | `/etc/s-box/sbox.json`（前半段） | **Sing-box 官方 SFA/SFI/SFW** |
| `clall()` | `/etc/s-box/clmi.yaml`（前半段） | **Mihomo / Clash.Meta** |

因为有 Argo 临时/固定等多种 vmess 变体，`sbox.json`/`clmi.yaml` 会根据 `tls` 开关与 Argo 是否运行，从若干段（`sb.sh:1609/1764/1890/1987/2081/2178/2270/2309` 等）拼装。

### SFA/SFI/SFW（`sbox.json`）

- 顶层：`log`、`http_clients`、`dns`（fakeip + cn + proxy，`dns-out` 走 `detour:"proxy"`）、`inbounds`（`tun` gvisor 栈）、`route`（rule_set 远程拉 `geosite-cn.srs`/`geoip-cn.srs`，`clash_mode` 直连/代理），`experimental`（`clash_api`，`external_controller=127.0.0.1:9090`，`external_ui=ui`）。
- `outbounds`：把 5 个协议的客户端出站（vless/vmess/hy2/tuic/anytls）+ 4 个 Argo vmess 变体 + 一个 `selector`（`proxy`，默认 `auto`）+ 一个 `urltest`（`auto`，探测 `http://www.gstatic.com/generate_204`，间隔 10m）+ `direct`。
- 每个 outbound 使用 `sball()` 的字段（如 vmess 有 `transport.ws`、`packet_encoding:packetaddr`，tuic 有 `congestion_control:bbr`、`udp_relay_mode:native` 等）。

### Mihomo / Clash.Meta（`clmi.yaml`）

- 顶层：`port: 7890`、`allow-lan`、`mode: rule`、`dns`（`fake-ip`、`0.0.0.0:1053`、阿里/腾讯 nameserver、Cloudflare DNS-query）。
- `proxies`：vless-reality（`reality-opts.public-key/short-id`、`client-fingerprint: chrome`、`flow: xtls-rprx-vision`）、vmess（`ws-opts.path`+`headers.Host`）、hysteria2（`ports`、`skip-cert-verify`、`alpn:[h3]`）、tuic5（`reduce-rtt`、`udp-relay-mode`、`congestion-controller`）、anytls（若内核支持），以及 Argo 变体。

### 可选分支字段

- `sbany1/clany1/sbany2/clany2`：仅非 1.10 内核时输出 anytls 节点（`sb.sh:1228-1263`）。
- `sbhy2ports`：若有 hy2 多端口跳跃，输出 `"server_ports": [...]`（`sb.sh:1207-1213`）。

## 2. 本地 HTTP 订阅（BusyBox httpd 方案）

`changeserv` → `subportipsub()`（`sb.sh:3102-...`）会启动一个裸 HTTP 服务来暴露配置为订阅：

```bash
# 取订阅 token：默认就是代理 UUID
subtoken="$(sed 's://.*::g' /etc/s-box/sb.json | jq -r '.inbounds[0].users[0].uuid')"
mkdir -p /root/websbox/"$subtoken"
ln -sf /etc/s-box/clmi.yaml /root/websbox/"$subtoken"/clmi.yaml
ln -sf /etc/s-box/sbox.json  /root/websbox/"$subtoken"/sbox.json
```

- 由 `websbox`（脚本内置的 HTTP 服务）监听随机高端口（`subport.log`），提供 `/clmi.yaml`、`/sbox.json`、`/jhsub.txt`。
- 订阅地址形如 `http://<server_ip>:<port>/<subtoken>/clmi.yaml`。
- 该路径 **无 TLS、无 HTTP 认证、无速率限制**（这正是上游被本项目点名规避的弱点）。

## 3. Gitlab 私有订阅（`gitlabsub`，`sb.sh:3225-3303`）

把 `/etc/s-box` 初始化为 git 仓库，推送 `sbox.json`、`clmi.yaml`、`jhsub.txt` 到私有 Gitlab 项目，得到：

```
https://gitlab.com/api/v4/projects/<userid>%2F<project>/repository/files/sbox.json/raw?ref=<branch>&private_token=<token>
https://gitlab.com/api/v4/projects/<userid>%2F<project>/repository/files/clmi.yaml/raw?ref=<branch>&private_token=<token>
https://gitlab.com/api/v4/projects/<userid>%2F<project>/repository/files/jhsub.txt/raw?ref=<branch>&private_token=<token>
```

- 用 `token` 作 remote 凭据，`git push -f`（用 `expect` 脚本批量提交）。
- 支持多 VPS 共用项目 + 各自分支（`gitlabml` → `:分支名`）。
- 订阅链接 / 二维码由 `clsbshow()`（`sb.sh:3305-3330`）输出。

## 4. 聚合订阅（`jhsub.txt`）

`sbshare` 把各协议 `*.txt` 顺序追加到 `jhdy.txt`，再 dump 为 `jhsub.txt`（`sb.sh:3962-3971`），即“聚合节点”内容，可粘到支持 sing-box/v2ray 的订阅客户端，或经 `websbox`/Gitlab 以订阅链接分发。

## 5. Telegram 通知（可选，`tgnotice`，`sb.sh:3063-3073`）

`changeserv` 的 3 选项可把“链接+配置”拆包成多段（`sing_box_client1..4.txt`、`clash_meta_client1/2.txt`、`jhsub.txt`），用 Bot API 依次推送各协议链接到指定 chat_id。脚本里 URL、`telegram_id` 由用户输入。

## 6. 全链路数据流（产物视角）

```
sing-box 内核 (/-c /etc/s-box/sb.json)
   └─ 服务器端 5 inbound（vless/vmess/hy2/tuic/anytls）+ warp/argo + route
入口: 用户从以下任一处拿到配置：
   ├─ 终端打印的分享链接 + 二维码（resvless/resvmess/... ）
   ├─ /etc/s-box/sbox.json  → SFA/SFI/SFW
   ├─ /etc/s-box/clmi.yaml  → Mihomo/Clash.Meta
   ├─ /etc/s-box/jhsub.txt  → 聚合订阅
   ├─ http://IP:port/<token>/...       → 本地订阅（websbox）
   └─ https://gitlab.com/api/v4/...    → Gitlab 订阅
```
