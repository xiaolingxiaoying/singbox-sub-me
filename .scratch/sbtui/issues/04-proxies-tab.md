# 代理页：分组选择与延迟测试

Status: resolved
Type: task
Blocked by: 03

## 交付范围

- clash_api 客户端（GET/PATCH `/proxies`、GET `/proxies/<group>/delay`、PUT/DELETE 组选择）。
- 代理页 UI：左侧组列表 / 右侧组内节点（含当前选中、延迟数值着色：绿<200ms、黄<500ms、红/超时）。
- 交互：↑↓ 移动、Enter 选择节点、`t` 测当前节点、`T` 测全组（并发测、显示进度）、`r` 刷新。
- 出站模式切换（规则/全局/直连，PATCH `/configs` mode），仪表盘同步显示。

## 验收标准

- [ ] 真机连 sbctl 订阅：组内切换节点后新连接走所选节点（对照出口 IP）。
- [ ] 延迟测试数值显示正确，超时显示 Timeout。
- [ ] `cargo fmt/clippy/test` 通过（clash_api 客户端单测用本地 mock server）。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：代理页组/节点列表、Enter 切换、t/T 延迟测试、出站模式显示已实现。
