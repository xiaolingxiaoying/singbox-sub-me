# 实现订阅链接矩阵的路由扩展

Status: resolved
Type: task

## 目标

把订阅路由从 4 种固定格式扩展为可扩展的链接矩阵（新格式 + QR + index），保持现有安全模型。

## 交付范围

- `src/subscription.rs::parse_route` 重构为返回「凭据 + 路由目标」的结构，路由目标涵盖：
  既有 4 格式、`sing-box-full.json`、`sing-box-<ver>.json` 系列、`clash-1.18.yaml`、`shadowrocket.txt`、`qr/<format>`、`index`。
- `SubscriptionFormat` 扩展（或引入 `SubscriptionRoute` 枚举）：每种路由有 path_name / artifact_name / content_type；QR 为 `image/svg+xml`，index 为 `text/html; charset=utf-8`。
- 新工件名：`subscription-sing-box-full.json`、`subscription-sing-box-<ver>.json`、`subscription-clash-1.18.yaml`、`subscription-shadowrocket.txt`，纳入 `generated_artifacts` 与事务化写入/回滚。
- `read_authorized`/`subscription_http_response` 适配新路由；QR 与 index 页不需要 `subscription-userinfo`。
- `sbctl sub`：输出全矩阵表格（链接 + 内容说明 + 标注）；`sbctl sub --format <id>` 保持可用。
- `sbctl qr [format]`：按格式渲染终端二维码（缺省 `sing-box-full`）。
- 既有行为不回归：`uri`、`uri.txt`、`sing-box.json`、`clash.yaml` 响应字节不变。

## 验收标准

- [ ] 全部新路由 GET 200 且 Content-Type 正确；错误凭据/未知路径/带 query 一律 404。
- [ ] 既有 4 格式的响应内容与升级前逐字节一致（存量客户端不受影响）。
- [ ] `sbctl sub` 打印全矩阵与标注；`sbctl qr sing-box-full` 可渲染。
- [ ] `cargo fmt/clippy/test` 通过；新增单测覆盖 parse_route 与响应头。

## 相关规格

`.scratch/subscription-upgrade/spec.md`

## Comments

- 2026-09-14：路由矩阵、parse_route 重构、CLI sub/qr、sbctl sub 全矩阵均已实现并在 debian:12/ubuntu:22.04 验收通过。
