# sing-box 客户端完整配置版本差异化调查报告（1.10.0 → 2026-09 最新稳定版）

- 调查日期：2026-09-14（2026-09-19 增补 1.10/1.11 适配与官方内核下载）
- 调查方式：联网核实 GitHub Releases/Tags API、sing-box 官方文档（sing-box.sagernet.org）、mihomo 官方 wiki（wiki.metacubex.one）、MetaCubeX 仓库、Shadowrocket 官方 Telegram 频道（ShadowrocketNews）及各协议官方 URI 规范。
- 标注约定：所有无法从一手来源核实的内容均标注「未确认」。
- 2026-09-19 更新：经 GitHub Tags API 复核，最新稳定版为 **1.14.1**（1.15 仍为 alpha 预发布）；订阅适配覆盖 **1.10 → 1.14 最新 5 个稳定 minor 版本**。1.10/1.11 的差异化字段依据官方迁移文档（migration.md）的 Deprecated 形态核实。

---

## 0. 1.10 / 1.11 客户端适配增补（2026-09-19）

### 0.1 版本特性差异

| 特性 | 1.10 | 1.11 | 1.12 | 1.13 | 1.14 |
|---|---|---|---|---|---|
| DNS 服务器格式 | legacy（address 字符串） | legacy | 新 type 对象 | 新 type 对象 | 新 type 对象（legacy 已移除） |
| fake-ip | 顶层 `dns.fakeip` + `address: "fakeip"` 服务器 | 同左 | `type: "fakeip"` DNS 服务器 | 同左 | 同左（顶层对象已移除） |
| 路由规则动作（sniff/hijack-dns） | 无（入站 `sniff` 字段 + 特殊 `dns` outbound） | **有**（`action` 字段） | 有 | 有（旧写法已移除） | 有 |
| `route.default_domain_resolver` | 无（用 `{"outbound": "any"}` DNS 规则） | 同左 | 有 | 有 | 有（outbound DNS 规则项已移除） |
| AnyTLS | **无** | **无** | 有 | 有 | 有 |
| `cache_file.store_dns` | 无 | 无 | 无 | 无 | 有 |
| TUN `address` 合并写法 | 有（1.10.0 引入） | 有 | 有 | 有 | 有 |

来源：官方迁移文档（`docs/migration.md`：Migrate to new DNS server formats / Migrate legacy special outbounds / Migrate legacy inbound fields / outbound DNS rule items → domain resolver）、v1.11.15 源码 `option/dns.go`（`DNSFakeIPOptions` 含 `enabled`/`inet4_range`/`inet6_range`）、官方 deprecated 页。

### 0.2 生成器行为（src/subscription.rs）

- `SING_BOX_VERSION_PROFILES` 覆盖 1.10–1.14，每个 profile 携带 `typed_dns`、`route_rule_actions`、`supports_anytls`、`supports_store_dns` 四个开关。
- 1.10/1.11 工件：legacy DNS 服务器（`{"address": ..., "tag": ..., "detour": ...}`）、顶层 `dns.fakeip`、DNS 规则前置 `{"outbound": "any", "server": "dns-direct"}` 解析代理服务器域名、不含 `route.default_domain_resolver`、**不含 AnyTLS 节点**。
- 1.10 额外差异：tun 入站 `sniff: true`、路由规则 `{"protocol": "dns", "outbound": "dns-out"}` + 特殊 `{"type": "dns", "tag": "dns-out"}` outbound；1.11 起改用 `{"action": "sniff"}` / `{"protocol": "dns", "action": "hijack-dns"}`。
- **AnyTLS-only 部署**：1.10/1.11 工件没有可用节点，生成时跳过这两个工件并打印警告（中文），其余格式正常生成。
- 服务端与客户端的功能差异在每个版本条目的说明（notes）与订阅总览页中标注。

### 0.3 内核下载来源变更（2026-09-19）

- 服务端 sing-box 内核默认改为从官方仓库下载：`https://api.github.com/repos/SagerNet/sing-box/releases/latest`（该端点自动排除 draft 与 pre-release，返回最新稳定版）→ `releases/download/v<版本>/sing-box-<版本>-linux-<amd64|arm64>.tar.gz`。
- 下载后解压并执行 `sing-box version` 自检（确认版本与可执行性），再走原有的 check → 备份 → 替换 → 健康检查 → 回滚流程。
- 完整性依赖 HTTPS 与官方发布；需要固定版本与哈希校验时仍可使用签名 manifest 流程（`--manifest`）。

---

## 1. 版本时间线（1.12.0 → 2026-09-14）

| 版本 | 发布日期 | 说明 |
|---|---|---|
| 1.12.0 | 2025-08-04（`published_at: 2025-08-04T05:44:49Z`） | DNS 服务器对象重构、AnyTLS 协议、domain_resolver 等 |
| 1.12.x（最新 patch：1.12.24） | 1.12.24 发布于 2026-03-05（`published_at: 2026-03-05T14:19:56Z`） | 官方 changelog 显示 1.12 线补丁至 1.12.24 |
| 1.13.0 | 2026-02-28（`published_at: 2026-02-28T07:24:05Z`） | 移除 1.11 弃用项（block/dns 出站、sniff 等旧字段、WireGuard outbound、gso 等） |
| 1.13.x（最新 patch：1.13.21） | 1.13.21 发布于 2026-08-30 | 与 1.14.0 rc 并行维护 |
| 1.14.0 | 2026-08-31（`published_at: 2026-08-31T04:05:01Z`） | **移除旧版 DNS 服务器格式**、大量 1.12/1.13 弃用项落地 |
| 1.15.0-alpha.1 / alpha.2 / alpha.3 | 2026-09-04 / 2026-09-05 / 2026-09-13 | 预发布（sing-tun 自带协议栈、stack 选项弃用） |

