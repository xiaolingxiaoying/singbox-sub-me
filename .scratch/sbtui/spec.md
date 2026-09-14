# sbtui — 终端 TUI sing-box 代理客户端 spec

Status: ready-for-agent
日期：2026-09-14

## 背景

用户要一个终端 TUI 版的 sing-box 代理客户端，具备 Clash Party 的基本功能，交互简单直观。技术栈已确认：**Rust + ratatui**，与本仓库同 workspace。订阅端（阶段一 `.scratch/subscription-upgrade/`）产出的 `sing-box-full.json` 自带 clash_api 与 cache_file，是 TUI 的控制通道基础。

## 产品定位

- 运行在用户本地终端（Windows 优先，macOS/Linux 保证编译并在 CI 构建）。
- 管理 sing-box 内核生命周期，导入 sbctl 订阅，切换节点，测试延迟，控制系统代理/TUN，查看流量/连接/日志。
- 全键盘操作、顶部 Tab 切换、每个界面有明确的快捷键提示栏。

## 架构决策

- 根 `Cargo.toml` 增加 `[workspace]`，`crates/sbtui` 为成员；release workflow 增加 sbtui 多平台构建。
- 依赖方向：sbtui 不依赖 sbctl crate（客户端与服务端无共享代码必要）；共享的只有订阅 URL 格式约定。
- HTTP 客户端用 reqwest（rustls）；Windows 系统代理用 winreg + InternetSetOption 刷新。
- 数据目录：`~/.config/sbtui/`（Windows 为 `%APPDATA%\sbtui\`）：`profiles.toml`、`settings.toml`、`cache/`（内核、订阅缓存、日志）。
- 内核管理：从 GitHub Release 下载 sing-box（SHA-256 校验，镜像 URL 可配置）；`sing-box check` 预检；子进程托管 + 崩溃自动重启。
- 订阅导入：粘贴任意 sbctl 订阅链接自动归一化为 `/sub/<cred>/sing-box-full.json`；支持本地文件与 URI 列表；定时自动更新。
- 模式切换：TUI 本地改写配置 `inbounds` 段——system-proxy 模式用 mixed 入站（127.0.0.1:2080），TUN 模式用 tun 入站（需管理员/wintun）。
- 日志：内核配置写 `log.output` 文件，TUI tail。

## UI 结构（5 个 Tab）

1. 🏠 仪表盘：运行状态、当前节点、出站模式（规则/全局/直连）、实时上行/下行速率
2. 🧭 代理：分组+节点列表（clash_api `/proxies`），↑↓+Enter 选择，`t` 单节点测延迟 / `T` 全组
3. 🔗 连接：实时连接表（clash_api `/connections`），`x` 关闭选中连接
4. 📜 日志：实时内核日志 + `r` 查看当前分流规则
5. ⚙️ 设置：订阅档案管理（增删改/立即更新）、系统代理开关、TUN 开关、内核版本管理、镜像设置、开机说明

底部状态栏：模式提示 + 关键快捷键（Tab 切换、q 退出、? 帮助）。

## 首版功能范围（用户已确认全选）

- 基础：订阅导入/更新、代理组与节点选择、延迟测试、内核启停
- 系统代理开关（Windows 注册表 + WinINET 刷新；macOS networksetup；Linux gsettings/KDE/env 提示）
- TUN 模式（管理员权限检测与提权指引；Windows wintun.dll 随内核 zip 提供）
- 流量与连接面板（clash_api traffic/connections）
- 日志与规则页

## 验收

Windows 真机：导入生产订阅 → 测延迟 → 切节点 → 系统代理生效 → TUN 生效 → 速率/连接/日志正常。Linux/macOS 编译通过 + CI 构建。

## 边界

- 不做 GUI、托盘、主题系统；不做订阅转换（服务端已多格式）。
- 不自动修改防火墙；TUN/系统代理遵循「提示 + 用户确认」。
- 首版不支持多订阅聚合（单档案激活）。
