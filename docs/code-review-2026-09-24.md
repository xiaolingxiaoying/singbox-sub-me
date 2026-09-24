# 全项目代码审查与修复记录

日期：2026-09-24　　分支：`main`（`sbctl` 0.2.0）
需求来源：外部目标文档《sing-box sub project 计划》（服务端 / TUI 客户端 / GUI 客户端）。
待补清单：[known-gaps-after-merge.md](known-gaps-after-merge.md)（G1–G10）与
[target-spec-gap-and-verification-plan.md](target-spec-gap-and-verification-plan.md)（§1.2 的 G1–G17）。
构建与验证命令：[verification-and-build-flow.md](verification-and-build-flow.md)。

本文按"证据 → 修复过程 → 验证 → 提交"记录每一条。审查方式：主线先复核既有清单，
再用 4 个只读 subagent 分别覆盖服务端订阅层 / 部署与发布链 / client-core+TUI / sbgui。

## 0. 基线复核（动手之前）

`docs/known-gaps-after-merge.md` 的 10 条缺口**全部仍然开放**，逐条按该文档"复核方式"小节的命令实测：

| 缺口 | 复核命令 | 实测结果 |
| --- | --- | --- |
| G1 | `test -f scripts/winvm/verify.ps1` | 不存在 |
| G2 | `grep Version= packaging/windows/sbtui.wxs` | `Version="0.1.0"`；`:21` 快捷方式名 `sbtui` 指向 `sbgui.exe`；无 `wintun`；`release.yml` 无 `sbgui` |
| G3 | `grep 'let _ = template' src/subscription/template.rs` | `:162` 命中 |
| G4 | `grep -rn 'overrides/' crates/client-core/src` | 零命中 |
| G6 | `grep tray-icon crates/sbgui/Cargo.toml` | `:27` 仍在 |
| G7 | `git grep -c '\.note(' -- crates` | `client-core/src/controller.rs` 23 处 |
| G9 | `grep SING_BOX_VERSION_PROFILES src/subscription/profile.rs` | `:144` 硬编码表仍在 |

L1 基线（Windows 宿主）：`cargo fmt --all -- --check` 退出 0；
`cargo test --workspace --features sbctl/test-signing` 退出 0、无 failed。
环境核实：WSL 发行版名为 `Ubuntu-22.04` 与 `Debian`（均Stopped）；`vmrun.exe` 存在；Docker 29.6.2 可用。

---

## R1（G1）— Windows 真机验证流水线不存在

**证据**：`scripts/winvm/verify.ps1` 缺失。此前所有真机证据都来自 `.scratch/win11-vm/` 里
一次性手敲的 `vmrun` 命令与 `shot-guest.ps1`（`run6.log` 显示 7/7 OK，但**页清单里没有 `about`**，
且 `probe.txt` 显示 `appdata-sbtui-exists=False`——TUI 从未在客户机跑过一次）。
后果：G8/G15 的清单（CJK 字体回退、DPI、DWM、真实注册表代理、wintun 提权、孤儿回收）没有任何可重跑的入口。

**修复**：把一次性动作固化成三条子命令一致的流水线，新增三个文件。

1. `scripts/winvm/verify.ps1` —— 宿主侧编排，子命令 `parse-check | snapshot | revert | gui | tui | collect | all`。
   - 口令只从 `$env:WINVM_PASS` 读，仅进入 `vmrun` 的参数数组，从不 `Write-Host`；
     失败时只打印**参数形状与退出码**（`Invoke-Vmrun` 的 Fail 分支注释写明了这一点）。
   - VM 定位三级回退：`-VmPath` → `$env:WINVM_VMX` → `vmrun list` 里匹配 `Win11*.vmx` → 在常见
     Virtual Machines 目录下搜索。写死一条路径会在虚拟机换盘的那天失效。
   - `Ensure-Started` 用 `checkToolsRunningStatus` 轮询（60×5s）而不是固定 `sleep`。
   - `Finalize` 对比运行前后的**宿主指纹**（HKCU 代理三键、`netsh winhttp show proxy`、
     `sbctl/sing-box/sbgui/sbtui` 服务状态、PATH 条目数），有漂移就写 `host-drift.txt` 并明说——
     这是工单第三条验收"宿主环境无变化"的可执行形式。
   - `Assert-NoStrays` 从取回的两份报告里读 `stray-processes=`，不为 `none` 就落 `strays.txt`。
