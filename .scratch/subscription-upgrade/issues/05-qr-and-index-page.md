# 二维码 SVG 端点与 index 总览页

Status: resolved
Type: task
Blocked by: 01

## 目标

每条订阅链接都有可扫描的二维码链接，并提供中文总览页。

## 交付范围

- `qr/<format>`：对应该格式订阅 URL 的二维码 SVG（`qrcode` crate 的 SVG 渲染；无新增重依赖），Content-Type `image/svg+xml`，`Cache-Control: no-store`。
- `index`：中文 HTML 总览页（自包含、无外部资源）：
  - 全链接矩阵表格：链接、内容说明、适用客户端与版本标注
  - 每条链接内嵌二维码 SVG（手机扫码即导入）
  - 各客户端导入步骤（Shadowrocket / Clash Party / SFA / SFI / V2rayN / sing-box 命令行）
  - 流量信息（复用 `subscription-userinfo` 数据源）
- `sbctl sub` 展示每条链接的 QR 链接；`sbctl qr --all` 渲染全部终端二维码。

## 验收标准

- [ ] `qr/<format>` 返回合法 SVG 且像素图案与终端渲染一致（可解析校验）。
- [ ] index 页 HTML 包含全部矩阵条目、标注与二维码；无外部资源引用。
- [ ] 错误凭据/未知 format 一律 404；响应头与现有安全语义一致。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/subscription-upgrade/spec.md`

## Comments

- 2026-09-14：qr/<format> SVG 端点与 index 中文总览页已实现并在验收中断言。
- 2026-09-14（收口）：`src/qr.rs` 新增 SVG 可解析性与确定性单测；`sbctl qr --all` 渲染矩阵全部格式（CLI 测试按 ANSI 块数量断言）。
