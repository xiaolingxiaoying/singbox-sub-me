# 把剩余 21 处引擎文案换成 EventCode（issue 02 的第 2–3 步）

Status: needs-implementation
Type: task
Found: 2026-09-23，由一次只读清点得出（行号以当时 HEAD 为准，改动前先复核）。

## 前置事实（已落地，不要重做）

- `crates/client-core/src/event_code.rs`：`EventCode`（现有 14 个变体）、`EventRecord`、`EventLevel`，
  配 `render_zh()` / `render_en()` / `level()`；双语模板与占位符由测试锁死。
- `ClientSnapshot.event_records` + `status_level`；`Engine::note_event(code, args)` 同时写结构化记录
  与字符串历史（`a_recorded_event_also_lands_in_the_string_history`）。

## 待换的 21 处（`crates/client-core/src/controller.rs`）

明确**不动**的：`552 / 563 / 583 / 589` 是模式标签提示（`流量模式: {label}`、`出站模式: {label}`），
标签由代码给、值由数据给，不属于引擎文案；`619` 是 `{group} → {node}` 数据行；`359` 是失败漏斗（见下节）。

| 行 | 现字符串 | 新变体 |
| --- | --- | --- |
| 493 | 已关闭连接 | `ConnectionClosed` |
| 637 | 已关闭 N 条连接 | `ConnectionsClosedN`（{0}） |
| 656 | 内核已安装 | `CoreInstalled` |
| 709 | 警告：无法为内核建立系统级回收保护…sing-box 可能残留 | `OrphanGuardWarning` |
| 724 | 内核已启动 | `CoreStarted` |
| 729 | 内核已启动；系统代理 → 127.0.0.1:{port} | `CoreStartedSystemProxy`（{0}） |
| 731 | 内核已启动；系统代理设置失败: {error} | `CoreStartedSystemProxyFailed`（{0}） |
| 770 | 系统代理已关闭 | `SystemProxyDisabled` |
| 780 | 系统代理已开启 → 127.0.0.1:{port} | `SystemProxyEnabled`（{0}） |
| 817 | 延迟测试完成 | `DelayTestComplete` |
| 850 | 订阅更新失败（{error}）；继续使用上次缓存 | `SubscriptionUpdateUsingCache`（{0}） |
| 869 | 订阅已更新（N 个节点） | `SubscriptionUpdated`（{0}） |
| 900 | 订阅已存在，已切换到 {existing} | `SubscriptionAlreadyExists`（{0}） |
| 938 | 已导入 {name}，正在拉取订阅 | `SubscriptionImported`（{0}） |
| 976 | 已从文件导入并激活：{name}（N 个节点） | `ProfileFileImported`（{0}/{1}） |
| 1237 | 内核连续异常退出 N 次，已停止自动重启…端口占用后手动启动 | `AutoRestartStopped`（{0}/{1}） |
| 1248 | 内核崩溃；N 秒后自动重启（第 n 次） | `CoreCrashRestartScheduled`（{0}/{1}） |

zh 模板必须与现字符串**逐字相同**，否则下面那批断言与金标准会动。

## 会碰到的测试与金标准（本 ticket 的真正风险面）

- `crates/client-core/src/state.rs:833`、`crates/sbtui/src/style.rs:123`、
  `crates/sbtui/src/view/logs.rs:113`、`crates/sbgui/src/pages/logs.rs:313` 四组测试直接断言中文串
  （`内核已启动`、`导入订阅失败: timeout`、`需要先安装内核`、`就绪`）。
- `crates/sbtui/src/view/mod.rs:395` 的夹具把 `status: "内核已启动"` 烘进 **17 份渲染金标准**。
  只要 zh 渲染逐字不变，金标准不该动；**动了就必须逐帧看 diff**，不允许 `INSTA_UPDATE` 一把过。

## 失败漏斗只能编码一半（设计结论，别重复尝试）

`Engine::operation_error`（`controller.rs:357`，`note` 在 `:359`）把 `anyhow::Error` 的 `Display`
内插进 `{label} 失败: {message}`。自家 `bail!`/`context` 那批（`:351/485/525/556/571/646/667/773/776/879/987`
以及 `core.rs`、`system_proxy.rs`）可以逐个换成变体；但漏斗尾部是 tokio fs、reqwest、`try_wait`
之类任意字符串，**不可能穷举成代码**。收口标准因此应当是：已知来源全部编码 + 尾部保留原文并显式
归入一个"未知细节"变体，而不是假装全部可枚举。

## 建议顺序

1. 先加 17 个变体与 `level()`，让 `event_code.rs` 的双语/占位符测试先绿；
2. 按行号批量替换调用点（单独一次提交，金标准应零变化）；
3. 再处理漏斗：能编码的编码，尾部显式承认；
4. 最后补 issue 02 第 5 步——zh 与 en 两套全页截图。L4 腿现在可用：
   `SBGUI_LANG=en bash scripts/sbgui-shot/shot.sh`，EN 下不得出现 CJK 字形。
