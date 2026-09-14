# 日志与规则页

Status: resolved
Type: task
Blocked by: 04

## 交付范围

- 日志页：tail 内核日志文件（`log.output`），自动滚动 + 暂停；级别过滤（info/warn/error）；复制当前行。
- 规则查看：从激活配置解析 route.rules 与 rule_set 列表展示（静态渲染，不做热更新）；`r` 刷新。

## 验收标准

- [ ] 日志实时滚动、暂停/恢复可用；终端缓冲不失控（上限 + 截断）。
- [ ] 规则页与激活配置一致。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：日志页（内核日志文件）与规则页已实现（Logs 页按 r 切换，静态渲染激活配置的 route.rules 与 rule_set 来源）。