2. `scripts/winvm/shot-guest.ps1` —— 从 `.scratch/win11-vm/shot-guest.ps1` 正式化：
   默认页清单**补上 `about`（8 页）**；每页结果附带取回时的 PNG 字节数（0 字节的 PNG 不算通过）；
   manifest 记录 `exe-exists/exe-bytes`、guest 总/空闲内存、CJK 字体族；结尾写 `stray-processes=`。
   DWM 裁剪、`SetProcessDPIAware`、重试逻辑保持原样——那部分是 `run6.log` 已经验证过的资产。
3. `scripts/winvm/tui-guest.ps1` —— 客户机内跑 TUI 冒烟（`--version/--help/--print-dir`，
   用 `ProcessStartInfo` 捕获 stdout/stderr 与退出码，15s 超时后 Kill，因为 `runProgramInGuest`
   不返回子进程输出）；权限探测（`net session` + `WindowsPrincipal`）；`wintun.dll` 两处位置探测；
   **真实注册表系统代理往返**：先快照 HKCU 三键 → 写入 → 复核 → `finally` 里恢复并断言恢复成功；
   DPI；孤儿进程与 2080/2081 残留监听口。

**修复过程中的两次自我破坏（记录以免重犯）**：第一次 `Edit` 用错了锚点，把
`Get-HostFingerprint` 的函数头替换成了 `function Finalize {`，制造出两个同名 `Finalize`
且前者体内是指纹逻辑；第二次 `switch` 里引用了不存在的 `Collect-Processes`。两者都是靠
下面的门抓到的，不是靠读代码。

**验证**：新增 `parse-check` 子命令，用 `Parser::ParseFile` 对三个脚本各取错误数——这正是
`.scratch/win11-vm/run-one.ps1` 每次跑之前手工做的动作（`run6.log` 里的 `parse-errors=0`），
现在它是入口自带的第一道门，不需要 VM 就能跑。

- 干净跑：三个脚本 `OK`，退出 0。
- **变异检验**（新门必须能红）：在临时副本里往清单塞进一个语法错误的
  `__mutation_probe.ps1` → 输出 `PARSE __mutation_probe.ps1 3 error(s)`、退出码 **1**，
  而真实三个脚本仍报 OK。临时副本已删除，仓库内无残留。

**提交**：见 `git log` 中 `feat(winvm): ...`（本文件随该提交一起入库）。

**仍未闭环**：`all` 这条腿需要一次真实 VM 运行才能算 L5 证据（需先建 `clean-base` 快照，
并把 `target/release/sbgui.exe`、`sbtui.exe` 构建出来）。在那之前，本条只能声称
"入口已建成且其自身的门能红"，不能声称 Windows 平台行为已验证。

---

## R2（G6 + G2）— 死依赖与 Windows 打包/发布链

**证据**：`crates/sbgui/Cargo.toml:27` 声明 `tray-icon = "0.21"`，全仓零使用（`git grep tray_icon -- crates` 空）；
`packaging/windows/sbtui.wxs:6` 版本 `0.1.0` 对不上 workspace 的 `0.2.0`；`:21` 名为 `sbtui` 的开始菜单
快捷方式 `Target` 是 `sbgui.exe`；`.github/workflows/release.yml` 从头到尾不构建 `sbgui`，
资产列表里没有 `.msi`，`crates/sbtui/packaging/install.ps1` 也从未上传。

**修复**：删依赖；版本号跟 root `Cargo.toml`（并注释说明为什么不是 crate 版本）；
`sbgui`/`sbtui` 各一条快捷方式（`ly` 不建，它是同一个 TUI 的 PATH 启动器）；
`release.yml` 新增 `build-sbgui` 与 `build-msi`（WiX v4，产物 `sbtui-windows-amd64.msi`），
`package` 的下载与上传列表补上 `sbgui-windows-amd64.exe`、`.msi`、`install.ps1`。
删除依赖使 `Cargo.lock` 少了 609 行（`tray-icon` 拖进 muda/gtk 子树）。

