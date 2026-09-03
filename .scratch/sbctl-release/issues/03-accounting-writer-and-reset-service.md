# 实现受限 accounting state writer 与 reset timer

Status: needs-triage
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

- [ ] 同一 cycle key 的重复 timer 执行不重复建立或覆盖账期。
- [ ] 跨月停机后 timer 恢复并建立正确 baseline。
- [ ] boot ID 改变或单方向 counter rollback 时保留既有累计值，并保留另一方向的有效增量。
- [ ] HTTP 订阅、`status` 和普通 `traffic` 读取不写 accounting state。
- [ ] 并发读取永远只能看到完整 state 文件。
- [ ] systemd unit 明确 `Persistent=true` 且可在 fixture/真实 systemd 中验证。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0016、ADR-0017
