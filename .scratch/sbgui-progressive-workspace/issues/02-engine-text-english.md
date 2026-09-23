# 英文界面下引擎状态与事件文案仍是中文：改成共享层输出事件码

Status: ready-for-agent
Type: task

## 进度（2026-09-23：第 1 步 + 第一批已完成）

**第 1 步（只加不改）已落地。** 新增 `crates/client-core/src/event_code.rs`：
`EventCode` 枚举 + `zh()`/`en()` + `level()`，`EventRecord { code, args }` 用 `{0}`/`{1}`
位置参数展开——照 `RuleKind` 的形状。`ClientSnapshot` 增加 `event_records`，与 `events`
同序同上限，**只由 `push_record` 一处写入**，避免两份列表漂移。`Engine::note_event(code, args)`
与既有 `note()` 并存。

**第一批转换完成：poll 路径的 6 处**（`ProxyGroupRefreshFailed`、`SubscriptionAutoUpdateFailed`、
`CoreAutoRestartFailed`、`ConnectionRefreshFailed`、`CoreExitedUnexpectedly`、
`CoreStatusCheckFailed`）。中文模板逐字复制被替换的 `format!` 串，所以
**`crates/sbtui/src/view/snapshots/` 的 17 个渲染金标准一字未改仍然通过**——这就是"行为保持"的证据。
新增 `a_recorded_event_also_lands_in_the_string_history` 钉住双列表配对与中英两版渲染。

**剩余（按今天重新点数的实测值，不是在旧数字上加的）**：`controller.rs` 里 `.note(` 仍有 30 处、
含中文串字面量的行 64；其他文件 `clash_api.rs` 9、`core.rs` 6、`subscription.rs` 6、
`system_proxy.rs` 4、`state.rs` 34（含测试与注释）。第 2 步继续按文件分批，第 3 步的 23 条
错误链单独一轮。

**尚未动**：消费侧（`chrome.rs:290`、`style.rs:58-68`、`log_level_of`）仍靠中文子串判断颜色与级别；
`ClientEvent`/`recv()` 这条死缝也还没决定删还是复活。第 4、5 步之前，英文界面照旧。

**第 4 步已开始：状态颜色不再靠猜中文。** `ClientSnapshot` 新增 `status_level:
Option<EventLevel>`，`note_event` 写入、`note` 置 `None`。消费侧优先读它，仅在 `None`
（还没迁完的调用点）时退回原来的子串猜测：`sbtui/src/style.rs` 的 `status_color_at`
（被 `view/mod.rs` 页脚使用，`App` 也带上 `status_level` 并在本地"进行中"文案处置 `None`）、
`sbgui/src/chrome.rs` 的工具栏。新测试 `an_engine_severity_beats_the_word_guess` 断言
**级别必须压过措辞**（`Some(Error)` 配"一切正常"仍判红、`Some(Info)` 配"导入订阅失败"判绿），
这正是英文界面丢色的根因。

**第二批转换 8 处**（语言无关的那批）：`CoreAutoStartFailed`、`OperationAlreadyRunning`、
`ProfileActivated`、`SettingsSaved`、`CoreStopped`、`LocalProfileNeedsNoUpdate`、
`ProfileUrlUpdated`、`ProfileDeleted`。累计 14 处。

**刻意没转的三处**：`流量模式: {}`、`流量模式: {}（内核已重启）`、`出站模式: {}`。
它们的 `{0}` 是 `TrafficMode::label()` / `OutboundMode::label()` 的**中文枚举标签**，
转成事件码只是把中文从模板挪进参数，英文界面照旧混中文——属于本文第 24 行记录的
"其内插的枚举 label"问题，必须和 label 一起解决，否则是假进展。

**穷举测试当场抓到一个假失败**：`every_code_declares_both_languages_and_fills_every_placeholder`
原先要求每个码渲染后都含 `ARG`，对无占位符的码必然不成立。改成按模板是否含 `{0}` 分支断言，
并新增"两种语言对是否需要参数必须一致"的检查。码表现在由 `EventCode::ALL` 驱动，新增变体不会漏测。

**剩余**：`controller.rs` 还有约 22 处 `.note(`（含 3 处刻意保留的模式提示）；第 3 步的 23 条
错误链未动；`events` 字符串列表与 `log_level_of` 的中文猜级别未动；`ClientEvent`/`recv()`
死缝未决；GUI 设置页 TUN 开关仍走 `UpdateSettings`。第 5 步（中英全页截图验收）未做。

---

维护者 2026-09-22 选定方向：**共享层返回机器可读的事件码 + 结构化参数，各界面自行渲染**（三条可选路里最干净、改动面最大的一条）。本文是动手前的完整清点，数字是今天重新点的，不是在旧数字上加的。


## 现状机制

- `Engine::note(impl Into<String>)`（`crates/client-core/src/controller.rs:1272`）一次调用同时写两处：`snapshot.status`（`:1274`）与 `push_event`（`:1275`）。
- `Snapshot::push_event`（`state.rs:381`）存的是 `VecDeque<String>`（`state.rs:319`，上限 200）——**文本即数据**，这是问题的根。
- `push_log`（`state.rs:389`）只装内核自己的原始输出（`controller.rs:1248`），不含客户端中文，本工单不涉及。
- `status: String` / `busy: Option<String>`（`state.rs:335`、`:338`）另有直接写入点：`controller.rs:218-222`、`264`、`308`、`373`、`389`、`407`。
- `ClientCommand::label()`（`command.rs:106`）产中文，喂给 `busy`（`controller.rs:308`）与通用失败漏斗 `operation_error`（`controller.rs:355`，`note(format!("{label} 失败: {message}"))`）。
- `ClientEvent` / `ClientError{operation, message}`（`event.rs:5-27`）是一条**已存在但两个界面都没在用**的结构化通道（`recv()` 在 `sbgui/src`、`sbtui/src` 里零命中）——可以复活它而不是新造一套。

