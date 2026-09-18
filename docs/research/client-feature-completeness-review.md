# 客户端功能完成度审查：`sbtui` / `sbgui`（对照 Clash Party 与 sing-box）

- 审查日期：2026-09-18
- 审查对象：`crates/sbtui`（终端客户端）、`crates/sbgui`（GPUI 桌面客户端）、`crates/client-core`（共享控制面）
- 审查基线：`HEAD = 2e5209e` **加**未提交的工作区改动（见 §1 哈希表）
- 审查方法：逐文件源码阅读；本地 `cargo check` / `cargo test` 实测；对照 Clash Party 官方文档与 sing-box 官方文档、参考仓库 `.reference-sing-box/`
- 本文只记录审查结论，**不修改任何业务代码**

> **基线漂移警告**：审查期间 `crates/sbgui/src/main.rs` 正被并发修改（2281 → 2512 → 2814 行，先后新增「规则」页）。本文所有结论以 §1 表中的内容哈希为准；§9 给出可直接重跑的复核命令，供未来版本重新判定。

---

## 0. 结论摘要

| 组件 | 判定 | 一句话说明 |
| --- | --- | --- |
| `client-core` | **完成度最高，已是可信地基** | 订阅归一化、内核生命周期、clash_api、系统代理全部收敛到这一层；但两条命令（`ImportSubscription` / `RemoveProfile`）目前只有 GUI 使用 |
| `sbtui` | **基本可用，但未完成** | 功能面最广（订阅、内核、节点、系统代理、TUN、流量、连接、日志、只读规则、设置齐全），但**没有接入 `ClientController`**，自己重复实现了一整套引擎；另有 2 项"文档宣称已实现、代码并未实现"的功能 |
| `sbgui` | **骨架完成，交互层约 60%** | 7 个页面全部由真实数据驱动（概览、订阅、节点、规则、连接、日志、设置），但**完全没有文本输入**、**退出不清理系统代理**、**不显示状态与错误**、无托盘、无 i18n、0 个单元测试 |

实测通过项（本次运行）：

```text
cargo check -p sbtui --offline                 → Finished
cargo check -p sbgui --offline                 → Finished
cargo test -p client-core -p sbtui --lib       → client-core 27 passed / sbtui 10 passed
sbgui 单元测试数量                              → 0
```

两个客户端 crate 中不存在 `TODO` / `FIXME` / `unimplemented!` 标记（全仓检索无命中），因此"未完成项"不会在源码里自述，必须靠本文这类审查记录。

---

## 1. 基线与复现方式

### 1.1 文件基线

```text
git rev-parse HEAD = 2e5209e7166037713f5ba29d3a48b9f9ae089c8f
```

| 文件 | 内容哈希（前 10 位） | 行数 | 修改时间 |
| --- | --- | --- | --- |
| `crates/sbtui/src/lib.rs` | `d808a09cb2` | 2731 | 2026-09-18 15:08:22 |
| `crates/sbgui/src/main.rs` | `7006ef8165` | 2814 | 2026-09-18 21:59:18 |
| `crates/client-core/src/controller.rs` | `f397eb66bb` | 862 | 2026-09-16 21:41:17 |
| `crates/client-core/src/command.rs` | `b471fc69e1` | 76 | 2026-09-16 21:37:02 |
| `crates/client-core/src/core.rs` | `8c22c625b2` | 389 | 2026-09-16 16:54:20 |
| `crates/client-core/src/subscription.rs` | `b2c1c77770` | 568 | 2026-09-16 16:35:58 |
| `crates/client-core/src/system_proxy.rs` | `5cf5d881df` | 517 | 2026-09-16 16:35:53 |
| `crates/client-core/src/settings.rs` | `7bac38307d` | 214 | 2026-09-16 16:35:46 |
| `crates/client-core/src/clash_api.rs` | `bebf7a4044` | 550 | 2026-09-16 16:38:54 |

复核哈希：

```bash
for f in crates/sbtui/src/lib.rs crates/sbgui/src/main.rs crates/client-core/src/*.rs; do
  printf '%s %s\n' "$(git hash-object "$f" | cut -c1-10)" "$f"
done
```

### 1.2 本次运行的验证命令

```bash
cargo check -p sbtui --offline
cargo check -p sbgui --offline
cargo test -p client-core -p sbtui --lib --offline
cargo clippy -p sbgui --offline
```

### 1.3 相关既有文档

| 文档 | 与本审查的关系 |
| --- | --- |
| `docs/client-description.md` | 客户端定位与功能说明；本审查发现其中 3 处与代码不一致（见 §3.2） |
| `docs/research/client-review-and-upstream-gap.md` | 2026-09-16 的上一轮审查；其中的"遗留 1：TUI 尚未走 `ClientController`"至今未完成 |
| `crates/sbtui/README.md` | TUI 使用说明；其中 SHA-256 校验的表述与代码不一致 |
| `crates/sbgui/README.md` | GUI 页面与设计规则说明 |
| `DESIGN.md` / `PRODUCT.md` | 视觉与产品约束 |
| `.reference-sing-box/docs/clients/desktop/features.md` | 官方桌面客户端的服务模式（SFW）能力描述 |
| `.scratch/gui-redesign-20260917/spec.md` | GUI 重设计的需求与已验证项 |

