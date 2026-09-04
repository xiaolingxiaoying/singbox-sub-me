# 无域名 5 节点方案：sing-box-yg 实现分析与 sbctl 改造计划

日期：2026-09（对齐仓库，实际以提交为准）
范围：分析 sing-box-yg 如何做到"不需要域名也能用满 5 个协议节点"，并给出在 sbctl 上落地该能力的改造计划。
来源：`docs/sing-box-yg/`（04、06、02、01）、`.reference-sing-box-yg/sb.sh`、sing-box 核心约定。

---

## 0. 结论速览

sing-box-yg 的"无域名也能跑 5 个节点"这一能力，本质是**把"证书/握手里绑定的名字"与"真实可解析的域名"彻底解耦**：

1. VLESS Reality 本来就不需要证书，只用一个"伪装域名"(decoy SNI) 做真实站点指纹伪装。
2. 另外 4 个 TLS 协议（VMess WS / Hysteria2 / TUIC / AnyTLS）改用**自签证书 + 一个固定的假 SNI(`www.bing.com`)**，并要求客户端 `skip-cert-verify`（或 `pinSHA256`）。证书不必是真的、域名不必能解析。
3. 所有节点的服务器地址填的是 **VPS 的 IP**，不是域名。
4. 订阅分发同样不依赖域名：用脚本内置裸 HTTP 服务(`websbox`)在**随机高端口**暴露 `http://<服务器IP>:<端口>/<token>/…`（token = 节点 UUID），备选推私有 GitLab。

而 sbctl（本项目）目前"具备了一半"：已经有 `certificate_mode = SelfSigned`（自动生成自签证书 + 客户端自动 `skip-cert-verify`），已经有 `ip-fallback` 的明文 HTTP IP 订阅；但被**两处限制**卡死，无法在 IP 模式下启用除 VLESS 外的 4 个协议。

**一句话密钥**：只要"客户端与服务端对同一个假域名完成握手、且客户端跳过证书校验"，就能在没有真域名的情况下把 5 个协议都用起来。sbctl 缺的是**允许这 4 个协议 + 给它们一个独立的假 SNI/自签证书**。

---

## 1. sing-box-yg 无域名机制详解

### 1.1 4 个 TLS 协议：自签证书 + 假 SNI + skip-cert-verify

| 协议 | 服务端 inbound 关键字段 | 客户端链接关键参数 |
| --- | --- | --- |
| VMess WebSocket | `tls.server_name=www.bing.com`、自签 bing 证书 | `tls:"tls"`、`sni=www.bing.com` |
| Hysteria2 | 自签证书、`alpn:["h3"]` | `sni=www.bing.com`、`pinSHA256=<自签指纹>`（或 `insecure`） |
| TUIC v5 | 自签证书、`congestion_control=bbr`、`alpn:["h3"]` | `sni=www.bing.com`、`insecure=1` |
| AnyTLS | 自签证书 | `sni=www.bing.com`、`allowInsecure=1` |

要点（见 `04-five-protocol-nodes.md`）：
- 服务端地址是 VPS IP（`$sb_*_ip`），**不是域名**。
- `server_name`/`sni` 用假域名 `www.bing.com`，自签证书 CN 也指向它。
- 客户端因 `skip-cert-verify`（或 `pinSHA256`）不校验证书真伪，因此"域名是否是真实存在/可解析"完全不重要，只要客户端和服务端对同一个字符串握手即可。

### 1.2 VLESS Reality：天然不需要证书

- `tls.reality.handshake.server = <伪装域名>`（默认 `apple.com`）+ `private_key`/`short_id`。
- 客户端 `security=reality`、`sni=<伪装域名>`、`pbk=<公钥>`、`sid=<short_id>`、`fp=chrome`。
- Realty 只在 TLS 层伪装成真实站点的指纹，不需要也不校验真实证书。

### 1.3 订阅分发：也无需域名

- 本地订阅：脚本内置 HTTP 服务 `websbox`，监听随机高端口，目录 `/root/websbox/<token>/` 软链 `clmi.yaml`、`sbox.json`、`jhsub.txt`，地址形如 `http://<服务器IP>:<端口>/<token>/clmi.yaml`。
- token 默认取 `sb.json` inbounds[0].users[0].uuid。
- 备选：推到私有 GitLab（`https://gitlab.com/api/v4/projects/<id>/repository/files/...`），用 token 作凭据。
- 注意：上述本地订阅**无 TLS、无认证、无速率限制**，也是 vps-sub-meter 项目点名要规避的弱点。

