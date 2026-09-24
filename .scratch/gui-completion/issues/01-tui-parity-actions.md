# GUI 功能对齐 TUI 的六项缺口

Status: ready-for-agent（第 6 条覆写页已落地 fbecb86；第 1、2 条 2026-09-24 落地，见 R27 与本轮 `feat(gui)` 提交；3、4、5、7 仍开放）
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

## 2026-09-24 复核（工单 02 第 3 条落地后重数）

`grep -oh 'ClientCommand::[A-Za-z]*'` 三份清单对出来：TUI 用到 20 个、GUI 用到 20 个，
**差集只剩三个**：`ImportProfileFile`、`SetProfileUrl`、`SetTrafficMode`。
前两个就是本工单第 1 项；第三个是本轮新发现的，见下面第 7 项。

7. **内核运行时 GUI 切不了 TUN，TUI 能切**（新发现，不是回归）。
   TUI 的 `m` 走两段确认再发 `SetTrafficMode{mode, restart: true}`
   （`crates/sbtui/src/input.rs:633-651`，提示语写明"将重启内核；TUN 需要管理员/root 权限"）；
   GUI 三处开关只会发 `UpdateSettings{traffic_mode}`，落到 `set_traffic_mode(restart: false)`，
   内核在跑时被引擎拒绝。工单 02 第 3 条把这条死点击治好了（置灰 + 说明），
   但**能力仍然缺**：GUI 用户想换 TUN 必须自己先停内核再开。
   补齐的做法照 TUI：内核运行时开关保持不可直接点，点一次进入"再点一次确认切换并重启内核"，
   第二击发 `SetTrafficMode{restart: true}`；确认态与 `confirm_stop_core` / `confirm_clear_override`
   同一套模式，两击之间任何别的输入都要把它复位。
   注意这条改的是"置灰"的语义，工单 02 第 3 条的两道测试（`tun_toggle` / `switch_track`）
   要一起改判据，别留一道自证已过时的门。

   ✅ **2026-09-24 落地**（`feat(gui)` 51bb662）：设置页 TUN 分区在内核运行时多一个
   「切换并重启内核」按钮——第一击预备、第二击发 `SetTrafficMode{restart: true}`。
   **与上面建议不同的一点**：开关本身保持不可点，`tun_toggle` / `switch_track` 两道测试
   原样保留。理由是重启会断连接，不该藏在和"免费设置"同一个手势里；能力做成第二个显式控件之后，
   "置灰"的语义并没有变，所以那两道门不是在自证过时，它们仍在守一条仍然成立的规则。
   预备态存的是**目标模式而不是布尔**：旧的预备态若跨过一次内核停止再启动，
   会让下一次不同方向的"第一击"直接重启——这条是新测试里的第三个断言。
   变异两次都咬：忽略预备态 → 红；`restart: true` 改成 `false` → 红。

## 派工边界（本轮教训）

写代码的 agent 只做到"实现 + 单测 + 三道门本地绿"为止，**截图取证由人做**：
工单 02 那批 4 项就是 agent 自报三门绿后停在"接下来抓画面"，
画面一张没拍、改动整批未提交，接管成本比它自己写完还高。

8. ✅（2026-09-24 已修并闭环，`fix(gui)` 396bcc8 + 5e36366，见 R29）**订阅页操作列在 860 下被裁。**
   真正的根因不是轨道宽度，而是**列宽之和 > 内容区**：表格写死 `min_w(600)`，而 860 窗口去掉侧栏与
   内缩后只剩约 604 px 可画，超出部分由 `overflow_x_scroll` 承担，**被裁的永远是最后画的那个按钮**。
   修法：窄屏隐藏「流量与节点」（配额在概览页有）、`min_w` 由列 token 相加得出而不是第二个字面量、
   三个列宽进 `theme.rs`，并新增一道算术门（列之和 ≤ 内容区且余量 ≥ 60px；
   变异检验：`SUB_ACTION_NARROW` 250→420 会红）。中英两种语言在 860×640 与 1440×900 都看过帧。
   **保留这段以免结论被美化**：第一次修复（`699522f`，轨道 250/268）只对了一半——
   中文过了，英文帧把 `Delete` 照样切在卡片右缘。是那张英文帧推翻了我自己的判断。

   原始记录（修复前的现象与错误诊断）：三个按钮（更新 / 编辑链接 / 删除，未启用那行是 设为当前 / 编辑链接 / 删除）之后，
   `action_width` 给了 narrow 168 / wide 250 并加 `flex_wrap`。`flex_wrap` 只在**轨道内**换行，
   轨道本身比最宽那对按钮窄，于是折行后仍越出卡片右缘，最后一个字被裁、按钮不可点。
   证据：`.scratch/shots-ticket01/860x640-subscriptions.png`（被裁）与
   `860x640-subscriptions-url-editor.png`（同一列在较窄的一对下正常）——
   所以范围是"最宽标签对"，不是全部行。这正是工单 04 修过的同一类缺陷。
   修法二选一，别再用"先写一个数再看帧"的方式：
   ① 窄屏改图标按钮（复用现成的 `mini_action`，带 tooltip 与 `aria` 文本）；
   ② 操作列宽度按**当前语言最宽那一对按钮 + 间隙**算，并把表格最小宽随之抬高，
      让窄屏走横向滚动而不是裁切。
   验收：860×640 与 1440×900 各一张，三种按钮组合（active / 未启用 / 只读）都不裁；
   并加一道静态门，钉住"操作列宽度不得写成裸字面量"。
