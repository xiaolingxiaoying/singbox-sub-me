# 目标 spec 差距补齐与分层验证计划

日期：2026-09-23　　适用分支：`refactor/structure`（`sbctl` 0.1.26）
需求来源：外部目标文档《sing-box sub project 计划》（服务端 / TUI 客户端 / GUI 客户端三部分）
本文取代的旧文件：无。`singbox-sub-plan.md` 是历史方案，见 [implementation-plan.md](implementation-plan.md) 第 6 节末尾的处置说明。

配套阅读：[CONTEXT.md](../CONTEXT.md)、[DESIGN.md](../DESIGN.md)、[PRODUCT.md](../PRODUCT.md)、[implementation-plan.md](implementation-plan.md)、[release-readiness-and-vps-test-plan.md](release-readiness-and-vps-test-plan.md)、[subscription-modes-testing.md](subscription-modes-testing.md)、`docs/adr/`、`docs/research/`。

---

## 1. 结论先行

把目标文档逐条对照代码之后，**服务端的要求大部分已经实现，不需要重写**。真正的缺口集中在两处：

1. **订阅内容不够丰富** —— 目标文档要求订阅携带分流、规则、外部资源、策略组、DNS、嗅探的模板，而今天没有"模板"这个概念，且 Mihomo 订阅完全没有嗅探。
2. **GUI 有已确认的 bug，且几乎没在真 Windows 上跑过** —— 目标文档对 GUI 的要求只有一句"先把功能完成了，同时 UI 不能有 Bug"，而这条现在被明确违背。

### 1.1 已经满足的项（经代码核实，不必再投入）

| 目标文档要求 | 现状 | 证据 |
|---|---|---|
| 支持 Mihomo / sing-box / Shadowrocket / v2rayN 四类订阅 | 8 类格式、按 minor 展开后共 **12 份订阅工件**全覆盖，另有按客户端族的推荐表 | `src/subscription/profile.rs:30-45`、`:48-59`、`:304-365` |
| 统一内部节点模型 | `CanonicalNode` 五变体，所有工件同源派生；Reality 公钥在生成时由私钥重导出，陈旧密钥对无法出厂 | `src/canonical.rs:6-22`、`:35-38`、`:39-77` |
| 使用最新稳定版内核 | 直接取官方 `releases/latest`，且下载的候选内核必须先接受生成的服务端配置 | `src/cli/commands/install.rs:100-127`、`src/update.rs:131-134` |
| `subscription-userinfo` 显示已用/总流量 | `upload/download/total/expire` 四键齐全，且账期故障时宁可不发也不伪造 | `src/subscription/serve.rs:390-426`、`tests/acceptance/verify.sh:135-169` |
| 订阅链接要有安全边界 | 256-bit path credential、两次常量时比较、统一 404、全链路脱敏、可回滚轮换、16KiB/5s/30s/32 连接整形、SNI 不匹配拒绝握手 | `src/config.rs:973-980`、`src/subscription/serve.rs:366-377`、`:481-498`、`docs/adr/0002`、`0013` |
| 五协议 | VLESS Reality / VMess WS / Hysteria2 / TUIC / AnyTLS 端到端 | `src/config.rs:296-324` |
| 简单部署在 Ubuntu/Debian | preflight 只接受 `ID=debian\|ubuntu` + `amd64\|arm64` + 真实 systemd，失败即关闭 | `src/preflight.rs:37-95` |

### 1.2 差距清单（本文编号，后文按此引用）

- **G1** 没有"模板"概念：`src/override_template.rs` 只是两份管理员手写文件的 deep-merge（`:18-19`、`:101`、`:132`），无目录、无可选、无按客户端族分组。
- **G2** 嗅探缺失：`src/subscription/render/clash.rs` 全文 `sniff` 匹配数为 **0**；sing-box 侧只有裸 `{"action":"sniff"}`（`render/singbox.rs:146`），无 `override_destination`。
- **G3** 外部资源窄：sing-box 仅 2 个 `.srs`、Clash 仅 4 个 `.mrs`，只有 CN + private（`render/singbox.rs:161-225`、`render/clash.rs:150-178`）；且在 `ClientRuleProfile::Minimal` 下**整类消失**（`singbox.rs:159`、`clash.rs:138`）。
- **G4** 策略组薄：sing-box 固定 1 selector + 1 urltest + direct，Clash 固定 3 组；无 fallback / load-balancing / 区域分组。
- **G5** 节点原生协议链接**已生成但从不展示**：`sbctl sub` 只打印订阅 URL（`src/cli/commands/serve.rs:52-121`），`sbctl nodes` 只打印 `protocol: TRANSPORT port`（`src/lifecycle.rs:531-544`），index 页根本不碰 `canonical::nodes`（`src/index_page.rs:37-78`）。
- **G6** `subscription-userinfo` 缺 `profile-update-interval`（Clash/Verge/Shadowrocket 会认这个键）。
- **G7** "最新稳定版往前 5 个版本"是**写死的 1.10–1.14 固定带，不会滑动**：`SING_BOX_VERSION_PROFILES` 是常量数组（`src/subscription/profile.rs:136-182`），`latest_version_profile()` 返回 `.last()`（`:185-189`），`sing-box-full.json` 就用它（`src/subscription/artifacts.rs:298`）。上游一发 1.15：服务端装 1.15，`sing-box-full.json` 仍指向 1.14，`sing-box-1.15.json` 直接 404。真核校验存在但被 `#[ignore]` 且环境变量缺失时**静默跳过**（`tests/version_profiles.rs:62-65`）。**→ 部分关闭**：静默跳过与 CI 带检查已在 Phase 0.5 解决，"已装内核比表更新"改为只告警并在 `status` / `status --json` 暴露已在 Phase 3(4) 解决；运行期选目标也已在 §Phase 3(3) 落地（`sing-box-full.json` 问内核再选档，公开入口保留不咨询内核的等价包装）。
- **G8** 无按源 IP 限流（只有并发上限）。**→ 2026-09-23 已关闭**，见 §Phase 7 与 §11 的 PR(G8) 小节。
- **G9** TUI 的"覆写配置文件内容"：客户端侧完全没有查看/编辑能力，唯一的 override 是内部自动的（`crates/client-core/src/core.rs:160-184`、`:549-587`）。
- **G10** TUI 的"显示入站"：整个客户端树没有任何一处读取 `config["inbounds"]`；规则视图还是藏在 Logs 页里的一个开关（`crates/sbtui/src/view/logs.rs:16-20`）。
- **G11** TUN 不是"开关"：内核运行时切换被直接拒绝（`crates/client-core/src/controller.rs:626-630`），全仓无提权路径、无服务模式。
- **G12** 非 Windows 的孤儿保护是假的：`core.rs:316-322` 什么都没做就返回 `true`，导致 `controller.rs:668-672` 的告警**永远不可能触发**。
- **G13** Linux 系统代理只自动化 GNOME，其他 DE 直接打印手动 `export http_proxy=…`（`system_proxy.rs:403-435`）。
- **G14** GUI 已知 bug：connections 页确定性显示 0 行且流量/内存/连接数一起冻结（`.scratch/sbgui-progressive-workspace/issues/01-connections-poll-flake.md`，3/3 复现）；61 条引擎中文字符串 + 23 条传播错误串破坏英文界面（issue 02）；时间列原始截断 ISO 串（issue 05）；两栏宽度 ±11–16.5px 漂移（issue 07）；`tray-icon` 声明后零使用（`crates/sbgui/Cargo.toml:27`）。
- **G15** 真实 Windows 只跑过一次、8 页里 5 页通过（issue 04，`ready-for-human`，此前阻塞在 VM 口令）。CJK 字体回退、DPI、DWM 边框、注册表真实写入、TUN 提权、孤儿回收全部未验证。
- **G16** Windows 打包靠手工且写错：`packaging/windows/sbtui.wxs:6` 版本号 0.1.0 对不上 workspace 的 0.1.26；`:21` 名为 "sbtui" 的开始菜单快捷方式实际启动 `sbgui.exe`；无 `wintun.dll` 组件、无 `requestedExecutionLevel`；没有任何 workflow 构建它。**sbgui 今天从不被 release 构建或发布。**
- **G17** 两条已改代码路径无回归测试（`.scratch/structure-refactor-20260921/report.md` finding #2、#5）；客户端三 crate 共 77 个测试**全是纯单元测试，零快照/零渲染测试**（ratatui `TestBackend` 从未使用）。

---

## 2. 已锁定的产品决策

| 项 | 决策 |
|---|---|
| 本轮重心 | GUI 无 bug + 真机验证优先，服务端模板能力随后 |
| 模板形态 | 建**编译期内置的模板目录**（catalog），新增配置轴 `client_template`。不做磁盘上的管理员模板文件，理由见 §4 |
| TUI 覆写 | 只读查看生效配置 + 规则片段开关；**不做**任意 JSON 编辑 |
| VMware 角色 | Windows 11 guest 专责 sbgui 真机验证。guest 账号 `Test`，口令由维护者在会话中提供，**不写入仓库任何文件** |
| TUN | 引导式重启 + Windows UAC 自提权提示；**不做**完整服务模式（那是独立产品支柱） |
| 订阅边界 | 保持单 credential（轮换即撤销），只加按源 IP 令牌桶限流；不做多租户分发 |
| 最终确认环境 | 复用现有 VPS，仅在最后阶段做一次确认，不当首个调试场 |
| `subscription-userinfo` | 加 `profile-update-interval`，**不加** `refresh` |
| ISP 分组 | **wontfix**（无 ASN/地理数据源）；先补节点命名与区域元数据，再按区域分组 |
| MSI 防火墙规则 | **不加**，维持 ADR-0018 排除自动改防火墙；文档给手动清单 |

---

## 3. 审计纠正（先划掉不该做的工）

这几条是读代码后对既有判断的更正，直接减少工作量：

1. **"订阅无撤销能力"不成立。** `sbctl credential rotate`（`src/cli/commands/system.rs:59-79`）带写失败回滚，index 页明确指引运维执行它（`src/index_page.rs:110`）。只有一个部署 credential 时，**轮换就是撤销**。
2. **策略组被一个未列入的目标文档要求挡住。** `CanonicalNode::tag()` 返回 `&'static str`（`src/canonical.rs:104-112`）——一协议一节点，无显示名、无区域。不先补节点元数据，任何"区域分组"都是空谈。
3. **分组 tag 词汇三处不一致，组数一多就炸。** 服务端 sing-box 用 `🚀节点选择 / ♻️自动选择 / direct`（`src/subscription/render/mod.rs:112-114`），服务端 Clash 用 `🌍选择代理节点 / ♻️自动选择 / 🎯全球直连`（`render/clash.rs:72,87,97`），而客户端 `client-core` 硬编码 `🚀节点选择 / 🎯直连`（`crates/client-core/src/clash_api.rs:14-18`）——**与两个 clash 工件都不匹配**。任何组数变更前必须先统一 tag，并让客户端按 `route.final` + 组类型**发现**选择组，而不是硬编码名字。
4. **mihomo 侧没有版本纪律。** `ci.yml:99-118` 只用一个 v1.19.30 同时验 `clash.yaml` 与 `clash-1.18.yaml`，而 legacy 工件存在的全部理由（`profile.rs:226-230`）从未被它自己的目标内核跑过。
5. **`expire=` 已经是各客户端渲染"刷新/重置日期"的事实字段。** 在这个四键约定下再加 `refresh=` 是冗余；真正缺的是 `profile-update-interval`。需要单独决策的是 `expire` 的语义命名（账期重置 vs 账号到期——本项目目前没有到期概念），这是文档问题不是代码问题。

