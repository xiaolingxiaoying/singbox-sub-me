# GUI 三条实测缺陷：IME 被解绑、导出无视筛选、表头与行差 1px

Status: needs-implementation（A 未开工；B/C 未开工）
Type: bug
Found: 2026-09-24（R26），行号以当日 HEAD 为准

## A. Windows 上 GUI 的输入框没有输入法（最重）

`sbgui` 全库没有一处实现 gpui 的 `InputHandler`（`grep -rn "InputHandler" crates/sbgui/src` 零命中），
文本靠 `on_key_down` 逐字符 append 到 `TextField.text`。而 pinned 那版 gpui 的 Windows 后端
（`~/.cargo/git/checkouts/zed-*/739fdbe/crates/gpui_windows/src/events.rs`）：

- `:699-702` `update_ime_enabled()` = `with_input_handler(|h| h.query_accepts_text_input()).unwrap_or(false)`
  —— **没有 handler 就是 false**；
- `:722` 于是执行 `ImmAssociateContextEx(handle, HIMC::default(), 0)`，把该窗口的 IME 上下文解绑。

结果：中文用户在订阅链接、档案名、镜像、关键字过滤……**任何一个框里都打不出拼音/候选**，
只能从别处粘贴。目标是 Windows 11 上的 GUI，所以这条按缺陷算，不按"a11y 待办"算。

### 要做的

1. 用一个真正的输入元素替换手写的 `key_char` append（gpui 的 `Input` / `ElementInputHandler` 一类），
   让窗口有 handler 可问，从而 `query_accepts_text_input()` 为真。
2. 迁移时保住现有行为，逐条列出来当验收：Ctrl+V 粘贴、Enter 提交、Esc 复位、
   失焦时用快照值同步（`sync_fields`）、只读态、以及**新增**的 Home/End/Delete/方向键与光标位置。
3. 组合中的文本要能显示（underlined composition），提交前不落进 `TextField.text`。

### 验收（这条不能只靠单测）

- 单测能证的只有"我们注册了 handler / 查询返回真"，**证不了 IME 真的可用**；
  真凭据是 Windows 真机：VM 里挂微软拼音，在订阅链接框里打"家庭实验室"并上屏，留截图。
  → 与 G8 的 VM 清单合并做（工单 `04-real-windows-run.md`）。
- 在真机跑通之前，`docs/client-description.md` 里不要出现"支持中文输入"这类说法。

## B. 日志页「导出」与「复制」不是同一批内容

`pages/logs.rs:123-136` 先按级别与关键字过滤出 `kernel` / `events`，画面行与
`copy_text`（`:161-170`）都来自这两个向量；`:272-290` 的导出闭包却从
`view.snapshot.core_logs` / `event_lines()` 重新取**未过滤**全量。
用户筛到 Error、屏幕三行、点导出拿到几百行。

### 要做的

- 把"这一批要带走的行"抽成一个纯函数（例如 `log_lines_for_export(level, query, snapshot)`），
  复制、导出、以及画面**三处共用**；顺带解决 `copy_text` 每次绘制都重建再 clone 的浪费（第 9 条同源）。
- 单测：给定一份混合级别/命中关键字的快照，断言复制文本 == 导出文本 == 画面行集合；
  并做变异检验（把导出改回未过滤，测试必须红）。
- 明确决定"导出是否受 180/60 行画面截断影响"：屏幕是窗口，文件应当是**过滤后的全部**，
  这个取舍写进代码注释与 `docs/client-description.md`，别留给下一个人猜。

## C. 规则表头与数据行差 1px

`components.rs:440` `table_head_row` 用 `px(14.0)`，`:532` `rule_row` 用 `px(13.0)`；
列宽（40/124/170）一致，但表头没有 `border_b_1`、首行没有 `border_t_1`。

- 修法：表头/行共用一组常量（行高、左右内缩、分隔线归属），像日志/连接页已经共用的 `ROW_X` 那样。
- 门：复用 `theme.rs` 里那套**扫自己源码**的静态断言写法，钉住"表头与行不得各自写死行高"。
- 证据：860×640 与 1440×900 各一张规则页截图，量边界。

## 相关但另开工单

第 2 条（设置页保存语义六套并存）与第 7 条其余部分（`?` 帮助浮层、页面切换键、
Esc 关浮层、Tab 焦点遍历）不在本工单：前者是语义决策，见工单 02 第 2 条；
后者是增量功能，可单独派工。

## 2026-09-24 实施前测量（G13 为什么这一轮不动手）

按当日 pinned 的 gpui（`~/.cargo/git/checkouts/zed-*/739fdbe`）实测：

- `InputHandler` 定义在 `crates/gpui/src/platform.rs:1796`，**不是** `gpui/src/input.rs`；
  要实现的成员含 `selected_text_range` / `marked_text_range` / `text_for_range` /
  `replace_text_in_range` / `replace_and_mark_text_in_range` / `unmark_text` / 平台粘贴等，
  全部签名是 `(&mut self, ..., &mut Window, &mut App)`，**区间单位是 UTF-16**。
- `crates/gpui/src/elements/` 里**没有** `Input`/`TextInput` 元素（只有 div、text、list、
  uniform_list、svg、image、canvas、anchored、deferred、animation、container_query、surface）。
  也就是说 gpui 只给 trait，控件要自己写——zed 自己的文本框就在 zed 侧而不是 gpui 侧。
- 本仓库 `crates/sbgui/src/state.rs` 的 `TextField` 只有一个 `text: String` 与 `focus`，
  **没有光标下标、没有选区、没有 marked（组合中）区间**，而这些正是 IME 协议的必需项。

结论：这是一次**输入控件重写**（状态模型 + 渲染 + 事件），不是一次绑定改动；
而且它的验收是"真机上挂微软拼音能上屏"，那需要 G8 的 Win11 VM（当前卡在快照与解锁口令，用户侧）。
所以本轮不开始——半套输入控件会把一个能用的 ASCII 路径换成一个不能用的"看起来高级"路径，
代价比收益大。要动时的最小顺序写在下面。

## 动手时的最小顺序（每一步都可单独验证）

1. 先给 `TextField` 加 `caret: usize` 与 `selection: Option<Range<usize>>`（UTF-16 边界换算写成纯函数，
   带单测：中文/emoji 混合串上断言 `utf16_index(byte_index)` 与反向换算一致）。
2. 再实现一个**只读**的 `InputHandler`：`query_accepts_text_input()` 返回 true、
   `text_for_range`/`selected_text_range` 报真实值，`replace_text_in_range` 先只处理"无 marked"的插入。
   这一步的判据是**gpui 不再解绑 IME**——在 Windows 上可用 `update_ime_enabled` 的分支验证：
   解绑发生在 `with_input_handler(...).unwrap_or(false)` 为 false 时（`gpui_windows/src/events.rs:699-702`），
   注册了 handler 且查询为真就走 `IACE_DEFAULT` 分支（`:709`）。
3. 然后才加 marked text（组合态）：`replace_and_mark_text_in_range` + `unmark_text`，
   并把组合中的文本渲染成下划线；此时才谈"不丢字"。
4. 删掉 `chrome.rs` 里手写的 `key_char` append 与 Ctrl+V 分支（有 handler 后由 IME/平台粘贴驱动），
   保留 Enter/Esc 的提交与复位语义。
5. 真机验收（G8 的 VM）：微软拼音输入"家庭实验室"并上屏，截图；
   同时回归 Ctrl+V、Home/End/Delete、方向键、Esc 复位、失焦同步（`sync_fields`）。
