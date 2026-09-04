# 5 个协议节点的生成与每个节点的具体实现

sing-box 的 inbound（入站）各自绑定一个端口，多个协议共享同一进程、同一个 UUID。配置由 `inssbjsonser()`（`sb.sh:422-877`）以 **heredoc** 一次性生成。

## 0. 总体逻辑

- **一个 UUID 打通四个协议**：`uuid=$(/etc/s-box/sing-box generate uuid)`（`sb.sh:417`）。vmess 的 ws path 也派生自它：`${uuid}-vm`。
- **vless-reality 单独一对密钥**：`sing-box generate reality-keypair` → `private_key`（服务端）+ `public_key`（写入 `/etc/s-box/public.key` 给客户端）；`short_id` 来自 `sing-box generate rand --hex 4`（`sb.sh:2536-2541`）。
- **两套模板**：
  - `sb10.json`：给 `1.10` 系列内核（wireguard 用旧 `outbounds[].type=wireguard`，`route.rules[].geosite` 支持，**无 anytls**）；
  - `sb11.json`：给非 1.10（wireguard 用新 `endpoints[]`，`route.rules[].action=resolve`，**含 anytls**）。
- 结束后按 `[[ "$sbnh" == "1.10" ]] && num=10 || num=11` 拷贝为 `sb.json`（`sb.sh:876`）。
- `sb.json` 的 inbound 数组**顺序固定**：`[0]=vless, [1]=vmess, [2]=hy2, [3]=tuic, [4]=anytls`。后续所有提取（`results_vl_vm_hy_tu`、`sbshare`）都按下标取字段。

## 1. Vless-Reality-Vision

### 服务端 inbound（`sb10.json` / `sb11.json` 中的 `[0]`）

```json
{
  "type": "vless",
  "sniff": true, "sniff_override_destination": true,
  "tag": "vless-sb",
  "listen": "::", "listen_port": <port_vl_re>,
  "users": [ { "uuid": "<uuid>", "flow": "xtls-rprx-vision" } ],
  "tls": {
    "enabled": true,
    "server_name": "<ym_vl_re>",            // 默认 apple.com
    "reality": {
      "enabled": true,
      "handshake": { "server": "<ym_vl_re>", "server_port": 443 },
      "private_key": "<private_key>",       // 服务端私钥
      "short_id": ["<short_id>"]
    }
  }
}
```

### 实现要点

- **Vless**：VMess 的兄弟协议，用 UUID 认证；`flow=xtls-rprx-vision` 开启 XTLS 并发/vision 优化，减少握手开销。
- **Reality（服务端伪装）**：`handshake.server=<域名>:443` 指向一个真实站点（默认 `apple.com`），sing-box 用真实站点的 TLS 指纹回应探针；`private_key` 是生成的 x25519 私钥，客户端需用对应 `public_key`（`pbk`）。
- **short_id**：reality 的会话短 ID，用于减少握手数据，客户端（`sid`）与之一致。
- 该 inbound 使用 `listen: "::"`（IPv4/IPv6 通吃，宿主有 v6 则同时监听）。

### 客户端 share link（`resvless`，`sb.sh:1088-1103`）

```bash
vl_link="vless://$uuid@$server_ip:$vl_port?encryption=none&flow=xtls-rprx-vision&security=reality&sni=$vl_name&fp=chrome&pbk=$public_key&sid=$short_id&type=tcp&headerType=none#vl-reality-$hostname"
```

| 参数 | 来源 |
| --- | --- |
| `$uuid` | 统一 UUID |
| `$vl_port` | vless 端口 |
| `sni=$vl_name` | `sb.json.inbounds[0].tls.server_name` |
| `pbk` | `/etc/s-box/public.key`（公钥） |
| `sid` | `sb.json.inbounds[0].tls.reality.short_id[0]` |
| `fp=chrome` | 伪装浏览器指纹 |

用 `qrencode -o - -t ANSIUTF8` 打印二维码。

## 2. Vmess-Ws（可选 TLS + 可选 Argo）

### 服务端 inbound（`sb.json` 的 `[1]`）

```json
{
  "type": "vmess",
  "sniff": true, "sniff_override_destination": true,
  "tag": "vmess-sb",
  "listen": "::", "listen_port": <port_vm_ws>,
  "users": [ { "uuid": "<uuid>", "alterId": 0 } ],
  "transport": {
    "type": "ws",
    "path": "<uuid>-vm",
    "max_early_data": 2048,
    "early_data_header_name": "Sec-WebSocket-Protocol"
  },
  "tls": {
    "enabled": <tlsyn>,
    "server_name": "<ym_vm_ws>",
    "certificate_path": "<certificatec_vmess_ws>",
    "key_path": "<certificatep_vmess_ws>"
  }
}
```

