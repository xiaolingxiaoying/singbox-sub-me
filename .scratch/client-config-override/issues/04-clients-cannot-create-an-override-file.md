# 两个客户端都无法"创建"覆写文件（引擎命令没人发）

Status: needs-implementation
Type: task
Found: 2026-09-24（R25 的 TUI 能力审计）
Blocked by: 01

## 事实

`ClientCommand::SetOverride { profile, contents }` 在引擎里声明并处理
（`crates/client-core/src/command.rs:77`、`crates/client-core/src/controller.rs:542`），
但 `grep -rn SetOverride crates/sbtui/src crates/sbgui/src` **零命中**——没有任何 UI 发它。

于是"覆写配置文件内容"这条需求今天的样子是：
引擎会合并、会校验、会报告保留字段冲突；TUI/GUI 的覆写页会显示脱敏后的生效配置、
开关规则片段、两击删除、原样显示解析错误；**但用户要自己用编辑器把 JSON 放到
`<data dir>/overrides/<sha256(profile)>.json`**。命令存在、显示存在、写入口不存在。

## 边界（先读，别照着做成编辑器）

`docs/adr/0023-client-config-overrides-share-the-merge-engine.md` 与 `PRODUCT.md` 明确否决了
内置 JSON 编辑器（工单 02 的作废说明也写了）。本工单**不是**翻案：
做的是**装载**（load），不是编辑（edit）——
用户在外部准备好文件，客户端负责把它放进正确路径、跑校验、把引擎原话回显出来。

## 要做的

1. TUI：覆写页加一个键（与现有键位不冲突），提示输入一个**已有文件的路径**，
   读出来交给 `SetOverride`；GUI：覆写页加"从文件装载"按钮，走同一条命令。
2. 装载前后必须复用引擎已有的校验，不要在 UI 侧另写一套判断：
   合并文本要过 `sing-box check`（这条已经是引擎行为），坏文件要**拒绝落盘**并显示原话，
   保留字段冲突逐个片段报出来——这两条 TUI 现在已经在"显示"侧做了，装载侧要用同一批结果。
3. 路径与档案名：装载的是**当前档案**的覆写（文件名是 `sha256(profile)`），
   所以要么由 UI 明确说"将写入 <档案名> 的覆写"，要么让用户选档案；不能默默写到别的档案上。
4. 覆盖已有覆写文件前必须二次确认（与 `O` 删除同一套两击模式），
   并保留"上一次的覆写文件"可回退的说明或备份——引擎侧已有回滚拷贝的约定，先看 ADR 再决定。

## 验收

- 单测：装载成功路径、坏 JSON 被拒且不落盘、目标档案与显示档案一致、二次确认在中间态被别的键复位。
  这些都要**先红后绿**，并说明把生产代码改成什么样能让它红（变异检验）。
- 截图/快照：GUI 两张尺寸（860×640、1440×900）各一张装载态；TUI 一张 insta 帧。
- 文档：`docs/client-description.md` 的覆写一节补上"怎么把文件放进去"，
  现在那段只描述了页面上能看到什么。
