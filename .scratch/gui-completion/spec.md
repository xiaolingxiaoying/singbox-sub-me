# sbgui 功能补齐与 UI Bug 清零

Status: ready-for-agent

## 目标（目标文档原文）

「先把功能完成了。同时 UI 不能有 Bug。」

## 已有工单（不重复）

- `.scratch/sbgui-progressive-workspace/issues/01-connections-poll-flake.md`（P0，轮询停摆）
- `.scratch/sbgui-progressive-workspace/issues/02-engine-text-english.md`（P1，事件码）
- `.scratch/sbgui-progressive-workspace/issues/04-real-windows-run.md`（P1，真机证据）
- `.scratch/sbgui-progressive-workspace/issues/05-connections-columns-truncation.md`（P1，截断）

## 本目录工单

- `issues/01-tui-parity-actions.md`：本地配置导入、改订阅链接、档案命名、连接排序、日志暂停、帮助浮层。
- `issues/02-ui-consistency-and-a11y.md`：日志自动滚动失效、设置保存语义、TUN 开关置灰、
  空输入提示、单实例失败提示、对比度、键盘路径、硬编码颜色入 token、1px 对齐。
- `issues/03-release-matrix-and-ci-gate.md`：sbgui 进入 release 矩阵与 CI 截图门禁；
  修 exit-confirm 截图尺寸缺陷；打包 install.ps1/MSI。

## 验收

- 8 页 × 860×640 / 1440×900 × zh/en 截图无裁切、无英文界面中文混入、日志跟随有效。
- 功能与 TUI 对齐清单逐项打勾（附截图）。
- Release 资产含 `sbgui.exe`；CI 有 GUI 冒烟 job。
