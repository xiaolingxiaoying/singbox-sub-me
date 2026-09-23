# 订阅链接详细指南

sbctl 在同一份节点模型上生成多种订阅格式。所有链接都在 `/sub/<凭据>/...` 路径内，凭据只放在 path 中，拒绝 query 参数；每条链接都有对应的二维码链接（`qr/<格式>`），`index` 总览页一次性列出全部链接、标注与二维码。

| 链接 | 内容 | 适用客户端 |
| --- | --- | --- |
| `/sub/<cred>/sing-box.json` | 仅 outbounds 节点列表（历史格式，逐字节稳定） | sing-box 全版本 |
| `/sub/<cred>/sing-box-full.json` | 最新稳定版完整客户端配置 | sing-box 最新稳定版（V2rayN 6.6+ 亦可导入） |
| `/sub/<cred>/sing-box-1.10.json` | 1.10.x 适配完整配置 | sing-box ≥1.10.0, <1.11.0（无 AnyTLS 节点） |
| `/sub/<cred>/sing-box-1.11.json` | 1.11.x 适配完整配置 | sing-box ≥1.11.0, <1.12.0（无 AnyTLS 节点） |
| `/sub/<cred>/sing-box-1.12.json` | 1.12.x 适配完整配置 | sing-box ≥1.12.0, <1.13.0 |
| `/sub/<cred>/sing-box-1.13.json` | 1.13.x 适配完整配置 | sing-box ≥1.13.0, <1.14.0 |
| `/sub/<cred>/sing-box-1.14.json` | 1.14.x 适配完整配置 | sing-box ≥1.14.0 |
| `/sub/<cred>/clash.yaml` | 现行稳定版 mihomo 配置 | mihomo 现行稳定版（rule-set 分流） |
| `/sub/<cred>/clash-1.18.yaml` | 旧版兼容配置 | mihomo 1.18.x / 不想用远程规则集 |
| `/sub/<cred>/uri` | 明文分享 URI（每行一条） | 通用 |
| `/sub/<cred>/uri.txt` | 明文 URI 整体 Base64 | V2rayN 等 |
| `/sub/<cred>/shadowrocket.txt` | Shadowrocket 适配 Base64 URI | Shadowrocket (iOS) |
| `/sub/<cred>/qr/<格式>` | 对应链接的二维码（SVG） | 手机扫码导入 |
| `/sub/<cred>/index` | 中文总览页（按客户端速查 + 全链接 + 标注 + 二维码 + 导入步骤） | 浏览器 |

## 按客户端选择（主流客户端速查）

| 客户端 | 推荐订阅链接 | 说明 |
| --- | --- | --- |
| Clash Party | `clash.yaml` | mihomo 内核订阅，导入后自动更新节点 |
| Clash Verge | `clash.yaml` | mihomo 内核；内置内核较旧时改用 `clash-1.18.yaml` |
| sing-box | `sing-box-full.json`；按客户端实际内核版本选 `sing-box-<版本>.json` | 1.10/1.11 不支持 AnyTLS 节点（1.12.0 才加入） |
| V2rayN | `uri.txt`（Base64 URI，默认内核） | 6.6+ 可直接导入 `sing-box-full.json`（内置 sing-box 内核） |
| Shadowrocket | `shadowrocket.txt` | 五协议均支持，需 ≥ 对应协议最低版本（见下文） |

`sbctl sub` 会先输出以上按客户端速查，再输出完整矩阵；`index` 总览页顶部同样提供该速查表。

## 完整客户端配置包含什么

`sing-box-full.json`（及各版本文件）与 `sing-box.json` 的区别：

- `log`：info + 时间戳；
- `dns`：fake-ip 模式（`client_dns_mode` 可切 redir-host）；直连侧 AliDNS（223.5.5.5），代理侧 DoH（1.1.1.1，detour 选择组）；局域网、系统连通性检测与 NTP 域名走直连解析；
- `inbounds`：tun（auto_route/strict_route/mixed 栈，v4+v6 地址）；
- `outbounds`：🚀节点选择（selector）+ ♻️自动选择（url-test，探测 URL 可配置）+ 五协议节点 + direct；
- `route`：AI 域名（chatgpt/openai/x.com 等）优先走选择组；geosite-cn / geoip-cn rule-set 直连（standard 档）；私有地址直连；`default_domain_resolver` 指向直连 DNS；
- `experimental`：clash_api（127.0.0.1:9090，供 SFA/SFW 面板与 sbtui 使用）+ cache_file（记住选择与 fake-ip 映射）。

## sing-box 版本差异（1.10 → 1.14）

差异以官方 changelog 研究为准（`docs/research/sing-box-client-version-differences.md`）：

