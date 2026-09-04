# Warp、Argo 与出站/分流

本文关注 inbound 之外的“出站（outbound）”骨架：Warp-Wireguard、Socks5、Argo，以及它们如何被 `route.rules` 分流。

## 1. WARP-Wireguard 出站账户生成：`warpwg()`（`sb.sh:3332-3381`）

脚本自己注册一个新的 Cloudflare Warp 账户（不走 `warp-yg`，直接用 Cloudflare 的客户端注册接口），产出 wireguard 所需的三样东西：`private_key`、`client v6`、`reserved`。

```bash
warpcode(){
  reg(){   # 生成 x25519 密钥对 → POST https://api.cloudflareclient.com/v0a2158/reg
    keypair=$(openssl genpkey -algorithm X25519 | openssl pkey -text -noout)
    private_key=...  private_key 从 priv 段 base64
    public_key=...   # 传给 API 的 key
    response=$(curl -sL --tlsv1.3 -X POST 'https://api.cloudflareclient.com/v0a2158/reg' \
      -H 'CF-Client-Version: a-7.21-0721' -H 'Content-Type: application/json' \
      -d '{ "key": "<public_key>", "tos": "<UTC now>" }')
  }
  reserved(){  }   # 解析响应里的 client_id → base64 → bytes → [r,r,r]
}
output=$(warpcode)
if 结果无 private_key（失败）：用一组硬编码兜底（2606:4700:110:860e.../g9I2sgUH.../[33,217,129]）
else 解析出 pvk / v6 / res
```

产出变量：`$pvk`（私钥）、`$v6`（Warp 分配给客户端的 /128 地址）、`$res`（reserved 字节）。

## 2. WARP 出站如何写入配置

### sb10（1.10，旧结构）：`outbounds[].type=wireguard`（`sb.sh:575-596`）

```json
{ "type":"direct", "tag":"warp-IPv4-out", "detour":"wireguard-out", "domain_strategy":"prefer_ipv4" },
{ "type":"direct", "tag":"warp-IPv6-out", "detour":"wireguard-out", "domain_strategy":"prefer_ipv6" },
{ "type":"wireguard", "tag":"wireguard-out",
  "server":"$endip", "server_port":2408,
  "local_address":[ "172.16.0.2/32", "${v6}/128" ],
  "private_key":"$pvk",
  "peer_public_key":"bmXOC+F1FxEMF9dyiK2H5/1SUtzH0JuVo51h2wPfgyo=",
  "reserved":$res }
```

- `endip`：`v6()`（`sb.sh:150-173`）根据是否有 v6 决定 `2606:4700:d0::a29f:c001`（走 v6）或 `162.159.192.1`（走 v4）。
- 额外还有 `direct`（`domain_strategy:$ipv`）、Socks5 `socks-out`（127.0.0.1:40000）、`block` 等 outbound。

### sb11（非 1.10，新结构）：`endpoints[]`（`sb.sh:724-811`）

```json
"endpoints": [{
  "type":"wireguard", "tag":"warp-out",
  "address":[ "172.16.0.2/32", "${v6}/128" ],
  "private_key":"$pvk",
  "peers":[ { "address":"$endip", "port":2408,
              "public_key":"bmXOC+F1FxEMF9dyiK2H5/1SUtzH0JuVo51h2wPfgyo=",
              "allowed_ips":[ "0.0.0.0/0", "::/0" ], "reserved":$res } ]
}]
```

新内核把 wireguard 从 `outbounds[].detour` 改为 `endpoints[]`，`route` 用 `outbound:"warp-out"` 引用。

## 3. 域名分流（route.rules）

### sb10（`sb.sh:597-...`）

用 `domain_suffix` + `geosite` 两键分别命中，把命中域名交给不同出站优先走：

```json
{ "outbound":"warp-IPv4-out", "domain_suffix":["yg_kkk"], "geosite":["yg_kkk"] },
{ "outbound":"warp-IPv6-out", ... },
{ "outbound":"socks-IPv4-out", ... },
{ "outbound":"socks-IPv6-out", ... },
{ "outbound":"vps-outbound-v4", ... },
{ "outbound":"vps-outbound-v6", ... },
{ "outbound":"direct", "network":"udp,tcp" }
```

底层由 `changefl()`/`changef()`（`sb.sh:3542-...`）维护“哪些域名走哪条道”，默认占位符是 `yg_kkk`，用户可自定义（三通道分流，菜单 5）。

### sb11（`sb.sh:811-...`）

```json
{ "action":"sniff" },
{ "action":"resolve", "domain_suffix":["yg_kkk"], "strategy":"prefer_ipv4" },
{ "action":"resolve", "domain_suffix":["yg_kkk"], "strategy":"prefer_ipv6" },
{ "domain_suffix":["yg_kkk"], "outbound":"socks-out" },
{ "domain_suffix":["yg_kkk"], "outbound":"warp-out" },
{ "outbound":"direct", "network":"udp,tcp" }
```

新内核用 `action: sniff/resolve` 做协议级分流（不再依赖老 geosite）。

## 4. Socks5 / WARP-plus 代理（`inssbwpph`，`sb.sh:4156-4294`）

脚本自身还内置一个 `sbwpph` 二进制（`sbwpph_amd64/arm64`，从本仓库 `main` 拉取），用于提供：

- 模式 1：本地 Warp 代理：`/etc/s-box/sbwpph -b 127.0.0.1:<port> -$sw46 --endpoint 162.159.192.1:2408`，默认端口 40000。
- 模式 2：多地区 Psiphon-VPN 代理：加 `--cfon --country US`（及国家码，脚本列出 30 余国家）。
- 成功取到出口 IP（`curl --socks5 localhost:<port> icanhazip.com`）后把启动命令存到 `/etc/s-box/sbwpph.log`，并由 `aplws5()` 写入开机自启（cron 或 OpenRC）。
- 该 Socks5（40000）被 sb10 的 `socks-out`（`server_port:40000`）与 sb11 的 `socks-out` 引用，从而能把某些域名经由本地 Warp/Psiphon 出口。

## 5. Argo 隧道与 vmess 的关系（回顾）

- Argo 是**入站侧**的 CDN 隧道（Cloudflare → 本地 vmess 端口），让 vmess-ws 能通过 *trycloudflare.com*（临时）或自建域名（固定，`Zero Trust→网络→连接器`，配 token）暴露，且 VPS 无需公网入站端口、可用 CF 优选 IP。
- 仅在 `inbounds[1].tls.enabled=false` 时可用（`cfargo_ym`，`sb.sh:2359-2377`）。
- 相关的 `/etc/s-box/sbargoym.log`（固定域名）、`sbargotoken.log`（token）、`argo.log`（临时域名）、`argo.service`/cron 注入参见 [04](./04-five-protocol-nodes.md) 第 2 节。

## 6. 状态展示：`showprotocol` / `sbymfl`（`sb.sh:4040-4154`, `3433-...`）

启动菜单会调用 `showprotocol` 展示各协议端口、证书形式、Argo 状态、多端口跳跃、Warp/Socks5 状态与各分流通道是否启用；`sbymfl` 读取 `route.rules` 判断 warp-ipv4/ipv6、socks-ipv4/ipv6、vps-ipv4/ipv6 是否“未分流”（默认占位符 `yg_kkk` 即为未分流）。
