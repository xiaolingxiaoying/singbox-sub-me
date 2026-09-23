# 订阅体系升级 spec

Status: ready-for-agent
日期：2026-09-14

## 背景

当前 sbctl 的订阅产物只有四种（sing-box 纯 outbounds、Clash YAML、URI、Base64 URI）：

- sing-box 客户端订阅没有任何 DNS/route/代理组/clash_api（`src/subscription.rs::sing_box`），无法直接导入使用。
- Clash 订阅的 DNS、分流规则、代理组全部硬编码（`clash()`），不使用 rule-set，AI 分流规则由用户在 Clash Party 里手工覆写（`.scratch/vps-connectivity-hardening/novixlink-override.yaml`）。
- 没有任何二维码端点；`sbctl qr` 只在终端渲染当前唯一订阅 URL。
- 没有按 sing-box / mihomo 版本做差异化，也没有 Shadowrocket 专属适配。
- external-proxy 与 ip-fallback 两种订阅模式从未被端到端实测。

用户需求（原话归纳）：

1. sing-box 订阅写详细配置，按 1.12.0 → 最新每个版本做优化并标注，多给出几条订阅链接。
2. mihomo 系给出大版本更新后调整的订阅链接。
3. Shadowrocket 给出适配优化后的订阅链接。
4. 所有订阅链接都有可扫描的二维码链接。
5. DNS、Rule、外部资源、覆写、代理组在项目中进一步优化（配置化）。
6. VPS 配置繁琐易遗漏：邮箱、证书体验优化。
7. external-proxy 与 ip-fallback 两种模式需要测试打通。

## 设计决策

- **安全模型不变**：全部新路由仍在 `/sub/<credential>/...` 路径内，恒定时间凭据比较，拒绝 query 参数，错误一律 404，成功响应带 `subscription-userinfo`（index/QR 页除外，QR 为 image/svg+xml）。
- **存量兼容**：`sing-box.json`、`clash.yaml`、`uri`、`uri.txt` 四条既有链接的语义保持（`sing-box.json` 仍是纯 outbounds）；完整版、版本版、Shadowrocket 版走新链接。
- **版本 profile 化**：sing-box 完整客户端配置按 minor 版本生成 profile（1.12、1.13、…最新），差异来自官方 changelog 研究（见 issues/02），不凭记忆写字段；每个 profile 用对应版本真核 `sing-box check` 验证。
- **供应商中立（ADR-0018）**：rule_set 外部资源 URL 必须可配置（默认 jsDelivr + MetaCubeX/meta-rules-dat），提供 minimal（内置规则）/ standard（远程 rule-set）两档。
- **覆写在服务端**：新增 `etc/sbctl/overrides/` 深度合并机制 + `sbctl config override` CLI，把 Clash Party 手工覆写收编为服务端统一分发。
- **配置化**：`dns_mode`、`rule_profile`、`rule_set_base_url`、`latency_probe_url` 进入 `DeploymentConfig` + wizard。
- 证书/邮箱只做体验优化（状态命令、校验、清单输出），不改变现有 Certbot/deploy-hook 架构。

## 链接矩阵（目标态）

| 链接 | 内容 | 标注 |
| --- | --- | --- |
| `/sub/<cred>/sing-box.json` | 纯 outbounds（现状） | 全版本兼容 |
| `/sub/<cred>/sing-box-full.json` | 最新稳定版完整客户端配置 | sing-box 最新稳定版 |
| `/sub/<cred>/sing-box-1.12.json` | 1.12.x 优化完整配置 | sing-box 1.12.x |
| `/sub/<cred>/sing-box-1.13.json`… | 后续每个已发布 minor 一个 profile | 逐版本标注 |
| `/sub/<cred>/clash.yaml` | mihomo 现行稳定版优化配置 | 标注最低版本 |
| `/sub/<cred>/clash-1.18.yaml` | 旧大版本兼容版 | mihomo 1.18.x |
| `/sub/<cred>/uri`、`/sub/<cred>/uri.txt` | 明文/Base64 URI（现状） | 通用 |
| `/sub/<cred>/shadowrocket.txt` | Shadowrocket 适配 Base64 URI | Shadowrocket |
| `/sub/<cred>/qr/<format>` | 对应链接的二维码 SVG | 与上表一一对应 |
| `/sub/<cred>/index` | 中文总览页：全链接 + 标注 + 内嵌二维码 + 导入步骤 | — |

## 里程碑

- M1：链接矩阵路由 + sing-box 版本 profiles + 完整配置生成（issues 01–02）
- M2：mihomo 升级 + Shadowrocket 适配 + QR/index（issues 03–05）
- M3：DNS/规则/覆写/代理组配置化（issue 06）
- M4：邮箱/证书体验 + 双模式验收 + 文档（issues 07–09）
- 完成后用户在真机（Shadowrocket、Clash Party、SFA、V2rayN）验证一轮，再进入 `.scratch/sbtui/` 阶段。

## 边界

- 不做 WARP/Argo/Psiphon，不接管防火墙与反代（对齐 ADR-0018 与 README 安全边界）。
- 不引入 query 参数覆写；覆写只走服务端文件。
- sing-box 1.11 及更早版本不做 profile（用户明确从 1.12.0 起）。
