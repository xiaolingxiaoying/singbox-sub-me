# VMware Windows 11 客户机流水线

Status: ready-for-agent
Type: task
Blocked by: sbctl-v0.2/issues/01

## 事实

- `vmrun`：`C:\Program Files\VMware\VMware Workstation\vmrun.exe`；VM：`Win11-sbtui-test`。
- 既有资产：`.scratch/win11-vm/shot-guest.ps1`（GUI 7 页截图、DWM 裁剪、CJK 检查、重试）、
  `run-one.ps1`、`probe.ps1`；结果：`run6.log` 7/7 OK，缺 About 页。
- 客户机 4GB 内存曾导致 proxies/connections EXITED；建议加到 6–8GB 或分批 4 页。
- `probe.txt` 显示 `appdata-sbtui-exists=False`，TUI 从未在客户机运行过。

## 动作

新增 `scripts/winvm/verify.ps1`，子命令 `snapshot|revert|gui|tui|collect`：

1. 首次手工建立 `clean-base` 快照；脚本默认先 `revertToSnapshot`。
2. 投递：`target/release/sbgui.exe`、`sbtui.exe`、`ly.exe`、
   `scripts/winvm/shot-guest.ps1`（从 `.scratch/win11-vm/shot-guest.ps1` 正式化）。
3. GUI：8 页（含 `about`）截图 + manifest + 每页 `.err.txt`；内存紧张时分两批。
4. TUI：`runProgramInGuest` 非交互冒烟 `sbtui --help/--version/--print-dir`；
   再在 Windows Terminal 中交互运行并截屏（配置/节点/连接/日志/设置 + 覆写浮层）。
5. 权限探测：`net session` 判断 `Test` 是否管理员；若是，则跑 TUN 用例（启动-停止-恢复），
   否则记录 skipped 及原因。
6. 取回证据到 `.scratch/winvm/<timestamp>/`；最后 revert 快照。
7. 口令只从 `$env:WINVM_PASS` 读取；脚本不落盘任何口令。

## 验收

- 一条命令产出：8 张 GUI PNG + TUI 截图 + manifest（无 EXITED）+ 权限探测结果。
- 客户机 revert 后无 sbgui/sbtui/sing-box 残留进程（脚本断言）。
- 宿主机在运行前后服务与网络配置无变化（脚本输出对比结果）。