**⚠ 未验证声明**：两个新 job **没有在 GitHub runner 上跑过一次**，只能算"写好了、可能红"。
`build-msi` 现在是 `package` 的硬依赖，所以首次发布可能因为这两个 job 而失败；
补救动作是用 `workflow_dispatch` 在一个测试 tag 上先跑一遍。`packaging/windows/README.md` 记了本地构建命令行。

---

## R3（新发现，最高价值）— 标准 `vmess://` 链接从来没有被导入成功过

**证据（红→绿）**：`crates/client-core/src/subscription.rs` 的 `parse_uri` 假定
`vmess://<userinfo>@<host>:<port>` 形态，而 v2rayN / NekoBox / Shadowrocket 导出的是
`vmess://<base64 json>`——**URI 里根本没有 `@host`**，地址在载荷内部。
按老代码走，`userinfo` 为空、base64 串被当成 host，`decode("")` 之后
`from_slice` 失败，一条 vmess 节点必然解析失败。

新测试 `imports_a_standard_vmess_link_the_way_v2rayn_exports_it` 在修复**前**判红：
`vmess link must import: 订阅里没有可导入的节点（跳过 1 行无法解析）`；修复后判绿。

这条为什么严重：目标文档点名要支持 v2rayN 类订阅，而 vmess 是它的默认导出形态；
且在本次"坏行不再毁掉整表"的改动之前，一行 vmess 会让**整个订阅**导入失败。

**修复**：`vmess` 且不含 `@` 时改走载荷解析；两条 vmess 形态共用同一个
`vmess_outbound()`（地址/端口/uuid 缺失一律显式报错，不再产出空 uuid 的节点），
`ps` 作显示名、`host` 归 WS 的 Host 头、`sni` 归 TLS，transport 只认
`ws|grpc|http`，其余留给内核默认——否则一句陌生的 `net` 会让整份配置过不了 `sing-box check`。

## R4（新发现）— 同一批解析缺陷

| 缺陷 | 证据 | 修复 | 测试 |
| --- | --- | --- | --- |
| 每一行都会毁掉整表 | `parse_uri_list` 用 `parse_uri(line)?`，而 `:433` 对未知 scheme `bail!` | 坏行跳过并计数，全空才报错并带上计数 | `one_unreadable_line_does_not_lose_the_readable_ones`、`a_list_of_only_junk_says_so_instead_of_looking_empty` |
| vless 无条件 TLS | `tls` 闭包写死 `{"enabled": true}`，而 Xray 的 VLESS `security` 默认 `none` | `security` ∈ {tls, reality} 才建 TLS 块；不启用时不再携带 `server_name` | `vless_tls_follows_the_security_parameter`（4 个形态） |
| vmess `aid` 只读字符串 | `.as_str().and_then(parse)`，数字形态静默变 0 | 字符串/数字都接受 | `vmess_alter_id_accepts_a_number_and_a_string` |
| 垃圾行造出空节点 | `vless://not-even-a-uri` 解析"成功"：host=整串、uuid=空 | uuid/host 缺失显式 `bail!` | 同上第一条测试 |

## R5（新发现）— 内核子进程只被信号、未被回收

**证据**：`controller.rs` 两处 `if let Some(mut child) = self.child.take() { let _ = child.child.kill().await; }`。
`kill()` 只投递信号，进程对象要到 reap 之后才释放，而重启路径下一步就是探测 mixed 端口。

**修复**：抽 `reap_child()`（kill → `wait()`，外面套 5 s 超时防止句柄被继承时卡死引擎任务），两处调用它。
**因果未证明**：这一改的动机是时序推理，我没有复现出"端口已被占用"的实例，
所以注释只写不变量、不写事故；`tests` 里也没有针对它的用例。

## R6（新发现）— 清空日志会让两份事件表错位

**证据**：`ClientCommand::ClearLogs` 清 `events` 不清 `event_records`，而 `state.rs:535` 的
`push_record` 明确"两份列表只在这里一起写，因此不会漂移"，`state.rs:503` 的遍历器又是按序配对读它们的。
**修复**：两条一起清。附带把 `controller.rs:584` 的 `eprintln!` 记为 G7 批次处理
（它和 23 处 `.note(`、`operation_error` 漏斗、`ClientCommand::label()` 是同一个 i18n 迁移，不该拆半做）。

## R7（新发现）— TUN 判定问的是"是不是 root"，不是"能不能建隧道"

