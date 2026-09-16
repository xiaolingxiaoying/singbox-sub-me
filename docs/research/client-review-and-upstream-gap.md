# 客户端审查与上游差距分析（sbtui / sbgui）

审查日期：2026-09-16。本文记录对当前项目三部分（服务端 `sbctl`、终端客户端 `sbtui`、
桌面客户端 `sbgui`）的现状审查，对三个参考项目（`SagerNet/sing-box`、
`mihomo-party-org/clash-party`、`clash-verge-rev/clash-verge-rev`）的可借鉴能力梳理，
以及本轮对两个客户端所做的改进与遗留事项。

## 1. 三部分现状

### 1.1 服务端 `sbctl`

- 单 crate、无 workspace 时代即存在，测试充分（153 个库测试），文档与 21 篇 ADR 完整，
  覆盖签名 manifest、原子提交、回滚、证书生命周期、流量账期、五种协议与订阅矩阵。
- 审查发现一个跨 crate 的真实缺陷（见 1.4），已在本次修复。
- 发布流程 `release.yml` 以 `cargo build --release` 构建整个 workspace；`ci.yml` 以
  `cargo test --workspace` 跑全部测试。因此 workspace 成员的特性合并会直接影响发布契约。

### 1.2 终端客户端 `sbtui`

功能完整：订阅归一化与兼容回退、内核下载/校验/`sing-box check`、系统代理与 TUN、
clash_api 的组选择/延迟/流量/连接、日志与分流规则、OSC 52 复制、自动更新与崩溃退避重启。
审查发现的问题：

1. **共享代码靠 `#[path]` 文本包含**。`crates/client-core/src/lib.rs` 用
   `#[path = "../../sbtui/src/*.rs"] pub mod …` 把 `sbtui` 的源码编译进 `client-core`，
   导致同一份代码被编译两次、同名类型互不兼容，也让两个 crate 的边界形同虚设。
2. **代理组高亮失效**。`App::group_list` 从未 `select`，`draw_proxies` 却用它渲染高亮，
   键盘选择时左侧组列表没有高亮。
3. **延迟测试是串行的且阻塞 UI**。`T` 逐个 `await`，大组耗时 `节点数 × 5s`。
4. **代理组几乎不刷新**。仅在 `groups.is_empty()` 时拉取，节点在其他地方变化后界面不更新。
5. **端口/延迟地址写死**：`LOCAL_MIXED_PORT`、clash_api 的探测 URL 都不可配置。
6. **连接/日志不可过滤**，连接表也没有展示命中规则与出站链路。
7. **流量只有两条对数刻度的文本条**，没有时间维度的历史。
8. `settings.toml` 里的 `traffic_mode`、`auto_start` 等没有在 UI 中生效或可改。

### 1.3 桌面客户端 `sbgui`

改进前是一个**静态样机**：五页全部由硬编码字符串渲染，所有按钮 `on_click(|_, _, _| {})`，
没有任何 `client-core` 依赖，也不反映真实内核、订阅或连接状态。它的存在价值只是确认
GPUI 能编译、能开窗。

### 1.4 跨 crate 缺陷：签名 manifest 的规范字节不稳定（已修复）

`src/release.rs::canonical_bytes` 用 `serde_json::to_value` + `to_vec` 生成签名载荷。
加入 workspace 成员后，某个依赖打开了 `serde_json/preserve_order`，使 `Value` 内的
对象从 `BTreeMap`（键排序）变成 `IndexMap`（插入序），于是规范字节从「键排序」变成
「字段声明序」，`cargo test --workspace` 因此失败，**且已发布的签名 manifest 会因为
构建环境是否包含该特性而无法通过校验**。安装脚本按 `jq -S -c 'del(.signature)'` 的
排序口径生成签名载荷，与代码期望一致，所以正确修复是显式按键排序（而不是改测试）。
本次已改为递归按键排序的确定性序列化，恢复发布契约。