- 截至 2026-09-14 的最新稳定版：**1.14.0**（无 1.14.1；1.15.0 仅有 alpha）。
- 来源：
  - GitHub Tags/Releases：https://github.com/SagerNet/sing-box/tags 、https://api.github.com/repos/SagerNet/sing-box/releases/tags/v1.12.0 、https://api.github.com/repos/SagerNet/sing-box/releases/tags/v1.13.0 、https://api.github.com/repos/SagerNet/sing-box/releases/tags/v1.14.0 、https://api.github.com/repos/SagerNet/sing-box/releases/tags/v1.12.24
  - 官方 changelog（含 1.12.0–1.12.24、1.13.x、1.14.x 各条目）：https://sing-box.sagernet.org/changelog/
  - 1.12.0 / 1.13.0 / 1.14.0 发布说明：https://github.com/SagerNet/sing-box/releases/tag/v1.12.0 、https://github.com/SagerNet/sing-box/releases/tag/v1.13.0 、https://github.com/SagerNet/sing-box/releases/tag/v1.14.0

---

## 2. 1.12.0 关键变化（相对 1.11）

### 2.1 DNS 服务器新格式（servers 变为带 type 的对象数组）

- 新写法：`dns.servers` 为对象数组，每个对象必须有 `type` 与 `tag`。可用 type：`local`、`hosts`、`tcp`、`udp`、`tls`、`https`、`quic、h3`、`dhcp`、`fakeip`、`resolved`、`tailscale` 等（`mdns` 为 1.14.0 新增；文档首页标注 `type` 字段 "Changes in sing-box 1.12.0"）。
- 官方迁移示例：
  - `{"address": "tls://1.1.1.1"}` → `{"type": "tls", "server": "1.1.1.1"}`
  - `{"address": "https://1.1.1.1/dns-query"}` → `{"type": "https", "server": "1.1.1.1"}`
  - `"local"` → `{"type": "local"}`；`"1.1.1.1"` → `{"type": "udp", ...}`；`tcp://`、`quic://`、`h3://`、`dhcp://` 均变为对应 type
  - `rcode://refused` → DNS 规则动作 `"action": "predefined"` + `"rcode": "REFUSED"`
- **legacy 字符串格式兼容性**：1.12.0 中仍兼容但输出弃用告警（"legacy DNS servers is deprecated in sing-box 1.12.0 and will be removed in sing-box 1.14.0"），**1.14.0 中移除**。`type` 留空时回退 legacy 格式。
- 来源：
  - https://sing-box.sagernet.org/configuration/dns/server/
  - https://sing-box.sagernet.org/migration/ （Migrate to new DNS server formats）
  - https://sing-box.sagernet.org/deprecated/ （Legacy DNS server formats：1.12.0 弃用、1.14.0 移除）
  - https://sing-box.sagernet.org/configuration/dns/server/legacy/
  - https://github.com/SagerNet/sing-box/issues/3585 （弃用告警示例）

### 2.2 fakeip 新写法

- 独立的顶层 `dns.fakeip` 段被移除，改为 **DNS 服务器 type `"fakeip"`**，字段为 `inet4_range`（默认 `198.18.0.0/15`）与 `inet6_range`（默认 `fc00::/18`）：

```json
{
  "type": "fakeip",
  "tag": "fakeip",
  "inet4_range": "198.18.0.0/15",
  "inet6_range": "fc00::/18"
}
```

- 配合 DNS 规则 `{"query_type": ["A", "AAAA"], "action": "route", "server": "fakeip"}` 使用（迁移文档保留该示例模式）。
- fakeip 持久化：`cache_file.store_fakeip`。
- 注意：该文档页目录锚点显示 `inet6_address`，但正文结构示例与默认值均为 `inet6_range`，以 `inet6_range` 为准。
- 来源：
  - https://sing-box.sagernet.org/configuration/dns/server/fakeip/
  - https://sing-box.sagernet.org/migration/ （FakeIP 段迁移）
  - https://sing-box.sagernet.org/configuration/experimental/cache-file/

### 2.3 dns.rules 新写法

- 自 1.11.0 起，规则内直接写 `server`、`disable_cache`、`rewrite_ttl`、`client_subnet` 已弃用，改为规则动作：`"action": "route"`（默认动作，可不写 action 但必须把 `server` 放进动作参数，`server` 为必填）、`"action": "route-options"`、`"action": "reject"`（`method: default|drop`）等。
- 1.12.0 起，DNS 规则的 **`outbound` 匹配项弃用**（官方注明 "will be removed in sing-box 1.14.0"），改用 domain_resolver（见 2.4）。
- 1.14.0 起，route 动作里的 `strategy` 选项本身也弃用（1.16.0 移除）。
- 来源：
  - https://sing-box.sagernet.org/configuration/dns/rule/
  - https://sing-box.sagernet.org/configuration/dns/rule_action/

### 2.4 domain_resolver / route.default_domain_resolver（1.12 要求）

