# TUN 模式

Status: resolved
Type: task
Blocked by: 04

## 交付范围

- 配置改写：system-proxy 模式（mixed 127.0.0.1:2080 入站）与 TUN 模式（tun 入站：auto_route/strict_route/auto_detect_interface，inet4_address 198.18.0.1/30）之间切换激活配置的 inbounds 段，其余段保持订阅原样。
- 权限：检测当前进程是否有管理员/root；无则给出明确提权指引（Windows：以管理员运行终端；Linux：setcap 或 sudo 运行提示）。
- Windows：确认 wintun.dll 与内核同目录（下载器保证），缺失时提示。
- 切换 TUN 需重启内核生效；设置页二次确认；仪表盘显示当前模式。

## 验收标准

- [ ] Windows 管理员终端：TUN 开启后全局流量走 sing-box（对照出口 IP），关闭后恢复。
- [ ] 非管理员时给出可操作的指引而不是静默失败。
- [ ] `cargo fmt/clippy/test` 通过（inbounds 改写逻辑单测）。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：inbounds 本地改写（mixed/tun）+ 模式切换提示已实现；真机管理员验证待用户执行。
