# sbtui — 终端 sing-box 代理客户端

sbtui 是一个跑在终端里的 sing-box 代理客户端（TUI），对标 Clash Party 的基本功能：
订阅管理、节点切换、延迟测试、系统代理 / TUN、流量与连接、日志查看。全键盘操作。
概览页会将本机、系统代理、当前节点和公网出口绘制为一条语义网络路径；窄终端会自动降级为紧凑视图。按 `?` 可随时查看完整快捷键。

## 构建与运行

```bash
cargo build --release -p sbtui
# 产出两个等价的客户端二进制：
#   Windows: target/release/sbtui.exe 和 target/release/ly.exe
#   Linux/macOS: target/release/sbtui 和 target/release/ly
# `ly` 是短启动名（对齐服务端 sbctl 的 `ly`），运行的是同一个 TUI。
```

数据目录：Windows `%APPDATA%\sbtui`，Linux/macOS `~/.config/sbtui`。
`settings.toml`（应用设置）与 `profiles.toml`（订阅档案）缺失时会自动创建。

## 安装与 `ly` 快捷方式

`ly` 与 `sbtui` 是同一个客户端编译出的两个二进制，客户端机器上运行 `ly` 即打开 TUI。

- **Linux / macOS**：把 `sbtui`、`ly` 与 `packaging/install.sh` 放在一起，然后

  ```bash
  sudo sh install.sh          # 安装到 /usr/local/bin/{sbtui,ly}
  ly                          # 等同 sbtui
  ```

- **Windows**：右键 `packaging\install.ps1` → 用 PowerShell 运行（无需管理员）：

  ```powershell
  powershell -ExecutionPolicy Bypass -File install.ps1
  ly                          # 任意终端可用（ly.exe 写入 %LOCALAPPDATA%\Microsoft\WindowsApps）
  ```

## 首次使用（5 步）

1. 启动 `sbtui`，按 `5` 进入「设置」页；
2. 按 `n` 输入档案名，再粘贴订阅链接（任意 sbctl 订阅后缀都会自动归一化成
   `sing-box-full.json` 完整配置）；
3. 按 `d` 下载 sing-box 内核（从 GitHub Release，可用 `r` 设置镜像前缀）；
4. 按 `u` 拉取订阅（之后 `s` 启动内核）；
5. 回到「仪表盘」（`1`）确认系统代理已开启，去「代理」（`2`）测延迟、切节点。

## 快捷键

| 键 | 作用 | 作用范围 |
| --- | --- | --- |
| `Tab` / `1`–`5` | 切换页签 | 全局 |
| `s` | 启动 / 停止内核（崩溃后自动退避重启） | 全局 |
| `p` | 开 / 关系统代理 | 全局 |
| `m` | 切换 系统代理 / TUN 模式（需按两次确认；下次启动生效；TUN 需管理员权限 + wintun.dll） | 全局 |
| `u` | 更新订阅（失败时回退上次缓存） | 全局 |
| `↑↓` / `k j` | 移动选择 | 代理 / 连接 / 设置 |
| `Enter` | 切换节点（代理页）/ 激活档案（设置页） | 代理 / 设置 |
| `t` / `T` | 测当前节点 / 并发测全组延迟（超时显示为「超时」） | 代理 |
| `o` | 出站模式循环切换（规则 → 全局 → 直连） | 全局（内核运行时） |
| `r` | 日志页切换 日志 / 分流规则 视图 | 日志 |
| `Space` | 暂停 / 恢复日志滚动 | 日志 |
| `l` | 日志级别过滤（全部 → info+ → warn+ → error） | 日志 |
| `/` | 关键字过滤（连接表 / 日志） | 连接 / 日志 |
| `c` | 复制当前行（OSC 52 剪贴板，支持 SSH） | 日志 |
| `x` / `X` | 关闭选中连接 / 关闭全部连接 | 连接 |
| `S` | 切换连接表排序（下载 / 上传 / 主机 / 目标） | 连接 |
| `n` | 新增档案（输入链接） | 设置 |
| `f` | 从本地文件导入订阅 | 设置 |
| `e` | 修改选中档案的订阅链接 | 设置 |
| `Delete` | 删除选中档案（按两次确认） | 设置 |
| `v` / `d` | 设置内核版本 / 下载内核 | 设置 |
| `r` | 设置镜像前缀 | 设置 |
| `a` | 设置订阅自动更新间隔（分钟，0 = 关闭） | 设置 |
| `P` | 设置本地混合代理端口 | 设置 |
| `U` | 设置延迟测试地址 | 设置 |
| `g` / `y` | 切换「启动时自动启动内核」/「内核就绪后自动开启系统代理」 | 设置 |
| `q` | 退出（系统代理开着时会询问是否保留） | 全局 |

## 订阅与内核

- 订阅支持 sbctl 四种格式链接 + 二维码/总览页链接（自动归一化），也支持纯
  sing-box JSON 链接与 Base64 URI 列表（自动转换）；
- 兼容旧版 sbctl：若完整客户端配置端点不存在，且原始链接是 `sing-box.json`，
  客户端会回退到该裸节点端点，并在本地补齐选择器、入站与 Clash API；
- 订阅缓存按档案存放在 `cache/`，更新失败时保留上次缓存；
- 内核从 sing-box 官方 Release 下载并进行 SHA-256 校验；Windows 的 TUN 模式另需
  将 `wintun.dll` 放在内核同目录；
  `v` 可固定版本，`r` 可配置镜像前缀加速下载；
- 启动内核前会先用 `sing-box check` 校验本地生成的运行时配置。

## TUN 模式

- Windows：需要管理员权限运行终端（`m` 切换时会自动检测并提示），且内核
  zip 自带的 `wintun.dll` 与 `sing-box.exe` 同目录（下载器已保证）；
- 切换模式（`m`）后重启内核（`s` 停止再启动）生效；
- 非 root/管理员环境按 `s` 启动 TUN 会被直接拒绝并给出指引。

## 订阅自动更新

设置页按 `a` 修改自动更新间隔（分钟，`0` 表示关闭），修改会写入 `settings.toml`；
内核运行期会按该间隔自动拉取激活订阅。`u` 手动更新始终可用。

## 界面特性

- 概览页的实时流量面板带有近 5 分钟的上下行速率 sparkline，并显示累计与峰值。
- 「节点」页 `T` 会并发探测整组延迟，失败节点显示为红色「超时」。
- 「连接」页展示目标、主机、网络、命中规则与出站链路，`/` 可按关键字过滤。
- 「日志」页 `l` 按级别、`/` 按关键字过滤，`Space` 暂停，`c` 复制。
- 设置页可在线修改混合端口、延迟测试地址、自动更新间隔与两个自动化开关。

## 与 sbctl 的关系

sbtui 是客户端；服务端用 [sbctl](../../README.md) 部署。服务端的
`sing-box-full.json` 订阅自带 `clash_api`（127.0.0.1:9090）与 `cache_file`，
sbtui 通过它完成组选择、延迟测试与流量统计。