---

## 4. 阶段计划

### Phase 0 —— 证据底座 + 修掉唯一的确认 GUI bug

放在最前，因为后续每个阶段的安全故事都依赖"默认字节没有动"，而 issue 01 会让当前所有 GUI 截图证据失效。

| # | 工作 | 文件 |
|---|---|---|
| 0.1 | **issue 01：先写会红的测试，再修。** 已核实的机制：`poll()` 在 `.await` **之前**就推进了 `last_connections_at / last_traffic_at / last_proxies_at`（`controller.rs:1012,1016,1024`），而 `controller.rs:283-286` 的内层 `select!` 让 `command_rx.recv()` 与 `self.poll()` 竞争——命令一到达，`poll()` 的 future 被丢弃，**时间戳已前移但数据没取**，该刷新槽位被永久消耗。这与"三个指标一起冻结、3/3 确定性复现"完全吻合。修法：时间戳移到确实取到数据之后；并让 poll 跨迭代持有 future，或交给专门的遥测任务。`watch_core_exit()` 的提前 return（`:1138`）是**另一条**独立的饥饿路径，必须分开验证——不要让实现者只修后者。 | `crates/client-core/src/controller.rs:265-300`、`:1000-1090`、`:1138` |
| 0.2 | **工件金标准**：固定 `DeploymentConfig` 下全部 12 个订阅工件 + `sing-box-server.json` 的 `insta` 快照。这是 ADR-0021 的字节冻结规则**之外**那些文件的唯一安全网。 | 新增 `src/subscription/snapshots/`，测试挂在 `artifacts.rs` |
| 0.3 | **客户端渲染测试（目前完全不存在）**：`crates/sbtui/tests/render.rs` 用 ratatui `TestBackend` + insta 逐 tab 断言；`sbgui` 在 9 个纯函数测试之外补纯列布局函数。 | `crates/sbtui/*`、`crates/sbgui/src/pages/*` |
| 0.4 | **CI 加未用依赖检查**（`cargo machete` / `udeps`）。`tray-icon` 之所以能声明后零使用，是因为它是 Windows-only 依赖，Linux CI 看不见。 | `.github/workflows/ci.yml` |
| 0.5 | **版本带检查 + sniff 字段探针**。CI 步骤拉 `SagerNet/sing-box/releases/latest`（与 `src/update.rs:133-134` 同一端点），其 `major.minor` ≠ `latest_version_profile().version` 即判红。同时在真实多内核矩阵上回答：每个 minor 是否接受 `action:sniff` 的 `sniff` 列表？是否接受 `override_destination`？这是 Phase 2 的输入，不能靠猜。 | `ci.yml:73-97`、`tests/version_profiles.rs` |
| 0.6 | 把"复制进 WSL ext4 再构建"固化为默认开发腿，并确认 `tests/acceptance/run.sh` 从 WSL 内跑通。 | 文档 |

**出口**：0.1 在现有 `scripts/sbgui-shot` 的 connections 场景 3/3 绿；金标准入库；带检查绿；sniff 字段矩阵有逐 minor 的引用。
**并行性**：0.2–0.6 彼此独立可并行；0.1 独立。

### Phase 1 —— GUI bug 集 + Windows 真机现实（本轮重心）

**入口条件**：Phase 0.1 已合并。

- **issue 02 英文化 + 事件码**：引入 `EventCode`，把 61 条引擎中文串 + 23 条传播错误串改成机器可读码，由 UI 层查 `tr!` 表。**同一 PR 一并决定** `ClientEvent` / `recv()`（`crates/client-core/src/event.rs`、`controller.rs:144`）这条死缝——两个 UI 都在轮询 snapshot 并**用中文字符串匹配来决定颜色**（`crates/sbtui/src/style.rs:58-68`、`crates/sbgui/src/chrome.rs:290`）。建议删 `recv()`，事件码进 snapshot。
- **issue 05**：建连时间列走 `format.rs` 的 `age_label`；rule 列宽处理。
- **issue 07**：两栏宽度漂移。
- **`tray-icon`**：删依赖，或真正做实（close-to-tray 是独立产品项，本轮不承诺）。
- **G15 真 Windows 清单**（VMware Win11 guest，逐项留证据）：CJK 字体回退、按监视器 DPI、DWM 标题栏与拖拽/关闭、**真实的 `HKCU\...\Internet Settings` 代理写入及其恢复**、`wintun.dll` 检测与 TUN 提权、Job Object 孤儿内核回收（`core.rs:295-313`）。复用 `.scratch/win11-vm/capture-vm.ps1` + `shot-guest.ps1` 先例。
- **G16 打包**：版本号对齐；拆清 `sbtui`/`sbgui` 两个组件各自的快捷方式与目标；补 `wintun.dll` 组件与 `requestedExecutionLevel` 清单；`release.yml` 增加 sbgui + MSI 构建。

**出口**：8/8 页在真机通过并有存档产物；CI 产出可安装、可升级的 MSI。

### Phase 2 —— 让订阅内容变丰富（G1 / G2 / G3 / G5 / G6）

**需要新写 ADR-0022**：「订阅模板是编译期内置目录」，写明分层顺序、`Standard` 对模板前输出字节冻结、管理员模板文件不在范围内。

渲染管线：

```text
canonical nodes → template（内置）→ client_rule_profile（是否用 CDN）→ Overrides（管理员两份文件）→ artifact
```

**为什么不选另外两种形态**：

- *磁盘上的管理员模板目录*：一份无法针对 5 个内核 minor 编译的文件，就无法保证过 `sing-box check`；而 ADR-0021 规定 override 畸形会中止重生成——把默认内容放进这类文件，等于让管理员的一次粘贴拖垮**默认订阅**。且真实诉求是"内容薄"，不是"不可配"。
- *复用 `ClientRuleProfile` 轴*：`minimal` 今天的语义是**整类删掉**外部资源（`render/singbox.rs:159`、`render/clash.rs:138`）。把丰富度折进它，`minimal` 就同时表示"不连 CDN"和"没有分流"——正是 G3 的回归。保持 `minimal` = 永不联系规则 CDN，并给模板配**内联规则孪生**，让分流在无 CDN 下仍然存活。

改动清单：

- 新建 `src/subscription/template.rs`：`enum ClientTemplate { Standard, Global, Split }` + `struct TemplateSpec { groups, rule_sets, inline_rules, dns, sniff, final_group }`。**`Standard` 必须逐字节复现今天的输出**（由 0.2 的金标准证明）。
- `src/config.rs`：`client_template` 字段 `#[serde(default)]` → `Standard`；`--client-template` 参数（`src/cli/args.rs`）+ 向导项（`src/wizard.rs`）。
- `render/singbox.rs` + `render/clash.rs`：消费 `TemplateSpec` 而非内联字面量。`src/override_template.rs` 语义不变，只补注释说明分层顺序。
- **G2 嗅探**：~~sing-box 把 `:146` 的裸 `{"action":"sniff"}` 换成带 `sniff` 列表与 `override_destination`，两个可选键由新的 profile 布尔门控~~ —— **此路已被 Phase 0.5 的真核探针否决，见 §4.2**。~~Clash 侧补顶层 `sniffers: [domain, http, tls, quic]` + `dns-hijack: any:53`；legacy 分支用 `sniff: true`~~ —— **这条也在 2026-09-23 被真核探针否决，三处皆错，见 §4.3**。实际落地：两份 Clash 工件统一追加
  `sniffer: {enable: true, sniffing: [http, tls, quic]}`；不写 `dns-hijack`（它在 `tun:` 下、默认已是 `0.0.0.0:53`，从订阅里写 `tun:` 块会覆盖客户端自己的 TUN 设置）；不写 `override-destination`（它会让嗅探出的域名替换原始目的地址，属于不该静默代客户做的决定）。sing-box 侧的"嗅探模板"可表达空间只有"有/无"两态，现有写法已经是正确的。
- **G3 外部资源**：规则集变成模板上的数据（`geosite/{cn,private,ads,proxy,openai,netflix,telegram}`、`geoip/{cn,private,lan}`），每项配一个由编译期内联列表渲染的 `minimal` 孪生。`client_rule_set_base_url` 保持唯一 CDN 旋钮。
- **G5 节点原生链接展示**：三件事——(a) index 页在客户端矩阵与链接列表之间插入「节点与原生分享链接」区块（该页已在 256-bit credential 之后，暴露面等同 `uri` 工件，**不新增边界**）；(b) 从 `render/uri.rs:120` 抽出 `node_uri(config, node)`，用现有确定性测试（`artifacts.rs:499-534`）证明重构字节中性；(c) `sbctl status nodes --uri` 只输出到运维者自己的 stdout，按 ADR-0013 永不进 journal。**不要**加进 `sbctl sub` 的默认输出——那条命令常被管道进日志与截图。
- **G6**：header 追加 `profile-update-interval=<小时>`；键序与四个既有键由 header-shape 测试锁死；账期故障时"宁可不发也不伪造 header"的降级行为（`serve.rs:410-416`、`verify.sh:131-134`）必须保持。

**PR 切分**：(a) tag 统一 + `node_uri` 抽取（纯重构，金标准不得移动）→ (b) `Standard == 今天` 的模板轴 → (c) 嗅探 + 外部资源 → (d) G5 展示 → (e) G6 header。(a) 必须最先、单独合并。

**出口**：启用 `--client-template split` 会让组数/规则集/sniffers 增长，而 `subscription-sing-box.json`、`uri`、`uri.txt`、`shadowrocket.txt` **逐字节不变**；5 内核 + 双 mihomo 真核矩阵通过；`Standard` 金标准未动。

### Phase 3 —— 让 5 版本窗口真正滑动（G7）

**决定：编译期表 + CI/发布期带检查 + 生成期真实核选目标。不做运行时拉取的 capability manifest。**

1. **不变式测试**（`src/subscription/profile.rs`）：注册表必须是 N 个连续 minor 且止于所跟踪的最新稳定；`supported` 区间必须首尾相接；`notes` 非空。
2. **CI/发布带检查**（Phase 0.5 已建）：上游发布 1.15 = 一次红构建，补救动作是 5 行注册表条目 + notes + 抬 `ci.yml:83` 的 pinned 内核列表。顺带给 mihomo job 补 `clash-1.18.x`（§3 纠正 4）。
3. **运行时选目标**：把 `artifacts.rs:298` 的 `latest_version_profile()` 换成 `resolve_full_profile(config, nodes, kernel_bin)`——先试 `.last()`，用现成的 `check_sing_box_config`（`:375-395`）校验**客户端**配置，不接受则沿表向下走；无内核二进制时回退 `.last()`（fixture 与 `--root` 测试）。今天只有**服务端**工件被真核校验（`:80-83`），这一步才让 `profile.rs:217` 那句"与服务端运行的最新稳定版一致"成为事实。它必须是 `(config, 已装内核 minor)` 的**纯函数**，否则安装事务的 `artifacts_changed` 比较会抖动。
4. **仅告警、绝不断服**：安装/更新后若已装 minor 高于注册表顶，打印中文告警并在 `sbctl status` / `status --json` 暴露；工件继续提供最新的已知良好 profile。