**证据**：`system_proxy.rs` 的 unix 分支跑 `id -u` 并把它当硬门（`controller.rs:700` 直接 `bail!`），
而该函数自己的注释写着"advisory"。带 `CAP_NET_ADMIN` 的 systemd 服务、或在 `tun` 组里的用户，
明明能创建 TUN 却被拒。另外它在 async 主循环里 spawn 子进程。

**修复**：改为"设备本身"——`/dev/net/tun` 是字符设备且能读写即通过，root 作为兜底。
新测试 `tun_device_gate_rejects_a_writable_regular_file` 钉住两半：普通文件可读写但不是字符设备 → false；
`/dev/null` 是字符设备且可读写 → true（回答的是谓词本身，选定哪个设备由调用方负责）。
**边界**：`CAP_NET_ADMIN` 那条真实路径在 L1/L2 都证不了，需要在真 Linux 主机或带 capability 的容器里验，记入 R12 待办。

## R8（新发现，需产品确认）— `subscription-userinfo` 的 upload/download 是反的

**证据链**：`src/traffic.rs:548-565` 读的是 `sys/class/net/<iface>/statistics/rx_bytes` 与 `tx_bytes`，
即 **VPS 网卡自己**的方向；`TrafficReport{received=rx, transmitted=tx}`；
`serve.rs` 把它们打成 `upload={transmitted}; download={received}`。
对客户端而言：它上传的字节到达 VPS 就是 VPS 的 rx。所以下载为主的用法在 v2rayN / Clash Verge /
Shadowrocket 里会显示成巨额"上传"。

**但这是被记录过的决定**：`docs/implementation-plan.md:301` 有一行
"`download=RX`、`upload=TX`"，说明当年是**有意**按服务端视角写的，不是笔误。
因此这一条不属于"照清单修"，属于**翻一个已文档化的线上契约**：改动波及
`serve.rs` 两处测试常量、`tests/cli/subscription_formats.rs:1278`、`tests/acceptance/verify.sh:104`。

**改之前先查过有没有"反向补偿"**：`crates/client-core/src/subscription.rs:128-133` 的
`parse_userinfo` 把 `upload=` 直接写进 `info.upload`、`download=` 写进 `info.download`，
**没有任何倒置**；连接页的 `connection.upload/download` 走的是 clash_api 另一条数据路径，与此无关。
所以服务端这一改不会被客户端二次翻转。`subscription.rs` 里那两条 `upload=71; download=36`
只是解析器的任意数字，不声明方向语义，因此无需跟着改。

本轮我按消费方语义（客户端视角）改了实现与全部锚点，并在代码注释里写清两个视角；
**若维护者判定要保持服务端视角，回退就是还原这 4 处**，本文件与提交信息都足以定位。
（2026-09-24 维护者已确认：采用客户端视角。）

## R9（新发现）— `regenerate` 不清理被取代的工件；external-proxy 模式自我限流

- `remove_stale_artifacts` 只在 `apply_config_transaction` 里调用（`artifacts.rs:225`），
  而 `regenerate`（`:73`）同样会写工件。于是一份没有任何当前 profile 再生成的
  `subscription-sing-box-1.10.json` 会永久留在一个**合法 URL** 后面，正是该函数注释里写的
  "旧 host 与旧凭据"场景。修复：`regenerate` 末尾同样清理，且**只告警不回滚**
  （新工件已经落盘且正确，删失败不该把健康部署退回去）。
  新测试 `regenerate_prunes_artifacts_the_current_profiles_no_longer_generate` 三条断言：
  被取代的删掉、自己拥有的仍被覆盖、管理员手放的文件不许动。
- `IpBudget` 以 `stream.peer_addr()` 为键，而 external-proxy 模式下 peer 恒为 `127.0.0.1`：
  所有真实用户共用 60 令牌、之后 1 req/s，属于自我造成的宕机。修复：该模式跳过限流并注明
  每客户端限额属于前置代理；**不**去信任 `X-Forwarded-For`——可伪造的转发头会让限流变成
  "这个订阅是否存在"的预言机，而那正是统一 404 要藏的东西。
  这条**没有**新增测试（既有的 5 条令牌桶单测与接线测试都不区分模式），记入 R12。

## R10（新发现）— 部署链四处