| 版本 | DNS servers | 特殊出站/ sniff 字段 | 客户端兼容性说明 |
| --- | --- | --- | --- |
| 1.10.x | legacy 字符串格式 + 顶层 fakeip | 入站 `sniff` 字段 + 特殊 `dns` outbound | **不含 AnyTLS 节点**；无 `store_dns`；无 `default_domain_resolver`（用 `outbound: any` DNS 规则） |
| 1.11.x | legacy 字符串格式 + 顶层 fakeip | 规则动作（`action: sniff` / `hijack-dns`） | **不含 AnyTLS 节点**；无 `store_dns` |
| 1.12.x | 新对象格式（legacy 告警） | 仍可用（弃用） | geoip/geosite 字段已移除，tun 用 `address`；无 `store_dns` |
| 1.13.x | 新对象格式 | 已移除，改规则动作 | WireGuard 出站移除（本配置未用）；无 `store_dns` |
| 1.14.x | legacy 格式移除 | 已移除 | DNS 规则 `outbound` 项移除；`cache_file.store_dns` 可用；与服务端运行的最新稳定版一致 |

生成策略：每个版本 profile 只带该版本内核能接受的字段——1.10/1.11 用 legacy DNS 与顶层 `dns.fakeip`，1.10 的路由规则不用动作式写法，任何版本都不会收到它不认识的字段。`store_dns` 仅 1.14 工件携带。AnyTLS 节点只在 1.12+ 工件中出现；**当部署只启用了 AnyTLS 时，1.10/1.11 工件不会生成**（生成时打印警告，其余格式不受影响）。

## mihomo 差异（1.18 → 1.19）

- `clash.yaml` 使用 rule-providers（远程 `.mrs`，`@meta` 分支：geosite/geoip 的 cn 与 private 四个规则集）+ 三组代理组（🌍选择代理节点 / ♻️自动选择 / 🎯全球直连）+ AI 域名分流；需要 mihomo ≥1.14（rule-set）。
- `clash-1.18.yaml` 保留内置 GEOIP,CN 直连写法，不引用任何远程规则集，适合旧内核或不想加载远程规则的场景。
- 两份 Clash 工件都带 `sniffer: {enable: true, sniffing: [http, tls, quic]}`：mihomo 默认**关闭**嗅探（`Enable: false` 且不选任何协议），不写就没有"从 TLS SNI / HTTP Host 还原真实域名再分流"的能力。键名与取值是用 CI 同一 pin 的内核（v1.19.30）实测出来的：`domain`、`dns` 会被直接拒绝（`not find the sniffer[domain]`），`override-destination` 故意不写（它会改变远端服务器看到的目的地），`dns-hijack` 也不写（它属于 `tun:`，默认已是 `0.0.0.0:53`，从订阅里输出 `tun:` 会覆盖客户端自己的 TUN 设置）。
- 1.19.6 起配置内所有本地路径被限制在 workdir 内：rule-providers 的 `path` 均为相对路径 `./ruleset/*.mrs`，符合该限制。

## Shadowrocket 适配说明

`shadowrocket.txt` 是 Base64 URI 列表，与通用 `uri.txt` 的区别（依据 `docs/research/` §6）：

- 密码与 SNI 一律百分号编码（Shadowrocket 2.2.44 修复 URI 密码解码，特殊字符必须编码到达）；
- TUIC 增加 `udp_relay_mode=native`；
- AnyTLS 遵循官方 anytls-go URI 规范（路径斜杠、`insecure`、`sni`，去掉非标准 `security` 参数）；
- 五协议均支持：VLESS Reality ≥2.2.16、TUIC ≥2.2.12、Hysteria2 ≥2.2.35、AnyTLS ≥2.2.64。

导入：Shadowrocket → 首页右上扫码或「添加配置」粘贴链接 → 打开「自动更新」。

## 覆写模板（服务端统一分发）

不想在每个客户端里手工维护覆写时，把规则放到服务器上：

```bash
sbctl config override show      # 查看路径与合并语义
sbctl config override edit clash     # $EDITOR 编辑（默认给出示例）
sbctl config override validate  # 校验
sbctl config override clear     # 删除并重新生成
```

合并语义（ADR-0021）：对象递归合并；数组整体替换；**键名为 `rules` 的数组前插**到生成规则之前。影响工件：sing-box-full + 各版本文件、clash.yaml、clash-1.18.yaml；`sing-box.json` 与 URI 格式不受影响。

## 客户端模板配置

`sbctl` 菜单「订阅中心 → 12. 客户端模板配置」（或向导主题）可调：

- `client_dns_mode`：fake-ip（默认）/ redir-host
- `client_rule_profile`：standard（远程 rule-set，默认）/ minimal（全部内置规则，不访问规则 CDN）
- `client_rule_set_base_url`：默认 `https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat`（`@sing`/`@meta` 分支由 sbctl 附加；换镜像只改这一处）
- `client_latency_probe_url`：默认 `http://aliyun.com/generate_204`（选择组含 DIRECT，探测必须国内可达）

修改后 `sbctl restart` 重新生成并生效。

## 安全边界

- 凭据只走 URL path；query 参数、错误凭据、未知路径一律 404。
- 所有响应带 `Cache-Control: no-store`；订阅凭据泄露时执行 `sbctl credential rotate` 全部作废。
- IP fallback 模式为明文 HTTP，仅建议无域名时临时使用。
