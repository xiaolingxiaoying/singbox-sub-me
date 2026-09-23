# core 快照发布入站摘要 + 两个 UI 渲染

Status: ready-for-agent
Type: task
Blocked by: sbctl-v0.2/issues/01

## 动作

1. `state.rs`：新增
   `InboundSnapshot { kind, tag, listen, port, mtu, stack, auto_route }`（字段按类型可选）。
2. `controller.rs`：解析 `cache/active-config.json` 的 `inbounds`，与 rules 相同的发布时机
   （内核启动时 + 引擎初始化读缓存），发布到快照。
3. TUI：日志页 `r` 规则视图顶部增加「── 入站 ──」区块；设置页概览区显示入站摘要。
4. GUI：规则页增加「入站」标签/区块；连接表增加「入口」列（优先 inbound tag，未知时显示 inbound_ip）。
5. 测试：解析单测（mixed/tun/未知类型容错）、TUI `rules_lines` 渲染断言、GUI 组件单测。

## 验收

- 两个客户端在同一份配置上显示一致。
- 未运行内核时显示缓存配置的入站；缓存缺失时显示空态而不是 panic。