字段级工作（带检查会立刻要求）：`SingBoxVersionProfile` 加 `tun_stack: Option<&'static str>`（去掉 `render/singbox.rs:136` 无条件的 `"stack":"mixed"`——`research/sing-box-client-version-differences.md:244` 记 1.15 弃用）、`rule_set_download_detour: bool`、`sniff_override_destination: bool`。

**第三份 `stack:"mixed"` 必须同步**：`crates/client-core/src/core.rs:571`。加一条测试断言客户端注入的 TUN inbound 匹配它所启动自的那个 profile——今天两者毫无关联，客户端可以悄悄把服务端为某 minor 删掉的字段塞回来。

**串行**：它改的就是 Phase 2 刚改过的生成入口。
**出口**：临时抬高注册表顶模拟上游滑动 → CI 判红；1.14 主机上的 `sing-box-full.json` 仍指向 1.14。

### Phase 4 —— 节点身份与策略组（G4）

**入口**：Phase 2、3 已合并。

`src/config.rs:33+` 五个协议节点结构加 `name` / `region`；`src/canonical.rs:39-77`、`:104-112` 把 `&'static str` tag 换成 owned 显示名（**tag 保持稳定**）；`wizard.rs` / `cli/args.rs` 加参数；`render/{singbox,clash}.rs` 按模板出 `url-test` / `fallback` / `load-balancing` / 按区域分组。ISP 分组 wontfix。

**风险**：改 `tag()` 会改所有非冻结工件的 `outbounds[].tag`，以及 `client-core` 读回的组成员（`clash_api.rs:14`）。→ tag 保持稳定、name 只做加法，否则一次更新会打散所有客户端的"当前节点"显示。

### Phase 5 —— 客户端入站可见 + 覆写只读与规则片段（G9、G10）

- **G10 入站**：`ClientSnapshot.inbounds: Vec<InboundInfo>`，由新的 `state::parse_inbounds` 产生（紧邻现有 `parse_route_rules`，`controller.rs:677`）；新建 `Tab::Rules`（`crates/sbtui/src/app.rs:12-27`）+ `view/rules.rs`，同屏展示入站（type / listen / port，来自刚启动那份配置）与出站（rules、rule_set 来源、`final`、clash mode）；移除藏在 Logs 页内的 `show_rules` 开关（`view/logs.rs:16-20`）。
- **G9 覆写（已定档位）**：`ClientSnapshot.active_config: Option<String>`，从 `cache/active-config.json`（`core.rs:68` 写入）读取并脱敏 `secret` 与凭据，新 TUI 页展示；**规则片段开关**——每 profile 一份放在客户端数据目录下的文件，在 `core::start_managed` 之前用与服务端相同的语义 deep-merge 进缓存配置（`controller.rs:663`）。`deep_merge` 目前在服务端 crate 的 `src/override_template.rs`：**抽成一个小共享 crate**（一处语义，符合 ADR-0021"逐字记录"的要求），不要在 `client-core` 复制一份实现。
- 把这条边界写进 `PRODUCT.md`（今天 `:30-35` 只说"保留现有能力"，**全仓没有任何一处记录过覆写边界**），并为共享 merge 写 ADR-0023。

与 Phase 6 **可并行**（文件不重叠）。

### Phase 6 —— 本地运行时正确性（G11 部分、G12、G13）

- **G11 TUN**：把静默拒绝变成引导。`ClientCommand::SetTrafficMode { mode, restart: bool }`；内核运行时要求显式 `restart:true`，然后 stop → `adapt_inbounds`（`core.rs:549-589`）→ start，沿用 ADR-0003 的回滚姿态。Windows 上 `can_use_tun` 为假时用 `ShellExecuteW("runas")` 提示自提权重启。
- **G12 孤儿保护现在是假的**：`core.rs:316-322` 什么都没做就返回 `true`，导致 `controller.rs:668-672` 那句"警告：无法为内核建立系统级回收保护"**永远不可能触发**。在 spawn 路径用 `Command::pre_exec` 实现 `PR_SET_PDEATHSIG`，协作退出时配 `setsid` / `killpg`；macOS 上让 `attach_orphan_guard` **诚实返回 false**，使既有告警真的响。Linux 侧可在 Docker 内证明。
- **G13 非 GNOME 系统代理**：补 KDE（`kwriteconfig6`），保留 env 回退文案。**P2，排在 G15 之后**——它的可验证性最差。

**出口**：容器内 `pkill -9 sbtui` 后 `ss -ltnp` 无孤儿 sing-box 占 mixed 口；TUN 切换要么成，要么明确告诉你该做什么。

### Phase 7 —— 服务边界限流（G8）

`src/subscription/serve.rs` accept 路径（紧邻三处 semaphore：`:140`、`:176`、`:214`）加按源 IP 令牌桶，单测用 fixture 时钟。**必须保持统一 404**——限流响应不得让探测者分辨订阅是否存在。多 credential / 按人分发不在范围内（ADR-0018 的单管理员边界）。完全并行。

### Phase 8 —— 两条无测试的已修路径（G17）

`.scratch/structure-refactor-20260921/report.md:63-64` 的 finding #2（固定口启动检测）与 #5（一次短暂成功把崩溃重启计数清零，`controller.rs:1155-1175`，可参照 `:1360-1375` 的写法）：前者预绑 mixed 口断言启动被拒，后者断言 `restart_attempts` 活过一次 sub-`STABLE_RUN` 的成功。报告自己说得很清楚：**要结案就得补能红的用例，不是补注释。**

---

### 4.2 真核字段探针（Phase 0.5 产出，2026-09-23）

用五个真实内核逐个字段变体跑 `sing-box check`，结果**推翻了原计划给 sing-box 加嗅探细节的做法**：

| 位置 | 字段形态 | 1.10 | 1.11 | 1.12 | 1.13 | 1.14 |
|---|---|---|---|---|---|---|
| `route.rules[]` | `{"action":"sniff"}` | ✗ 无 action | ✓ | ✓ | ✓ | ✓ |
| `route.rules[]` | `action:sniff` + `sniff:[…]` | ✗ | ✗ | ✗ | ✗ | ✗ |
| `route.rules[]` | `action:sniff` + `override_destination` | ✗ | ✗ | ✗ | ✗ | ✗ |
| `inbounds[tun]` | `"sniff": true` | ✓ | ✓ | ✓ | ✗ |  |
| `inbounds[tun]` | `"sniff_override_destination": true` | ✓ | ✓ | ✓ | ✗ |  |
| `inbounds[tun]` | `"sniff": [ … ]` | ✗ |  | ✗ |  | ✗ |

三条结论：

1. 协议列表与"覆写目标"在 1.10–1.14 **任何版本**都不是路由规则动作的字段；入站的 `sniff` 只接受
   bool，数组一律被拒。照原计划改会让每一份客户端工件都过不了内核校验。
2. 1.13 起入站的 `sniff` / `sniff_override_destination` 被移除，嗅探只能由裸
   `{"action":"sniff"}` 表达。因此 `src/subscription/render/singbox.rs:146` 对 1.11+ 已经是
   **唯一正确**的写法，`:138-142` 给 1.10 用 `tun.sniff` 也正确（金标准 + 真核校验已证）。
3. 目标文档的"嗅探模板"在 sing-box 侧只有"有/无"两态；**G2 的实际工作量全在 Mihomo 侧**
   （`sniffers` / `sniff-vars` / `dns-hijack`）。

复现方法（容器内，无需 Rust）：对每个 minor 生成一份最小配置
`{"inbounds":[{"type":"tun","tag":"tun","address":["172.19.0.1/30"],<变体>}],"route":{"final":"direct"},"outbounds":[{"type":"direct","tag":"direct"}]}`
（1.10 用 `inet4_address`），跑 `sing-box check -c` 打印 ACCEPT/REJECT。
注意 1.11+ 用 `inet4_address` 会先撞上 "legacy tun address fields" 错误而掩盖 sniff 的结论——
探针配置本身也要跟着版本走。

同一批内核还确认了：`generated_profiles_pass_a_real_sing_box_check` 在把断言从 `checked >= 1`
收紧到 `checked == 5` 之后仍然全绿（1.10.7/1.11.15/1.12.25/1.13.21/1.14.1 各自接受自己的工件），
带检查在 `SBCTL_UPSTREAM_LATEST=v1.14.1` 下通过。

### 4.3 mihomo 嗅探键探针（Phase 2 PR(c) 产出，2026-09-23）

计划原文写的是"顶层 `sniffers: [domain, http, tls, quic]` + `dns-hijack: any:53`，legacy 用
`sniff: true`"。用 CI pin 的同一个核（mihomo **v1.19.30**，`~/bin/mihomo`）探针后，**三处全错**：

| 计划里的写法 | 真核结果 |
| --- | --- |
| 顶层 `sniffers:` | 没有这个键；正确路径是 **`sniffer.sniffing:`**（`RawConfig` 里 sniffer 块的 tag 是 `sniffer`，列表字段的 tag 是 `sniffing`） |
| 值含 `domain` / `dns` | `not find the sniffer[domain]`、`not find the sniffer[dns]`，被核**拒绝**；实测只有 `http`/`tls`/`quic` 通过 |
| `sniff: true` | 顶层 `sniff` 不是布尔；`sniffer.sniff` 是 `map[string]RawSniffingConfig`（每个嗅探器的细分配置） |
| `dns-hijack: any:53` | 它在 **`tun:`** 下，且默认值已经是 `0.0.0.0:53`；从订阅里输出 `tun:` 块会覆盖客户端自己的 TUN 设置 |

探针方法与 §4.2 同源，并补了必要的一环：**`mihomo -t` 会静默忽略未知键**（控制项
`bogus-key-xyz: [nope]` 被接受），所以"配置通过"不证明键存在。判据是**用错的值去打**：
`sniffer.sniffing: [bogus]` 报 `not find the sniffer[bogus]`，才算这个键真的被解析。脚本
`.scratch/mihomo-sniff-probe.sh`，输出 `.scratch/mihomo-sniff-probe.txt`。

同时确认默认 `Enable: false` 且嗅探器列表为空——**G2 是真差距**，只是修复方式与计划写的不一样。
落地后 `mihomo accepted subscription-clash.yaml` 与 `subscription-clash-1.18.yaml` 双双通过
（真核门：`MIHOMO_BIN=~/bin/mihomo cargo test --test clash_mihomo -- --ignored`）。

---

## 5. 差距 → 证明方式 → 验证腿

腿的编号：U 单元测试 / C `tests/cli`（需 `--features sbctl/test-signing`）/ D Docker 验收 / X Xvfb 截图 / W VMware Win11 / V 真实 VPS。

