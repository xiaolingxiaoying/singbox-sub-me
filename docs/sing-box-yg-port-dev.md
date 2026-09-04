# sing-box-yg 功能移植开发文档

日期：2026-09（对齐仓库，以提交为准）
前置：见 `docs/sing-box-yg-port-plan.md`（功能分析与优先级）。本文只写**已确认移植项**的落地设计、文件改动、验证与验收。
原则：遵守本仓库 ADR——非 root（0012）、发布签名清单（0014）、事务化/回滚（0007）、凭据与端点分离（0002）、Rust 控制面边界（0006）、供应商中立（0018）。

---

## 0. 范围

本期移植（阶段 0 + 阶段 1 + 阶段 2 的可选部分）：

| 编号 | 功能 | 优先级 |
| --- | --- | --- |
| F1 | 服务端 5 inbound 加 `sniff` + `sniff_override_destination` | P0 |
| F2 | vmess-ws 加 `max_early_data` + `early_data_header_name` | P0 |
| F3 | tuic `congestion_control=bbr`（已具备，仅确认） | P0 |
| F4 | hy2/tuic 端口跳跃（`port_range` + iptables + 客户端 `ports`/`mport`） | P1 |
| F5 | 完整客户端配置（可选：`dns`/`tun`/`route`/`clash_api`/`selector`/`urltest`） | P1 |
| F6 | 内核 BBR+FQ 系统调优（可选命令/文档） | P2 |

**明确不做**：WARP-Wireguard、Argo/Cloudflare 隧道、Socks5/Psiphon（sbwpph）、acme-yg、GitLab 订阅、Telegram、防火墙清理、geosite 多通道分流（依赖上述出站）。理由见计划文档 4.4。

---

## 1. F1 — 服务端 sniff / sniff_override_destination

### 设计
在 `src/subscription.rs::sing_box_server` 中，给每个 inbound 的 JSON 对象加两个字段：
```json
"sniff": true,
"sniff_override_destination": true
```

### 改动点
- `src/subscription.rs::sing_box_server`（当前 `fn sing_box_server`，生成 `inbounds`）：
  - 在 5 个分支（VlessReality / VmessWebsocket / Hysteria2 / Tuic / Anytls）的 `json!({...})` 里统一加入 `"sniff": true, "sniff_override_destination": true`。
- 由于每个分支都返回一个独立 `json!`，可先构造一个共享的 `server_common`（或直接用字段合并），避免重复。推荐：抽出
  ```rust
  fn inbound_tls_common(cert: &Value, server_name: &str, alpn: &[&str]) -> Value
  ```
  或在 `json!` 里逐字段写。为最小改动，直接在 5 处 `json!` 内加两行即可。

### 验证
- 单测：`subscription::tests` 增加「server 配置含 sniff」断言（在 `no_domain_ip_fallback_artifacts...` 或新增 server 配置测试，需 root 写 `/var/lib` 的用例除外）。
- `cargo test --lib`；若跑真实 `sing-box check` 则用 `check_sing_box_config`。
- 验收：`sbctl config init` 后生成的 `sing-box-server.json` 中 5 个 inbound 均含 `sniff`。

---

## 2. F2 — vmess-ws max_early_data

### 设计
`transport` 对象加：
```json
"max_early_data": 2048,
"early_data_header_name": "Sec-WebSocket-Protocol"
```

### 改动点
- `src/subscription.rs::sing_box_server` 的 `CanonicalNode::VmessWebsocket` 分支：
  - `"transport": {"type": "ws", "path": path}` → `"transport": {"type": "ws", "path": path, "max_early_data": 2048, "early_data_header_name": "Sec-WebSocket-Protocol"}`。
- `src/subscription.rs::sing_box`（客户端 outbound）的 vmess 分支：`"transport": {"type": "ws", "path": path}` → 同样加 `max_early_data`/`early_data_header_name`（客户端侧对应字段，sing-box 客户端也支持）。
- `src/canonical.rs` 无需改动（path 已在 `CanonicalNode::VmessWebsocket`）。