> 集成测试 `tests/cli.rs` 中的 5 个用例在 Windows 上失败（它们调用 `sh` 脚本、
> `certbot`、`sysctl` 等 Unix 假件，Windows 报 `os error 193`），与本次改动无关，
> Linux CI 上通过。

## 2. 上游能力梳理

### 2.1 `SagerNet/sing-box`（数据面契约）

- 客户端完整配置由服务端生成，包含 `experimental.clash_api`（本客户端控制通道）、
  `tun` 入站、`route`/`rule_set`、`selector`/`urltest` 出站组。
- `urltest` 组自带 `url`、`interval`、`tolerance`、`idle_timeout`，说明「自动选择」是
  内核职责而非 UI 职责；UI 需要展示组类型并区分可选择（selector）与自动（urltest）。
- clash_api 提供 `/proxies`、`/proxies/:name/delay`、`/configs`、`/traffic`、`/connections`，
  正是客户端全部控制能力的来源。本项目的 `client-core::clash_api` 已覆盖这些端点。
- 客户端文档（`.reference-sing-box/docs/clients/`）确认 TUN、系统代理、订阅是客户端边界，
  内核不做这些。

结论：内核能力已经足够，客户端的差距在**呈现与交互**，而不是协议。

### 2.2 `clash-verge-rev`（Tauri + React，最接近的能力参照）

可借鉴且已采纳：

- **并发组延迟测试 + 每个节点的延迟状态**（`use-group-delays.ts`、`delay.ts`）。
- **连接表展示规则与链路**，并支持排序与过滤（`components/connection/*`、
  `use-connection-data.ts`）。
- **流量历史与实时图表**（`use-traffic-data.ts`、`traffic-monitor-worker.ts`）。
- **系统代理状态机 + 自动开启**（`use-system-proxy-state.ts`、`use-system-state.ts`）。
- **订阅用量与配置编辑**（`use-clash.ts`、`use-profiles.ts`）。
- **设置项即 UI**（混合端口、测试 URL、自动化开关等）。

暂未采纳（记为路线图）：服务模式安装器（Windows Service 以支持无管理员 TUN）、
托盘与全局快捷键、多语言、自动更新器、WebDAV 备份。

### 2.3 `clash-party`（Electron + TS）

- 覆写（override）与 Smart Core 规则、Sub-Store 深度集成、WebDAV 备份、流量历史数据库
  （`src/main/traffic/database.ts`）、托盘图标裁剪等，是「长期运营一个机场客户端」的形态。
- 对本项目的意义：客户端可以逐步增加**订阅覆写**与**多档案管理**，但项目定位是单管理员的
  私有订阅，Sub-Store/机场运营能力不在范围内。

结论：两个 GUI 上游都证明「控制台三件套」——**概览仪表盘、节点页、连接页**——是核心，
其次是订阅与设置。这与本项目 `PRODUCT.md` 的方向一致。

## 3. 本轮改进

### 3.1 架构：`client-core` 成为真正的共享控制面

- 把 `clash_api`、`core`、`settings`、`subscription`、`system_proxy` 从 `sbtui/src`
  **移动**到 `client-core/src`，删除 `#[path]` 包含；`sbtui` 通过
  `pub use client_core::{...}` 复用，路径 `crate::clash_api::…` 不变。
- `Settings` 新增 `mixed_port`、`test_url`、`auto_start`、`auto_system_proxy`、
  `traffic_mode`（全部 `#[serde(default)]`，向后兼容旧 `settings.toml`）。
- `data_dir_for(app)` 支持 TUI 与 GUI 使用各自数据目录，避免争抢同一内核进程。
- `core::adapt_inbounds` 与系统代理改为使用配置端口；`clash_api::delay_with` 支持自定义
  探测地址。

### 3.2 引擎：`ClientController` 可被两个 UI 同时使用

- 重写 `controller.rs` 为常驻引擎：`snapshot()` 非阻塞读取、`send()` 非阻塞提交命令，
  后台按 500ms 节拍轮询流量（1s）、连接（2s）、代理组（3s），维护速率历史、日志 tail、
  订阅自动更新、崩溃指数退避重启与系统代理联动的启停。