- 旧写法 `{"outbound": "any", "server": "local"}` 的 DNS 规则被弃用。1.12 起域名出站的解析器通过 Dial Fields 的 **`domain_resolver`** 指定：
  - 字符串形式：`"domain_resolver": "local"`
  - 对象形式：`"domain_resolver": {"server": "local", "strategy": "prefer_ipv4", "rewrite_ttl": 60, "client_subnet": "1.1.1.1"}`
  - 或全局默认：`"route": { "default_domain_resolver": { "server": "local" } }`（`route.default_domain_resolver` 自 **1.12.0** 加入；出站级 `domain_resolver` 优先于该默认值）
- 出站 `domain_strategy` 字段弃用，`strategy` 移入 `domain_resolver.strategy`。
- 实践要求：为「需要解析域名的出站/服务器地址」显式配置 `domain_resolver` 或 `route.default_domain_resolver`，否则只能依赖已弃用的 outbound DNS 规则项。
- 来源：
  - https://sing-box.sagernet.org/migration/ （outbound DNS rule items → domain resolver；outbound domain_strategy → domain resolver）
  - https://sing-box.sagernet.org/configuration/route/ （`default_domain_resolver`，since 1.12.0）
  - https://github.com/SagerNet/sing-box/releases/tag/v1.12.0

### 2.5 1.12.0 移除 / 弃用的客户端字段（相对 1.10/1.11）

**在 1.12.0 中直接移除（写进去即报错/无效）：**

| 移除项 | 原弃用版本 | 替代 |
|---|---|---|
| GeoIP / Geosite 字段（`route.geoip`、`route.geosite`、DNS 规则 `geoip` 等） | 1.8.0 | rule_set |
| TUN `inet4_address`/`inet6_address`/`inet4_route_address`/`inet6_route_address`（含 exclude 变体） | 1.10.0 | 合并为 `address`、`route_address`、`route_exclude_address` |
| TUN `gso` | 1.11.0 | 无（"no longer works in TUN"；tun 页标注移除于 1.12.0，deprecated 页标注 1.13.0，两页存在出入——以两处均已移除对待，未确认精确版本） |

**1.12.0 起弃用（有告警，后续版本移除）：**

| 弃用项 | 移除版本 | 替代 |
|---|---|---|
| legacy DNS 服务器格式（address 字符串） | 1.14.0 | 新 DNS 服务器格式 |
| DNS 规则 `outbound` 项 | 1.14.0 | domain_resolver |
| 出站 `domain_strategy` | （dial fields 迁移） | `domain_resolver.strategy` |
| legacy ECH 字段 `pq_signature_schemes_enabled`、`dynamic_record_sizing_disabled` | 1.13.0 | 标准库 ECH（`with_ech` 构建标签也在 1.12.0 移除） |

**1.11.0 已弃用、1.12 中仍兼容、1.13.0 中移除**（见第 3 节）：block/dns 特殊出站、inbound 旧字段（sniff 等）、direct 出站 `override_address`/`override_port`、WireGuard outbound。

- 来源：https://sing-box.sagernet.org/deprecated/ 、https://sing-box.sagernet.org/configuration/inbound/tun/ 、https://github.com/SagerNet/sing-box/releases/tag/v1.12.0

### 2.6 urltest / selector 组字段

- **urltest**：唯一必填字段是 **`outbounds`**（出站 tag 列表）。默认值：`url` = `https://www.gstatic.com/generate_204`，`interval` = `3m`，`tolerance` = `50`（毫秒），`idle_timeout` = `30m`，`interrupt_exist_connections` = `false`（仅影响 inbound 连接）。
- **selector**：同样必填 `outbounds`；另有 `default`（初始选中项）、`interrupt_exist_connections`。本次调查未逐字段核验 selector 页（`default` 字段名以官方页为准）：https://sing-box.sagernet.org/configuration/outbound/selector/ （未确认/未逐一核实）
- urltest 来源：https://sing-box.sagernet.org/configuration/outbound/urltest/

### 2.7 clash_api 与 cache_file 写法

- **clash_api**（https://sing-box.sagernet.org/configuration/experimental/clash-api/）：
  - `external_controller`：API 监听地址，留空禁用 Clash API
  - `external_ui`：静态资源目录（相对配置目录或绝对路径），服务路径 `http://<external-controller>/ui`
  - `external_ui_download_url`：UI zip 下载地址，默认 Yacd-meta GitHub Pages 归档
  - `external_ui_download_detour`：下载 UI 使用的出站 tag
  - `secret`：API 密钥（`Authorization: Bearer ${secret}`）；监听 0.0.0.0 时官方建议必须设置
  - `default_mode`：默认 Clash 模式，留空为 "Rule"
  - `access_control_allow_origin` / `access_control_allow_private_network`：1.10.0 新增的 CORS 选项
  - 1.8.0 已弃用并迁走的旧字段：`store_mode`、`store_selected`、`store_fakeip`、`cache_file`、`cache_id`
- **cache_file**（https://sing-box.sagernet.org/configuration/experimental/cache-file/，since 1.8.0）：
  - `enabled`、`path`（默认 `cache.db`）、`cache_id`、`store_fakeip`
  - `store_rdrc`/`rdrc_timeout`：1.9.0 加入，**1.14.0 弃用**（1.16.0 移除）
  - `store_dns`：1.14.0 新增；`buffer_size`/`flush_interval`：1.15.0 新增