### 1.4 全链路（产物视角）

```
sing-box 内核 (-c /etc/s-box/sb.json)
   └─ 服务端 5 inbound：vless-reality / vmess-ws / hy2 / tuic / anytls（自签证书，SNI=www.bing.com）
入口（任选其一拿到配置）：
   ├─ 终端打印的分享链接 + 二维码
   ├─ /etc/s-box/sbox.json → SFA/SFI/SFW
   ├─ /etc/s-box/clmi.yaml → Mihomo / Clash.Meta
   ├─ /etc/s-box/jhsub.txt → 聚合订阅
   └─ http://<服务器IP>:<端口>/<token>/... → 本地订阅（websbox）
```

---

## 2. sbctl 现状：已具备 vs. 卡点

### 2.1 已经具备的能力

- `certificate_mode = SelfSigned`：`src/subscription.rs::ensure_self_signed_certificate` 自动生成自签证书（长有效期）。
- 客户端工件自动 `skip-cert-verify` / `insecure`：`src/subscription.rs::client_skip_cert_verify`。
- `ip-fallback` 订阅模式：`http://<IP>:<http_port>/sub/<凭据>/...` 明文分发（`src/subscription.rs::subscription_url`）。
- 统一节点模型：`src/canonical.rs::nodes()`，host 可取 IP。

### 2.2 卡点

| 卡点 | 位置 | 说明 |
| --- | --- | --- |
| ① 验证禁止 4 个 TLS 协议 | `src/config.rs` `validate()`（`SubscriptionMode::IpFallback` 分支） | 只要启用 `vmess-websocket/hysteria2/tuic/anytls` 就报 `"VMess WebSocket, Hysteria2, TUIC, and AnyTLS require a domain subscription mode"`。该理由并不成立——它们只需自签 + 假 SNI。 |
| ② SNI 锁死 = 订阅主机 | `src/canonical.rs::nodes()`：`let tls_server_name = &config.subscription_host;` | ip-fallback 时 `subscription_host` 是 IP，导致 TLS 协议 `sni` 与自签证书 `CN` 都是 IP。IP 不适合作 SNI / 证书 CN。sing-box-yg 用假域名解决；sbctl 缺这个解耦字段。 |
| ③(连带) 自签证书 CN | `src/subscription.rs` `ensure_self_signed_certificate`：`CertificateParams::new(vec![config.subscription_host.clone()])` | IP 作为 CN 不合适，需改为假 SNI。 |

> 结论：**订阅分发（HTTP over IP）已经有了；缺的是"允许这 4 个协议 + 给它们一个独立的假 SNI / 自签证书"。**

---

## 3. 改造计划

### 3.0 核心设计

给部署新增一个**"协议 TLS 服务器名" `protocol_sni`**——它是 `vmess/ht2/tuic/anytls` 的 `sni` 以及自签证书的 `CN`，与 `subscription_host`（可解析域名）解耦。

取值规则：
- `certificate_mode == SelfSigned` 且 `subscription_host` 为 IP（无域名）→ 用配置的假 SNI（默认 `www.bing.com`，可改）。
- 域模式（`subscription_host` 为域名）→ 默认等于 `subscription_host`（现状不变）。

### 3.1 Phase 1 — 配置模型

- `src/config.rs`：
  - `DeploymentConfig` / `DeploymentOptions` 增加 `protocol_sni: Option<String>`。
  - `new_with_ports` / `apply_options` 接入，TOML 序列化(`#[serde(default, skip_serializing_if)]`)。
  - 默认：域模式= `subscription_host`；无域名 IP 模式 = `www.bing.com`。

### 3.2 Phase 2 — 放开验证

- `src/config.rs::validate()`：
  - `ip-fallback`（或无域名）分支**允许** `vmess-websocket/hysteria2/tuic/anytls`，条件：`certificate_mode == SelfSigned` 且 `protocol_sni` 为合法主机名。
  - 保留既有安全约束：`subscription_host` 必须是 IP、`http_port > 1024`、不与协议端口冲突。
  - 移除原来的 `"require a domain subscription mode"`。