## 要改的字符串总数：61，另有 23 条从错误里带进来

| 来源 | 数量 | 位置 |
| --- | --- | --- |
| `note()` 中文模板 | 35 / 36 处 | `controller.rs` 266, 303, 357, 489, 534, 545, 551, 566, 599, 618, 635, 671-673, 685, 690, 692, 724, 731, 741-744, 778, 792, 811, 830, 861, 899, 937-940, 963, 996, 1026, 1033, 1046, 1086, 1145, 1151, 1185-1189, 1196-1200（`:581` 是 `{group} → {node}`，纯配置数据，不用改） |
| 直接写 `status`/`busy` | 5 | `controller.rs` 219, 220, 221, 264, 372 |
| 默认 status | 1 | `state.rs:371` |
| `ClientCommand::label()` 各臂 | 20 | `command.rs:106-131`，其内插的枚举 label 在 `clash_api.rs:157-159`、`system_proxy.rs:24` |
| 合计 | **61** | |
| 经由 `{message}` 插进 note 的中文错误 | +23 | `controller.rs` 111, 350, 451, 481, 559, 608, 629, 646, 649, 655, 658, 666, 734, 737, 789, 840, 909, 913, 948, 957, 969, 976；`core.rs` 54-55, 112, 119, 132, 139, 388-389；`system_proxy.rs` 433, 479, 489 |

## 消费侧

| 字段 | GUI | TUI |
| --- | --- | --- |
| `status`/`busy` | `sbgui/src/chrome.rs:285-293` 原样渲染；颜色靠 `status_text.contains("失败")/("错误")` 猜（`:290`） | `sbtui/src/app.rs:219`、`255-257` → footer `view/mod.rs:133` 原样；颜色 `style.rs:58-68` 猜 `失败/错误/崩溃/需要/未/成功/启动/已` |
| `events` | `pages/dashboard.rs:798` → `:990-1005` 原样；级别由 `log_level_of(&line)` 猜中文（`:994`）；`pages/logs.rs:55-62`、`213-215` | `view/dashboard.rs:144-153`、`view/logs.rs:88-95`、`input.rs:146` |

**引擎产出的文本没有任何一处走 `tr!`**（`lang.rs:41`）。唯一被桥接的是 `age_label`/`usage_label`（`lang.rs:75`、`:93`），它们的中文半边直接委托给 core。

## 可以照抄的形状

`RuleKind`（`state.rs:77-92`）就是现成的范式：`#[derive(Serialize, Deserialize)]` 枚举 + core 内的 `zh()`（`:95`）/`en()`（`:111`）+ 结构化载荷（`RouteRuleSnapshot { kind, value }`，`state.rs:130-132`），界面侧 `tr!(locale, rule.kind.zh(), rule.kind.en())`（`sbgui/components.rs:486`）。另有 `OutboundMode::as_str()`（`clash_api.rs:164`）、`DelayLevel`（`format.rs:82-99`）、`LogLevel`（`state.rs:436-453`）三处同类先例。`log_level_of`（`state.rs:462`）现在靠中文子串猜级别，改完码表后它应当直接读码。

## 会被改坏的测试（动手前先记下）

`controller.rs:1416`、`controller.rs:1515`、`state.rs:553-560`、`state.rs:577/578/583/588/592`、`format.rs:114/116/128/133/138/145`、`sbtui/src/style.rs:91-100`（测试名就叫 `status_color_reads_the_chinese_status_vocabulary`）、`sbtui/src/view/logs.rs:128/134`、`sbtui/src/view/settings.rs:189/192`、`sbgui/src/pages/logs.rs:316-326`、`sbgui/src/lang.rs:170-172`。另有非测试的耦合：`sbgui/src/components.rs:860` 用 `label.contains("订阅")` 做导航。

## 建议的落地顺序（每步都要能单独编译通过）

1. 在 `client-core` 定义事件码枚举 + 参数结构，先只加不改：`note()` 保留，新增 `note_event(code, args)`，`Snapshot` 增加 `VecDeque<EventRecord>`，`events()` 暂时把 record 渲染成中文以维持两个界面不变。
2. 逐文件把 `note()` 调用点换成 `note_event()`，每换一批跑 `cargo test --workspace`；上面那批断言中文的测试按新语义改写，不要靠放宽断言蒙过去。
3. 错误链（那 23 条）单独一轮：`anyhow` 的中文 message 换成错误码 + 上下文，`operation_error` 变成 `{ code, context }`。这一步最容易被跳过，跳过就还是半中半英。
4. GUI 渲染层接 `tr!`，`chrome.rs:290` 与 `log_level_of` 改读码；TUI 渲染中文半边，行为与今天一致即可（TUI 目前没有语言概念，不要顺手给它加）。
5. 用 `scripts/sbgui-shot/` 抓中英两套全页图验收（`SBGUI_LANG` 已支持），中英各一张都不允许出现整句中文混进英文界面。`DEMO_CORE` 必带，否则全是空态。

## 未决

`busy` 的"进行中"文案带 `…` 拼接（`chrome.rs:287`）在英文下语序是否成立，等第 4 步有真图再定，不要现在猜。
