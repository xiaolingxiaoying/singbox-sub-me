# S11：subscription-userinfo 的 total 语义定案

Status: ready-for-agent
Type: task
Blocked by: 01

## 现状

- 实现：`monthly_traffic_limit > 0` 时 `total` 返回限额，否则返回当前已用总量
  （`src/subscription/serve.rs:397-401`）；测试同时钉住两种行为
  （`tests/cli/subscription_formats.rs:759,1080`）。
- 工单记录却说「已改为当前账期已用总量」（`.scratch/sbctl-release/issues/05:36`），文档与代码冲突。

## 决策（建议）

`total = 管理员配置的月流量上限`；未配置限额时不输出 `total`（而不是把已用量冒充总量，
客户端会把已用量当上限显示为 100%）。

## 动作

1. 定案后同步：`serve.rs`、`tests/cli/subscription_formats.rs`、
   `.scratch/sbctl-release/issues/05-read-only-traffic-subscription.md`、README/订阅指南。
2. 增加「无限额时不带 total」的用例；有额度时 `total == limit`。

## 验收

- 三个消费方（代码、测试、文档）表述一致。
- `cargo test -p sbctl --features test-signing` 相关用例通过。