### 验证
- 单测：订阅产物（sing-box server 与 client）的 vmess 节点含 `max_early_data`。
- 注意：若走 Clash/Mihomo（`clash` 渲染），Mihomo 的 `ws-opts` 需对应的 `max-early-data`/`early-data-header-name`；确认后按需补充（`src/subscription.rs::clash`）。

---

## 3. F3 — tuic congestion_control=bbr（确认项）

- `src/subscription.rs::sing_box`/`sing_box_server`/`uri` 的 tuic 分支已含 `"congestion_control": "bbr"` 与 `congestion_control=bbr`。无需改动，仅在测试中固化断言。

---

## 4. F4 — hy2/tuic UDP 端口跳跃

### 设计
- 配置：`DeploymentConfig` 增加可选 `udp_port_range: Option<String>`（如 `"10000-10100"`），或按协议分别 `hysteria2_port_range`/`tuic_port_range`（推荐按协议分开，更贴合现模型）。存入 `config.toml`，`validate()` 校验格式（`<start>-<end>`、10000-65535、start<=end、不与协议单端口/HTTP 端口冲突）。
- 服务器侧：生成 iptables NAT 规则，把该 UDP 端口区间的入站包 DNAT 到 hy2/tuic 的实际监听端口：
  ```
  iptables -t nat -A PREROUTING -p udp --dport <start>:<end> -j DNAT --to-destination 127.0.0.1:<actual_port>
  ```
  （具体目标地址依配置，IPv4/IPv6 分别处理）。由管理员以 root 通过 `sbctl` 命令执行，需**幂等**（先删除同名规则再插入）且**可回滚**（记录删除命令）。
- 客户端侧：订阅产物
  - hy2 URI/Clash/sing-box：`ports` 或 `mport=<range>`。
  - tuic URI/Clash/sing-box：`ports=<range>`。
- 端口范围变更走 ADR-0007 的事务化路径（校验 → 应用 → 若失败回滚）。

### 改动点
- `src/config.rs`：
  - `DeploymentConfig`/`DeploymentOptions` 增加 `hysteria2_port_range`、`tuic_port_range: Option<String>`（`#[serde(default)]`）。
  - `validate()`：校验范围格式与冲突；并保证范围不与 `http_port`、其他协议端口、另一个范围重叠。
  - `new_with_ports`/`apply_options` 接入。
- `src/subscription.rs`：
  - `uri`/`clash`/`sing_box`：hy2 与 tuic 分支在设置了范围时输出 `mport`/`ports`。
- 新增 `src/firewall.rs`（或并入 `lifecycle.rs`）：
  - `install_udp_port_hop_rules(config)` / `remove_udp_port_hop_rules(config)`：幂等生成/删除 iptables 规则（root 由管理员调用）。
  - 提供 dry-run 输出命令供人工执行。
- `src/main.rs`：新增 `sbctl firewall port-hop {enable|disable|status}` 子命令（或并入 `deploy`），仅在管理员 CLI 下调用。

### 验证
- 单测：范围校验、订阅产物含 `mport`/`ports`。
- 验收（需 root 环境）：`sbctl firewall port-hop enable` 后 `iptables -t nat -L` 有对应规则，`disable` 后清除；hy2/tuic 客户端可用 `mport`/`ports` 连上。
- 安全：规则必须可回滚、幂等、不影响其他协议/应用；明确提示「需 root、建议 dry-run」。

---

## 5. F5 — 完整客户端配置（SFA/SFI/SFW）

### 设计
- 默认仍输出当前「纯 outbounds」的 `subscription-sing-box.json`（保持兼容）。
- 新增可选开关 `client_profile: Option<String>`（如 `basic`/`full`），当 `full` 时生成含以下段的 `subscription-sing-box-full.json`：
  - `log`、`dns`（fake-ip + `direct`/`proxy` 服务器组，`detour` 到 `proxy` 出站）、`inbounds.tun`（gvisor）、`route`（`rule_set` 拉 `geosite-cn.srs`/`geoip-cn.srs` 或内置直连域名）、`experimental.clash_api`（`127.0.0.1:9090`）、`outbounds`（5 节点 + `selector`/`urltest` + `direct`）。
- 遵循 ADR-0018：`route`/`dns` 用**可配置**的规则来源（默认内置 minimal 规则，不强制远程 rule_set），保持供应商中立。

