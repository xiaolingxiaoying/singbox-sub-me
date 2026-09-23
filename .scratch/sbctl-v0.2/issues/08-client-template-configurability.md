# S9：客户端模板可配置化（策略组/规则集/DNS/TUN）

Status: ready-for-agent
Type: task
Blocked by: 01

## 现状

- 策略组名/类型/成员硬编码（`src/subscription/render/mod.rs:110-114`、`render/clash.rs:69-101`），
  只有探测 URL 可配（`DeploymentConfig::client_latency_probe_url`）。
- rule-set/rule-provider 集合固定（sing-box 两个、clash 四个）；只能换 base URL。
- DNS 服务器列表、fake-ip 段、cache 选项、TUN 参数（地址/mtu/stack/auto_route）均固定。
- 覆写机制（ADR-0021）已能整段替换，但普通用户不便使用。

## 动作（先设计后实现）

1. 设计 `ClientTemplate` 扩展字段并写入 ADR：
   - `proxy_groups`: 名称/类型/成员顺序/默认选中/探测 URL/interval/tolerance；
   - `rule_sets`: 增删条目（tag/类型/URL/下载策略）；
   - `dns_servers`: 直连/代理两侧服务器列表、fake-ip 段、cache 选项；
   - `tun`: address/mtu/stack/auto_route/strict_route。
2. 配置校验：非法组合（空组、重复 tag、未知协议节点成员）在 `config validate` 报错。
3. 生成器按模板输出；字段与现有覆写合并语义保持一致（rules 前插）。
4. 向导「客户端模板」主题逐项可改；`sbctl config show` 输出脱敏摘要。
5. 测试：单测覆盖每个字段的默认值与自定义值；真核 CI（sing-box 五版本 + mihomo）保持通过；
   `config validate` 拒绝非法模板的用例。

## 验收

- 自定义策略组名/默认节点/规则集增删后，生成的 sing-box 与 clash 工件在真核通过。
- 默认值生成的工件与升级前逐字节一致（除有意新增字段），保证既有用户不漂移。
- ADR 与 `docs/subscription-guide.md` 更新。
