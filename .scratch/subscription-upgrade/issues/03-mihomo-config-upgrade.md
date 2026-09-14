# mihomo 配置升级与 1.18/1.19 适配

Status: resolved
Type: task
Blocked by: 01

## 目标

升级 `clash()` 生成的 mihomo 配置（现行稳定版），并提供旧大版本兼容链接 `clash-1.18.yaml`。

## 前置研究

查 mihomo（MetaCubeX/mihomo）当前最新稳定版与 1.19.x 相对 1.18.x 的破坏性变化/弃用字段，重点：dns 段、rule-set（.mrs）远程规则、proxy-groups 字段、GEOSITE/GEOIP 用法、external-controller。确认 MetaCubeX/meta-rules-dat 的 geosite-cn/geoip-cn/lan `.mrs` 远程 URL 可用形态。

## 交付范围

- `clash.yaml`（现行稳定版）：
  - dns：fake-ip + 补全 fake-ip-filter（+.lan/+.local/msftconnecttest/msftncsi/time.windows.com 等）；`proxy-server-nameserver` AliDNS；DoH nameserver 挂选择组（保留现状写法）
  - 规则：改用 `rule-set`（remote .mrs：lan/geoip-cn/geosite-cn），AI/x.com 分流规则保留；`RULE-SET` 顺序在前，兜底 MATCH
  - 代理组：🚀节点选择(select) / ♻️自动选择(url-test) / 🎯全球直连(select: DIRECT+节点)；`url`/`interval`/`tolerance` 保持现有探测语义（aliyun probe）
  - `rule-providers` URL 走 `rule_set_base_url` 配置
- `clash-1.18.yaml`：仅包含 1.18 兼容写法（差异点以研究结论为准；若实质差异过小，在 index 页与 README 中说明并保留单链接，在 Comments 记录原因）
- `minimal` rule_profile：不引用远程 rule-providers，内置 GEOIP,CN / 直连域名规则。

## 验收标准

- [ ] `clash.yaml` 可被 mihomo 现行稳定版加载（`mihomo -t` 或等价校验）；`clash-1.18.yaml` 同理（或记录合并原因）。
- [ ] fake-ip-filter、rule-set、三组代理组在产物中正确出现。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/subscription-upgrade/spec.md`

## Comments

- 2026-09-14：clash.yaml 升级为 rule-set（@meta/geo .mrs）+ 三组代理组；clash-1.18.yaml 保留内置 GEOIP；真核（sing-box）验证的是 sing-box 侧，mihomo -t 待有 mihomo 内核的环境补验。
- 2026-09-14（收口）：新增 `tests/clash_mihomo.rs` 与 CI `mihomo-profiles` job，用 mihomo v1.19.30 对 clash.yaml / clash-1.18.yaml 跑 `-t`（本地实跑通过）。