| 差距 | 精确证明 | 腿 |
|---|---|---|
| G1 | 金标准：`Standard` 字节不变；`--client-template split` 增加组与规则集；`sbctl config override show` 列出生效模板 | U, C, D |
| G2 | Clash 侧：两份工件都带 `sniffer.enable: true` + `sniffing: [http, tls, quic]`、各只出现一次，且**不含**被真核拒绝或不该由订阅写出的键（U：`both_clash_artifacts_enable_the_sniffers_the_pinned_core_accepts`）；pin 住的 v1.19.30 `mihomo -t` 接受两份工件（C：`tests/clash_mihomo.rs`）。sing-box 侧可表达空间只有"有/无"两态，现有写法已经正确（§4.2），所以不再有逐 minor 门控键要断言 | U, C |
| G3 | 按 模板 × `client_rule_profile` 断言 `route.rule_set[]` / `rule-providers:`；**`minimal` 必须保留内联规则**（针对 `singbox.rs:159`、`clash.rs:138` 那个回归） | U, C |
| G3 可达性 | 规则 CDN 在国内、以及手机走蜂窝网的实际拉取 | **仅 V** |
| G4 | 按模板 + 具名节点的金标准；真核逐 minor `check`；组名能 round-trip 过 `client-core` 的代理发现 | U, C, D |
| G5 | `tests/cli`：index 响应含每条节点 URI；`sbctl status nodes --uri` 输出 N 行且**不含 credential**；金标准证明 `uri` 工件字节中性 | U, C, D |
| G6 | header-shape 测试（键序 + 四个既有键完整 + 新键存在）+ 账期故障降级测试保持"不伪造 header" | U, C, D |
| G7 | 连续性单测；CI 对 `releases/latest` 的带检查；`resolve_full_profile` 用"拒绝 `.last()` 的桩内核"单测；模拟滑动时发布门判红 | U, C, D + CI |
| G8 | 令牌桶单测用 fixture 时钟（突发、每秒一个的回补、长时间空闲不回攒超过突发、地址之间互不影响、无欠费的地址不留在表里）；**再加一条过真实 listener 的接线测试**，断言限流确实在请求路径上，且被限流时"真凭据"与"错凭据"的响应**逐字节相同**、不含 credential。实现选择与 §Phase 7 字面不同：超限回 **429 + Retry-After** 而不是 404，理由见 §11 的 PR(G8) 小节 | U；D 腿的洪水断言**尚未加**（见同一小节的坑） |
| G9 | 合并语义单测（共享 `deep_merge`）、TUI 配置页金标准、带 override 启动的冒烟测试（假核） | U, X, W |
| G10 | `TestBackend` 对 Rules tab 的金标准（入站 + 规则）；`parse_inbounds` 单测 | U, X |
| G11 | `SetTrafficMode{restart:true}` 先停后起的单测；**TUN 本身只能在 W 证明** | U, D, W |
| G12 | 容器内 `kill -9` 客户端后 `ss -ltnp` 无孤儿 sing-box。需要 privileged systemd，因此属 `verify.sh`（或新建 `verify-orphan.sh`），**不能只放单测** | D |
| G13 | 真实桌面会话 → W（Windows 覆盖注册表）+ 一台 Linux 桌面。**不要假装单测能证明它** | W, 人工 |
| G14-01 | 新增 `client-core` 测试：假 clash_api 供 3 条连接、每 100ms 注入一条命令、3 秒后断言 `active_connections == 3`（必须先红）；再由 X 出 3/3 帧 | U, X |
| G14-02 | `EventCode` 单测；英文 locale 金标准渲染断言 sbgui 各页**无 CJK 字形** | U, X |
| G14-05/07 | 纯布局单测 + X 金标准 | U, X |
| G15 | issue 04 清单在 VMware guest 上逐项存档 | **仅 W** |
| G16 | CI 构建 MSI；W 安装/升级并断言版本串、快捷方式名与目标、卸载保留 `%APPDATA%` | CI, **W** |
| G17 | Phase 8 两条单测 + Phase 0.3 全部金标准 | U, X |
| ACME / Direct HTTPS / 公网 80-443 | Docker 已能模拟 socket activation（`verify.sh:40-61`），但真实证书 | **仅 V** |
| 手机导入订阅 | Shadowrocket / index 二维码 | **仅 V** |

---

## 6. 分层验证阶梯

路由规则：**一个变更算完成 = 能证伪它的最便宜那条腿已经跑过。**
渲染字节 → L1+L3；轮询与生命周期 → L1+L3；GUI 像素 → L1+L4；GUI **平台**行为 → L5；服务边界 → L3；真实世界导入 → L6。

### L1 —— 宿主原生 `cargo test`（只读/编译，不跑目标应用）

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --features sbctl/test-signing
cargo test -p sbctl --no-default-features --test release_trust   # 证明生产构建拒绝开发 key
python3 -m unittest discover -s scripts -p 'test_*.py'
```

`Cargo.toml:15-18` 把 `cli` 测试声明为 `required-features = ["test-signing"]`，**裸 `cargo test` 会静默跳过整套 CLI 集成测试**。
`scripts/dev-signing-key.hex` 是仓库公开提交的 Ed25519 开发私钥种子，只被 `cfg!(test)` / `test-signing` 构建信任（`src/release.rs:44`），生产构建由 `prepare-installer.py:11` 明确拒绝其公钥。

**不能证明**：systemd、TUN、wintun、DPI、DWM、注册表、真实内核字段接受度（`verify.sh:27` 的假核就是 `#!/bin/sh` + `exit 0`）、CJK 渲染。

### L2 —— WSL Ubuntu，树复制到 ext4（日常开发腿，**永不做发布证据**）

按 **ADR-0008**，WSL2 只是开发宿主，产出只能写成"尚未失败"，不能写成"已验证"。复制到 `~/src/` 而非 `/mnt/c`：避开 CRLF 视图差异，且 `Cargo.toml` 隐含的 `aws-lc-sys` / `openssl` 依赖在 `/mnt/c` 上极慢（`release-readiness-and-vps-test-plan.md:29-30` 已记录）。

```bash
# 在 Windows Git Bash 执行：宿主目录只读，写入落在 WSL 的 ext4
wsl -d Ubuntu-22.04 -- bash -lc 'mkdir -p ~/src'
wsl -d Ubuntu-22.04 -- bash -lc 'rsync -a --delete \
  --exclude target --exclude target-* --exclude .git --exclude node_modules \
  /mnt/c/Users/ranly/Documents/singbox-sub-me/ ~/src/singbox-sub-me/'
wsl -d Ubuntu-22.04 -- bash -lc 'cd ~/src/singbox-sub-me && \
  cargo clippy --workspace --all-targets --all-features -- -D warnings && \
  cargo test --workspace --features sbctl/test-signing'
```

真实核矩阵也在这条腿本地跑（1.10.7 / 1.11.15 / 1.12.25 / 1.13.21 / 1.14.1 缓存到 `~/bin`，导出 `SING_BOX_BIN_1_10..1_14`，跑 `cargo test --test version_profiles -- --ignored`），并把它改成**环境变量缺失即失败**，而不是像今天 `tests/version_profiles.rs:62-65` 那样静默 `continue` 只断言 `checked >= 1`。真核 mihomo 门同理：把 CI pin 的 v1.19.30 放到 `~/bin/mihomo`，跑 `MIHOMO_BIN=~/bin/mihomo cargo test --test clash_mihomo -- --ignored`。

**前置（否则整条腿以 build script panic 失败，症状像代码错误）**：`sbgui`(GPUI) 在 Linux 需要系统库，
`sudo apt-get install -y pkg-config libfontconfig1-dev`（本机 WSL 已装，fontconfig 2.13.1）。
装不了包的环境用 `cargo test -p sbctl -p client-core -p sbtui --features sbctl/test-signing`，
覆盖服务端与两个客户端的全部门，只跳过 sbgui。

**不能证明**：PID 1 systemd、命名空间、cgroup 委托、DWM、注册表、DPI、wintun。
**已知陷阱**：WSL 的 DNS 继承 Windows 宿主。仓库已记录容器因宿主 Clash Verge 继承 `198.18.0.0/15` 假 IP 段而导致 apt/cargo 失败（`structure-refactor-20260921/report.md` §三，"宿主状态，不是仓库状态"）——**启动任何一条腿之前先确认宿主代理客户端的状态**。

### L3 —— Docker 验收（**ADR-0014 要求的发布门之一**）

现成套件：三镜像矩阵 debian:12-slim / ubuntu:22.04 / ubuntu:24.04（`tests/acceptance/run.sh:15`），`privileged` + `cgroup:host` + systemd 作 PID 1；`run.sh:43-53` 每镜像按 bootstrap → fixture verify → real verify 顺序执行；`run.sh:32-41` 先轮询 `systemctl is-system-running`（接受 `degraded`）。

```bash
# 从 Git Bash 执行（Docker Desktop 的 CLI 挂在 Windows 侧，WSL 的 PATH 上没有 docker）
cd ~/Documents/singbox-sub-me
cargo build --release                                    # 生产工件
cargo build --release -p sbctl --features test-signing --target-dir target-fixtures
SBCTL_ARTIFACT=./target/release/sbctl \
SBCTL_TEST_ARTIFACT=./target-fixtures/release/sbctl \
sh tests/acceptance/run.sh
```

单容器调试可走 `docker compose -f docker-compose.acceptance.yml`，用 `BASE_IMAGE=ubuntu:22.04` / `ubuntu:24.04` 切换发行版（`README.md:378-387`）。

**两个工件必须是不同二进制，这是刻意的**：生产构建的信任锚拒绝开发 key，所以缺 `SBCTL_TEST_ARTIFACT` 时 bootstrap 与 fixture verify 必然失败（`run.sh:7-13`）。
这一腿新接管：**G12 孤儿回收**（`kill -9` 后 `ss -ltnp`）、`ip a` / `ip rule` 的 Linux TUN 断言、G8 限流下仍返回统一 404。
**不能证明**：真实 ACME 与公网 80/443、真实 GitHub 下载信任（用假 manifest）、GPUI、DWM、DPI、注册表，以及任何 Windows 行为。

### L4 —— Xvfb GUI 截图（`scripts/sbgui-shot/`，Linux 像素腿，**今天未进 CI**）

`rust:1-bookworm` + Xvfb + ImageMagick + `fonts-noto-cjk`（其 Dockerfile 注释写明"Noto CJK matters: the UI is Chinese"），逐页 `xvfb-run` + `import -window root`。
`inside.sh:45-95` 为 connections 页起了 `fixture/slow_origin.py`、经 `curl -x 127.0.0.1:2080` 造真流量、再**直连 clash_api**（从 `active-config.json` 读回每次随机的端点与 secret）、先等首个非零行再等 5s（两个轮询周期），并打印 `curl:` / `core log:` / `api rows:` —— 让空表成为诊断而不是猜测。**这套探针纪律要留在树里**，否则这条腿证明不了时序诚实性。它在每次运行之间 `pkill` 幸存的 sing-box，因为幸存者占着 mixed 口会让下一页自动启动成"端口已被占用"。
新接管：页面金标准、issue 02 的"EN locale 下不得出现 CJK 字形"、issue 07 布局、issue 01 的 3/3 帧回归。
**两个洞**：`shot.sh:18` 默认 7 页且 `PAGES` 里**没有 `About`**（正是 issue 04 的抱怨）；且 `.github/` 里**没有任何 job 引用它**。→ Phase 0 把它接进 CI 并补第 8 页。
**不能证明**：CJK 字体**回退**（容器字体链是人为完整的，那里通过的截图说明不了什么）、DPI 缩放、DWM 标题栏（`crates/sbgui/src/main.rs:120-140` 的 `titlebar: Some(...)` + `apply_windows_window_chrome`）、托盘、注册表、wintun。

