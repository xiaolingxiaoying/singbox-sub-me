# 实现 accounting period、UTC 默认值与 Pending first reset

Status: resolved
Type: task
Blocked by: 01

## 目标

实现完整的 accounting period 模型和版本化状态结构，修正当前对首个 Anchored-month reset、timezone 默认值及 DST 的不符合规格行为。

## 交付范围

- Natural-month 与 Anchored-month 的 period identity、next reset 和短月收敛。
- accounting timezone 默认 `UTC`，只接受有效 IANA timezone，不修改系统 timezone。
- `YYYY-MM-DDTHH:MM` 校验；不存在或含糊的 DST 本地时间拒绝保存。
- First reset instant 之前返回 `pending-first-reset`、零流量和首个 reset 时间。
- TrafficState schema version、cycle key、interface、baseline RX/TX、累计值、boot ID 和 correction 记录模型。

## 验收标准

- [ ] 自然月边界在 UTC 和非 UTC timezone 下计算正确。
- [ ] 锚定日 1–31 均可配置，28/29/30/31 日在短月收敛到月末。
- [ ] First reset instant 之前是合法 pending 状态，不返回错误。
- [ ] DST 不存在和重复的本地时间均被拒绝。
- [ ] 配置创建或修改不会调用 `timedatectl` 或修改 VPS 系统 timezone。
- [ ] schema mismatch 能被明确识别并进入可诊断错误路径。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0015、ADR-0016、ADR-0017

## Comments

- 2026-09-03：`TrafficState` 升级为 schema v2，新增 `cycle_key`、`interface`、`baseline_rx/tx`、`corrections`（`TotalAdjustment`/`SetDirection` 记录模型）；counter rollback 与 boot ID 语义保持不变。状态缺失/损坏/schema 不兼容进入可诊断错误路径（`ConfigError::StateCorrupt`/`StateSchemaMismatch`），不再静默重建。
- 2026-09-03：Anchored-month 首个 reset 之前改为合法 `pending-first-reset` 状态，报告零流量并把首个 reset 时间作为 next reset，且不写状态文件；`local_datetime` 不再接受含糊 DST 时间。
- 2026-09-03：`config init` 的 accounting timezone 默认固定为 `UTC`，移除读取 `/etc/timezone` 的系统时区回退；anchored reset 保存前校验本地时间存在且无 DST 歧义。
- 2026-09-03：验证通过 `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、40 个库测试、34 个 CLI 测试及 `sh -n` acceptance 语法检查；acceptance 脚本新增 pending-first-reset 与 DST 拒绝检查。
- 2026-09-03：code review 后保留的判定：网卡变更视为新账期（不同网卡计数器空间不可比较）；`local_datetime` 对自然月边界同样严格拒绝不存在/含糊 DST 时间（罕见且可诊断）；`StateSchemaMismatch` 用于“能解析但版本不符”的文件，真实 v1 状态文件因缺字段落入 `StateCorrupt`，两者均可诊断。
- 2026-09-03：延后项（不在本票据范围）：spec 的“旧账期进入历史记录”、订阅 503 脱敏、`subscription-userinfo` 的 `total` 口径，分别由后续订阅与写入者票据处理。
