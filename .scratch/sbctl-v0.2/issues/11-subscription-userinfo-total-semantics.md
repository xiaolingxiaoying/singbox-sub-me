# S11：subscription-userinfo 的 total 语义定案

Status: resolved
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

## Comments

采纳决策建议：`total` 代表管理员设置的月度额度；`monthly_traffic_limit == 0` 时完全省略 `total`，而不是输出当前用量。这样客户端不会把已用量误当作总量或额度；`upload`、`download`、`expire` 与 `profile-update-interval` 仍正常输出。有额度的分支保持 `total=<configured limit>`。

实现修改 `subscription_userinfo()` 的两种分支，并同步 `docs/subscription-guide.md` 与 `.scratch/sbctl-release/spec.md`。测试覆盖有额度时 `total=999`、无限额时无 `total`，以及 total-only correction 后响应仍不出现伪额度。

验证：

- TDD red：更新无限额精确等式后，旧实现失败，实际仍打印 `total=112`。
- `cargo test -p sbctl --features test-signing the_userinfo_header_locks_its_key_order_and_names`：通过。
- `cargo test -p sbctl --features test-signing --test cli subscription_userinfo_total_reflects_a_total_only_correction`：通过。
- `cargo test -p sbctl --features test-signing`：202 个库测试、93 个 CLI 测试与版本 profile 测试通过。
- 初次全 workflow run `36131353579` 的 server-acceptance 发现旧验收脚本仍要求无限额返回 `total=0`；现已更新它与新语义一致，要求不输出 `total` 并显式拒绝伪额度。修复后的全量 Actions run `36133020517` 全部通过。VPS 当前生产二进制摘要 `45dacacda646d9b433a6a41455ee7c596accfceb2a23830db0bf59a66776793f`，由 run `36131353579` 构建；部署后的 HTTPS 订阅返回 200，实际配置额度分支仍正确输出 `total=536870912000`。
