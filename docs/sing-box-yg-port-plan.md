# sing-box-yg 功能分析与移植计划

日期：2026-09（对齐仓库，以提交为准）
范围：系统梳理 sing-box-yg（`docs/sing-box-yg/` 六篇 + `.reference-sing-box-yg/sb.sh`）在 **5 个协议节点生成全流程** 中的功能，重点讲清 **BBR** 与 **WARP**，再逐项评估哪些能移植到本项目 `sbctl`，给出优先级与阶段计划。
参考：`docs/sing-box-yg/01..06`、`.reference-sing-box-yg/sb.sh`、`docs/research/upstream-analysis.md`、本仓库 ADR（0006、0018、0012、0009、0014）。

---

## 0. 结论速览

sing-box-yg 是典型的「一键 Bash 分发器 + 内核下载器 + 配置生成器」。它的绝大多数功能可归为六类，其中真正值得移植到 `sbctl` 的是**与协议/配置直接相关的少量纯配置增强**：

| 类别 | 代表功能 | 对本项目 |
| --- | --- | --- |
| 协议节点生成 | vless-reality / vmess-ws / hy2 / tuic / anytls | **已实现**（等价能力） |
| 客户端产物 | 分享链接、二维码、SFA/Mihomo 配置、聚合订阅 | **大部分已实现**；缺「完整客户端配置」 |
| 订阅分发 | websbox 本地 HTTP、GitLab、Telegram | **本地 HTTP 已实现**；GitLab/Telegram 属外部服务，建议缓做 |
| 出站/分流 | WARP-Wireguard、Argo、Socks5/Psiphon、geosite 分流 | **未实现**，且依赖外部服务/第三方二进制，建议缓做 |
| 系统调优 | 内核 BBR+FQ、端口跳跃、CDN 优选端口 | **BBR 内核调优可选移植**；端口跳跃需适配；CDN 端口属提示 |
| 安全/运维 | ACME 证书、防火墙清理、root 常驻 | **ACME 已用 Certbot**；防火墙清理/root 常驻是明确要避免的 |

**一句话**：移植价值最高的是「纯配置/协议增强」（sniff、max_early_data、hy2/tuic 端口跳跃、完整客户端配置）与「可选系统调优」（内核 BBR+FQ）；WARP/Argo/Socks5/Psiphon 这类依赖 Cloudflare/第三方服务的出站，应作为独立决策，不建议直接照搬。

---

## 1. sing-box-yg 功能总览（5 节点全流程）

### 1.1 公共基础（所有节点共享）

| 项 | 说明 | 来源 |
| --- | --- | --- |
| 统一 UUID | 4 个协议（vless/vmess/hy2/tuic）共用同一个 UUID；vmess 的 ws path 派生自它 | `sb.sh:417` |
| Reality 密钥对 | `sing-box generate reality-keypair` → 服务端 `private_key` + 客户端 `public_key`（写入 `/etc/s-box/public.key`） | `sb.sh:2536-2541` |
| short_id | `sing-box generate rand --hex 4` | `sb.sh:2541` |
| 证书两态 | 自签（`CN=www.bing.com`，36500 天，`openssl prime256v1`）/ 域名证书（acme-yg） | `sb.sh:241-316` |
| 端口 | 5 个互不重复、未占用的 10000-65535 随机端口；vmess 走 CDN 优选端口 | `sb.sh:359-421` |
| sniff | 每个 inbound 均带 `"sniff": true, "sniff_override_destination": true` | `sb.json` 各 inbound |
| 内核版本 | 1.10 系列用 `sb10.json`（无 anytls、wireguard 用 `outbounds[]`），其余用 `sb11.json` | `sb.sh:876` |

### 1.2 每个节点的功能

**① Vless-Reality-Vision**
- inbound：`users[].uuid` + `flow=xtls-rprx-vision`；TLS `server_name` = 伪装域名（默认 `apple.com`）；reality `handshake.server=<decoy>:443` + `private_key` + `short_id`。
- 客户端：`vless://...?security=reality&sni=<decoy>&fp=chrome&pbk=<公钥>&sid=<short_id>&flow=xtls-rprx-vision`。
- 无需证书（Reality 只做指纹伪装）。