### L5 —— VMware Windows 11 guest

**唯一**能证明以下各项的腿：CJK 字体回退、按监视器 DPI、DWM 边框与拖拽/关闭、真实注册表代理写入及其恢复、`wintun.dll` 与 TUN 提权、Windows 侧孤儿回收、MSI 安装/升级/卸载（卸载须保留 `%APPDATA%`）。拥有 G14 / G15 / G16 以及 G11 / G13 的 Windows 半边。
**宿主隔离**：一切构建与运行都在 guest 内；**先打快照**；不把二进制、PATH、注册表项或常驻服务写回宿主。
**不能证明**：任何服务端行为（systemd unit、ACME）；由于 NAT + 虚拟网卡遮蔽了云安全组语义，它也不能替代 VPS。

### L6 —— 真实 VPS（最后确认，不作首个调试场）

唯一能证明：ACME / Direct HTTPS 在公网 80/443 端到端（ADR-0009 / 0011）、真实网络下从 GitHub 装最新稳定内核、规则 CDN 对目标人群的可达性（G3，**含国内可达性——VPS 本身可能并不代表国内**）、手机导入 `shadowrocket.txt` / `sing-box-full.json` / index 二维码。
遵循 `release-readiness-and-vps-test-plan.md:20-24` 的原则：VPS 是确认环境，不是调试环境——**先在 L1–L3 跑干净再上机**。
复用 `subscription-modes-testing.md` 的三模式 runbook（Direct 回归 `:16-32`、external-proxy `:33-69`、ip-fallback 自签五协议 `:71-95`，每模式 6 项通用清单 `:99-106`），不要另写一套。
> 现有 VPS 已在承载生产（`64.81.29.67`，Ubuntu 22.04，sbctl 占公网 80/443，见 `.scratch/vps-connectivity-hardening/spec.md`）。**任何变更前先 `sbctl status` 并备份 `/etc/sbctl`，确认回滚路径后才动手。**

### 关于"不影响 Windows 宿主"

- 只有编辑、编译、`cargo test` 类的逻辑测试在宿主跑；任何功能性、可视化、systemd、TUN、注册表相关的运行一律下沉到 L2/L3/L4/L5。
- CRLF 风险已在工具里处理完：`.gitattributes:1` 强制 `*.sh text eol=lf`，`tests/acceptance/Dockerfile:13` 仍防御性 `sed -i 's/\r$//'`，`run.sh:19-30` 内建 `cygpath -w` 与 `MSYS_NO_PATHCONV=1`，`shot.sh:5-6` 明确要求 Git Bash。**不要"顺手修"这些脚本。**
- 两个 scratch 脚本硬编码了过期路径 `/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me`（`.scratch/refactor-structure-20260921/wsl-test.sh`、`verify-cli.sh`），复用前需改。`verify-cli.sh` 那 23 条 `--help` 字节比对是有价值的门，值得提升进正式 `scripts/`。
- 工具链无锁定：仓库没有 `rust-toolchain` 文件，CI 用 stable，edition 2024（`Cargo.toml:4`）；无 nightly、无 musl、无 `clang` 硬需求。

---

## 7. 文档与 ADR 变更

- **ADR-0022**：订阅模板是编译期内置目录；分层顺序；`Standard` 字节冻结；管理员模板文件不在范围内。
- **ADR-0023**：客户端配置可见性与规则片段合并（共享 `deep_merge` 语义）。
- `PRODUCT.md:30-35`：写入覆写边界（今天**没有任何一处记录过**）与 `active_config` 承诺。
- 同步修正 `singbox-sub-plan.md` 的落后内容（`implementation-plan.md:208` 已要求，仍未做）。
- `.scratch/sbgui-progressive-workspace/issues/04` 的 `ready-for-human` 阻塞（VM 口令）由本轮解除，须回写结论。

---

## 8. `.scratch/` ticket 分解

新目录 `.scratch/subscription-capability-20261002/`，`spec.md` 放目标 spec 原文（按 `docs/agents/issue-tracker.md` 的本地 Markdown 约定）。票：

`01-connections-poll-starvation`、`02-artifact-goldens`、`03-testbackend-render-harness`、`04-ci-unused-deps`、`05-sing-box-band-check`、`06-sniff-field-probe`、`07-client-template-axis`、`08-tag-vocabulary-unify`、`09-clash-sniffers`、`10-external-resources-catalog`、`11-node-uri-display`、`12-userinfo-interval-key`、`13-full-profile-target-selection`、`14-tun-stack-profile-gating`、`15-node-names-and-groups`、`16-tui-rules-inbounds-page`、`17-event-codes-kill-chinese-matching`、`18-orphan-guard-pdeathsig`、`19-guided-tun-restart`、`20-ip-token-bucket`、`21-gui-column-and-time-columns`、`22-remove-tray-dep`、`23-win11-vm-verification`、`24-msi-ci-build-and-fix`、`25-client-core-report-findings-2-and-5`、`26-sbgui-shot-into-ci`。

前置决策阻塞：`07`（ADR-0022）、`15`（节点 schema）。`16`（G9 档位）与 `19`（G11 档位）已决策。

---

## 9. 两个排期陷阱

1. **默认模板绝不能动字节。** `apply_config_transaction` 只要**任一**工件变化就重启被管服务（`src/subscription/artifacts.rs:177-215`）。一次不小心的 `Standard` 默认值翻转会改 `sing-box-full.json`，让每台升级的 VPS 在会话中途重启 sing-box。Phase 0.2 的金标准就是把这个从线上事故变成构建失败。
2. **`download_detour` / `stack` / DNS 服务器形态是"三处一份"的变更**：`src/subscription/render/singbox.rs`、`crates/client-core/src/core.rs:549-589`、以及版本注册表。今天三者毫无关联 → 客户端可以悄悄把服务端为某 minor 删掉的字段塞回去。Phase 3 必须加上那条对比测试。

---

## 10. 实现落点（关键文件）

- 服务端：`src/subscription/profile.rs`、`src/subscription/render/{singbox,clash,mod,uri}.rs`、`src/subscription/artifacts.rs`、`src/subscription/serve.rs`、`src/subscription/template.rs`（新建）、`src/config.rs`、`src/canonical.rs`、`src/index_page.rs`、`src/override_template.rs`、`src/wizard.rs`
- 客户端：`crates/client-core/src/{controller,core,system_proxy,state,clash_api,event,command}.rs`、`crates/sbtui/src/{app,input,style}.rs` + `view/`、`crates/sbgui/src/{chrome,app,main}.rs` + `pages/`
- 门禁与工具：`.github/workflows/{ci,release}.yml`、`tests/version_profiles.rs`、`tests/clash_mihomo.rs`、`tests/acceptance/*`、`scripts/sbgui-shot/*`、`packaging/windows/sbtui.wxs`

---

## 11. 执行进度（2026-09-23）

### 已完成

**Phase 0.1 —— connections 轮询饿死：已修复并结案。**
根因确认是取消，不是 ticket 猜的早退：`poll()` 在 `.await` 之前推进三个刷新槽位时间戳
（`controller.rs:1012/1016/1024`），而 `:283-286` 的内层 `select!` 让命令赢过 `poll()`，
future 被丢弃后槽位已消耗、数据未取。修复为"请求完成后打戳，且打完成时刻"，并把
`auto_update_due()` 改成无副作用谓词（它原本在判定里就写掉 `last_auto_update`，而它恰是这条路径上
最长的请求）。`watch_core_exit()` 那条不是稳态冻结源，已在 ticket 里更正。

红→绿证据：`a_cancelled_poll_does_not_consume_the_connections_slot` 修复前失败于
`active_connections: 0`，修复后通过；另加 `a_failed_refresh_still_consumes_its_slot` 守住
"延后打戳不会把不可达内核变成每 tick 重试"。
A/B 证据（L4，同镜像同种子，探针读数四轮都是 `api rows: 3`）：基线渲染 0 行 / 徽标 0 / 0 B/s，
修复后 r1、r3、r4 三轮都是 3 行 / 徽标 3 / 192.3 KiB/s。`.scratch/sbgui-issue01-*`。

**Phase 0.2 —— 工件金标准：已落地。**
13 份快照（12 个订阅工件 + `sing-box-server.json`）+ 一个跨进程字节一致性测试。JSON/YAML 按显式
键排序钉内容，文本工件（`uri` / `uri.txt` / `shadowrocket.txt`）按字节钉，与 ADR-0021 的冻结范围
一致。金标准顺带把 G2 记成证据：两份 clash 快照里 `sniff` 出现次数为 **0**。

**新发现（原计划里没有）：订阅工件的键序随构建形态变化。**
`cargo tree -e features --workspace -i serde_json` 显示 workspace 构建统一打开了
`serde_json/preserve_order`，根包构建没有——同一份配置，两种构建产出不同字节。这直接威胁
ADR-0021 冻结的 `vmess://` 载荷（base64 内部就是 JSON）。已在 `src/subscription/render/uri.rs`
用 `vmess_payload()` 手工按字典序拼装载荷收口，**修复后 base64 行与修复前的生产字节逐字节相同**。
剩余的 `artifacts_changed` 误重启风险记在
`.scratch/subscription-capability-20261002/issues/27-build-shape-dependent-json-order.md`，
需要产品决策（三个选项已列，未默认选第一个）。

**Phase 0.3 —— sbtui 渲染测试：已完成。**
`crates/sbtui/src/view/mod.rs` 末尾新增 `render_tests`：一个足够密的 `ClientSnapshot` 夹具 +
5 个 tab 的 `TestBackend` 金标准 + Logs 页与 rules 视图两张对照帧。已验证非空且可复现
（连跑 3 次零 `.snap.new`；每帧 24 行、约 5 KB；connections 帧确含 `东京-A`/`节点选择`/`KiB`）。
数据目录绝对路径归一化为 `<DATA_DIR>`，否则 Settings 页会把临时目录名画进帧里。

### L2 腿已经建成，并且立刻抓了两个跨平台缺陷

`Ubuntu-22.04`（**不是** `Ubuntu`）里装好 rustup/cargo 1.98.1，rsync 到 `~/src` 的循环可用，
`python3 -m unittest discover -s scripts` 在这一腿通过（2 项）。

在 Linux 上跑同一份树时暴露了两处只在 Windows 上看不到的问题，两处都是我这轮新加的门自己的缺陷：

1. **订阅金标准带平台路径分隔符**：`sing-box-server.json` 里的证书/缓存路径由 `Path::join`
   生成，Windows 上捕获的金标准在 Linux 必然失败。已在 `canonical_artifact` 里把反斜杠规范化
   ——sbctl 只支持 Debian/Ubuntu（`src/preflight.rs:37-39`），生产字节永远是 POSIX 形态。
2. **sbtui Settings 帧按平台变化的是"长度"而不只是内容**：真实临时目录名与
   `system_proxy::platform_label()` 的中英标签长度不同，**截断点**就不同，事后替换无法还原。
   解法是把数据目录换成定长占位量 `<DATA_DIR>`（从源头消除长度差异），并把 Settings 这一帧
   按 OS 各存一份（`tab-4-windows.snap` / `tab-4-linux.snap`）；其余四帧只渲染快照状态，
   跨平台共用一份。