| 问题 | 证据 | 修复 |
| --- | --- | --- |
| 重跑安装会 `ETXTBSY` | `install.sh:127` `install -m 0755 … /usr/local/bin/sbctl` 原地截断正在服务且尚未 stop 的二进制；Rust 侧早已 copy+rename（`lifecycle.rs:119-137`） | `.sbctl.new` + `mv -f`，注释解释 inode 语义 |
| 首条文档化安装命令 100% 不可用 | `docs/installation.md:13` 让用户从 `raw.githubusercontent.com/.../scripts/install.sh` 取，而仓库那份是**模板**，`install.sh:6-9` 见到占位公钥就退出 2 | 改成发布工件 `releases/latest/download/install.sh`，并写明为什么不能用仓库那份 |
| 发布验收腿一步都跑不完 | `run.sh` 在 `set -eu` 下硬要求 `SBCTUI_ARTIFACT`（`:9`、`:16`），而 `release.yml` 的 acceptance job 只给两个变量 | 下载 `sbtui-bin-ubuntu-22.04`、`chmod 0755`、导出第三个变量，并把 `needs` 扩到 `build-sbtui` |
| 交付的内核不是"最新稳定版" | `release.yml:151,172` 在 tag 触发（无 input）时静默取 `SING_BOX_VERSION=1.12.0`，而 CI 已验证到 1.14.1 | **未修**：需要决定是"发布时自动解析上游 latest"还是"必须显式传参"，属产品口径 |

## R11 — 本轮撤回的建议（都查证过，别再照做）

1. **"MSI 缺 wintun.dll 组件"**：错。`settings.rs:209` 把 wintun 解析为 `<data-dir>/core/wintun.dll`，
   由内核下载器从 sing-box 的 zip 里解出（`core.rs:503`），缺失时 `controller.rs:703` 指引重下内核。
   打进 Program Files 只会制造第二个过期真相源。
2. **"MSI/清单缺 `requestedExecutionLevel`（TUN 需要）"**：错。给主程序清单加 `requireAdministrator`
   会让**每一次**启动都弹 UAC，包括从不用 TUN 的用户。正确形态是只在 TUN 路径上自提权
   （`ShellExecuteW("runas")`），那是 G11 未完成的半边，不是打包项。
3. **"fetch/download_core 应加 `.no_proxy()`"**：错。客户端从不在自己进程里设置代理
   （`system_proxy.rs:433` 只**打印**建议的 export），而这两处访问的是公网——
   强推 `no_proxy()` 会切断"只能经代理出网"用户的订阅更新与内核下载。`clash_api` 的 `no_proxy()`
   是对的，因为那是回环上的自管内核。我一度照做，核实后已还原，并把理由写进 `fetch` 的文档注释。
4. **"CI 监听 `master` 所以 push 不触发"**：不成立。远端默认分支就是 `origin/master`，本地 `main` 尚未推送；
   `pull_request` 触发与分支名无关。真正的风险是"分支策略与远端不一致"，属仓库操作，不是代码缺陷。
5. **"`normalize_url` 不处理 `/sub/<cred>/` 尾斜杠"**：不成立。`rsplit_once("/sub/")` +
   `tail.split('/').next()` 已经取到 credential，实测路径正常。

## R12 — 审查确认存在、本轮未修（按影响排序）