**② Vmess-WebSocket（可选 TLS + 可选 Argo）**
- inbound：`users[].uuid` + `alterId=0`；`transport.type=ws`，`path=<uuid>-vm`，**`max_early_data=2048` + `early_data_header_name=Sec-WebSocket-Protocol`**（降低 CDN 白屏）。
- TLS 两态：`tlsyn=false` 用自签 bing 证书（此时可叠加 Argo）；`tlsyn=true` 用域名证书（Argo 不可用）。
- 端口：TLS 开走 `2053/2083/2087/2096/8443`；TLS 关走 `8080/8880/2052/2082/2086/2095`（CDN 优选）。
- **Argo**：`cloudflared tunnel` 临时隧道（trycloudflare.com）或固定隧道（`run --token`，systemd `argo.service`），把 vmess 端口暴露给 CF，VPS 无需公网入站端口。

**③ Hysteria-2**
- inbound：`users[].password=<uuid>`；TLS `alpn=["h3"]`；`ignore_client_bandwidth=false`；证书两态。
- **端口跳跃**：iptables NAT 做多端口复用（`dpts`），客户端用 `&mport=...`；自签时客户端 `pinSHA256`。

**④ Tuic-V5**
- inbound：`users[].uuid` + `password`（都填 uuid）；**`congestion_control=bbr`**；TLS `alpn=["h3"]`；证书两态；同样可多端口跳跃。
- 客户端：`tuic://uuid:uuid@...?congestion_control=bbr&udp_relay_mode=native&alpn=h3&sni=...&insecure=<自签1/域名0>`。

**⑤ AnyTLS**（仅非 1.10 内核）
- inbound：`users[].password=<uuid>`；`padding_scheme=[]`；TLS `certificate_path/key_path` 两态。

### 1.3 客户端产物与订阅分发

| 产物 | 说明 |
| --- | --- |
| 分享链接 + 二维码 | `vless://`、`vmess://<base64>`、`hysteria2://`、`tuic://`、`anytls://`，`qrencode` 打码 |
| `/etc/s-box/sbox.json` | **SFA/SFI/SFW 完整客户端配置**：`dns`（fakeip + cn/proxy）、`inbounds.tun`（gvisor）、`route.rule_set`（远程拉 `geosite-cn.srs`/`geoip-cn.srs`）、`experimental.clash_api`（127.0.0.1:9090）、`selector`/`urltest` 出站 |
| `/etc/s-box/clmi.yaml` | Mihomo/Clash.Meta 配置：`proxies` + `dns`（fake-ip）、`mode: rule` |
| `/etc/s-box/jhsub.txt` | 聚合订阅（各协议链接顺序追加） |
| `websbox` | 内置裸 HTTP 服务，监听随机高端口，`http://<ip>:<port>/<token>/clmi.yaml|sbox.json|jhsub.txt`；**无 TLS/无认证/无速率限制** |
| GitLab | `git push` 配置到私有 GitLab 项目，`?private_token=` 访问 |
| Telegram | `tgnotice()` 用 Bot API 推送链接/配置 |

### 1.4 出站与分流

- **WARP-Wireguard 出站**：`warpwg()` 通过 Cloudflare 客户端注册 API（`api.cloudflareclient.com/v0a2158/reg`）自建 WARP 账户，产出 `private_key / client v6 / reserved`；在 sing-box 配置里做 `outbounds[].type=wireguard`（1.10）或 `endpoints[]`（非 1.10）的 WARP 出站。
- **Argo**：`cloudflared` 隧道（入站侧 CDN，见 1.2②）。
- **Socks5 / Psiphon 代理**：内置预编译 `sbwpph` 二进制，本地起 `127.0.0.1:<port>` 的 Warp 代理或 `--cfon --country <国家>` 的多地区 Psiphon 代理。
- **域名分流 `route.rules`**：`domain_suffix` + `geosite`（1.10）或 `action: sniff/resolve`（非 1.10）把命中域名交给 `warp-IPv4/IPv6`、`socks-IPv4/IPv6`、`vps-IPv4/IPv6`、`direct` 不同出站。

---

## 2. 重点讲解：BBR 与 WARP

### 2.1 BBR（两层含义，勿混淆）