### 2.8 tun inbound 推荐写法与平台注意点

来源：https://sing-box.sagernet.org/configuration/inbound/tun/

- 推荐骨架字段：`interface_name`（留空自动）、`address`（1.10.0 起的合并写法，IPv4/IPv6 前缀）、`mtu`、`auto_route: true`、`strict_route`；并在 `route` 段设 `auto_detect_interface: true`（或 `default_interface`）避免回环。
- `auto_route`：将默认路由指向 TUN；**Android 上系统 VPN 优先，除非开启 `route.override_android_vpn`**。
- `strict_route`：
  - Linux：使不受支持的网络不可达；配合 `auto_redirect` 可重定向/放行 SO_BINDTODEVICE 流量（1.13.3 有更新）
  - Windows：防止多宿主 DNS 解析泄漏；**可能导致部分应用（如 VirtualBox）异常**
- `route_address` / `route_exclude_address`（1.10.0+）：auto_route 下的自定义包含/排除路由；`route_address_set`/`route_exclude_address_set`（1.11.0+）依赖 rule-set，**Android 图形客户端因 VpnService 路由数限制不可用（DeadSystemException）**。
- `auto_redirect`：仅 Linux（nftables），官方推荐（性能优于 tproxy）；已 root 的 Android 完整支持。
- `stack`：`gvisor`/`system`/`mixed`；**1.15.0 起弃用（1.17.0 移除），1.15 起使用 sing-tun 自带协议栈**。`gso` 已移除。`endpoint_independent_nat` 自 1.11.0 起无效（1.14.0 由 UDP NAT 新字段替代）。
- 1.14.0 新增 `dns_mode`/`dns_address`（`disabled`/`native`/`hijack`，默认 hijack 改为修改接口 DNS + 平台防火墙；Windows 下 strict_route 配合 WFP 过滤器阻断非 TUN 接口的 53 端口）。
- 平台注意：
  - **Windows**：strict_route 可能影响部分应用（VirtualBox 等）；1.14 hijack 行为变化见上。
  - **Android**：`include_package`/`exclude_package`/`include_android_user`；VPN 优先级问题见 `override_android_vpn`。
  - **iOS/macOS（图形客户端）**：TUN 由平台层接管；`platform.http_proxy`、`bypass_domain`；`match_domain` 仅限 Apple 图形客户端。1.12.0 新增 `loopback_address`。
  - `auto_detect_interface` 仅支持 Linux/Windows/macOS（route 层字段）。

---

## 3. 1.13.0 关键变化（2026-02-28）

- **legacy DNS 未在 1.13 移除**：旧 DNS 服务器格式 1.13 中仍可用（弃用告警），**1.14.0 才移除**。来源：https://sing-box.sagernet.org/deprecated/
- **1.13.0 移除的 1.11 弃用项**（客户端配置必须改）：
  - **block / dns 特殊出站移除** → 改用路由规则动作：`"action": "reject"`、`"action": "hijack-dns"`
  - **inbound 旧字段移除**（`sniff`、`sniff_override_destination`、`domain_strategy` 等监听字段）→ 改用规则动作（sniff 动作等）
  - **direct 出站 `override_address`/`override_port` 移除** → 路由选项/规则动作
  - **WireGuard outbound 移除** → WireGuard **endpoint**
  - TUN `gso` 移除；legacy ECH 字段移除
  - 来源：https://sing-box.sagernet.org/deprecated/ 、https://sing-box.sagernet.org/migration/ （1.11.0 各迁移节）
- **1.13.0 行为变化**：
  - 本地 DNS（`type: local`）改用平台原生解析（Linux systemd-resolved DBus / getaddrinfo，Apple getaddrinfo/libresolv），可用 `prefer_go` 退回纯 Go 解析
  - 默认 TCP keep-alive 初始周期 10 分钟 → 5 分钟
  - 最低 Android 6.0（Android 5.0 需单独 legacy 构建）；编译需 Go 1.24+
  - 新特性：NaiveProxy outbound（QUIC/ECH/UDP over TCP）、kTLS、Chrome Root Store 证书、TLS curve 偏好/pinned pubkey SHA256/mTLS/ECH `query_server_name`、Linux/Windows Wi-Fi 状态路由规则、`bypass` 规则动作（auto_redirect 预匹配）
  - 来源：https://github.com/SagerNet/sing-box/releases/tag/v1.13.0 、https://sing-box.sagernet.org/changelog/
- **patch 重点**：1.13.8 修复 fake-ip DNS 在未配置地址类型时应返回 SUCCESS；1.13.16 默认移除 AnyTLS 客户端元数据（有厂商借此画像用户）。来源：https://sing-box.sagernet.org/changelog/
- rule_set / route 规则字段：**未发现 1.13.0 针对本场景（rule_set 引用、route 规则匹配字段）的破坏性变化**（remote rule-set 的 `download_detour` 弃用发生在 1.14.0）。未确认是否存在文档未列出的边缘变化。

---

## 4. 1.14 及以后（1.14.0 稳定版 2026-08-31；1.15.0-alpha 至 2026-09-13）

### 4.1 1.14.0 移除项（客户端完整配置必查）

