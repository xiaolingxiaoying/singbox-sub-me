# 系统代理开关

Status: resolved
Type: task
Blocked by: 04

## 交付范围

- Windows：winreg 写 `HKCU\...\Internet Settings`（ProxyEnable/ProxyServer/Override），`InternetSetOption(INTERNET_OPTION_SETTINGS_CHANGED|REFRESH)` 广播刷新。
- macOS：`networksetup -setwebproxy/-setsecureproxy/-setsocksfirewallproxy`（记录原状态，可恢复）。
- Linux：GNOME gsettings / KDE 优先，桌面不可用时输出 env 变量提示。
- 设置页开关：开启=写系统代理指向 mixed 入站（127.0.0.1:2080）；关闭=恢复原值；退出程序时若开启则提示是否保留。
- 仪表盘显示系统代理状态。

## 验收标准

- [ ] Windows 真机：开关后系统代理生效（浏览器走代理），恢复干净。
- [ ] 非 Windows 平台编译通过，逻辑以 trait 隔离。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：Windows 注册表 + InternetSetOptionW 刷新已实现；macOS networksetup、Linux gsettings 分支已实现（CI 覆盖编译）。