**BBR**（Bottleneck Bandwidth and Round-trip propagation time）是 Google 提出的 TCP 拥塞控制算法：它不依赖丢包反馈，而是持续估算「瓶颈带宽」与「最小 RTT」，据此设定发送速率，从而在高丢包/高延迟链路上获得更高吞吐与更低排队延迟（对比 CUBIC/Reno）。

sing-box-yg 里 BBR 出现**两层**：

1. **内核级 TCP BBR + FQ**（`bbr()`，`sb.sh:4031-4038`）
   - 仅 KVM（排除 openvz/lxc），执行 `bash <(curl -Ls .../teddysun/across/master/bbr.sh)`。
   - 该脚本最终设置 `net.core.default_qdisc=fq` 与 `net.ipv4.tcp_congestion_control=bbr`（老内核会先装新内核）。
   - 作用对象：**走 TCP 的协议**（vless-reality、vmess-ws、anytls）在系统层加速。
   - 属于**系统级调优**，需要 root，且与内核/发行版相关，不应由非 root 数据面管理。

2. **QUIC/TUIC 协议级 `congestion_control=bbr`**
   - 服务器 inbound 与客户端链接里的 `congestion_control: "bbr"`（`sb.sh:519/763/1492/1601`）。
   - 这是 TUIC 的 QUIC 拥塞控制算法，**与内核无关**，只要客户端/服务端都支持即可。

**对照本项目**：`sbctl` 的 tuic 节点**已经输出** `congestion_control: "bbr"`（`src/subscription.rs` 的 tuic inbound/outbound），即协议级 BBR 已具备。缺的是**内核级 BBR+FQ 调优**——这是「系统优化」而非「sbctl 数据面功能」，适合做成**可选的、由管理员 root 执行的一次性动作**（或文档化的一行命令），不应内嵌到非 root 守护进程。

### 2.2 WARP（Warp-Wireguard 出站）

**Cloudflare WARP** 是 Cloudflare 的 VPN/代理服务。sing-box-yg 不调用 `warp-yg` 脚本，而是**自己注册**一个 WARP 账户，拿到 WireGuard 所需的 `private_key / client v6 / reserved`：

- `warpcode()`（`sb.sh:3332-3381`）：`openssl genpkey -algorithm X25519` 生成密钥对 → `POST https://api.cloudflareclient.com/v0a2158/reg`（`CF-Client-Version` 头 + `{"key": <public_key>, "tos": <UTC now>}`）→ 解析响应得到 `private_key`、`client v6`、`reserved`（由 `client_id` base64 得来）；失败时用一组硬编码兜底。
- 写入 sing-box 配置为 WARP 出站：
  - 1.10：`outbounds[].type=wireguard`（`server=$endip:2408`，`local_address=["172.16.0.2/32","<v6>/128"]`，`private_key`，`peer_public_key=bmXOC+...`，`reserved`）。
  - 非 1.10：`endpoints[].type=wireguard`（结构新，`route` 用 `outbound:"warp-out"` 引用）。
- `endip` 由有无 IPv6 决定（`2606:4700:d0::a29f:c001` 走 v6 / `162.159.192.1` 走 v4）。
- **域名分流**：`route.rules` 把指定域名（默认占位 `yg_kkk`，可自定义）交给 `warp-IPv4/IPv6` / `socks-IPv4/IPv6` / `vps-IPv4/IPv6` 出站，用于解锁 Netflix/ChatGPT 或隐藏真实 IP。

**对照本项目**：`sbctl` 无 WARP。引入 WARP 意味着：依赖 Cloudflare 的注册 API（外部服务）、把 WARP 作为代理出站、并配套一套域名分流规则。这与本仓库 ADR-0018「保持上游兼容性为行为级、且供应商中立」相冲突，因此建议**作为独立决策缓做**；若将来要做，也应作为「可选 outbound 插件」而非默认路径，且分流规则需抽象为供应商无关的「出站通道」模型。

---

## 3. 当前项目对照（sbctl vs sing-box-yg）

