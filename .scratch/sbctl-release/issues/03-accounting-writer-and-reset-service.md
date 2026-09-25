# 实现受限 accounting state writer 与 reset timer

Status: resolved
Type: task
Blocked by: 02

## 目标

将 accounting state 的写入权限制为 reset service/timer 和显式管理员修正命令，并实现可恢复的周期 reset reconciliation。

## 交付范围

- `sbctl-accounting-reset.service` 与每分钟运行的 `sbctl-accounting-reset.timer`。
- `Persistent=true`、cycle key 去重、跨停机补执行。
- 指定网卡 RX+TX 采集、首次观察、正常增量、boot ID 变化和 counter rollback。
- operation lock、临时文件、atomic rename 及状态 schema 校验。
- 为 traffic/status/subscription 提供只读 state 读取路径。

## 验收标准

- [x] 同一 cycle key 的重复 timer 执行不重复建立或覆盖账期。
- [x] 跨月停机后 timer 恢复并建立正确 baseline。
- [x] boot ID 改变或单方向 counter rollback 时保留既有累计值，并保留另一方向的有效增量。
- [x] HTTP 订阅、`status` 和普通 `traffic` 读取不写 accounting state。
- [x] 并发读取永远只能看到完整 state 文件。
- [x] systemd unit 明确 `Persistent=true` 且可在 fixture/真实 systemd 中验证。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0016、ADR-0017

## Comments

- 2026-09-25：本地 `cargo test -p sbctl --lib --features test-signing` 通过（199 passed）。新增 `traffic::tests::a_reset_after_several_missed_periods_starts_at_the_current_period_baseline`，验证 timer 错过多个自然月后第一次恢复执行会为当前周期建立新 baseline，后续同周期执行只累计计数器增量。
- 同周期去重由 `a_repeated_reset_for_the_same_cycle_key_does_not_reestablish_the_baseline`、CLI `accounting_reset_establishes_state_once_and_repeated_resets_do_not_reestablish_it` 覆盖；重启、boot ID 变化与单方向计数器回退由 `traffic` 测试覆盖。
- 只读边界由 `tests/cli/traffic.rs` 和 `tests/acceptance/verify.sh` 覆盖；完整 state 原子读取由 `config::tests::concurrent_reads_observe_only_complete_state_versions` 覆盖；timer 的 `Persistent=true` 由 `tests/cli/install.rs` 验证，真实 systemd gate 最近一次通过 [Actions run 36113899276](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36113899276)。
