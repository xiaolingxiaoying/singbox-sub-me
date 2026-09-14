# sing-box 内核管理

Status: resolved
Type: task
Blocked by: 02

## 交付范围

- 内核下载：GitHub Release 最新/指定版本（amd64/arm64，按宿主平台），SHA-256 校验，zip 解压（Windows wintun.dll 一并保留）；下载 URL 镜像前缀可配置（设置页）。
- 版本管理：检测当前内核版本（`sing-box version`）、固定版本、升级。
- 生命周期：`sing-box check` 预检激活配置 → 启动子进程（日志写文件）→ 健康监测（clash_api ping）→ 崩溃自动重启（退避）→ 停止/重启命令。
- 激活配置落盘：订阅缓存 + 本地模式改写（见 issue 06）合并生成 `cache/active-config.json`。

## 验收标准

- [ ] 全新环境一键下载内核并启动（可用假 sing-box 脚本做单测）。
- [ ] 崩溃后自动重启且仪表盘状态同步；手动停止不复活。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：内核下载（zip 解包、SHA-256、平台识别）、check/启动/停止、clash_api 健康等待已实现。
