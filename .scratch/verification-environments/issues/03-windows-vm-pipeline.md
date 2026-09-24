# VMware Windows 11 客户机流水线

Status: ready-for-human
Type: task
Blocked by: sbctl-v0.2/issues/01

## 2026-09-24 进展（commit `feat(winvm)`）

入口已建成：`scripts/winvm/verify.ps1`（`parse-check | snapshot | revert | gui | tui | collect | all`）
+ 正式化的 `scripts/winvm/shot-guest.ps1`（默认 8 页，含 `about`）+ 新增 `scripts/winvm/tui-guest.ps1`
（TUI 冒烟、权限探测、**真实 HKCU 代理写入-校验-恢复往返**、DPI、wintun 位置、孤儿与残留监听口）。
口令只从 `$env:WINVM_PASS` 读；`Finalize` 会比对宿主代理/winhttp/服务/PATH 指纹，
把"运行前后宿主环境无变化"变成可读结论；两份客户机报告都写 `stray-processes=`，由 `Assert-NoStrays` 判定。

`parse-check` 子命令自身做过变异检验（塞进一个语法错的文件 → 退出 1，真实三脚本仍 OK）。

**本工单仍未结案的部分**：`all` 这条腿**尚未真跑一次**——需要先有 `clean-base` 快照、
`target/release/sbgui.exe` 与 `sbtui.exe`，以及 issue 04 清单里那几项（CJK 回退、按监视器 DPI、
DWM、真实注册表、wintun 提权、Job Object 回收）的逐项存档。在那之前，Windows 平台行为只能说"未失败"。

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