教训写在这里给后面的 Phase 用：**新加的门必须在 L2 上跑一遍才算立**。只在本机生成的金标准
等于把 CI 写死在自己的 OS 上。

### Phase 8 的第一条已完成，并做了变异检验

`a_brief_success_does_not_reset_the_restart_allowance`：短暂成功（不足 `STABLE_RUN`）不得清零
自动重启计数。先确认它绿，再把 `reset_restarts_after_stable_run` 的条件改成恒真，测试**如期变红**
（`a sub-STABLE_RUN success must not clear the counter`），随后还原——证明这不是同义反复。
finding #2（预绑 mixed 口断言启动被拒）待做。

### 当前门禁状态（两种构建形态、两个平台）

Windows 宿主：fmt 干净、`clippy --workspace --all-targets --all-features -D warnings` 0 错、
`cargo test --workspace --features sbctl/test-signing` 全绿（341 项）。
Linux（WSL，`--exclude sbgui`）：同一份树全绿 + scripts 腿通过。
Docker：五个真内核各自接受自己的工件，`server_config` 过 1.14，带检查过。

### Phase 0.5 已完成（两半都完成）

带检查那一半见下；**sniff 字段探针的结论在 §4.2，它推翻了原计划给 sing-box 加嗅探细节的做法**，
并把 G2 的工作量整体挪到 Mihomo 侧。

真核校验在 Docker（Linux）里用五个真实内核实跑通过：`checked` 从 5 个内核全部到位，
`server_config_passes_the_latest_real_core_check` 与带检查（`SBCTL_UPSTREAM_LATEST=v1.14.1`）
三条全绿。

### Phase 1 已开始：issue 05 的时间列

`sbgui` 的连接页把内核的绝对 RFC3339 串原样塞进"建立时间"列（`pages/connections.rs:294-297`），
在列宽下渲染成被截断的 `2026-09-23T0…`，且同一秒内建立的连接看起来完全一样。改为显示相对时间：
`client_core::format::seconds_since()` 做纯解析（新增 `chrono` 依赖，与根 crate 同版本，
不引入新的锁文件条目），措辞留在 UI 层复用已有的 `lang::age_label(locale)` ——
这样不碰 issue 02 定下的"引擎不出中文"方向。`setting_line` 的 value 参数放宽为
`impl AsRef<str>` 以接受 owned 值。无法解析的非空值原样透出而不是隐藏。

新增测试：`seconds_since_reads_the_core_timestamp_and_rejects_junk`（含未来时钟偏移不得回绕）、
`the_started_column_shows_an_age_rather_than_the_raw_timestamp`（中英双语 + 空值 + 垃圾值）。

**这条改动本身也是"截图才算证据"的理由**：第一版只改了 `pages/connections.rs` 的
`setting_line` 分支，L4 截图回来一看**仍然是 `2026-09-23T0…`**——用户看到的那一列由
`components.rs:714-725` 的 `connection_row` 渲染，`setting_line` 那处是选中连接后展开的
"连接详情"面板。两处现在都走 `established_label`。只跑单测会以为已经修好了。

### Phase 0.5（带检查部分）已完成


`tests/version_profiles.rs` 新增三条：

- `the_version_registry_is_a_contiguous_chained_band`（不忽略，无需内核与网络）：注册表必须是
  连续 minor、`supported` 区间首尾相接、`notes` 非空。
- `the_registry_tracks_the_latest_stable_sing_box_release`（`--ignored`，读
  `SBCTL_UPSTREAM_LATEST`）：注册表顶必须等于上游最新稳定 minor。**已双向验证**：喂
  `v1.14.1` 通过，喂 `v1.15.0` 失败并给出"加 profile + 补 notes + 抬 ci.yml 固定内核"的补救说明。
- `ci.yml` 的 `sing-box-profiles` 作业新增一步，从 `releases/latest` 取 tag 并导出该变量；
  取不到 tag 时**主动失败**，避免这个门被静默跳过。`release.yml` 的 `checks` 作业复用
  `ci.yml`，所以发布门自动覆盖。

顺带修掉一个会让门失效的老问题：`generated_profiles_pass_a_real_sing_box_check` 原来断言
`checked >= 1`，即 5 个内核里只有 1 个被设置时仍然全绿。现在断言
`checked == SING_BOX_VERSION_PROFILES.len()`，缺任何一个 `SING_BOX_BIN_<maj>_<min>` 都判红。

当前上游最新稳定版核实为 **v1.14.1**（与注册表顶一致，所以这个门今天是绿的），
mihomo 最新为 v1.19.31（CI 固定 v1.19.30，属有意固定）。


### Phase 2 PR (a) 的第一半：客户端改为**发现**选择组（字节中性）

`client-core` 原来硬编码 `🚀节点选择` 来找选择组，还带着一个**两边都对不上**的
`DIRECT_TAG = "🎯直连"`（服务端 sing-box 是 `direct`、Clash 是 `🎯全球直连`），且
`AUTO_TAG`/`DIRECT_TAG` 在客户端里零使用。后果：Mihomo 订阅的组叫 `🌍选择代理节点`，
第三方订阅爱叫什么叫什么，"当前节点"在这些订阅上静默失效。

新增 `state::find_selector_group(groups, rules)`：先问 `route.final`（内核已经告诉我们答案），
再退回 `🚀节点选择`（保住本项目自身 profile 的旧行为），再退回组**形状**（selector 类型 →
第一个非 urltest 组 → 第一个组）。`controller.rs` 与 `sbtui/view/proxies.rs` 都改走它；
`AUTO_TAG`/`DIRECT_TAG` 删除。**服务端一个字节都没动**，13 份订阅金标准与 18 项渲染金标准
全部未变，符合 ADR-0021 对冻结工件的要求。

这轮又两次验证了"测试必须能红"：
- 第一版 `the_selector_group_follows_route_final_not_a_hardcoded_name` 删掉 `route.final` 分支
  后**仍然通过**——回退链恰好也选中同一个组，测试根本没约束它。补了
  `route_final_decides_when_more_than_one_manual_group_exists`（两个手动组、只有 final 能区分），
  变异版随即判红。
- 变异检验用的备份是在补测试**之前**拍的，`cp` 还原时把新测试一起冲掉了；靠"干净跑 2 passed、
  还原后只剩 1 passed"这个数量差发现。教训：备份要在编辑之后拍，并且还原后要复数一遍测试数。

### Phase 1 issue 02：第 2 批 + 第 4 步（状态颜色不再靠猜中文）

事件码表累计 14 个码，`EventCode::ALL` 驱动穷举测试。`ClientSnapshot.status_level` 让
`sbtui`（`style::status_color_at` + `App.status_level`）与 `sbgui`（`chrome.rs` 工具栏）**优先读
引擎给出的级别**，只在未迁移的调用点上退回原来的中文子串猜测。新测试
`an_engine_severity_beats_the_word_guess` 断言级别压过措辞——英文界面丢色的根因就在这里。

**刻意没转三处**：`流量模式: {}`、`流量模式: {}（内核已重启）`、`出站模式: {}`。它们的 `{0}` 是
`TrafficMode::label()` / `OutboundMode::label()` 的中文枚举标签，转成事件码只是把中文从模板挪进
参数，英文界面照旧混中文——属于 ticket 第 24 行记的"内插枚举 label"问题，必须与 label 同批解决，
否则是假进展。

穷举测试当场抓到自己写的一个假判据（要求每个码渲染后都含参数，对无占位符的码必然不成立），
已改为按模板是否含 `{0}` 分支，并加"两种语言对是否需要参数必须一致"的检查。

当前 `controller.rs` 仍有约 22 处 `.note(`；第 3 步的 23 条错误链、`log_level_of` 的中文猜级别、
`ClientEvent`/`recv()` 死缝、GUI 设置页 TUN 开关、第 5 步中英全页截图均未动。

门禁：Windows 与 Linux(WSL) 双平台 fmt/clippy 0 错、workspace 348 项全绿；sbtui 渲染金标准
未变动，说明这批转换没有改变任何用户可见文本。

### Phase 6 G11：引导式 TUN 重启（重启部分完成，提权部分未做）

`ClientCommand::SetTrafficMode` 改为 `{ mode, restart: bool }`。内核未运行时直接生效；运行中且
`restart:false` 时**给出指名出路的拒绝**；`restart:true` 时 stop → `adapt_inbounds` → start。
若新模式起不来（TUN 无权限、缺 `wintun.dll`），**恢复原模式并重新拉起**，落实 ADR-0003
"失败的变更不得把部署留在半生效状态"。TUI 的确认文案从"下次启动内核时生效"改成"将重启内核"，
并发送 `restart:true`——今天它终于是一个开关而不是偏好。

两条新测试。其中 `a_mode_switch_while_the_core_runs_is_refused_without_a_restart` 一开始也是**假绿**：
只断言 `is_err` 加设置未变，而"尝试切换→重启失败→回滚"同样产生错误且同样恢复原设置，变异版
（删掉拒绝分支）照样通过。改成断言**拒绝理由本身**（消息须含"重启内核"）后，变异版判红，
并且它的失败输出顺带证明了回滚路径真的在跑：
`切换流量模式失败，已恢复原模式: TUN 模式需要管理员/root 权限`。

**未做**：Windows 上 `can_use_tun()` 为假时的 `ShellExecuteW("runas")` 自提权重启提示。
`update_settings` 里的混合端口拒绝、以及 GUI 设置页那个走 `UpdateSettings` 的 TUN 开关
（`pages/settings.rs:348-361`，需要 `toggle_line` 改成携带命令而非 patch）也都还在原地。
TUN 本身只能在 L5（Windows 真机）证明。

### Phase 6 G12：完成，且有可证伪的证据

已实现：`crates/client-core/src/core.rs` 的 `prearm_orphan_guard` 在 fork/exec 之间用
`prctl(PR_SET_PDEATHSIG, SIGTERM)` 挂保护，并检查 `getppid()` 关掉"父进程已先死"的竞态窗口
（窗口内让 spawn 失败，而不是留下无保护的孤儿）。`attach_orphan_guard` 拆成三份诚实版本：
Linux `true`、Windows 保留 Job Object、**macOS 返回 `false`**——原来那句"什么都没做却返回
`true`"是假陈述，它让 `controller.rs:668-672` 的警告永远不响，现在会响。client-core 新增
unix `libc` 依赖（与根 crate 同版本）。

端到端证明在 `crates/client-core/tests/orphan_guard.rs`，**已验证可证伪**：把
`prearm_orphan_guard` 降级成"什么都不做但仍报成功"的变异版，测试判红
（内核 `state=S`、被重新收养、仍在运行）；钩子装上时判绿（`/proc` 条目消失）。
两条让测试一度假通过的陷阱已记录在该文件里，值得当作写这类测试的检查单：

1. **用 `/proc/<pid>` 是否存在判断存活是错的**：僵尸进程保留该条目且 `PPid` 仍指向父进程，
   于是"已退出的内核"通过了"内核已启动"的断言；等父进程死后被 init 回收，条目消失，看起来
   正好等于"保护生效"。`alive()` 现在读状态字母并把 `Z` 当作已死。
