# 英文界面下引擎状态与事件文案仍是中文：改成共享层输出事件码

Status: ready-for-agent
Type: task

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