---

## 2. 架构现状：目标 vs 实际

### 2.1 文档承诺的架构

`docs/client-description.md` §3 与 `crates/client-core/src/lib.rs` 顶部注释都承诺：

```text
sbtui / sbgui  →  client-core  →  本机 sing-box + Clash API + OS 代理
```

并明确写道："界面只负责渲染 `ClientSnapshot`，并通过 `ClientCommand` 向控制层发送操作命令。这样可以保证终端客户端和桌面客户端使用相同的业务逻辑。"

### 2.2 实际架构

```text
sbgui ──► ClientController            ✅ 完全符合：400ms 轮询 snapshot()，操作走 send(ClientCommand)
           │
           └─ client-core：clash_api / core / settings / subscription / system_proxy

sbtui ──► 自建 App 事件循环            ❌ 双引擎：自行实现 refresh/start/log/restart
           │
           └─ 仅通过 `pub use` 复用 client-core 的模块（clash_api、core、settings、subscription、system_proxy）
              第 9 行 pub use 了 ClientController，但全文从未使用
```

证据：

| 事实 | 证据 |
| --- | --- |
| TUI 只用 `pub use` 引入共享类型 | `crates/sbtui/src/lib.rs:9`、`:14` |
| TUI 自建状态机 | `crates/sbtui/src/lib.rs:184`（`struct App`） |
| TUI 自建内核启动/停止 | `crates/sbtui/src/lib.rs:635`（`start_core`）、`:705`（`stop_core`） |
| TUI 自建订阅更新 | `crates/sbtui/src/lib.rs:727`（`update_subscription`） |
| TUI 自建轮询与日志 tail | `crates/sbtui/src/lib.rs:427`（`refresh_proxies`）、`:475`（`refresh_connections`）、`:332`（`tail_core_log`） |
| TUI 自建崩溃退避重启 | `crates/sbtui/src/lib.rs:384`（`watch_core_exit`）、`:405`（`schedule_restart`） |
| 引擎侧存在同名逻辑 | `crates/client-core/src/controller.rs`：`start_core_inner`、`update_subscription`、`refresh_proxies`、`tail_core_log`、`schedule_restart` |

### 2.3 双引擎带来的功能后果

这不是"代码洁癖"，而是可观察的行为差异：

| 影响 | `sbtui`（自建引擎） | `sbgui`（共享引擎） |
| --- | --- | --- |
| 长耗时操作是否冻结界面 | **会冻结**。`u` 更新订阅（HTTP 超时 30s）与 `T` 整组测延迟（最长 5s）在按键分支里 `await`，期间 `run_app` 的 tick 分支与 `terminal.draw` 都不执行（`crates/sbtui/src/lib.rs:548` 起的 `run_app`，`:883` 的 `u` 分支，`:1217` 的 `test_group_delays`） | **不冻结**。命令交给引擎任务，UI 保持 400ms 重绘（`crates/sbgui/src/main.rs:main` 中的轮询循环） |
| 命令集覆盖 | 使用自己的一套逻辑 | 使用 `ClientCommand`，因此获得 TUI 没有的 `ImportSubscription` / `RemoveProfile` |
| 反向差异 | TUI 独有：本地文件导入、档案改名/改链接、二次确认删除、规则视图、连接过滤与排序、日志级别过滤与 OSC52 复制 | GUI 无以上能力 |
| 已定义但两个 UI 都未使用 | 无——`ClientCommand::RestartCore` 由 GUI 概览页头"重启内核"按钮使用（`crates/sbgui/src/main.rs:721`），`ClientCommand::Refresh` 由 GUI 规则页"刷新状态"按钮使用（`crates/sbgui/src/main.rs:1427`）；仅 TUI 未使用这两个命令（它用自建启停/重启逻辑） | 同左 |

> 上轮审查（`docs/research/client-review-and-upstream-gap.md` §5.1）把"TUI 尚未走 `ClientController`"列为遗留项 1，本次复核确认**未做**。

---

## 3. `sbtui` 功能清单

### 3.1 已完成且经代码验证

