# sbgui 发布矩阵与 CI 门禁

Status: ready-for-agent
Type: task
Blocked by: sbctl-v0.2/issues/04

## 现状

- `release.yml` 只构建 sbctl 与 sbtui；sbgui 不在发布链。
- CI 只有原型 job，没有任何 GUI 构建/截图门禁。
- `scripts/sbgui-shot/inside.sh:160-162` 的 exit-confirm 帧未传 `SBGUI_SIZE`，
  文件名与尺寸不符。
- `crates/sbtui/packaging/install.ps1` 未作为 release 资产上传；
  `packaging/windows/sbtui.wxs` 快捷方式指向 sbgui.exe 但 MSI 手工构建。

## 动作

1. 修 `inside.sh` 尺寸传递；截图 manifest 记录真实尺寸。
2. `release.yml` 增加 Windows sbgui 构建与上传：
   `sbgui-windows-amd64.exe`；资产集合与 `install.ps1`、`sbtui.wxs` 说明对齐。
3. CI 增加 GUI 冒烟 job：在 `scripts/sbgui-shot/Dockerfile` 镜像内跑 8 页 × 1440×900 截图，
   断言每张非空、manifest 无 FAILED（可先 `workflow_dispatch`/nightly，避免拖慢 PR）。
4. `install.ps1` 纳入 release 资产；MSI 要么纳入构建，要么在 README 明确标记手动。
5. Windows VM 流水线（验证环境工单 03）作为发布前人工门禁。

## 验收

- 新 tag 的 Release 资产包含 `sbgui.exe` 与 `install.ps1`。
- CI GUI job 在故意破坏布局（如 860 宽裁切）时能失败。
- README/packaging 文档同步。