- **legacy DNS 服务器格式正式移除**——address 字符串写法在 1.14.0 直接不可用，必须改为 type 对象。
- **DNS 规则 `outbound` 项移除**（1.12 弃用的落地）。
- 官方 changelog 明确 "deprecated features removed entirely"（旧版各条目到期全部移除）。
- 来源：https://sing-box.sagernet.org/deprecated/ 、https://github.com/SagerNet/sing-box/releases/tag/v1.14.0 、https://sing-box.sagernet.org/changelog/

### 4.2 1.14.0 新弃用（1.16.0 移除）

| 弃用项 | 替代 |
|---|---|
| DNS 规则 legacy 地址过滤字段（无 `match_response` 的 `ip_cidr`/`ip_is_private`）、`rule_set_ip_cidr_accept_empty` | `action: "evaluate"` + `match_response: true` 响应匹配 |
| route 动作内 legacy `strategy` 选项 | （迁移指南） |
| `dns.independent_cache` | 无（DNS 缓存始终按传输分组，直接删除字段） |
| `cache_file.store_rdrc` / `rdrc_timeout` | `cache_file.store_dns`（乐观缓存） |
| remote rule-set `download_detour`；隐式默认 HTTP client | `http_clients` + `route.default_http_client`（`http_client`） |
| 内联 ACME（`tls.acme`） | `certificate_provider`（type: acme） |
| Hysteria v1 调优字段（`recv_window_conn` 等） | HTTP/2 & QUIC 参数统一 |

- 行为变化：`ip_version`/`query_type` 现在也作用于内部 DNS 解析；与 legacy 地址过滤字段组合会在**启动时直接报错**。rule-set 合并匹配语义修正：合并匹配仅限单个非反向默认规则，其余按 "other" 字段匹配。
- 来源：https://sing-box.sagernet.org/deprecated/ 、https://sing-box.sagernet.org/migration/ （1.14.0 节）、https://sing-box.sagernet.org/changelog/

### 4.3 1.14.0 对本场景有用的新特性

- DNS：`evaluate` 动作与响应匹配字段（`response_rcode`、`response_answer` 等）、`race`/`speculative` 并行评估、乐观 DNS 缓存（后台刷新）、`dns.timeout`、mDNS 服务器、`preferred_by`。
- 缓存：`cache_file.store_dns: true`。
- TUN：`dns_mode`/`dns_address`（默认 hijack 行为变化：修改接口 DNS + 平台防火墙；Windows WFP 阻断非 TUN 接口 53 端口）、`netns`、MAC 过滤、UDP NAT 选项。
- 客户端发行变化：iOS/tvOS 以 "sing-box MT" 重回 App Store（新开发者账号）；SFM 退出 macOS App Store 改独立分发（**无配置/设置迁移**）；新增 Windows/Linux 桌面客户端。
- 需要 Go 1.25+（仅影响自编译）。
- 来源：https://sing-box.sagernet.org/changelog/ 、https://sing-box.sagernet.org/configuration/dns/rule_action/ 、https://sing-box.sagernet.org/configuration/inbound/tun/ 、https://sing-box.sagernet.org/configuration/experimental/cache-file/

### 4.4 1.15.0（预发布，alpha.3 截至 2026-09-13）

- sing-tun 内置自有 TCP/IP 栈，**`stack` 选项弃用（1.17.0 移除）；客户端配置只需删除该字段**；1.16.0 起 CLI 需 `ENABLE_DEPRECATED_TUN_STACK=true` 才能继续用旧栈。
- Android 完整 `auto_redirect`；`on_demand` endpoint 选项（WireGuard/Tailscale/OpenVPN/OpenConnect）；`cache_file.buffer_size`（默认 1MB）与 `flush_interval`。
- 来源：https://sing-box.sagernet.org/migration/ （1.15.0 节）、https://sing-box.sagernet.org/changelog/ 、https://github.com/SagerNet/sing-box/tags

### 4.5 版本差异化要点速览（给订阅服务生成的建议）

| 客户端版本 | DNS servers | fakeip | block/dns 出站 | outbound DNS 规则 | sniff 字段 | WireGuard | geoip/geosite |
|---|---|---|---|---|---|---|---|
| 1.12.x | 新格式可用，legacy 告警 | `type: fakeip` + `inet4/6_range` | 仍可用（弃用） | 可用（弃用） | 可用（弃用） | outbound 可用（弃用） | **已移除** |
| 1.13.x | 同上 | 同上 | **移除**，用 rule action | 可用（弃用） | **移除**，用 rule action | **移除**，用 endpoint | 已移除 |
| 1.14.x | **legacy 移除** | 同上 | 移除 | **移除** | 移除 | endpoint | 已移除 |

---

## 5. mihomo（Clash.Meta）

### 5.1 版本现状

- 最新稳定版：**v1.19.30**（GitHub tags 最新，2026-09 中旬附近；精确发布日未逐一核实）。来源：https://api.github.com/repos/MetaCubeX/mihomo/tags?per_page=100 、https://github.com/MetaCubeX/mihomo/releases
- v1.19.0（相对 1.18.10）：新增 mieru 协议、provider `size-limit` 等，**发布说明未标注破坏性变化**。来源：https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.0

### 5.2 1.19.x 的破坏性变化 / 弃用（相对 1.18.x）