| 域 | 能力 | 证据 |
| --- | --- | --- |
| 页面 | 概览 / 节点 / 连接 / 日志 / 设置 5 页；`Tab`、`1`–`5` 切换 | `crates/sbtui/src/lib.rs:37`（`TAB_TITLES`）、`:1587`（`draw`） |
| 概览 | 语义网络路径（本机 → 系统代理 → 当前节点 → 公网出口）、运行状态、当前节点与延迟、订阅用量、上下行 sparkline（300 样本 × 500ms tick，约 2.5 分钟窗口；注意 GUI 引擎侧才是 300 × 1s = 5 分钟）、累计与峰值、最近事件 | `:1921`（`draw_dashboard`）、`:2098`（`draw_traffic_panel`）、`:2180`（`draw_dashboard_compact`，窄窗口 < 100×28 降级） |
| 节点 | 代理组列表 + 组内节点、当前节点标记、单节点测延迟、**并发整组测延迟**（失败显示"超时"）、自动组不参与手动切换 | `:2251`（`draw_proxies`）、`:1217`（`test_group_delays`，使用 `futures_util::join_all`）、`:1188`（单节点） |
| 出站模式 | 规则 → 全局 → 直连循环（`o`），通过 clash_api `PATCH /configs` 生效 | `handle_key` 的 `o` 分支；`client-core/src/clash_api.rs:302`（`set_mode`） |
| 连接 | 目标 / 主机 / 网络 / **命中规则** / **出站链路** / 上行 / 下行；`/` 关键字过滤；`S` 排序（下载/上传/主机/目标）；`x` 关单条、`X` 关全部 | `:2316`（`draw_connections`）、`:1260`（`visible_connections`）、`:1310`（`sort_connections`）、`:1268`（`close_selected_connection`）、`:1284`（`close_all_connections`） |
| 日志 | tail `cache/core.log`；`Space` 暂停；`l` 级别过滤（全部/info+/warn+/error）；`/` 关键字；`c` OSC52 复制（SSH 可用）；`r` 切换**只读分流规则视图** | `:2384`（`draw_logs`）、`:1026`（`load_rules`）、`:2510`（`copy_to_clipboard_osc52`） |
| 订阅 | 任意 sbctl 后缀归一化为 `sing-box-full.json`（含 `qr/`、`index`）；Base64/明文 URI 列表解析（vless / vmess / hysteria2 / tuic / anytls）；旧版裸 `sing-box.json` 兼容回退；本地文件导入；档案新增 / 改名 / 改链接 / 删除（`Delete` 二次确认）；更新失败回退缓存；`subscription-userinfo` 用量与到期剩余 | `client-core/src/subscription.rs:18`（`normalize_url`）、`:39`（`bare_sing_box_fallback_url`）；`crates/sbtui/src/lib.rs:787`（`fetch_subscription_with_compatibility`）、`:1426`（`commit_input`）、`:1339`（`delete_selected_profile`） |
| 内核 | GitHub Release 下载（`v` 固定版本、`r` 配置镜像、Windows 自动提取 `wintun.dll`）；`sing-box check` 预检；子进程托管；崩溃指数退避重启（2→4→8→16→30s） | `client-core/src/core.rs:81`（`download_core`）、`:31`（`check_config`）、`:48`（`start`）、`:72`（`restart_backoff`） |
| 网络接管 | 系统代理开关（`p`）：Windows 注册表 + WinINET 刷新 / macOS `networksetup` / Linux `gsettings`，并**备份原代理、关闭时恢复**；TUN 模式（`m`，二次确认、管理员权限检测、`wintun.dll` 检测） | `client-core/src/system_proxy.rs:141`（`enable`）、`:148`（`disable`）、`:487`（`can_use_tun`） |
| 设置 | 在线修改自动更新间隔 `a`、混合端口 `P`、延迟地址 `U`、内核版本 `v`、镜像 `r`、`g` 启动时自动启动内核、`y` 自动开系统代理 | `:2425`（`draw_settings`）、`:1426`（`commit_input`） |
| 安全与可发现性 | `?` 完整快捷键帮助；退出时若系统代理仍开着会二次确认（可选保留代理设置） | `:1807`（`draw_help_overlay`）、`:1785`（`draw_input_overlay`）、`:1850`（`draw_confirmation_overlay`）、`:548`（`run_app` 的退出分支） |

### 3.2 部分完成 / 名不副实 / 缺失