2. **清理跑在断言之前**：先 `kill -9` 掉幸存内核再断言它不存在，等于让被检对象销毁自己的证据。
   现在存活状态只采样一次，断言用采样值。

`starting_a_core_arms_the_orphan_guard_and_reports_it_truthfully` 另外钉住接线与"按平台诚实上报"。

### 待办（Phase 0 剩余）

- **0.3 的 sbgui 半边**：issue 07 的两栏漂移属**原型**（React/CSS），且 ticket 明写"动手前先把
  这条量清楚，不要照着猜测改"，而其测量产物（`.scratch/proto-flex/*.json` + `diff.py`）已不在
  仓库里。要么重跑原型测量，要么按 ticket 自己给的第 2 个选项在 `spec.md` 记"允许 ±16px"后结案
  ——这是产品裁决，不擅自选。
- **0.4**：**故意没有直接落地 CI 作业**，改为 ticket
  `.scratch/subscription-capability-20261002/issues/04-ci-unused-deps.md`。原因是上线即红的门
  比没有门更糟：`cargo-machete` 尚未在本仓跑过（当晚宿主到 GitHub release 资源网络不稳定），
  且必须先确认它如何对待 `cfg(windows)` / `cfg(unix)` 下的平台限定依赖（`winreg`、`windows`、
  `raw-window-handle`、`libc`）——Linux runner 很可能把它们全判为未用。
- **0.6**：文档已按实测更正（发行版名、docker 经 interop 可见、python3 可用、cargo 需自装）；
  仍待实测 `tests/acceptance/run.sh` 能否直接从 WSL 发起。

### 环境事实（影响 L1 门的执行方式）

Windows 宿主上 `python3` 是 Microsoft Store 的占位 stub，`python3 -m unittest discover -s scripts`
这一条腿在宿主**跑不了**；Rust 各腿（fmt / clippy / test / release_trust）在宿主正常。
scripts 那条腿要放到 L2（WSL）或容器里跑。当前宿主 L1 结果：fmt 干净、clippy `-D warnings` 0 错、
`cargo test --workspace --features sbctl/test-signing` 全绿（含新增 15 条测试）、
`cargo test -p sbctl --no-default-features --test release_trust` 通过。

另记一条 L4 的脆弱性：`scripts/sbgui-shot` 在容器内 cargo 拉依赖遇到
`spurious network error: SSL connect error` 时会**静默产出空输出目录**（本轮 `-r2` 就是这样），
应改为留下非空日志并明确失败。

### Phase 2 PR(a2) + PR(d)：`node_uri` 抽取与 G5 展示（已完成）

- **抽取**：`render/uri.rs` 现在把 `node_uri(insecure, node)` 与 `insecure_flag(config)` 作为唯一
  出口，`uri()` 与新增的 `subscription::node_share_link` / `lifecycle::node_share_links` 走同一条
  代码路径。13 份金标准在抽取后一个字节都没动（`cargo test -p sbctl --lib` 170 全绿）。
- 计划里写的命令名 `sbctl status nodes --uri` 在本仓库实际是 `sbctl node`，因此落地为
  **`sbctl node --uri`**；按 §Phase 2 的裁决，`sbctl sub` 的默认输出**没有**加入分享链接。
- **展示面**：index 页在客户端矩阵与「全部订阅链接」之间插入「节点与原生分享链接」表；
  交互菜单的节点页调用 `print_nodes(root, false)`，不带 `--uri`。
- **三条新证据，全部做过变异检验**：
  - U `the_share_links_shown_to_the_operator_are_the_uri_artifact_verbatim`：把 `join("")` 改成
    `join("\n")` 立刻判红——"行数"断言抓不到多余空行，抓到的是那条逐字节等式。
  - C `node_uri_flag_prints_the_native_share_links_and_plain_node_prints_none`：把
    `print_nodes(root, uri)` 改成 `print_nodes(root, false)` 立刻判红；同一测试断言不带 flag 时
    stdout 里不出现 `://`，以及 `--uri` 输出不含订阅 credential。
  - C `the_index_page_shows_every_native_share_link_the_uri_route_serves`：把 index 页节点循环改成
    `.take(2)` 立刻判红（3 协议 fixture，逐条比对 `/uri` 路由响应体经 HTML 转义后的每一行）。
- ADR-0022 已落盘：`docs/adr/0022-subscription-templates-are-a-compile-time-catalog.md`。
- 踩到的一条门失效形状：本仓库 CLI 测试的约定是**请求数正好等于 `max-requests`**，新测试发 2 个
  请求却传了 `6`，于是末尾 `server.wait()` 永不返回——表现为挂死而不是失败。已改为 2。
- 新代码第一版**同时**破坏了 `cargo fmt --check` 与 clippy `-D warnings`
  （`&esc(&node.tag())` 的 needless borrow），而 `cargo test` 全绿——三条门必须各自取退出码，
  不能只看测试。当前宿主 L1：fmt 0、clippy 0、workspace 测试全绿。

### Phase 2 PR(e)：G6 `profile-update-interval`（已完成）

- `subscription-userinfo` 现在是 `upload/download/total/expire/profile-update-interval`，
  新键**追加在末尾**；构造逻辑抽成 `serve.rs::subscription_userinfo(&TrafficReport)`，键序由
  U 测试 `the_userinfo_header_locks_its_key_order_and_names` 用**整串等式**锁死（含无额度时
  `total` 回落为已用字节这条分支）。
- 值是**对客户端的策略声明**（24 小时），不是服务端行为的描述：仓库里**没有**任何"订阅自动刷新
  间隔"配置字段（`grep interval src/config.rs` 为空），所以它不是"读一个已有旋钮"，而是一句
  "客户端一天拉一次就够"。这一点写进了常量文档注释，避免以后有人以为它反映服务端节奏。
- **跨 crate 契约**：`client-core::parse_userinfo` 用 `_ => {}` 忽略未知键，因此我们的两个客户端
  不会被自己的服务端骗到；新增 U 测试 `an_unknown_trailing_userinfo_key_is_ignored` 把这条 tolerance
  钉住。
- **一条"绿色覆盖不了断言"的实证**：做变异时把 `; profile-update-interval={}` 整个删掉，
  `cargo test -p sbctl --lib the_userinfo_header` 仍然是 **1 passed / 1 failed** —— 通过的正是那条
  降级测试，而既有的三条 header 断言（`verify.sh:104/180`、`tests/cli:957/1278`）全部写成
  `... total=…; expire=` **前缀匹配**，缺一个尾键它们一个都不会红。只有新增的整串等式会红。
  这就是为什么 §5 要求"键序由 header-shape 测试锁死"，而不是"有 header 就行"。
- 门：Windows 宿主 fmt 0 / clippy 0 / serve 12 条 + client-core 69 条全绿；WSL(L2) 复跑见下。

### L2（WSL）复跑 + 两条环境事实

`~/src/singbox-sub-me`（ext4，rsync 自宿主）在包含本轮全部改动后：
`cargo fmt --all --check` 0 处 diff、`cargo test -p sbctl -p client-core -p sbtui --features sbctl/test-signing`
**0 失败**（sbctl lib 177、cli 91、client-core 69、sbtui 18，共 358 通过）——G5 的两条 CLI 门与
G6 的 header 门在 Linux 上同样为绿，且**在 mihomo 之前**没有让金标准移动。

两条环境事实（都是"门为什么看起来红了"，不是代码缺陷）：

1. **`cargo --workspace` 在 WSL 需要系统库**：`sbgui`(GPUI) → `yeslogic-fontconfig-sys` →
   `pkg-config` + `fontconfig.pc`。缺失时 clippy/test 以 **build script panic** 失败，症状很像代码错误。
   已在本机 WSL 装上（`pkg-config --modversion fontconfig` 有值），并把这条前置写进 §6 L2 配方。
   在装不了包的环境里，日常腿用 `-p sbctl -p client-core -p sbtui` 即可覆盖本仓库所有服务端/客户端门。
2. **真核 mihomo 门基线已建立**：`~/bin/mihomo` = CI 同一个 pin **v1.19.30**
   （`mihomo-linux-amd64-compatible`），`MIHOMO_BIN=... cargo test --test clash_mihomo -- --ignored`
   在改动**之前**通过：`mihomo accepted subscription-clash.yaml` + `subscription-clash-1.18.yaml`。
   这条基线是 Phase 2 PR(c)（Clash `sniffers` / `dns-hijack`）的前置判据——没有它，改完再跑就是
   单变量对照缺失（§9 的排期陷阱）。CI 的 `mihomo-profiles` 作业用的是同一个 pin。

### Phase 7（G8）：按源 IP 的令牌桶（已完成）

`serve.rs` 新增 `IpBudget`：单地址瞬间可花掉 `REQUEST_BURST = 60` 次，之后
`REQUEST_REFILL = 1/秒` 回补，空闲不会回攒超过 60。计费点放在
`subscription_http_response` **读 method 与 path 之前**，因此：

- 真凭据与错凭据在被限流时得到**逐字节相同**的响应（测试直接断言 `valid == invalid`），
  探测者换不到"这个订阅是否存在"的信息——§Phase 7 要保的那条性质成立；
- 与计划字面的偏离：**超限回 `429 + Retry-After`，不是 404**。理由是 404 会让客户端 App
  把订阅判成"已失效"（不少客户端会显示失效或自动删除档案），而 429 会被按 `Retry-After` 重试；
  两者对探测者同样不可区分，但对真实用户只有一个是谎报。这条偏离写进了
  `docs/subscription-guide.md`。
- **ACME 挑战路径不受限**（`serve_direct_socket_activated` 处注释说明）：那条路径由
  Let's Encrypt 的验证服务器发起，掐断它等于让证书续期失败，进而把整个 HTTPS 拉下线。
- 读不到 peer 地址时**放过**（fail open）：fail closed 会在任何平台意外隐藏 peer 时
  把所有订阅一起停掉。这层是公平/滥用下限，不是安全边界，注释里写明了。
- 表大小有界：满额地址等同"没见过"，直接丢弃；只有在为新地址计费时做一次清扫（热路径不扫），
  并且超过 `MAX_TRACKED_PEERS = 4096` 时整表清空——能凑出四千人真实握手的攻击者不是这层防的对象。

**证据**：5 条 fixture 时钟单测 + 1 条过真实 listener 的接线测试；三条变异各自判红——
把计费点从响应函数里摘掉 → 接线测试红（179 里它一个）；把回补写死成 0 → 恢复测试红；
把清扫短路 → 表大小测试红（65 ≠ 1）。中途有一次"变异后测试仍绿"其实是**变异版本没被编译出结果、
grep 拿到的是上一轮输出**，重跑并显式看退出码才判定，属于 §feedback 的同型坑。

宿主门：fmt 0、clippy 0、`-p sbctl --lib` 179 全绿、`cargo test --workspace`
**364 全绿 / 0 失败**—— burst=60 的取值让既有 CLI 门（单进程最多 17 次请求）与
`tests/acceptance/verify.sh` 都不受影响，没有把绿门改成红门。
L2（WSL，`-p sbctl -p client-core -p sbtui`）同样 fmt 0 / clippy 0 / **364 全绿**，
即令牌桶与两条平台相关门在 Linux 上一起过。

