# TUI：覆写编辑界面

Status: ready-for-agent
Type: task
Blocked by: 01

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