| 项 | 证据 | 为什么没在这轮做 |
| --- | --- | --- |
| 引擎任务无监护 | `controller.rs:96` 丢弃 `JoinHandle`；panic 后快照永久停在最后一次 publish，`send()` 仍返回 Ok | 需要一个跨层可见性设计（事件码 + 工具栏状态），不是就地补丁 |
| GUI 缺 6 个 TUI 动作 | `ImportProfileFile`/`SetProfileUrl`/档案命名/运行中切 TUN/连接排序/日志暂停/帮助浮层 | G5 批次 |
| GUI 输入校验与静默失败 6 条 | `parse.rs:5-7` 端口下限与 TUI/引擎不一致；空输入静默关面板；单实例失败 release 构建下无人看见 | G5 批次 |
| 关闭路径主线程阻塞 3 s | `controller.rs:135-150` 自旋等待，窗口"未响应"，超时留孤儿内核 | G5 批次 |
| async 循环里同步阻塞 | `controller.rs:217`（`detect_version` 起子进程）、`:700`（`can_use_tun`）、全仓 `spawn_blocking` 零使用 | 与"引擎监护"同批做 |
| Direct 模式不探测 80/443 | `lifecycle.rs:57` 硬编码 socket 口；装了 nginx/Caddy/docker 的 VPS 上 `enable --now` 失败并全量回滚 | 需要改 preflight 并跑 L3 |
| certbot 假设已安装 | `certificate.rs:319` 直接 `run_command_output("certbot")`，裸 `No such file or directory` | 需要发行版包名映射 |
| Rust CLI 无 root 校验 | 非 root 执行 `sbctl install` 会先写完配置再在 `useradd` 上失败 | 小改动，下一批 |
| G3 模板轴、G4 客户端覆写 | `template.rs:162`；`client-core` 无 `overrides/` | 本轮剩余主线 |
| G7 i18n 收口 | `controller.rs` 23 处 `.note(`、`state.rs:619` 按中文子串判级别 | 与事件码同批 |
| 语言选择不持久 | `state.rs:257`、`Settings` 无该字段 | G5/G7 |
| `docs/adr/` 没有一篇 GUI/事件码/i18n ADR | 22 篇中 grep 零命中 | 文档债 |
| 无头截图腿只断言"PNG 存在且非空" | `inside.sh:172-186`；`api_probe` 只 echo 不断言，0 行照样 OK；未传 `DEMO_CORE` 时全拍空态 | 需要在 `inside.sh` 里把 `api rows:` 变成判据 |
| 订阅 `refresh` 字面键 | 见 known-gaps G10，`expire=` 承担该语义 | 产品决策，待确认 |

## R13（G3）— 订阅模板轴落地：`Global`/`Split` 不再是空壳

**证据**：`src/subscription/template.rs:162` 的 `let _ = template;` 让三个模板渲染出逐字节相同的工件；
`GroupSpec` 只带 `role`、`RuleMatcher` 只有 `Private`/`AiDomains`（审查 B4：**模板词汇表达不了**目标文档要的
策略组/分流内容，光把 `template` 用起来是不够的）。

**做法**（写入 subagent 完成，我独立复验）：先扩词汇（`GroupSpec` 带 tag/type/members、新增
`RuleMatcher`、rule-set 条目自带 URL 与内联孪生），再落三份目录：
- `standard` **一字未动**——`src/subscription/snapshots/` 的 13 份金标准在 `git status` 里**完全没出现**，
  `the_generated_artifact_set_matches_the_pinned_goldens` 仍绿；默认值保持 `standard`（翻默认会让每台升级的 VPS 中途重启内核）。
- `global`：除私有目标外全部走代理，广告阻断。
- `split`：CN 与私有直连、广告阻断、AI/流媒体/Telegram 各自成组。
- 新增组标签 `🔰代理分组/🤖AI服务/🎬流媒体/📲Telegram/⚠️故障转移`，**两种格式同名**（历史三个仍各叫各的，
  因为它们被冻结）；`client-core` 现在按 `route.final` 发现选择组，所以新增组不会让客户端"当前节点"失效。
- `geoip/lan` **故意不做成 URL**——配置的镜像解析不到它，宁可用编译期内联列表，也不留一个必然 404 的引用。
- **B5 收口**：`client_rule_profile = minimal` 过去会把整类 rule_set 删干净，CN 目标因此落到代理组；
  现在每条 rule-set 支撑的规则都带编译期内联孪生，`minimal` 在任何模板下都不出现 CDN 地址。
- **B7 收口**：向导的 client-template 选项真正推动该轴（此前问遍其它字段独独不问它），配置摘要打印模板与 rule profile。
  **未做**：`sbctl config init` 没有 `--client-template`——`client_rule_profile` 同样没有 CLI 开关，
  向导才是本仓约定，给一个字段单开一个旗标是不一致的表面。

**新增测试**：`each_template_resolves_to_its_own_catalog`、
`the_richer_templates_add_groups_rule_sets_and_their_own_cn_verdict`、
`minimal_rule_profile_names_no_rule_cdn_under_any_template`、
`every_rule_set_backed_rule_carries_a_minimal_twin`、
`the_cn_twin_carries_both_the_domain_and_address_lists_it_needs`、
`the_fallback_group_is_declared_for_clash_only`、
`every_template_and_minor_renders_a_referentially_sound_profile`、
`the_node_list_artifacts_are_byte_identical_across_templates`、向导专题测试。