- **v1.19.6（破坏）**：出于安全考虑，**配置文件中出现的所有路径（相对/绝对）被限定在 workdir（`-d` 目录）内**；额外路径用 `SAFE_PATHS` 环境变量放行（语法同操作系统 PATH：Windows 分号、其他冒号）。影响 external-ui、rule-providers `path` 等一切本地路径。同时 **proxy-groups 的 `routing-mark`、`interface-name` 被移除**，需改设在单个 proxy 上。来源：https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.6 、https://t.me/s/clashmeta?before=107 、官方 wiki 说明 https://wiki.metacubex.one/config/rule-providers/
- **v1.19.7**：回滚上一版的 RESTful API 不兼容改动（GUI 无法刷新配置）；预告 `/configs` 的 `path` 参数将限制在 workdir/SAFE_PATHS。来源：https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.7
- **v1.19.8**：`/configs` path 限制正式落地。来源：https://t.me/s/clashmeta?before=107 （消息 92）
- **v1.19.4**：anytls 协议更新到 version 2（服务端/客户端需配套）。来源：https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.4
- **DNS 段**：1.19.x 各 release notes 中**未发现 DNS 配置破坏性变化**；v1.19.15 的 "DNS module restructure" 为内部 chore 重构。未确认是否存在未写入 release notes 的行为变化。来源：https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.15
- **GEOSITE/GEOIP 用法**：1.19.x release notes 未发现 GEOIP/GEOSITE 规则的移除或破坏性变化；官方 wiki 现代示例已转向 rule-providers（.mrs）。来源：https://wiki.metacubex.one/example/conf/
- **external-controller**：未发现破坏性变化；现有字段含 `external-controller`、`secret`、`external-ui`、`external-ui-name`、`external-ui-url`、`external-controller-cors`（`allow-origins`/`allow-private-network`）、`external-controller-unix/pipe/tls` 等；v1.19.9 官方提醒不给 external-controller 设 secret 的风险；v1.19.18 起 TLS 证书/私钥本地文件支持热重载。来源：https://wiki.metacubex.one/config/general/ 、https://t.me/s/clashmeta?before=107 （消息 94）
- 附带：v1.19.6 曾导致部分 GUI（CFW、Clash Verge 等）订阅切换异常（issue 报告）。来源：https://github.com/MetaCubeX/mihomo/issues/2030

### 5.3 MetaCubeX/meta-rules-dat 的 rule-set 远程 .mrs URL 正确形态

- **官方 wiki 示例（raw.githubusercontent + meta 分支，这是 wiki 钦定形态）**：
  - 国内域名：`https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo/geosite/cn.mrs`（behavior: domain，format: mrs）
  - 国内 IP：`https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo/geoip/cn.mrs`（behavior: ipcidr）
  - 内网域名：`https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo/geosite/private.mrs`
  - 内网 IP：`https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/meta/geo/geoip/private.mrs`
  - 同路径存在 `.list`（text）变体：`.../geo/geosite/cn.list` 等
  - 来源：https://wiki.metacubex.one/example/conf/ 、https://github.com/MetaCubeX/meta-rules-dat
- **注意**："geosite-cn.mrs / geoip-cn.mrs" 这类带前缀命名在 wiki/README 中**未出现**（meta 分支为 `geo/geosite/cn.mrs` 命名；带前缀写法是否可用：未确认）。meta 分支目录超过 5600 个文件，`cn`/`private` 等确在列（截断列表未能直击，但 wiki 示例与 README 示例可证实路径模式）。
- **release 直链的实际情况**：meta-rules-dat 的 `latest` release 仅 28 个资产 = `BundleMRS.7z` + 全量数据库（`geosite.dat/db`、`geoip.dat/db`、`country.mmdb`、lite 变体、`GeoLite2-ASN.mmdb`）+ sha256sum，**没有单文件 geosite-cn.mrs 等资产**；单文件 .mrs 只能走 meta 分支 raw 链接或解包 BundleMRS.7z（配合 `path-in-bundle`）。来源：https://api.github.com/repos/MetaCubeX/meta-rules-dat/releases/tags/latest 、https://github.com/MetaCubeX/meta-rules-dat
- **jsDelivr**：README 官方下载表给出的镜像为 release 资产镜像 `https://cdn.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/<file>` 与 `https://testingcf.jsdelivr.net/gh/MetaCubeX/meta-rules-dat@release/<file>`（适用于 dat/db/mmdb/BundleMRS）；对 meta 分支上的单个 .mrs，jsDelivr 理论上支持分支引用（`@meta`），但本次未逐一验证可用性——**未确认**。来源：https://github.com/MetaCubeX/meta-rules-dat
- **mrs 限制**：`format: mrs` 仅支持 behavior `domain`/`ipcidr`，不支持 `classical`；`mihomo convert-ruleset domain|ipcidr yaml|text <输入.yaml> <输出.mrs>` 可自制。来源：https://wiki.metacubex.one/config/rule-providers/
- **sing-box 侧（.srs）**：同一仓库 sing 分支提供 sing-box rule-set（README 明示 sing 分支）；已核实路径命名无前缀，如 `https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/sing/geo/geoip/cn.srs`；geosite 目录按同模式推断为 `.../sing/geo/geosite/cn.srs`（geosite 目录文件名未逐一验证：未确认）。来源：https://github.com/MetaCubeX/meta-rules-dat 、https://github.com/MetaCubeX/meta-rules-dat/tree/sing/geo/geoip

---

## 6. Shadowrocket（iOS）适配

### 6.1 协议支持（以官方 Telegram 频道 ShadowrocketNews 更新说明为准）

