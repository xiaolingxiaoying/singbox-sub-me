# TUI：覆写编辑界面

Status: superseded + needs-verification（原 `$EDITOR` 编辑设计已被 ADR-0023 作废；只读页与片段开关已落地；仍缺 `O` 清空，见下）
Type: task
Blocked by: 01

## 2026-09-24 作废说明（先读这段再动手）

本工单要求的 `$VISUAL`/`$EDITOR` 编辑与内置多行编辑浮层**已被否决**，不要照着实现：
`docs/adr/0023-client-config-overrides-share-the-merge-engine.md` 与 `PRODUCT.md` 定的边界是
**只读展示合并结果 + 规则片段开关，不提供任意 JSON 编辑器**（理由写在 ADR 的 Rejected 段：
编辑器会把"覆写"变成"第二份配置来源"，而合并结果必须始终能被 `sing-box check` 判定）。

已落地的是第 7 个页签「覆写」（`crates/sbtui/src/view/config_override.rs`）：脱敏生效配置 outline、
片段列表与 `↑↓`/`Enter` 开关、引擎错误原样显示、保留字段逐片段报告。

本工单里**仍然成立**、且目前还缺的只有两条：
1. `O` 清空覆写（二次确认）。缺它不是审美问题：覆写文件名是档案名的 sha256，用户无法自己定位到
   那个文件，只读页又写"要改就在数据目录的 overrides/ 下改"，等于给了一个走不通的出口。
   `ClientCommand::ClearOverride` 与引擎侧删除已经实现并有测试，只差一个按键。
2. 页脚与帮助浮层的键位提示随新键位更新（这条已随只读页落地一部分）。

`SetOverride` 保持"只由文件与服务端 API 触达"是刻意的：两个界面都不发这条命令，它的测试就是它的契约。

## 原动作清单（保留作历史，勿照做）

## 动作

1. 设置页新增「配置覆写」区块：状态（无/有 + 字节数/错误）、`o` 查看/编辑、
   `O` 清空（二次确认）。
2. 编辑优先调用 `$VISUAL`/`$EDITOR`（Windows 回退 `notepad`）；无编辑器或非交互环境时，
   降级为内置多行文本编辑浮层（复用现有输入模态样式）。
3. 保存后调用 `SetOverride`；引擎校验失败时在状态行显示字段路径，不启动内核。
4. 页脚键位提示与帮助浮层同步更新。
5. 测试：输入路由（`o`/`O`、模态互斥）、空内容=清空、错误展示；`cargo test -p sbtui`。

## 验收

- WSL 与 Windows VM 中各完成一次：覆写 `dns.nameservers` → 重启内核 → 规则/日志无异常。
- 覆写非法字段时内核保持停止且错误可见。
