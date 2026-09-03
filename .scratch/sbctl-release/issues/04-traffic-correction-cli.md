# 实现 traffic set-used 流量修正

Status: resolved
Type: task
Blocked by: 02, 03

## 目标

提供管理员显式修正当前 accounting period VPS traffic 的 CLI 能力，不伪造网卡方向计数器。

## 交付范围

- `sbctl traffic set-used --bytes <TOTAL>`。
- `sbctl traffic set-used --rx <BYTES> --tx <BYTES>`。
- 独立 total adjustment 与 direction-aware correction 的持久化表示。
- 目标大于当前计数器时的 baseline reconciliation。
- 变更摘要、operation lock、原子提交和失败保持旧状态。

## 验收标准

- [x] `--bytes` 改变 reported total，但不改变 measured RX/TX 方向值。
- [x] `--rx/--tx` 分别设置方向值，且不修改真实 sysfs counter。
- [x] 目标值大于当前 counter 时，后续计数仍能正确累计。
- [x] 执行前展示当前账期、当前实际值、目标值和 next reset。
- [x] 参数互斥/缺失、负数、溢出和损坏 state 被拒绝且不写文件。
- [x] 并发 reset/correction 由同一 operation lock 串行化。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0016、ADR-0017

## Comments

- 2026-09-03：`sbctl traffic` 扩展为子命令组（裸 `traffic` 保持展示语义，`traffic show` 等价），新增 `traffic set-used`，其参数由 clap `ArgGroup` 约束：`--bytes` 与 `--rx/--tx` 互斥，`--rx` 与 `--tx` 成对出现，至少提供一个，负数和缺失参数在解析阶段即被拒绝（不写文件）。
- 2026-09-03：`--bytes <TOTAL>` 按“目标 - 当前 live 已报告总量”追加一条 `TotalAdjustment` 增量记录，方向值保持测量值；目标低于当前总量时以 `TotalTooLow` 拒绝。重复执行每次都对最新已提交总量计算增量，因此是幂等的 set 语义而非累计叠加。
- 2026-09-03：`--rx/--tx` 通过 baseline reconciliation 实现：把 `accumulated_rx/tx` 设为目标、`baseline_rx/tx` 重指当前 counter，并追加一条 `SetDirection` 审计记录；此后 counter 增量在修正值之上继续累计，boot ID/counter rollback 语义保持不变。`CorrectionRecord::SetDirection` 不再 override 报告值（其作用由 reconciliation 承担）。
- 2026-09-03：code review 修正：direction correction 必须把 `boot_id` 同步为当前测量 boot，否则重启后、reset 任务运行前执行的修正会因 boot 不匹配而吞掉增量并在下次 reset 永久丢失；新增该场景回归单测。
- 2026-09-03：schema 版本保持 v2 的判定：reset 任务从不写 corrections，发布版中不存在含 `SetDirection` 记录的 v2 状态，语义从 override 改为 reconciliation 只影响本命令新写入的已 reconcile 状态，无需 schema bump；交互式菜单/向导中的 Traffic correction 入口归 ticket-07（config wizard）范围。
- 2026-09-03：total-only 与 direction-aware 修正相互独立持久化：`--bytes` 不改方向值，`--rx/--tx` 不触碰既有 total adjustment。
- 2026-09-03：`set_used` 与 reset 共用 `DeploymentStore::acquire_operation_lock` + 临时文件 + atomic rename；变更摘要先于原子提交打印；state 缺失/损坏/schema mismatch/陈旧周期/pending 首 reset 均拒绝且不写文件。溢出（`--rx + --tx` 或含 adjustment 的目标总量超 u64）以 `Overflow` 拒绝。
- 2026-09-03：验证通过 `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、60 个库测试（新增 14 个 correction 单测，含并发 correction/reset 串行化）、43 个 CLI 测试及 `sh -n` acceptance 语法检查；acceptance 新增 total/direction correction 场景。