| 项 | 状态 | 证据与说明 |
| --- | --- | --- |
| 共享控制面接入 | ⚠️ 未完成 | §2；两个 UI"业务逻辑不会漂移"的承诺目前不成立 |
| **URI 列表订阅实际不可用** | ❌ 功能不成立 | `subscription::parse_uri_list`（`client-core/src/subscription.rs:275`）生成的 `raw` 只有 `{"outbounds":[...]}`，**没有 `experimental.clash_api`、没有 selector 组、没有 `route.final`、没有 inbounds**。客户端启动内核后强制探测 clash_api，约 5s 后杀掉内核：`crates/client-core/src/controller.rs:407`（`内核已启动但 clash_api 未响应；确认订阅配置包含 clash_api`）、`crates/sbtui/src/lib.rs:700`。只有 JSON 路径的 `wrap_bare_node_config`（`subscription.rs:221`）会补齐控制链路。`crates/sbtui/README.md` 宣称"Base64 URI 列表（自动转换）"可用 |
| **Clash/Mihomo 配置导入** | ❌ 功能不成立 | 客户端 crates 无 `serde_yaml`，无任何 YAML 解析；`clash.yaml` 仅作为 URL 字符串出现在归一化测试中。`docs/client-description.md` §4.1 将"Clash/Mihomo 配置"列为支持的订阅来源 |
| **内核 SHA-256 校验** | ❌ 功能不成立 | `crates/sbtui/README.md` 宣称"内核从 sing-box 官方 Release 下载并进行 SHA-256 校验"，但 `core.rs:81`（`download_core`）只做下载 + 解包；`sha2` 在两个客户端 crate 中**无任何调用点**（全仓检索 `sha256` / `Checksum` 无命中）。结合可自定义镜像前缀，等于允许从任意镜像执行未校验的二进制 |
| **q 键模态穿透** | ❌（基线缺陷，2026-09-19 已修复） | 退出拦截在 `run_app` 顶部先于输入框与帮助覆盖层判断（`crates/sbtui/src/lib.rs:572`）；在帮助页或输入档案名时按 `q` 会直接进入退出流程，与输入/帮助模态语义冲突 |
| 规则页 | ⚠️ 只读 | 可查看规则与 rule_set 来源，不能增删改；与 Clash Party 的"覆写"不是一个量级 |
| 延迟数据 | ⚠️ 仅内存 | 只在本次运行内有效，无历史、无排序、无持久化 |
| 流量统计 | ⚠️ 仅约 2.5 分钟内存环形缓冲（TUI 自采 300 样本 × 500ms tick；引擎侧 `state.rs` 的 300 样本按 1s 节奏才是 5 分钟） | `crates/sbtui/src/lib.rs:41`（`TRAFFIC_HISTORY`）；无持久化历史，关闭即丢失 |
| TUN | ⚠️ 需停核切换、需管理员/root | `controller.rs` 的 `SetTrafficMode` / `UpdateSettings` 在内核运行时直接拒绝；无服务模式（见 §6.2） |
| 控制通道 | ⚠️ 硬编码 | `clash_api.rs:11` `DEFAULT_CONTROLLER = http://127.0.0.1:9090`，且不支持 `secret`；服务端目前恰好生成 `127.0.0.1:9090` 且无 secret（`src/subscription.rs:1412`），任意一侧改动即失效 |
| 当前节点识别 | ⚠️ 依赖固定标签 | `clash_api.rs:14` `SELECTOR_TAG = "🚀节点选择"`；非 sbctl 生成的配置或改名组会导致当前节点为空/滞后 |
| 其他缺失 | ❌ | 覆写/合并、WebDAV 备份、Sub-Store、服务模式、配置热重载、允许局域网、每档案 UA/更新间隔、主题、i18n、客户端自更新器、订阅分享/二维码 |

---

## 4. `sbgui` 功能清单

### 4.1 已完成且真实接通

| 域 | 状态 | 证据 |
| --- | --- | --- |
| 运行模型 | 真实：`main()` 建 tokio runtime → `ClientController::start(dir)` → GPUI 每 400ms 轮询 `snapshot()` 重绘 | `crates/sbgui/src/main.rs:2726`（`main`）、`:137`（`struct Sbgui`） |
| 页面 | 概览 / 订阅 / 节点 / **规则** / 连接 / 日志 / 设置，共 7 页 | `crates/sbgui/src/main.rs:83`（`enum Page`）、`:785`（`content`）、`:1388`（`rules`） |
| 侧栏常驻控制 | 出站模式分段按钮、系统代理开关、TUN 选择（内核运行中锁定并显示"停核后切换"）、当前订阅与用量、实时上下行、内核状态 | `:322`（`sidebar`）、`:555`（`mode_selector`）、`:619`（`sidebar_switch`） |
| 概览 | 四项实时指标、真实采样折线（GPUI canvas）、当前节点/延迟/组类型、客户端事件、累计与峰值 | `:799`（`dashboard`）、`:2149`（`traffic_chart`） |
| 订阅 | 从剪贴板导入（HTTP/HTTPS）、激活、立即更新、删除、用量 | `:978`（`subscriptions`）、`:994`（`ImportSubscription`）、`:1121`（`RemoveProfile`） |
| 节点 | 组切换、节点网格、当前节点标记、单点测延迟、整组测延迟、自动组禁止手动切换、测试不触发切换 | `:1156`（`proxies`） |
| 规则 | 读取 `cache/active-config.json` 的 `route.rules` + `final`，展示匹配条件与出站 | `:1388`（`rules`）、`:2215`（`read_rule_rows`） |
| 连接 | 目标/主机/网络/规则/上行下行 + 行内断开 + 关闭全部 + 横向滚动 | `:1482`（`connections`）、`:2427`（`connection_row`） |
| 日志 | 内核日志（按级别着色）+ 运行事件流 | `:1557`（`logs`） |
| 设置 | 档案激活、内核版本/状态/镜像展示、检查并更新内核、流量模式切换、出站模式循环、`auto_start` / `auto_system_proxy` 开关 | `:1651`（`settings`）、`:1795`（`DownloadCore`）、`:1824` 与 `:2595`（`UpdateSettings`） |
| 窗口 | 自绘标题栏（拖拽/最小化/最大化/关闭）、DWM 圆角、内嵌图标资源、最小 860×640 | `:216`（`titlebar`）、`fn apply_windows_window_chrome` |