| 能力 | sing-box-yg | sbctl（当前） | 状态 |
| --- | --- | --- | --- |
| 5 协议节点 | vless/vmess/hy2/tuic/anytls | 同 | ✅ 等价 |
| Reality 密钥/短 ID | generate reality-keypair/rand | `config.rs` 生成 | ✅ |
| 自签/域名证书两态 | 自签 bing / acme-yg | `certificate_mode=SelfSigned/Domain`（Certbot） | ✅ |
| UUID 复用 | 4 协议共用 | 各协议独立凭据（ADR-0002 分离） | ✅（设计更优） |
| 证书类协议假 SNI | `www.bing.com` | `protocol_sni`（无域名特性） | ✅ |
| 分享链接/URI | vless/vmess/hy2/tuic/anytls | 同（`subscription-uri`） | ✅ |
| 二维码 | `qrencode` | `src/qr.rs`（终端 ANSI） | ✅ |
| 聚合订阅 | `jhsub.txt` | `subscription-uri.txt` | ✅ |
| Clash/Mihomo 配置 | `clmi.yaml` | `subscription-clash.yaml` | ✅ |
| 本地 HTTP 订阅 | websbox（无认证/限速） | IP-fallback + path credential | ✅（更安全） |
| 流量/额度 | 无 | 网卡 RX+TX 记账 + userinfo | ✅ |
| 非 root / 签名清单 / 回滚 | 无（root 常驻） | 有（ADR-0009/0012/0014） | ✅ 设计更优 |
| **sniff / sniff_override_destination** | 每个 inbound 都有 | **无** | ❌ 缺 |
| **vmess max_early_data** | `2048` + `Sec-WebSocket-Protocol` | **无** | ❌ 缺 |
| **hy2/tuic 端口跳跃** | iptables NAT 多端口 | 单端口 | ❌ 缺 |
| **SFA/SFI/SFW 完整客户端配置** | dns+fakeip+tun+rule_set+clash_api | 仅 `outbounds` | ❌ 缺 |
| **内核 BBR+FQ 调优** | `bbr()` → teddysun/across | **无** | ❌ 缺（系统级） |
| WARP-Wireguard 出站 | 有 | 无 | ❌（外部服务） |
| Argo/Cloudflare 隧道 | 有 | 无 | ❌（外部服务） |
| Socks5/Psiphon 代理 | 有（sbwpph 二进制） | 无 | ❌（第三方二进制） |
| geosite/geoip 分流 | 有（route.rules） | 无 | ❌（依赖上述出站） |
| GitLab 订阅 | 有 | 无 | ❌（外部服务，可选） |
| Telegram 通知 | 有 | 无 | ❌（外部服务，可选） |
| CDN 优选端口提示 | 有 | 无 | ⚠️ 可选提示 |

---

## 4. 功能移植分析（可移植 / 需适配 / 不做）

### 4.1 可移植（P0，低风险、纯配置、贴合设计）

1. **sniff / sniff_override_destination**：给 5 个 inbound 统一加 `"sniff": true, "sniff_override_destination": true`。纯配置，提升分流/日志准确性。
2. **VMess `max_early_data` + `early_data_header_name`**：ws 传输加 `max_early_data=2048`、`early_data_header_name="Sec-WebSocket-Protocol"`，降低 CDN 白屏概率。纯配置。
3. **TUIC `congestion_control=bbr`**（已具备，确认即可）：无改动，仅确认现有实现与客户端一致。

### 4.2 需适配（P1，价值较高但要改模型）

4. **hy2/tuic 端口跳跃（UDP port hopping）**：
   - 服务器侧：用 iptables NAT 把一个端口区间（`dpts`）复用给 hy2/tuic 的 UDP 端口，抗 QoS 丢包。
   - 客户端侧：hy2 加 `mport=<range>`、tuic 加 `ports=<range>`。
   - 需新增「端口范围」配置（如 `port_range`），生成 iptables 规则（root 由管理员执行），并在订阅产物输出 `ports`/`mport`。
   - 注意：iptables 改动需 `--no-start` 之外单独授权，且要幂等、可回滚；这与 ADR-0007「事务化端口变更」一致。

5. **完整客户端配置（SFA/SFI/SFW 的 `sbox.json`）**：
   - 当前 `subscription-sing-box.json` 只有 `outbounds`。可增强为可选的「完整客户端配置」：加 `dns`（fake-ip + 直连/代理）、`inbounds.tun`、`route`（`rule_set` 或内置域名分流）、`experimental.clash_api`、`selector`/`urltest` 出站。
   - 需谨慎：ADR-0018 建议「只发布 sing-box JSON 且保持行为级中立」。建议做成**可选项**（`client-profile` 开关），默认仍输出纯 outbounds，避免强绑定远程 rule_set 与特定 DNS。