### 改动点
- `src/subscription.rs`：新增 `sing_box_full_client(config, nodes)`；在 `generated_artifacts` 中按开关追加 `sing-box-full.json`。
- `src/config.rs`：`DeploymentConfig`/`DeploymentOptions` 增加 `client_profile`（`#[serde(default)]`，默认 `None`/`basic`）。
- `src/main.rs`：CLI 增加 `--client-profile`（install/config init）。
- `src/wizard.rs`：可选提问。

### 验证
- 单测：`full` 配置含 `dns`/`inbounds.tun`/`route`/`selector`/`urltest`，且 `basic` 不含。
- 验收：SFA/SFI/SFW 可导入 `sing-box-full.json` 并正常连上。

---

## 6. F6 — 内核 BBR+FQ 系统调优

### 设计
- 这是**系统级**动作，需要 root，与数据面解耦。
- 提供两种落地：
  1. `sbctl` 文档化命令（README/安装文档）：说明如何设置
     ```
     sysctl -w net.core.default_qdisc=fq
     sysctl -w net.ipv4.tcp_congestion_control=bbr
     ```
     并固化到 `/etc/sysctl.d/`。
  2. 或新增 `sbctl system bbr`（需 root）子命令，幂等写 sysctl 并持久化，返回当前值。

### 改动点
- 推荐先做**文档化**（README「性能调优」章节 + 可选 `sbctl system bbr` 子命令）。
- 若做子命令：`src/main.rs` 新增 `SystemCommand::Bbr`，写 `/etc/sysctl.d/99-sbctl-bbr.conf` 并 `sysctl -p`，检查当前 `tcp_congestion_control`。
- 明确：此命令不改变 sing-box 配置、不依赖内核版本检测，仅做 sysctl 写入；低内核（<4.9）不适用需提示。

### 验证
- 仅文档/系统命令，不进入数据面测试；单测针对「写 sysctl.d 文件内容」的纯函数。

---

## 7. 涉及文件汇总

| 文件 | 改动 |
| --- | --- |
| `src/config.rs` | 新配置字段（`hysteria2_port_range`/`tuic_port_range`/`client_profile`）+ `validate()` |
| `src/canonical.rs` | 无（字段已在节点模型） |
| `src/subscription.rs` | sniff、max_early_data、`ports`/`mport`、`sing_box_full_client` |
| `src/firewall.rs`（新） | iptables 端口跳跃规则生成/删除/status |
| `src/main.rs` | CLI：`--client-profile`、`firewall port-hop`、`system bbr` |
| `src/wizard.rs` | 可选提问（client_profile、端口范围） |
| `src/lifecycle.rs` | 若端口规则纳入安装生命周期（可选） |
| `README.md` | 性能调优（BBR）、端口跳跃、完整客户端配置说明 |
| `tests/` | 相应单测 + 验收用例 |

---

## 8. 里程碑与验收标准

- **M0（F1/F2/F3）**：单测全绿；`sing-box check` 通过；订阅产物含 `sniff`/`max_early_data`。改动最小、无配置破坏。
- **M1（F4）**：配置校验 + 订阅产物含 `mport`/`ports`；`iptables` 规则幂等/可回滚；`cargo clippy` 干净。
- **M2（F5）**：`--client-profile full` 生成完整客户端配置；`basic` 保持现状；可导入 SFA 验证。
- **M3（F6）**：`sbctl system bbr` 幂等写入 sysctl 并持久化（或文档化）。

## 9. 风险与边界

- **端口跳跃（F4）**：iptables 操作需 root，须幂等/回滚、dry-run，避免影响其他协议与应用（对齐 ADR-0007）。
- **完整客户端配置（F5）**：`dns`/`route` 若引入远程 `rule_set` 会绑定 CDN 与外部规则源，违反 ADR-0018；默认用内置 minimal 规则，`full` 才扩展。
- **BBR（F6）**：系统级，需 root；与数据面解耦；老内核不适用需提示。
- **不做项**：WARP/Argo/Psiphon 依赖外部服务与第三方二进制，维持「不做」，保持单机自建节点、供应商中立。