- `ClientCommand` 扩展为覆盖两个 UI 的完整操作集（含 `RestartCore`、`TestGroup`、
  `DownloadCore`、`UpdateSettings(SettingsPatch)`、`Refresh`）。
- `ClientSnapshot` 扩展为完整可渲染状态（档案、设置、速率历史、连接数、订阅用量、
  内核版本、事件与日志、忙碌标记、重启计数）。

### 3.3 `sbtui` 改进

- 修复代理组高亮；节点页选择、组列表与状态三者保持一致。
- `T` 改为 **并发** 测整组延迟，失败节点记为「超时」并以红色显示，状态行给出 `可用/总数`。
- 代理组每 3 秒刷新，`selected_member` 会按当前节点自动定位。
- 连接表新增 **规则** 与 **链路** 列，`/` 关键字过滤；日志新增 `/` 关键字过滤。
- 概览页实时流量面板新增近 5 分钟上下行 **sparkline**、累计与峰值。
- 设置页可在线修改自动更新间隔（`a`）、混合端口（`P`）、延迟地址（`U`）、
  「启动时自动启动内核」（`g`）与「内核就绪后自动开启系统代理」（`y`）。
- 启动时按 `auto_start` 自动拉起内核；TUN/系统代理模式持久化到 `settings.toml`。
- 订阅用量显示到期剩余天数。

### 3.4 `sbgui` 重写

- 由 `ClientController` 驱动：`main` 建立 tokio 多线程 runtime 并 `start` 引擎，
  GPUI 每 400ms 轮询 `snapshot()` 重绘。
- 五页全部接入真实数据：概览（指标卡、语义路径、流量条形图、快速操作）、
  节点（组切换、节点列表、单点/整组测延迟）、连接（断开单条/全部）、
  日志（内核日志按级别着色 + 事件流）、设置（档案激活、内核下载、模式与自动化开关）。
- 全局操作栏：更新订阅、系统代理、启动/停止内核；底部状态栏显示引擎状态与忙碌操作。

## 4. 验证

- `cargo fmt --all -- --check` 通过。
- `cargo clippy --workspace --all-targets -D warnings` 通过。
- `cargo test --workspace --lib`：`sbctl` 153、`client-core` 27、`sbtui` 8 全部通过。
- `sbgui` 通过 `cargo check`/`clippy`；GPUI 无法在无头环境做行为测试。

## 5. 遗留与建议

1. **TUI 尚未走 `ClientController`**。`sbtui` 仍保留自己的 `App` 与轮询逻辑，与引擎
   有重复。建议下一步把 `App` 收敛为「引擎快照 + UI 局部状态」，删除重复的 `start_core`/
   `refresh_*`，彻底消除双引擎。
2. **`sbgui` 缺少文本输入**。当前只能激活已有档案，无法在 GUI 内新增/编辑订阅链接、
   修改镜像或端口。需要一个输入组件（或复用系统剪贴板 + 简单输入框）。
3. **GUI 无托盘与快捷键**，也无「关闭到托盘」。可参考 clash-party/verge 的托盘菜单。
4. **服务模式**（Windows Service/systemd user unit）缺失，TUN 仍要求管理员权限，
   这是上游最实用的下一项能力。
5. **订阅覆写**与**多档案自动更新策略**可由服务端 `overrides/` 承担，客户端保持只读。
6. **集成测试在 Windows 无法运行**（`tests/cli.rs` 依赖 Unix 假件）。建议按
   `#[cfg(unix)]` 门控，或提供 Windows 等价假件，让本机也能完整跑 `cargo test --workspace`。
7. **CI 覆盖 GUI**：当前 workspace 会构建 `sbgui`，但 Linux 上 GPUI 需要额外的图形库；
   建议为 `sbgui` 增加独立的 Windows-only job，或在 CI 中显式跳过非 Windows 的 GUI 构建。