### 4.2 部分完成 / 缺失

以下每一条都在基线版本上重新检索过，命中数均为 0，可直接复核：

| 项 | 状态 | 证据 / 后果 |
| --- | --- | --- |
| **无文本输入** | ❌ | `TextInput` / `InputState` 命中 0；全文件交互仅 `on_click`。因此 GUI **不能**：改镜像、改混合端口、改延迟地址、改自动更新间隔、固定内核版本、编辑订阅链接、给档案命名。设置页的"混合端口 / 延迟地址 / 镜像 / 自动更新"只是只读行（`:2529` `setting_line`），可写的只有 4 个开关 + 2 个动作 |
| **订阅不归一化** | ❌ 与 TUI 行为不一致 | `normalize_url` 在 `crates/sbgui/src/main.rs` 命中 0；`normalize_url` 全仓只在 TUI 被调用（`crates/sbtui/src/lib.rs:1442`、`:1493`）。GUI 的 `ImportSubscription` 直接保存原 URL，`update_subscription` 直接抓取。后果：粘贴 `/sub/<cred>/clash.yaml`、`/qr/...`、`/index` 会拉到 YAML/HTML，`parse` 报"不是 sing-box JSON 或 URI 列表"。GUI 同样没有 `bare_sing_box_fallback_url` 旧版兼容回退 |
| **退出不清理系统代理** | ❌ 真实故障 | `system_proxy::disable` 命中 0；无 `on_window_should_close`、无退出确认。内核是 `kill_on_drop`（`core.rs:48`），窗口一关内核即死，但 `ProxyEnable=1` 仍指向 `127.0.0.1:<port>` → 浏览器与商店类应用直接断网。TUI 有退出确认（`crates/sbtui/src/lib.rs:548`），GUI 没有 |
| **不显示 status / busy** | ❌ | `snapshot.status` 命中 0。引擎错误（TUN 需管理员、系统代理设置失败、订阅更新失败）只出现在概览"客户端事件"的最近 4 条；`busy`（当前执行中的命令）完全不可见 |
| 无过滤 / 搜索 | ❌ | 连接表无关键字过滤与排序（TUI 有）；日志无级别过滤、无关键字、无复制 |
| 无托盘 / 无全局快捷键 / 无开机自启 | ❌ | `crates/sbgui/Cargo.toml:27` 声明 `tray-icon = "0.21"`，但 `tray` 在源码中命中 0；"启动时自动启动内核"是应用内 `auto_start`，不是 OS 级开机自启 |
| 删除档案无二次确认 | ⚠️ | `RemoveProfile` 点击即删；TUI 需要按两次 `Delete` |
| **`auto_start` 开关无实际行为** | ❌（基线缺陷，2026-09-19 已修复） | 引擎从不读取 `auto_start`（`controller.rs` 无该字段的处理分支），GUI 的开关只持久化；TUI 是自己实现的（`crates/sbtui/src/lib.rs:555`）。GUI 打开后重启客户端并不会自启内核 |
| 无 TUN 热切换 / 无服务模式 | ❌ | 切模式必须先停核；TUN 需管理员（见 §6.2） |
| 无主题 / 无 i18n | ❌ | 中文硬编码 + 单套配色 |
| 无测试 | ❌ | `crates/sbgui/src/main.rs` 中 `#[test]` 计数为 0；保障仅有 `cargo check` / `clippy` |
| 平台覆盖 | ⚠️ | README 与代码均面向 Windows（DWM 圆角、`.rc` 图标、`install.ps1`）；Linux/macOS 未验证 |

---

## 5. 与 Clash Party 的功能对照