**未完成**：D 腿（`verify.sh`）的洪水断言还没加。加的时候有个坑：同一进程里后续断言用的
是同一个 127.0.0.1 地址，洪水会把它们一起限成 429——必须放在该 server 实例的最后，
或为限流单独起一个实例。

### Phase 8（G17）：两条无测试的已修路径（已完成）

- **finding #2（固定口启动检测）**：新增 `a_busy_mixed_port_is_refused_before_the_runtime_config_is_rewritten`，
  用另一个监听器占住 mixed 端口，重放原场景，并同时断言**拒绝理由**（含 `已被占用` 与端口号）与
  `cache/active-config.json` **未被改写**——后半句才是这条路径的价值所在（拒启不能顺手毁掉上次
  已知良好的运行配置）。配套对照 `a_free_mixed_port_lets_startup_reach_the_config_write`：端口空着时
  启动会越过探测、把配置写下去再因缺少内核二进制而失败；没有这条对照，"`!exists()`" 可能只是
  因为写盘本来就会失败而恒真。变异检验：把早探条件改成恒假 → 前者判红、对照仍绿。
- **finding #5（短暂成功清零重启计数）**：`a_brief_success_does_not_reset_the_restart_allowance`
  断言 sub-`STABLE_RUN` 的成功不清零，且下一次崩溃从 3 续到 4 而非从 1 重来。
- 报告的 §五 表格两行已改为结案，并写明"条目 3/4/6 仍未复核"，不把整节当已验证。

### Phase 2 PR(c) 的 Clash 半边：G2 嗅探（已完成，但**结论与原计划不同**）

计划写的"顶层 `sniffers: [domain, http, tls, quic]` + `dns-hijack: any:53`，legacy 用 `sniff: true`"
**三处全错**，被 CI 同一 pin 的真核（mihomo v1.19.30）逐项否掉，证据与判据见 §4.3。落地为两份 Clash
工件统一 `sniffer: {enable: true, sniffing: [http, tls, quic]}`；`dns-hijack` 与
`override-destination` 都不写，理由写在 `render/clash.rs::clash_sniffer` 的注释里（前者属于 `tun:`
且默认已是 `0.0.0.0:53`，从订阅输出 `tun:` 会覆盖客户端自己的设置；后者改变远端看到的目的地，
不该静默代客户决定）。

- **金标准如期判红**，且只有两份 Clash 移动，其余 11 份逐字节不变——这正是 ADR-0021 要的形状：
  冻结的工件没动，客户端面的全量配置动了，且是一次**有记录的**产品决定。
- **真核门通过**：`MIHOMO_BIN=~/bin/mihomo cargo test --test clash_mihomo -- --ignored` →
  `mihomo accepted subscription-clash.yaml` + `subscription-clash-1.18.yaml`。这条门的判据在改动前
  刚建立为绿（单变量对照成立），并且已知它**能**拒绝错的 sniffer 名，所以"接受"这次有信息量。
- 宿主 fmt 0 / clippy 0 / `cargo test -p sbctl --lib` 173 全绿；Linux(WSL) `-p sbctl -p client-core
  -p sbtui` **358 全绿**。
- 新增 `both_clash_artifacts_enable_the_sniffers_the_pinned_core_accepts`，除了断言块存在且只出现一次，
  还断言两份工件**不含** `dns-hijack` / `sniffers:` / `override-destination` / `- domain` / `- dns`
  ——即把这次纠正本身钉住，防止有人照"网上常见写法"改回去。
- 记录一条既有测试的门失效风险：`config::tests::concurrent_reads_observe_only_complete_state_versions`
  在宿主全量跑 + WSL 并发构建时出现过一次 `PermissionDenied`（单独重跑 10/10、全量重跑 3/3 均绿）。
  与本仓改动无关，已开 ticket 28 处理它的重试构造，不当成已通过。

### Phase 2 剩余

(b) `ClientTemplate` 轴（`Standard` 必须字节复现今天）+ G3 规则集及其 `minimal` 内联孪生
（这两件是同一件事：孪生列表是模板的数据）。

### Phase 3(3)：`sing-box-full.json` 改为**问内核**再选目标（已完成）

- 新增 `select_full_profile`（纯函数，判定通过 `accepts(&Profile, &rendered)` 闭包注入，
  所以选择逻辑本身不需要 spawn 就能测）与 `resolve_full_profile`（唯一的 I/O 包装，
  `accepts` 就是 `check_sing_box_config(kernel, _)`）。沿注册表**从新到旧**走，
  第一个既渲染得出来、又被内核接受的档胜出。
- 三条不可破坏的边界都写成了测试：内核全盘接受时结果与今天**逐字节相同**（金标准未动即证据）；
  全部被拒 / 内核读不到 / 没有内核时**回落到表内最新档**，绝不因此让生成失败或产出空工件；
  向下走时**跳过**承载不了当前节点集的档（只有 AnyTLS 的部署不会掉进 1.10/1.11）。
- 穿线只落在"会写盘"的路径上：`regenerate` 与 `apply_config_transaction` 把它本来就收到的
  `sing_box_bin` 传下去，`install` 传刚下载/指定的那个内核，`config` 事务分支传已解析路径；
  公开的 `generated_artifacts(config, root)` 保留为"不咨询内核"的等价包装，
  所以 20 多个测试调用点与金标准一字未改。
- **接线证明**：`store_dns` 只有 1.14 档携带（金标准可查），于是用一个"含 `store_dns` 就退出 1"
  的 sh 桩内核就能精确表达"这台内核还没见过最新 minor"，断言 full 工件与 `sing-box-1.13.json`
  **逐字节相等**。变异检验：把工件那行改回 `latest_version_profile()` → 该测试判红，
  差异恰好是 `store_dns` 一行。测试 `#[cfg(unix)]`（桩要能执行），Linux 腿实测通过。

### Phase 3(4)：已装内核高于版本表顶时"只告警、不断服"（已完成）

- `profile.rs` 新增 `band_warning_for`（纯函数，输入 `ClientVersion`）、`parse_kernel_version`、
  `installed_kernel_version`（唯一一层 I/O，任何失败都 `None`）与 `kernel_band_warning` 组合入口。
- **只报一个方向**：等于表顶（健康态）与低于表顶都沉默。理由是告警一旦成为背景噪音，
  真正的缺口就会被淹没；旧内核本来就由各自的 per-minor profile 服务，没有可警告的东西。
- 暴露面：`sbctl status` 末尾一行 + `status --json` 的 `kernel_version_warning`（键恒在，
  无告警时为 `null`）。JSON 契约由 `tests/cli` 断言，**把字段删掉那条断言就判红**（变异检验过）。
  人类可读那半只有一行 `println!`，走的是与 JSON 完全相同的表达式，未单独再测。
- 比较按 `(major, minor)` 归一：注册表存 minor（`1.14`）而内核报完整版本（`1.14.1`），
  不归一的话每台正常安装的机器都会天天看到误报。
- **本项未含**：`sbctl restart` / `regenerate` 成功后的同一条打印（§Phase 3(4) 后半句），
  以及 §Phase 3(3) 的 `resolve_full_profile`（要先改公开 `generated_artifacts` 的签名）。
  两者继续记在 task #9。

### 已验证状态（2026-09-23 收尾）

Phase 7（G8）与 Phase 8（G17）之外，本轮**没有再动代码**；两批新门在两个平台各自复跑：
Windows 宿主与 WSL(L2) 都是 fmt 0 / clippy 0 / **366 通过 0 失败**（`-p sbctl --lib` 179、
cli 91、client-core 71、sbtui 18、其余为零散目标）。10 个本地提交在 `refactor/structure`，
**未推送**。

### 下一轮从 Phase 3 接手的三条实测事实

省掉重复推导：

1. **§Phase 3(3) 的真实成本是签名，不是算法。** 内核二进制路径只到得了
   `regenerate(store, config, sing_box_bin, update_active_config)`，而选择客户端 profile 的
   地方在它调用的 **公开函数** `generated_artifacts(config, root)` 里（`artifacts.rs:276`）。
   要让 `resolve_full_profile` 拿到内核，必须把 `Option<&Path>` 穿进这个 pub 函数，
   牵连 `regenerate` / `apply_config_transaction` / 金标准测试 / `tests/clash_mihomo.rs` 等调用点。
   并且它必须是 `(config, 已装内核 minor)` 的**纯函数**——否则安装事务的
   `artifacts_changed` 比较会抖动（这条是 §Phase 3(3) 自己写的，实测确认没有现成 seam 可走）。
2. **服务端没有"已装内核版本"的可复用 helper。** `src/update.rs:719` 那个 `--version` 探的是
   **sbctl 候选二进制**，不是 sing-box。§Phase 3(4) 的告警要自己新增一次 `sing-box version`
   调用与解析；`crates/client-core/src/core.rs:193-205` 已经在解析同一个字符串
   （`"sing-box version 1.14.1"`），格式可照抄但不能直接复用（跨 crate，且客户端那份是运行时依赖）。
3. **注册表存的是 minor（`"1.14"`），内核报的是完整版本（`1.14.1`）。** 比较"已装版本是否
   高于表顶"必须按 `(major, minor)` 归一，否则 `1.14.1 > 1.14` 会在每次正常安装后误报。

已实现的是第 3 条的归一（`parse_kernel_version` 丢补丁号），第 2 条的探测也已在
`profile.rs` 落地并被 `status` / `status --json` 使用。**下一步尝试时先读这三条**，
本轮在它们上面各撞过一次：

4. `src/cli/commands/config.rs` 里**至少两个函数**以
   `let result = store.load().and_then(|config| { let binary = sing_box_bin.unwrap_or_else(...) })`
   开头，`regenerate` 不是唯一的；要给 `regenerate` 的成功分支加打印，锚点必须带它上面
   那段"Regenerate always re-syncs…"注释，否则会命中两处。
5. 产品解析出的托管内核路径是**无扩展名**的 `<root>/usr/local/bin/sing-box`，所以一个
   "会报版本号的假内核"fixture 只能在 unix 上被执行；Windows 侧要覆盖同一条告警，必须走
   `--sing-box-bin` 指到 `.cmd`。附带结论：`sbctl status` 的告警在 Windows 上永远不会响，
   对 Linux 服务端工具无所谓，但别把它当成"两平台都测过"。
6. 本仓库的工作树**混用 LF 与 CRLF**（同一轮里 `serve.rs` 被 rustfmt 归一成 LF，而
   `status.rs`/`config.rs` 仍是 CRLF），脚本化改文件必须两种换行都试；本轮两次
   "模式没命中"都是这个原因，而不是代码变了。



## 12. 收尾里程碑：什么只能证明"尚未失败"

本地分支比 `master` 领先 65 个提交**且仍未推送**（`.scratch/structure-refactor-20260921/report.md`）；v0.1.26 仍由**开发 key 签名且没有 `install.sh`**（`release-readiness-and-vps-test-plan.md:140-143`）。

生产信任链端到端（从真实 GitHub Release 拉取带生产 key 的 `install.sh`、以及升级一台旧的 dev-key 安装）在配置好 `vars.SBCTL_RELEASE_PUBLIC_KEY_HEX` + `secrets.SBCTL_SIGNING_SEED` 并打出 tag 之前**无法排练**。这是一个独立的收尾里程碑，Phase 1 与 Phase 2 完成后要显式排进来，不要当成"CI 绿了就等于能发布"。