| 协议 | 加入版本 | 出处 |
|---|---|---|
| TUIC | **2.2.12**（"增加TUIC 后端支持"，同期加 UDP over TCP、DNS over HTTP3） | https://t.me/s/ShadowrocketNews?before=428 |
| VLESS **REALITY** | **2.2.16**（"add VLESS reality supports"，同期 "add Chrome 110 TLS fingerprint"） | https://t.me/s/ShadowrocketNews?before=432 |
| Hysteria2 | **2.2.35**（beta 1989 首次加入；2.2.38 加带宽设置；2.2.44/2.2.45 修 URI 密码 URL 解码、默认端口解析） | https://t.me/s/ShadowrocketNews?q=hysteria2 |
| AnyTLS | **2.2.64**（beta 2590） | https://t.me/s/ShadowrocketNews?q=anytls |

- 结论：**Shadowrocket 支持 vless reality 与 anytls**（分别 ≥2.2.16、≥2.2.64）；tuic ≥2.2.12；hysteria2 ≥2.2.35。当前主流分发版本（如 2.9.x 系列，见第三方版本页）均满足。第三方版本历史页：https://shadowrocketo.com.cn/download.html （非官方站点，仅参考；未确认）
- 订阅格式：base64 编码的 URI 列表（多行分享链接整体 base64）是标准订阅形态，Shadowrocket 支持解析；是否原生支持 Clash YAML 配置导入：**未确认**。来源：https://liolok.com/zhs/v2ray-subscription-parse/

### 6.2 各 URI 的参数要求 / 兼容性注意点

- **vless://（reality）**
  - 标准 share link 形如 `vless://uuid@host:port?encryption=none&flow=xtls-rprx-vision&security=reality&sni=<sni>&fp=<fp>&pbk=<publicKey>&sid=<shortId>&type=tcp#name`。REALITY 关键参数：`pbk`（公钥）、`sid`（shortId）、`fp`（uTLS 指纹）、`sni`、`flow`。
  - Shadowrocket 自 2.2.16 支持 REALITY；`encryption=none` 为 vless 标准必填参数。**Shadowrocket 是否要求额外参数（如必须显式 security=reality）**：未确认（官方无参数文档）。
  - 来源：https://t.me/s/ShadowrocketNews?before=432 ；XTLS share link 标准讨论：https://github.com/XTLS/Xray-core/discussions/716 （社区标准）
- **vmess://（base64 JSON）**
  - 格式：`vmess://base64(JSON)`，JSON 字段（v2rayN Draft 1 事实标准）：`v`("2")、`ps`、`add`、`port`、`id`、`aid`、`scy`（加密方式，如 auto）、`net`（tcp/ws/...）、`type`、`host`、`path`（ws 用）、`tls`、`sni`、`alpn`、`fp`。`sni`/`fp` 为后来扩展字段，非所有实现支持。
  - Shadowrocket 注意点：2022 起 V2Ray 服务端强制 AEAD，**aid 必须为 0**（官方频道明确提示「vmess 节点 2022 年起失效就把它额外 ID 设为 0」）。`scy` 建议显式给出。
  - 来源：https://github.com/v2fly/v2fly-github-io/issues/26 、https://github.com/2dust/v2rayn/wiki/Description-of-VMess-share-link 、https://t.me/s/shadowrocketnews?before=372
- **hysteria2://（mport、insecure、sni）**
  - 官方 URI 规范：`hysteria2://auth@host:port/?obfs=&obfs-password=&sni=&insecure=1&pinSHA256=&ech=`；**端口跳跃（multi-port）写在 host 的端口位**（如 `hysteria2://pass@host:443,8000-9000/`），官方规范**没有 `mport` 查询参数**。`insecure` 取 "1"/"0"。
  - `mport=...` 是面板生态的事实扩展参数；**Shadowrocket 是否解析 mport：未确认**（官方频道搜索 "mport" 无结果）。稳妥做法是同时输出「多端口 host 写法」或避免依赖端口跳跃。
  - Shadowrocket 相关修复：2.2.44 修 URL 密码解码、2.2.45 修 URI 默认端口解析（生成 URI 时密码需百分号编码）。
  - 来源：https://v2.hysteria.network/docs/developers/URI-Scheme/ 、https://v2.hysteria.network/docs/advanced/Port-Hopping/ 、https://t.me/s/ShadowrocketNews?q=hysteria2 、https://t.me/s/ShadowrocketNews?q=mport （无结果）
- **tuic://（alpn、congestion_control）**
  - 无官方 RFC；事实标准（dae/sing-box 下划线风格）：`tuic://uuid:password@host:port?congestion_control=bbr&udp_relay_mode=native&alpn=h3&allow_insecure=1&sni=<sni>`。密码含特殊字符需百分号编码（在 auth 段）。clash 系字段名为连字符（`congestion-controller`），解析器常需两种风格兼容。
  - Shadowrocket TUIC 自 2.2.12 支持；**Shadowrocket 对上述各查询参数（尤其 alpn/congestion_control）的完整解析矩阵：未确认**（无官方参数文档）。
  - 来源：https://github.com/daeuniverse/dae/discussions/182 、https://github.com/2dust/v2rayN/issues/6694 、https://wiki.metacubex.one/en/config/proxies/tuic/ 、https://t.me/s/ShadowrocketNews?before=428