Clash Party 特性来源：[官方手册](https://clashparty.org/docs/handson)、[覆写文档](https://clashparty.org/docs/guide/override)、[仓库](https://github.com/mihomo-party-org/clash-party)。

| 能力 | Clash Party | `sbtui` | `sbgui` |
| --- | --- | --- | --- |
| 订阅导入 / 更新 / 删除 / 自动更新间隔 | ✅ | ✅（间隔为全局） | ⚠️ 仅剪贴板导入、无间隔设置、不归一化 URL |
| 规则 / 全局 / 直连 | ✅ | ✅ | ✅ |
| 节点组切换 + 延迟测试 | ✅ | ✅（并发整组） | ✅（单点 / 整组） |
| 连接表（过滤 / 排序 / 断开） | ✅ | ✅ | ⚠️ 仅断开 |
| 流量曲线 + 累计 | ✅（含历史库） | ✅（5 分钟内存） | ✅（5 分钟内存） |
| 日志查看 / 过滤 | ✅ | ✅（级别 + 关键字 + 复制） | ❌ 只能看 |
| 规则页 | ✅（可视化 / 可编辑） | ⚠️ 只读 | ⚠️ 只读 |
| **覆写（YAML / JS 合并）** | ✅ 核心卖点 | ❌ | ❌ |
| **Sub-Store 集成** | ✅ | ❌ | ❌ |
| **WebDAV 备份 / 恢复** | ✅ | ❌ | ❌ |
| **托盘 + 关闭到托盘 + 托盘测延迟** | ✅ | n/a | ❌（依赖已声明未使用） |
| **主题切换** | ✅ 多主题 | ❌ | ❌ |
| **无需服务模式的 TUN** | ✅ | ❌ 需管理员 | ❌ 需管理员 |
| 内嵌内核 + 内核更新 | ✅（Smart / Mihomo 双内核） | ✅（仅官方 sing-box；可固定版本） | ⚠️ 仅下载最新（无版本输入） |
| 配置热重载 | ✅（Mihomo） | ❌ 需重启内核 | ❌ 需重启内核 |
| 每订阅独立 UA / 更新策略 | ✅ | ❌ | ❌ |
| 界面语言 | 多语言 | 中文 | 中文 |
| 客户端自更新 | ✅ | ❌ | ❌ |

**读法**：本项目已把 Clash Party 的"控制台三件套"（概览 / 节点 / 连接）加上订阅、系统代理、TUN、日志做出来，但 Clash Party 的"运营型能力"（覆写、备份、托盘、服务模式、主题、i18n）一个都没有。对"私有订阅 + 单管理员"的定位，覆写与 Sub-Store 可以明确不做；但**托盘（GUI 常驻）、服务模式（TUN 免提权）、配置热重载**属于日常体验的关键项。

---

## 6. 与 sing-box 的对照

### 6.1 已使用的 clash_api 只是子集

已实现（`crates/client-core/src/clash_api.rs`）：`/version`、`/proxies`、`PUT /proxies/:name`（`:246`）、`/proxies/:name/delay`（`:263`）、`GET/PATCH /configs`（`:302`）、`/connections`、`DELETE /connections/:id`、流式 `/traffic`（`:337`）。

未实现（sing-box 提供、Clash 生态客户端普遍使用）：

| 端点 / 能力 | 现状与影响 |
| --- | --- |
| `/logs` 流式日志 | 改为 tail 子进程 stderr 文件；功能够用，但丢失结构化级别与时间戳 |
| `/rules`、`/providers` | 规则权威来源；现在靠解析本地 `cache/active-config.json`（GUI `read_rule_rows`、TUI `load_rules`） |
| 组级测延迟 `/group/:name/delay` | 现在"测全组"是 N 次单节点并发请求，不触发 urltest 组自身的测试语义 |
| 配置热重载（`PUT /configs` 或 reload） | 每次订阅更新后必须重启内核才生效 |
| `secret` 鉴权、可配置 controller 端口 | 端口与鉴权均硬编码（§3.2） |

另外 `/traffic` 的用法是"每秒重新发起请求、只读第一个样本、1.5s 超时"（`clash_api.rs:337`，由 `controller.rs::refresh_traffic` 每秒调用），而不是保持一条长连接：功能正确，但对内核是无谓的连接开销。

### 6.2 与官方 "sing-box for Desktop" 的差距：服务模式

官方文档（`.reference-sing-box/docs/clients/desktop/features.md`）写明：

> **SFW runs sing-box as a system service, so no administrator elevation is required for daily use.**

本项目的两个客户端都直接 `spawn sing-box run`（`core.rs:48`），TUN 必须"以管理员身份运行终端/客户端"（`system_proxy.rs:487` 用 `net session` / `id -u` 判定），**没有服务模式**（Windows Service / systemd unit / launchd）。这是与官方客户端以及 Clash Party（"开箱即用，无需服务模式的 Tun"）最大的体验差距，也直接限制 TUN 的可用性。

---

## 7. 缺陷清单与优先级

### P0：正确性 / 可能造成用户损失

| # | 缺陷 | 证据 | 影响 |
| --- | --- | --- | --- |
| 1 | GUI 退出时不恢复系统代理，也无退出确认 | `crates/sbgui/src/main.rs` 无 `system_proxy::disable` / 关闭钩子；`core.rs:48` 为 `kill_on_drop` | 关窗即断网，用户不知原因 |
| 2 | GUI 订阅不归一化、无旧版回退 | `crates/sbgui/src/main.rs` 无 `normalize_url`；`controller.rs::update_subscription` 直接用 `profile.url` | sbctl 的 `clash.yaml` / `qr` / `index` 链接在 GUI 全部失败，与 `docs/client-description.md` §4.2 承诺矛盾 |
| 3 | URI 列表订阅缺少运行时包装 | `subscription.rs:275`、`controller.rs:407`、`sbtui/src/lib.rs:700` | README 宣称支持的能力实际无法启动内核 |
| 4 | 内核下载无完整性校验 | `core.rs:81`；`sha2` 无调用点 | 自定义镜像 + 无校验 = 供应链投毒路径 |
| 5 | 系统代理备份文件路径写死 `sbtui` | `system_proxy.rs:72`（`backup_path`）调用 `settings.rs:173`（`data_dir()` → `data_dir_for("sbtui")`） | 两个客户端"数据目录分离、互不干扰"的承诺在这条路径上不成立 |
| 6 | GUI 不显示错误与进行中状态 | `snapshot.status` 命中 0 | 用户点击"启动内核"后无任何反馈，失败只能靠概览的 4 条事件 |

### P1：能力补齐（对齐 Clash Party）

1. GUI 文本输入组件（镜像 / 端口 / 延迟地址 / 自动更新 / 内核版本 / 订阅链接 / 档案名 / 规则搜索）——GUI 从"看板"变成"客户端"的分水岭。
2. 托盘 + 关闭到托盘 + 开机自启 + 全局快捷键（`tray-icon` 已在依赖中）。
3. 服务模式（Windows Service / systemd user unit）→ 免提权 TUN；并实现 TUN / 系统代理热切换（一次操作完成"落配置 + 重启内核"）。
4. 订阅更新后的配置热重载，或"更新订阅 → 询问是否立即重启内核"。
5. 把 TUI 的连接 / 日志过滤与排序搬到 GUI。
6. 延迟历史与流量历史持久化（SQLite 或 JSONL 滚动文件），支撑节点质量判断。
7. TUI 收敛到 `ClientController` 并把长等待操作异步化，解决界面冻结。

### P2：可选

i18n 骨架、主题切换、每档案 UA / 更新间隔、允许局域网、客户端自更新器、订阅分享与二维码。

---

## 8. 判定

- **功能"部分完成"吗？——是。** 两者的主链路（导入订阅 → 下载/启动内核 → 切换节点 → 系统代理或 TUN → 查看流量/连接/日志）已真实跑通，且有测试与二进制产物（`dist/sbtui-*`、`dist/ly-*`、`dist/sbgui-clash-inspired.exe`）。但都还达不到"完成"：
  - `sbtui`：能力面最全（约等于 Clash Party 的控制台部分），存在**双引擎**的结构性未完成，加上两条"文档与代码不一致"的假承诺（URI 列表订阅、SHA-256 校验），以及 `u` / `T` 阻塞界面。
  - `sbgui`：信息架构与视觉已按 Clash Party 的组织方式重做，7 页数据全部真实，但交互层只做到"点按 + 展示"，缺少最基础的文本输入、退出清理与错误可见性，且零测试、无托盘、无服务模式。
- **相对 sing-box**：数据面契约定得很准（完整配置文件 + clash_api + 自动补齐入站与选择器），但只用了 clash_api 的子集，并缺少官方桌面客户端的**服务模式**——这是 TUN 体验的真正短板。
- **相对 Clash Party**：结构性差距集中在**覆写/合并、托盘与常驻、服务模式、主题与 i18n、订阅生态（Sub-Store / WebDAV）**。前四项属于"客户端基本素养"，后两项可以因项目定位（私有、单管理员订阅）而明确排除，并写入 `PRODUCT.md` 的边界。

---

## 9. 复核清单（供未来版本重跑）

以下命令可直接验证本文的关键结论。**注意：2026-09-19 的修复轮已改变多项期望值**，括号内为修复后的新期望：

```bash
# 1) GUI 是否存在文本输入（修复前 0 = 仍缺失；修复后期望仍为 0，文本输入未实现）
grep -c "TextInput\|InputState" crates/sbgui/src/main.rs

# 2) GUI 是否使用托盘（期望 0 = 依赖仍闲置）
grep -c "tray" crates/sbgui/src/main.rs

# 3) GUI 是否显示引擎状态/忙碌（修复前期望 0；修复后期望 > 0，页头状态行已实现）
grep -c "snapshot.status\|snapshot.busy" crates/sbgui/src/main.rs

# 4) GUI 是否在退出/关窗时清理系统代理（修复前期望 0；修复后期望 > 0，退出确认 + 恢复已实现）
grep -c "system_proxy::disable\|on_window_should_close" crates/sbgui/src/main.rs

# 5) 订阅归一化调用点（修复前只有 sbtui；修复后期望 client-core/controller.rs 也命中）
grep -rn "normalize_url" crates/

# 6) TUI 是否接入共享引擎（期望只有 pub use 一行；截至修订日仍未接入）
grep -n "ClientController" crates/sbtui/src/lib.rs

# 7) 内核下载是否校验完整性（修复前无输出；修复后期望 core.rs 命中 SHA-256 TOFU 记录）
grep -rn "sha256\|Sha256\|verify_or_record_hash" crates/client-core/src/core.rs

# 8) URI 列表产物是否补齐控制链路（修复前期望只有 outbounds；修复后期望 raw 走 wrap_bare_node_config）
grep -n "pub fn parse_uri_list" -A 20 crates/client-core/src/subscription.rs

# 9) RestartCore / Refresh 的 UI 调用点（期望看到 sbgui 两处命中；TUI 无）
grep -rn "RestartCore\|ClientCommand::Refresh" crates/sbtui/src crates/sbgui/src

# 10) 系统代理备份路径是否仍写死 sbtui（修复前期望命中 data_dir_for("sbtui")；
#     修复后期望 enable/disable 接收调用方的数据目录）
grep -n "fn backup_path" -A 3 crates/client-core/src/system_proxy.rs
```

### 2026-09-19 修订

**事实修正**（原审查结论与代码不符处）：

1. §2.3 与 §4.2 原 称"`RestartCore` / `Refresh` 两个 UI 都未使用""GUI 无重启内核按钮"——**有误**。复核确认 GUI 概览页头有"重启内核"按钮（`main.rs:721`），规则页有"刷新状态"按钮（`main.rs:1427`），两条命令均由 GUI 使用；仅 TUI 未使用（自建启停逻辑）。
2. §3.1 / §3.2 的 TUI 流量窗口原写"5 分钟"——TUI 自采缓冲为 300 样本 × 500ms tick ≈ **2.5 分钟**；5 分钟是引擎侧（1s × 300）的窗口，已在正文更正。
3. 补充基线遗漏缺陷两条：TUI 的 **q 键模态穿透**（帮助/输入模态下按 q 触发退出流程）与 GUI 的 **`auto_start` 开关无实际行为**（引擎从不读取该设置）。

**本轮已修复**（对应 §7 P0 清单，全部有测试或编译验证）：

- P0#1（GUI 退出断网）：注册 `on_window_should_close`，系统代理仍开启时弹出退出确认（恢复并退出 / 保留 / 取选）；"恢复并退出"路径同步调用 `system_proxy::disable(数据目录)`。
- P0#2（GUI 订阅不归一化）：引擎 `import_subscription` 现在调用 `normalize_url` 归一化存储，`update_subscription` 增加 `bare_sing_box_fallback_url` 旧版端点回退（`fetch_with_compat`）。
- P0#3（URI 列表不可用）：`parse_uri_list` 产物现在经 `wrap_bare_node_config` 补齐 selector / direct / route.final / clash_api / inbounds，新增 2 个测试断言完整控制链路（client-core 28 个测试通过）。
- P0#4（内核无校验）：`download_core` 计算归档 SHA-256 并按 tag 做 TOFU 记录（`core/verified-hashes.json`），同版本再次下载不一致即拒绝安装；两客户端安装提示均显示哈希前缀。sing-box 官方不发布校验文件，首次下载无法远程验证——该局限已在函数文档注明。
- P0#5（备份路径写死 sbtui）：`system_proxy::enable/disable` 改为接收调用方数据目录，TUI 与 GUI 引擎各用各的 `cache/system-proxy-backup.json`。
- P0#6（GUI 不显示状态）：页头新增引擎状态行（busy 进度 / `status` 结果 / 崩溃重启等待），全页可见。
- 附带：GUI 连接页现在真实按下载流量降序排序（修复"文案与实现不符"）；GUI `auto_start` 由引擎执行（与 TUI 行为一致）；TUI q 键模态穿透已修复。

**仍未完成**（承接原 P1 清单）：GUI 文本输入组件、托盘/服务模式/热切换、TUI 收敛到 `ClientController` 并异步化长操作、连接/日志的历史持久化。

回归验证：

```bash
cargo check -p sbtui --offline
cargo check -p sbgui --offline
cargo test -p client-core -p sbtui --lib --offline
cargo fmt --all -- --check
```

---

## 附录：审查边界

- 本文未评估：GUI 的原生窗口视觉验收（当前工具不支持截图与点击自动化，见 `.scratch/gui-redesign-20260917/spec.md` 与 `crates/sbgui/README.md` 的说明）、真实 VPS 端到端联调、非 Windows 平台上的 GUI 运行。
- 本文未修改任何业务代码，也未改动 `docs/client-description.md` 与 `crates/sbtui/README.md` 中与代码不一致的表述——这些应作为独立的文档修正任务处理。
