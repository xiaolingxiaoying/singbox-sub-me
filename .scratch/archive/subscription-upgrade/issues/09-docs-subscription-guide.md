# README 与订阅指南文档

Status: resolved
Type: task
Blocked by: 01, 02, 03, 04, 05, 06, 07, 08

## 目标

把新的订阅链接矩阵、版本标注、二维码与客户端导入方法写进文档。

## 交付范围

- README「订阅」章节重写：完整链接矩阵表（每条链接的内容、适用客户端与版本、对应二维码链接）、安全模型说明不变。
- 新增 `docs/subscription-guide.md`（中文）：
  - sing-box 各版本 profile 差异表（1.12 → 最新，逐版本标注：DNS 格式、弃用字段、适用建议）
  - mihomo 版本适配说明（现行版 vs 1.18 兼容版差异）
  - Shadowrocket 导入步骤与参数适配说明
  - QR/index 使用方法；override 覆写机制使用方法；DNS/rule_profile/latency_probe 配置说明
- CONTEXT.md 术语表补充：Version profile、Subscription route、Override template。
- 更新 `docs/installation.md`（certificate status、清单输出）。

## 验收标准

- [ ] README 链接矩阵与实际路由一一对应（人工核对表）。
- [ ] 指南中每个版本标注与研究结论一致，来源可追溯。
- [ ] 文档全中文、与现有文档风格一致。

## 相关规格

`.scratch/subscription-upgrade/spec.md`

## Comments

- 2026-09-14：README 订阅章节重写、docs/subscription-guide.md、CONTEXT.md 术语、ADR-0020/0021 完成。
