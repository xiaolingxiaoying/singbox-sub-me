# 客户端入站规则视图（client-core + TUI / GUI）

Status: ready-for-agent

## 目标（目标文档原文）

TUI 必须「显示入站，出站规则」。出站侧已实现（日志页 `r` 切换分流规则视图），入站侧无数据源、无界面。

## 现状

- `ClientSnapshot` 没有 inbounds 字段（`crates/client-core/src/state.rs:302-341`）。
- `core::adapt_inbounds` 只写不读（`core.rs:549-587`）。
- 连接元数据里有 `inbound_ip`（`clash_api.rs:132-133`），但两个 UI 都不显示入站 tag。

## 工单

- `issues/01-snapshot-and-ui.md`

## 验收

- 内核运行与未运行（读缓存配置）两种状态都能显示入站摘要。
- TUN 模式下显示 tun 的 address/mtu/stack/auto_route；系统代理模式显示 mixed 监听地址与端口。
- 不泄露任何凭据（入站本身无凭据，但仍需测试字符串不含 UUID/password）。