### 实现要点

- **Vmess**：UUID 认证，`alterId=0`（禁用旧版 AEAD 混淆，现代安全）。
- **WS 传输**：路径为 `${uuid}-vm`，开启 `max_early_data`（HTTP/2 `Sec-WebSocket-Protocol` 早数据传输）以降低 CDN 白屏概率。
- **TLS 两态**：
  - `tlsyn=false`（自签/直连模式）：`server_name=www.bing.com`，证书用自签 bing 证书；此时可叠加 **Argo** 隧道。
  - `tlsyn=true`（域名证书模式）：`certificate_path=/root/ygkkkca/cert.crt`；此态下 Argo 不可用（`cfargo_ym` 会拒绝，`sb.sh:2374-2376`）。
- **端口特殊**：走 CDN 优选端口（TLS 开：`2053/2083/2087/2096/8443`；TLS 关：`8080/8880/2052/2082/2086/2095`），以便套 CF/CDN 优选 IP。

### 客户端 share link（`resvmess`，`sb.sh:1104-1188`）

VMess 的分享链接是把一段 JSON **base64** 后拼成 `vmess://<base64>`。脚本根据 TLS 状态输出多种版本：

```bash
# 无 TLS 直连
echo '{"add":"'$vmadd_are_local'","aid":"0","host":"'$vm_name'","id":"'$uuid'","net":"ws","path":"'$ws_path'","port":"'$vm_port'","ps":"'vm-ws-$hostname'","tls":"","type":"none","v":"2"}' | base64 -w 0

# 有 TLS
"tls":"tls","sni":"'$vm_name'"

# 走 Argo 临时域名（host/sni=argo 域名，port 443）
# 走 Argo 固定域名（host/sni=argogd）
```

| 字段 | 来源 |
| --- | --- |
| `add` | `vmadd_are_local`（可改为 CF 优选 IP）或 `vmadd_argo`（Argo 域名） |
| `host` / `sni` | `vm_name`（TLS 域名）或 argo 域名 |
| `id` | UUID |
| `net`/`type` | 固定 `ws`/`none` |
| `path` | `ws_path` = `sb.json.inbounds[1].transport.path` |

### Argo 隧道（可选，`sb.sh:2379-2522`）

- `cloudflaredargo()`：从 `cloudflare/cloudflared` Releases 下载 `cloudflared-linux-$cpu` 到 `/etc/s-box/cloudflared`。
- 临时隧道：`/etc/s-box/cloudflared tunnel --url http://localhost:<vm_port> --edge-ip-version auto --no-autoupdate --protocol http2`，把 `trycloudflare.com` 域名写进 `/etc/s-box/argo.log`，并注入 cron/OpenRC 开机自启。
- 固定隧道：`cloudflared tunnel ... run --token <token>`，写 systemd/OpenRC 单元 `argo.service`，域名与 token 存 `sbargoym.log`/`sbargotoken.log`。

## 3. Hysteria-2

### 服务端 inbound（`sb.json` 的 `[2]`）

```json
{
  "type": "hysteria2",
  "sniff": true, "sniff_override_destination": true,
  "tag": "hy2-sb",
  "listen": "::", "listen_port": <port_hy2>,
  "users": [ { "password": "<uuid>" } ],
  "ignore_client_bandwidth": false,
  "tls": {
    "enabled": true,
    "alpn": ["h3"],
    "certificate_path": "<certificatec_hy2>",
    "key_path": "<certificatep_hy2>"
  }
}
```

### 实现要点

- **Hysteria2**：基于 QUIC 的 UDP 代理，密码认证（脚本直接复用 UUID）。
- `alpn=["h3"]`（HTTP/3）。
- 证书同样自签或域名证书二选一；`hy2_sniname=/etc/s-box/private.key` 时判定为自签（`result_vl_vm_hy_tu` 据此算 `pinSHA256`）。
- **端口跳跃**（可选）：脚本会通过 iptables NAT 做多端口复用，`sb.sh:978-991` 读取 `iptables -t nat -nL` 中匹配 hy2 端口的 `dpts`，拼成 `&mport=...`（客户端 `ports`）。

### 客户端 share link（`reshy2`，`sb.sh:1142-1174`）