### 4.3 可选系统调优（P2，管理员一次性动作，非数据面）

6. **内核 BBR+FQ**：提供 `sbctl` 之外的**系统级一键/文档化命令**（设置 `net.core.default_qdisc=fq`、`net.ipv4.tcp_congestion_control=bbr`），由管理员以 root 在部署前执行。不内嵌到守护进程；也可作为 `sbctl deploy bbr` 之类的可选命令，但必须明确「需 root、只动 sysctl、幂等」。
7. **CDN 优选端口提示**：在 `config`/`node` 输出中给出 vmess 建议端口（TLS 开/关两组）。低价值提示项。

### 4.4 不做 / 缓做（依赖外部服务或与设计冲突）

| 功能 | 原因 |
| --- | --- |
| WARP-Wireguard 出站 | 依赖 Cloudflare 注册 API + 分流规则；违反供应商中立（ADR-0018） |
| Argo / Cloudflare 隧道 | 依赖 Cloudflare 隧道 + token；把入口交给外部 CDN |
| Socks5 / Psiphon（sbwpph） | 依赖第三方预编译二进制 + Psiphon 服务 |
| acme-yg | 远程未校验脚本；本项目已用 Certbot（更安全） |
| GitLab 订阅 | 外部服务；本地 HTTP 订阅已够用 |
| Telegram 通知 | 需要 bot token；非核心 |
| 防火墙关闭 / iptables 清空 | 明确要避免（影响其他应用） |
| warp-yg / argosbx | 外部脚本/推荐项，非本脚本功能 |

---

## 5. 优先级与阶段计划

### 阶段 0（P0，最小改动，先落）
- 5 个 inbound 加 `sniff` + `sniff_override_destination`。
- vmess-ws 加 `max_early_data` + `early_data_header_name`。
- 单测 + 验收：sing-box `check` 通过，订阅产物字段含上述项。

### 阶段 1（P1，需设计决策）
- hy2/tuic 端口跳跃：新增 `port_range` 配置 + iptables 规则生成（幂等/可回滚）+ 客户端 `ports`/`mport` 输出。
- 完整客户端配置（可选开关）：`dns`/`tun`/`route`/`clash_api`/`selector`/`urltest`。

### 阶段 2（P2，可选）
- 内核 BBR+FQ 系统调优命令/文档。
- CDN 优选端口提示。

### 暂缓
- WARP / Argo / Socks5 / Psiphon / GitLab / Telegram / 分流通道：作为独立决策另行评估。

---

## 6. 需要拍板的点

1. **hy2/tuic 端口跳跃**：是否引入 iptables NAT（需 root 授权 + 幂等回滚）？还是仅支持 sing-box 原生「单端口」？
2. **完整客户端配置**：是否默认开启？`route`/`dns` 用内置规则还是远程 `rule_set`（CDN jsDelivr）？是否保持「供应商中立」（ADR-0018）？
3. **内核 BBR+FQ**：做成 `sbctl` 子命令还是仅文档化？是否接受「需 root 的系统级动作」？
4. **WARP/Argo**：是否立项为「可选出站插件」？还是维持「不做」，保持纯单机自建节点？

---

## 7. 参考来源

- `docs/sing-box-yg/04-five-protocol-nodes.md`：5 协议 inbound/客户端 share link/证书两态。
- `docs/sing-box-yg/05-warp-argo-outbounds.md`：WARP-Wireguard、Argo、Socks5、域名分流。
- `docs/sing-box-yg/03-singbox-kernel.md`：内核下载/运行/升级、`sb10/sb11` 模板。
- `docs/sing-box-yg/06-client-configs-subscription.md`：SFA/Mihomo 客户端配置、websbox/GitLab/Telegram 订阅。
- `.reference-sing-box-yg/sb.sh`：`bbr()`、`warpwg()`、`inssbjsonser()`、`sbshare()`、`sb_client()` 等。
- `docs/research/upstream-analysis.md`：上游安全批评与设计约束。
- 本仓库 ADR：0002（凭据分离）、0006（Rust 控制面边界）、0012（sing-box 非 root）、0014（发布门禁）、0018（供应商中立）。