### 3.3 Phase 3 — 节点 SNI 解耦

- `src/canonical.rs::nodes()`：把 `tls_server_name` 从 `config.subscription_host` 改为按 `protocol_sni` 取值。host 仍取 `proxy_host`(默认 `subscription_host`)，在无域名场景为 IP。
- 更新 `canonical.rs` 相关单测。

### 3.4 Phase 4 — 证书按 SNI 生成

- `src/subscription.rs`：
  - `certificate_tls_config` / `ensure_self_signed_certificate`：自签证书的 `CN` 与 TLS `server_name` 用 `protocol_sni`（主机名），不再使用 IP。
  - VLESS Reality 保持不变（用 `reality_decoy_sni`）。

### 3.5 Phase 5 — 客户端工件带 SNI + skip-verify

- `src/subscription.rs` 三份工件（sing-box / Clash / URI）：
  - `sni` / `server_name` 统一指向 `protocol_sni`。
  - 已由 `client_skip_cert_verify` 输出 `insecure`/`skip-cert-verify`，保持不变。
  - 确保 SFA / Mihomo / 通用订阅客户端三端都能用。

### 3.6 Phase 6 — 简化使用路径（对齐 sing-box-yg 的"一键"）

- 交互式安装 / `menu_install`：无域名（IP）分支**默认启用全部 5 个协议** + `SelfSigned` + 假 SNI。
- `scripts/install.sh`：去掉 ip-fallback 时的 `--disable-protocol vmess-websocket --disable-protocol hysteria2 --disable-protocol tuic --disable-protocol anytls`。
- 配置向导新增"协议伪装域名（protocol_sni）"选项，默认 `www.bing.com`。
- 结果：一行命令 → 选 IP 模式 → 直接拿到 5 节点 + `http://<IP>:<端口>/sub/...` 的订阅，无需域名、无需 certbot。

### 3.7 Phase 7 — 测试与文档

- 单测：`config.rs`(ip-fallback + SelfSigned + 5 协议合法、SNI=假域名)、`canonical.rs`、`subscription.rs`（工件含 `sni` + `insecure`）。
- 验收：`tests/acceptance/verify-real.sh` 增加一个"无域名 5 节点"真实 systemd 安装用例（fake sing-box 校验）。
- 文档：README / `docs/` 新增"无域名部署"章节。

---

## 4. 需要拍板的点

1. **模式命名**：放宽 `ip-fallback`，还是新增更贴切的 `ip` / `no-domain` 模式？
   - 建议：保留 `ip-fallback` 并放开（B 改动最小），后续可加别名。
2. **假 SNI 默认值**：`www.bing.com`（与 sing-box-yg 一致）？还是 `www.cloudflare.com` / 每节点随机？建议做成可配置、默认 `www.bing.com`。
3. **安全取舍**：自签 + `skip-cert-verify` 存在中间人可重构的风险（无真实证书校验）；明文 HTTP 订阅可能被嗅探/盗链。
   - 现状 `ip-fallback` 已用 path credential，建议保留；可在文档中明确提示该模式的安全边界。

---

## 5. 参考来源

- `docs/sing-box-yg/04-five-protocol-nodes.md`：5 协议 inbound / 客户端 share link / 自签与域名证书两态。
- `docs/sing-box-yg/06-client-configs-subscription.md`：`websbox` 本地 HTTP 订阅、GitLab 订阅、客户端配置生成。
- `docs/sing-box-yg/02-command-and-install-flow.md`、`01-repositories.md`：sing-box-yg 安装/依赖链路（acme-yg、warp-yg、cloudflared 等）。
- `.reference-sing-box-yg/sb.sh`：sing-box-yg 实际脚本（`inssbjsonser`、`resvless`/`resvmess`/…/`websbox`）。
- `.reference-sing-box/option/`：sing-box 各协议 inbound 结构。
  - 说明：`warp-yg`、`acme-yg`、`cloudflared`、`argosbx`、`meta-rules-dat`、`sing-box`、`vps-sub-meter` 等在 `.reference-*` 中可本地查看；本方案未依赖 warp/argo，仅借鉴其证书与 SNI 解耦手法。