```bash
hy2_link="hysteria2://$uuid@$sb_hy2_ip:$hy2_port?security=tls&alpn=h3&insecure=0&allowInsecure=0$hyps&sni=$hy2_name&pinSHA256=$SHA256#hy2-$hostname"
```

- `sni=$hy2_name`：自签时 `www.bing.com`（`$SHA256` 为 `openssl x509 -in cert.pem -outform DER | sha256sum`），域名证书时为域名。
- `hyps`：多端口参数 `&mport=...`（若配置）。
- 说明：脚本注释里还留了一个 `insecure=$ins_hy2` 的备选写法，但当前可执行行用 `insecure=0&allowInsecure=0`。

## 4. Tuic-V5

### 服务端 inbound（`sb.json` 的 `[3]`）

```json
{
  "type": "tuic",
  "sniff": true, "sniff_override_destination": true,
  "tag": "tuic5-sb",
  "listen": "::", "listen_port": <port_tu>,
  "users": [ { "uuid": "<uuid>", "password": "<uuid>" } ],
  "congestion_control": "bbr",
  "tls": {
    "enabled": true,
    "alpn": ["h3"],
    "certificate_path": "<certificatec_tuic>",
    "key_path": "<certificatep_tuic>"
  }
}
```

### 实现要点

- **TUIC v5**：基于 QUIC 的代理，可以同时用 UUID 与密码认证（脚本两个都填 UUID）。
- `congestion_control=bbr`；`alpn=["h3"]`。
- 证书两态同 hy2；多端口跳跃同 hy2（`tu5_ports` → `tu5zfport`）。

### 客户端 share link（`restu5`，`sb.sh:1176-1204`）

```bash
tuic5_link="tuic://$uuid:$uuid@$sb_tu5_ip:$tu5_port?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=$tu5_name&insecure=$ins&allowInsecure=$ins&allow_insecure=$ins#tu5-$hostname"
```

- `user:pass` 都是 UUID。
- `insecure=$ins`：自签=1、域名证书=0（`ins` 在 `result_vl_vm_hy_tu` 里由 `tu5_sniname` 是否等于 `/etc/s-box/private.key` 决定）。

## 5. AnyTLS

> 仅当 `[[ "$sbnh" != "1.10" ]]` 时生成（1.10 系列内核无 anytls）。

### 服务端 inbound（`sb.json` 的 `[4]`）

```json
{
  "type": "anytls",
  "tag": "anytls-sb",
  "listen": "::", "listen_port": <port_an>,
  "users": [ { "password": "<uuid>" } ],
  "padding_scheme": [],
  "tls": {
    "enabled": true,
    "certificate_path": "<certificatec_an>",
    "key_path": "<certificatep_an>"
  }
}
```

### 实现要点

- **AnyTLS**：基于 TCP + TLS + 随机填充来抗探测的相对较新协议；密码认证（复用 UUID），`padding_scheme` 为空数组。
- 同样有自签/域名证书两态。

### 客户端 share link（`resan`，`sb.sh:1190` 起）

```bash
an_link="anytls://$uuid@$sb_an_ip:$an_port?&sni=$an_name&allowInsecure=$ins_an&insecure=$ins_an#anytls-$hostname"
```

## 6. sing-box 侧协议实现对照（源码，供字段理解）

协议字段定义见 `.reference-sing-box/option/`：

| 协议 | inbound 结构 | 字段 |
| --- | --- | --- |
| vless | `option/vless.go` | `VLESSInboundOptions`：`users[].uuid/flow`，`InboundTLSOptionsContainer`（内含 reality） |
| vmess | `option/vmess.go` | `VMessInboundOptions`：`users[].uuid/alterId`，`transport`（`V2RayTransportOptions` ws） |
| hysteria2 | `option/hysteria2.go` | `Hysteria2InboundOptions`：`users[].password`，`ignore_client_bandwidth`，`InboundTLSOptionsContainer` |
| tuic | `option/tuic.go` | `TUICInboundOptions`：`users[].uuid/password`，`congestion_control` |
| anytls | `option/anytls.go` | `AnyTLSInboundOptions`：`users[].password`，`padding_scheme` |

## 7. 生成分享链接的通用流程

`result_vl_vm_hy_tu()`（`sb.sh:975-1087`）是“从 sb.json 读字段 → 归一化到变量”的翻译层，随后每个 `res*` 函数拼 URI 并 `qrencode` 打二维码。聚合时 `sbshare`（`sb.sh:3956-3981`）把各 `*.txt` 追加进 `jhdy.txt`，再 dump 到 `jhsub.txt` 作为聚合订阅内容。
