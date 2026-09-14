# 流量与连接面板

Status: resolved
Type: task
Blocked by: 04

## 交付范围

- clash_api `/traffic` 轮询（1s）：仪表盘实时上行/下行速率（人性化单位）+ 累计流量（会话内）。
- 连接页：`/connections` 表格（目标、规则、链路、上下行、开始时间），5s 自动刷新 + 手动刷新；`x` 关闭选中连接、`X` 关闭全部；按列排序。
- 订阅流量元数据显示：解析 `subscription-userinfo`（档案更新时缓存），仪表盘显示已用/总量/重置时间。

## 验收标准

- [ ] 速率曲线/数值随真实流量变化；连接列表反映当前连接，关闭后连接断开。
- [ ] `cargo fmt/clippy/test` 通过（traffic/connections 解析单测）。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：速率（connections 总量差分）、连接表、关闭连接、subscription-userinfo 展示已实现。
