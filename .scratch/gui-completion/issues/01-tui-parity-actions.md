# GUI 功能对齐 TUI 的六项缺口

Status: ready-for-agent（第 6 条覆写页已于 2026-09-24 落地，见 `feat(clients)` fbecb86；1–5 项仍开放）
Type: task
Blocked by: sbctl-v0.2/issues/01

## 事实

`rg 'ClientCommand::' crates/sbgui/src` 对比 `crates/sbtui/src`：GUI 缺
`ImportProfileFile`、`SetProfileUrl`；档案命名恒为 `None`（`app.rs:212-215`）；
连接无排序、日志无暂停、无帮助浮层。

## 动作

1. 订阅页：本地 JSON 文件导入（复用 `ImportProfileFile`，文件路径输入 + 选择提示）；
   「编辑链接」（复用 `SetProfileUrl`，两击确认后的行内编辑）。
2. 订阅导入面板增加「档案名称」输入（TUI `n` 已有）。
3. 连接页：排序切换（下载/上传/开始时间，对应 TUI `S` 四态）。
4. 日志页：暂停/继续按钮（冻结视图，恢复时补全期间新增行）。
5. 帮助浮层：`?` 键与标题栏入口，列快捷键与危险操作说明。
6. ✅（2026-09-24 新增，已落地）覆写页：TUI 已有第 7 页「覆写」（只读生效配置 + 规则片段开关），
   GUI 完全没有这一页。数据已在共享快照里（`override_summary` / `override_error` /
   `effective_outline`，均已脱敏），命令也已存在（`SetOverride` / `ClearOverride` /
   `ToggleOverrideFragment`，重读磁盘是引擎内部动作而非命令），所以这是纯渲染 + 开关的活，边界见
   `docs/adr/0023-client-config-overrides-share-the-merge-engine.md` 与 `PRODUCT.md`：
   不做任意 JSON 编辑器，坏文件要按引擎原话显示，保留字段冲突要逐个片段报出来。

## 验收

- 与 TUI 的功能对齐清单逐项有截图。
- 新增交互各有单测（状态机层）与截图证据（860×640 与 1440×900 各一张）。