**验证（宿主 L1，退出码各自取）**：fmt 0、clippy `-D warnings` 0、
`cargo test -p sbctl --lib` **196 绿**、`cargo test --workspace --features sbctl/test-signing` **404 行 ok / 0 失败**。
**变异检验**：把 `for_template` 改回"`let _ = template;` + 只返回 standard" →
`each_template_resolves_to_its_own_catalog` 与 `the_fallback_group_is_declared_for_clash_only` **判红**，
还原后 9 条全绿。这条特别重要：**金标准门抓不到空壳模板**（空壳恰恰保持字节相同），
只有按"模板之间必须不同"断言的测试能抓。

**尚未验证（不是"已验证"）**：Global/Split 的工件**没有过真核 `sing-box check`**。原因是环境：
`version_profiles --ignored` 报 `subscription artifact is unavailable: No such file or directory`，
查下去是 **WSL `~/bin` 里根本没有那 5 个内核**（只有 `mihomo`）——该测试自 Phase 0.5 起断言
`checked == 5`，缺二进制就长成"产品缺陷"的样子。补救脚本已入库
`scripts/dev/fetch-sing-box-cores.sh`，但此刻 **GitHub 从 WSL 不可达**（curl 20s 连接超时 ×4，
与既知的宿主代理假 IP DNS 污染一致），所以本轮跑不了。
另：真 `mihomo` 接受了 `subscription-clash.yaml`，`subscription-clash-1.18.yaml` 因 `mihomo -t` 要
从 GitHub 下 `geoip.metadb` 而超时——那份工件的字节与 HEAD 完全一致（金标准未动），
所以这条是宿主状态，不是本改动。

**2026-09-24 补记：真核矩阵已经跑通（默认模板）。** 宿主能连 GitHub、WSL 不能，所以内核在宿主下载后
暂存到 `C:\Users\ranly\tmp-sbcores`，由 `fetch-sing-box-cores.sh` 的 `STAGE_DIR` 分支拷进 `~/bin`。
之后 `scripts/dev/wsl-real-cores.sh` 的 version_profiles 腿：**3 passed / 0 failed**，
其中包含"5 个真实内核各自接受自己的工件"与"服务端配置过 1.14"。
仍**未验**的是新模板：那两个测试**只测默认模板**，所以"Global/Split 过真核"这件事今天没有门覆盖，
把它按模板参数化是下一批的第一项。mihomo 腿仍卡在下 `geoip.metadb`（同一网络原因，非工件问题）。


**由此暴露的一个真缺口**：`tests/version_profiles.rs` 与 `tests/clash_mihomo.rs` **只测默认模板**，
所以"新模板过真核"这件事今天没有任何门覆盖。需要把这两个测试按模板参数化——已在 R12 记为待办，
并且必须等 GitHub 可达时才能验证。

## R14 — 本轮环境事实（影响后续每一轮）

1. `wsl -d ... -- bash /mnt/c/...` 直接传路径会被 MSYS 改写成 `C:/Program Files/Git/mnt/c/...`；
   要么 `MSYS_NO_PATHCONV=1`，要么包在 `bash -c '...'` 里（本仓 `run.sh` 已用该旗标）。
2. 同样经 `bash -c` 传进去的 `$var` 会被外层 Git Bash 先展开成空串（我第一次抓内核就下成了
   `sing-box--linux-amd64.tar.gz`）。复杂命令**写成脚本文件**再调用，别拼字符串。
3. 本轮 `version_profiles` 的两个失败**不是代码回归**；判定依据是"金标准未动 ⇒ 送检字节与 HEAD 相同"，
   而不是"我觉得是网络问题"。



## 提交对应关系

- `feat(winvm)`：R1
- `fix(client-core)`：R3–R7；`fix(client-core)` 第二笔：L2 抓到的 unix 编译错误
- `fix(server)`：R8、R9
- `fix(packaging)`：R2
- `fix(release)`：R10
- `feat(subscription)`：R13（G3 模板轴）
- `docs`：本文件与 `docs/known-gaps-after-merge.md` 的状态回写，随各批提交
- `chore(dev)`：`scripts/dev/fetch-sing-box-cores.sh`（R13/R14 暴露的 L2 真核前置）