- **anytls://**
  - 官方 URI 规范（anytls-go 文档）：`anytls://[auth@]hostname[:port]/?sni=<sni>&insecure=1#name`，默认端口 443，密码需百分号编码；**sni 为 IP 地址时客户端必须不发送 SNI**。
  - Shadowrocket 自 2.2.64 支持 anytls。
  - 来源：https://github.com/anytls/anytls-go/blob/main/docs/uri_scheme.md 、https://t.me/s/ShadowrocketNews?q=anytls

### 6.3 「Shadowrocket 要求但其他客户端可省略」的参数

- **未确认**。Shadowrocket 无公开的 URI 参数官方文档；可确认的只有：vmess 的 `aid=0`（AEAD 时代必须，官方频道提示）、vless 标准 `encryption=none`、hysteria2 密码需 URL 编码（2.2.44 修复即为此问题）。建议生成器对上述字段始终显式输出，以兼容 Shadowrocket。

---

## 7. 嗅探字段的真核实测（2026-09-23，探针而非文档转述）

上面各节的字段结论来自官方文档与 release notes。嗅探这一项不同：**文档描述的形态在
1.10–1.14 的真实内核上全部无法解码**，所以这里记的是实测结果。

方法：容器内对每个 minor 生成一份最小配置并跑 `sing-box check -c`，逐字段变体打印
ACCEPT/REJECT。入站探针必须按版本换地址写法——1.11+ 用 `inet4_address` 会先撞上
`legacy tun address fields` 错误，把 sniff 的结论掩盖掉。

| 位置 | 字段形态 | 1.10 | 1.11 | 1.12 | 1.13 | 1.14 |
|---|---|---|---|---|---|---|
| `route.rules[]` | `{"action":"sniff"}` | ✗ 无 action | ✓ | ✓ | ✓ | ✓ |
| `route.rules[]` | `action:sniff` + `sniff:["http","tls","quic"]` | ✗ | ✗ unknown field | ✗ | ✗ | ✗ |
| `route.rules[]` | `action:sniff` + `override_destination` | ✗ | ✗ unknown field | ✗ | ✗ | ✗ |
| `inbounds[tun]` | `"sniff": true` | ✓ | ✓ | ✓ | ✗ | ✗ |
| `inbounds[tun]` | `"sniff_override_destination": true` | ✓ | ✓ | ✓ | ✗ | ✗ |
| `inbounds[tun]` | `"sniff": [ … ]` | ✗ | ✗ |  | ✗ |  |

要点：

1. **协议列表与"覆写目标"在 1.10–1.14 的任何位置都不是可写字段。** 想给订阅加"嗅探模板"
   （指定嗅探哪些协议、是否覆写目标地址）在 sing-box 侧没有表达空间。
2. 1.13 起入站的 `sniff` / `sniff_override_destination` 被移除，嗅探在 1.13+ 只能由裸
   `{"action":"sniff"}` 触发，细节交给内核默认值。
3. 因此 `src/subscription/render/singbox.rs:146`（1.11+ 用裸 action）与 `:138-142`
   （1.10 用 `tun.sniff`）现有写法已经是正确的，不需要改动。
4. "嗅探模板"这一需求真正能落地的是 **mihomo**：顶层 `sniffers` + `sniff-vars` +
   `dns-hijack`，见 §5。

---

## 主要来源汇总

- sing-box：https://sing-box.sagernet.org/changelog/ 、https://sing-box.sagernet.org/deprecated/ 、https://sing-box.sagernet.org/migration/ 、https://sing-box.sagernet.org/configuration/dns/server/ 、https://sing-box.sagernet.org/configuration/dns/server/fakeip/ 、https://sing-box.sagernet.org/configuration/dns/rule/ 、https://sing-box.sagernet.org/configuration/dns/rule_action/ 、https://sing-box.sagernet.org/configuration/route/ 、https://sing-box.sagernet.org/configuration/outbound/urltest/ 、https://sing-box.sagernet.org/configuration/experimental/clash-api/ 、https://sing-box.sagernet.org/configuration/experimental/cache-file/ 、https://sing-box.sagernet.org/configuration/inbound/tun/
- sing-box Releases：https://github.com/SagerNet/sing-box/releases/tag/v1.12.0 、.../v1.13.0 、.../v1.14.0 、https://github.com/SagerNet/sing-box/tags
- mihomo：https://github.com/MetaCubeX/mihomo/releases （v1.19.0/4/6/7/15 各 tag）、https://wiki.metacubex.one/config/rule-providers/ 、https://wiki.metacubex.one/config/general/ 、https://wiki.metacubex.one/example/conf/ 、https://t.me/s/clashmeta?before=107
- 规则数据：https://github.com/MetaCubeX/meta-rules-dat 、https://api.github.com/repos/MetaCubeX/meta-rules-dat/releases/tags/latest
- Shadowrocket：https://t.me/s/ShadowrocketNews （及 ?before=428、?before=432、?q=hysteria2、?q=anytls、?q=mport、?before=372 等检索视图）
- URI 规范：https://v2.hysteria.network/docs/developers/URI-Scheme/ 、https://github.com/anytls/anytls-go/blob/main/docs/uri_scheme.md 、https://github.com/daeuniverse/dae/discussions/182 、https://github.com/v2fly/v2fly-github-io/issues/26 、https://github.com/2dust/v2rayn/wiki/Description-of-VMess-share-link
