# 合并后已知缺口

日期：2026-09-24
适用分支：`main`（`sbctl` 0.2.0，合并提交 `234c82f` + 整理提交 `b55beeb`）

本文记录把 `refactor/structure` 并入主线后**仍未完成**的工作。每条给出可核对的证据（`文件:行`）、对应的既有工单，以及建议的下一步。范围、优先级与分层验证方法沿用
[target-spec-gap-and-verification-plan.md](target-spec-gap-and-verification-plan.md)（它列的是更完整的目标差距清单，本文只汇总合并后仍开放、且与三端目标直接相关的项）。

构建与验证命令见 [verification-and-build-flow.md](verification-and-build-flow.md)。
2026-09-24 的全项目审查与修复过程（含**本文若干建议被撤回**的证据）见
[code-review-2026-09-24.md](code-review-2026-09-24.md)。

## 状态总表

| # | 缺口 | 影响面 | 既有工单 | 状态 |
| --- | --- | --- | --- | --- |
| G1 | Windows 真机验证流水线缺失（`scripts/winvm/verify.ps1` 不存在） | GUI/TUI 平台行为 | `.scratch/verification-environments/issues/03-windows-vm-pipeline.md` | **入口已建成**（`all` 腿待一次真机运行） |
| G2 | Windows MSI 打包缺陷 + 发布链不含 `sbgui`/MSI | 发布 | `.scratch/gui-completion/issues/03-release-matrix-and-ci-gate.md` | **代码已修，runner 未验**；其中两条建议被撤回（见 R11） |
| G3 | 订阅模板 `ClientTemplate::{Global,Split}` 是空壳 | 服务端订阅内容 | `.scratch/subscription-capability-20261002/issues/30-client-template-axis.md` | **已落地并过真核**（5 内核 × 3 模板 = 15 组合全过 `sing-box check`）；mihomo 门亦已扩到 3 模板 × 2 规则档 |
| G4 | 客户端"覆写配置文件内容"未实现 | TUI/GUI | `.scratch/client-config-override/spec.md`（+ `issues/01`、`02`） | **引擎与 TUI 第 7 页已落地**（L1+L2+L3 全绿）；GUI 侧仍缺，已记进 G5 工单 01 第 6 条 |
| G5 | GUI 与 TUI 功能对齐 / UI 一致性 / a11y | GUI | `.scratch/gui-completion/issues/01`、`02` | **覆写页已落地**（`feat(clients)` fbecb86）；工单 01 的 1–5 项与 02 的 9 条仍开放，其中第 6 条已量化 |
| G6 | `tray-icon` 死依赖，且 CI 无未用依赖门禁 | GUI 构建 | `.scratch/subscription-capability-20261002/issues/04-ci-unused-deps.md` | **依赖已删**；CI 未用依赖门禁仍开放 |
| G7 | 英文界面仍残留中文（i18n 未收口） | GUI/TUI | `.scratch/sbgui-progressive-workspace/issues/03-engine-text-remaining-sites.md` | needs-implementation |
| G8 | Windows 真机验证覆盖不足 | GUI/TUI | `.scratch/sbgui-progressive-workspace/issues/04-real-windows-run.md` | ready-for-human（现可用 `verify.ps1 all` 驱动） |
| G9 | sing-box 版本窗口不自动滑动 | 服务端订阅 | target-spec-gap plan §Phase 3 | 部分关闭 |
| G10 | `subscription-userinfo` 无 `refresh` 键 | 服务端订阅 | target-spec-gap plan §2/§3 | 产品决策，非缺陷 |
| **R8** | `subscription-userinfo` 的 `upload`/`download` 与服务端 rx/tx 的对应关系 | 服务端订阅 | 本轮审查新增 | **已按客户端视角翻转，待维护者确认**（曾被 `implementation-plan.md:301` 有意记录为反向） |


---

## G1 — Windows 真机验证流水线缺失

> **2026-09-24 更新**：入口已建成（`scripts/winvm/verify.ps1` + 正式化的 `shot-guest.ps1` + 新 `tui-guest.ps1`），
> `parse-check` 门做过变异检验；但 **`all` 这条腿还没真跑过一次虚拟机**，所以 G8 那串平台行为仍是"未验证"。
> 见 R1 与 `feat(winvm)` 提交。下面原文保留作为设计与约束记录。

- 证据：`scripts/winvm/verify.ps1` **不存在**（`Test-Path` 为 false）。工单规定的入口是 `snapshot | revert | gui | tui | collect`。
- 现有可复用资产：`.scratch/win11-vm/`（`shot-guest.ps1`、`run-one.ps1`、`capture-vm.ps1`、`probe.ps1`、`manifest*.txt`、`shots-win/*.png`）。
- 约束（写进工单，勿违反）：VM 名 `Win11-sbtui-test`，`vmrun` 路径 `C:\Program Files\VMware\VMware Workstation\vmrun.exe`；guest 账号 `Test`，口令**只从 `$env:WINVM_PASS` 读取**，不写入仓库/日志/工单。
- 建议：按工单实现 `scripts/winvm/verify.ps1`，流程为 `revert 快照 → 启动 guest → copyFileFromHostToGuest 投递二进制与脚本 → runProgramInGuest -interactive 跑 GUI 8 页截图 / TUI 冒烟 → 取回 PNG 与日志 → 再次 revert`。

## G2 — Windows MSI 打包缺陷 + 发布链不含 sbgui

证据：
- `packaging/windows/sbtui.wxs:6` — `Version="0.1.0"`，与 workspace `0.2.0` 不一致。
- `packaging/windows/sbtui.wxs:21` — `Shortcut Name="sbtui" Target="[INSTALLFOLDER]sbgui.exe"`：名为 sbtui 的快捷方式实际启动 `sbgui.exe`。
- MSI 只含三个 exe，**缺 `wintun.dll`** 组件，且无 `requestedExecutionLevel` 清单（TUN 需要）。
- `.github/workflows/release.yml` 只构建 `sbctl` 与 `sbtui`（`:66`、`:92`），**不构建 `sbgui`，也不构建 MSI**（产物列表 `:213-214` 无 `sbgui.exe`/`.msi`）。
- `crates/sbtui/packaging/install.ps1` 也未作为 release 资产上传。

建议：版本号对齐 workspace；拆分 `sbtui`/`sbgui` 两个组件各自的快捷方式与目标；补 `wintun.dll` 组件与提权清单；`release.yml` 增加 `sbgui` + MSI 构建与上传。

## G3 — 订阅模板 `ClientTemplate::{Global,Split}` 是空壳

> **2026-09-24 更新**：本条已落地，见 `feat(subscription): give Global and Split real content` 与
> [code-review-2026-09-24.md](code-review-2026-09-24.md) R13。`for_template` 不再丢弃参数，
> 三档目录各自成立且 `standard` 逐字节未变；`minimal` 恢复"有分流但不碰 CDN"。
> **仍欠**：新模板从未过真核 `sing-box check`/`mihomo -t`——那两个测试只渲染默认模板，
> 把它们按模板参数化才是本条真正的收尾。下面原文保留作为背景。

- 证据：`src/subscription/template.rs:162` — `let _ = template;`（`for_template` 忽略模板参数）；`template.rs:10-11`、`:157-158` 注释明确 `Global`/`Split` 声明但未实现，三者输出逐字节相同。
- 现状：`Standard` 的规则集只有 `geosite-private` / `geoip-private` / `geosite-cn` / `geoip-cn` 四项，策略组 3 个；目标文档要求的 `ads`/`proxy`/`openai`/`netflix`/`telegram`/`lan` 目录、`fallback`/`load-balancing`/按区域分组、以及"内联规则孪生"均未落地。
- 设计约束：ADR-0022 规定 `Standard` 必须逐字节复现今天的输出（由 `src/subscription/snapshots/` 的金标准证明）；`minimal` 必须保留内联规则、永不联系规则 CDN。
- 建议：按 target-spec-gap plan §Phase 2 的 PR 切分实现（(b) 模板轴 → (c) 嗅探 + 外部资源 → (d) 节点分享链接展示 → (e) header）。

## G4 — 客户端"覆写配置文件内容"未实现

- 证据：`.scratch/client-config-override/spec.md` 要求每个 profile 一份 `overrides/<sha256>.json`，以与服务端相同的语义 deep-merge 进缓存配置，并在 TUI 只读查看 + 规则片段开关；两分支都**没有**任何存储、命令、merge 或 UI。
- 现状：`crates/client-core/src/core.rs` 只有内部自动改写（`runtime_config`、`adapt_inbounds`），不是用户覆写。
- 设计约束：`deep_merge` 目前在服务端 crate `src/override_template.rs`，应抽成共享小 crate，**不要**在 `client-core` 复制一份实现；覆写边界需写入 `PRODUCT.md` 并立 ADR。
- 建议：按 `issues/01-core-override-model.md` → `issues/02-tui-override-ui.md` 顺序实现。

> **2026-09-24 更新**：引擎侧（`crates/client-core/src/config_override.rs` + `crates/json-merge`）、
> `runtime_config` 的合并点与保留字段写回、TUI 第 7 页「覆写」（只读脱敏 outline + 片段开关 + `O` 清空）
> 都已落地，过 Windows L1、Linux L2（456 测试）与 Docker L3 验收。工单 02 里 `$EDITOR` 那套编辑设计
> 被 ADR-0023 否决并标注 superseded。剩下的只有 GUI 页与真机验收。过程与两处复核发现见 R21。

## G5 — GUI 与 TUI 功能对齐 / UI 一致性 / a11y

- 证据：`.scratch/gui-completion/issues/01-tui-parity-actions.md`（GUI 缺 `ImportProfileFile`、`SetProfileUrl`、档案命名、连接排序、日志暂停、帮助浮层，**以及 2026-09-24 新增的第 7 页「覆写」**）、`02-ui-consistency-and-a11y.md`（9 条：自动滚动饱和、保存语义三套、TUN 置灰、空输入静默、单实例失败静默、对比度、键盘/IME、硬编码颜色、长列表虚拟化）。
- 2026-09-24 进度：工单 01 第 6 条（覆写页）与工单 02 第 1/3/4/5/6 条已落地（R22、R23、R24）。
  工单 01 复核后**差集只剩三个命令**，其中 `SetTrafficMode` 是新发现的**能力缺口**：
  TUI 的 `m` 走 `restart: true` 能在内核运行时换 TUN，GUI 只会发 `UpdateSettings`，
  内核运行时被引擎拒绝——工单 02 第 3 条治好的是死点击，不是这个缺口（已记进工单 01 第 7 项）。
- 建议：逐条按工单实现；一致性项优先于纯视觉项。

## G6 — `tray-icon` 死依赖 + CI 无未用依赖门禁

- 证据：`crates/sbgui/Cargo.toml:27` 声明 `tray-icon = "0.21"`，全仓零使用。
- 根因：它是 Windows-only 依赖，Linux CI 看不见未用。
- 建议：删除该依赖（close-to-tray 是独立产品项，本轮不承诺）；CI 增加 `cargo machete` / `cargo udeps` 门禁。

## G7 — 英文界面仍残留中文（i18n 未收口）

- 证据：`.scratch/sbgui-progressive-workspace/issues/03-engine-text-remaining-sites.md`（needs-implementation）。
- 现状：`EventCode` 骨架已落地，状态/事件渲染已走 `tr!`；但仍有约 23 处 `.note(`（`crates/` 内 `git grep '\.note('` 计数）、`operation_error` 漏斗、`ClientCommand::label()`、以及"事件级别按中文字符串判断"未迁移。
- 建议：把剩余 `.note(...)` 调用点全部改成 `note_event(EventCode::…)`，级别由 `EventCode::level()` 决定；补一条"英文 locale 渲染零 CJK 字形"的截图断言。

## G8 — Windows 真机验证覆盖不足

- 证据：`.scratch/sbgui-progressive-workspace/issues/04-real-windows-run.md`（ready-for-human）。此前仅一轮 VM 截图，proxies/connections 页在 4GB guest 上 `EXITED`。
- 未验证项：CJK 字体回退、按监视器 DPI、DWM 标题栏与拖拽/关闭、真实 `HKCU\...\Internet Settings` 代理写入及恢复、`wintun.dll` 检测与 TUN 提权、Job Object 孤儿回收。
- 建议：guest 内存提到 6–8GB（或分批 4 页），逐项留证据；与 G1 的流水线一起做。

## G9 — sing-box 版本窗口不自动滑动

- 证据：`src/subscription/profile.rs:144` 的 `SING_BOX_VERSION_PROFILES` 是硬编码 5 项（1.10–1.14），`latest_version_profile()` 取 `.last()`；上游发布新 minor 时不会自动纳入。
- 已缓解：注册表连续性单测、CI 上游 `releases/latest` 带检查（红构建即提醒）、运行期用已装内核选档（`resolve_full_profile`）、"内核比表更新"只告警不断服务。
- 缺口：仍**需人工**加 5 行注册表条目 + notes + CI pinned 内核列表。属"可检测"而非"已滑动"。
- 建议：保持 CI 带检查；在发布 checklist 中加入"上游新 minor → 更新注册表"步骤。

## G10 — `subscription-userinfo` 无 `refresh` 键

- 现状：header 为 `upload=…; download=…; total=…; expire=…`，另加 `profile-update-interval=24`。
- 说明：这是**产品决策**（target-spec-gap plan §2 决策表、§3 第 5 条）：`expire=` 已承担"刷新/重置日期"语义，再加 `refresh=` 冗余。若目标文档严格要求 `refresh` 字面键，需重新决策并改 header-shape 测试。

---

## 关联的既有差距计划

更完整的目标差距（策略组薄 G4、外部资源窄 G3、TUN 仅检测不引导 G11、系统代理仅 GNOME G13、真实 VPS 发布门禁 No-Go 等）见
[target-spec-gap-and-verification-plan.md](target-spec-gap-and-verification-plan.md) 的 §1.2 差距清单与 §4 阶段计划；本文不重复其全文，只标注合并后仍开放且与三端目标直接相关的部分。

## 复核方式

每条缺口都能用以下命令独立复核（在仓库根）。**注意方向**：`grep` 一类的命令"无输出"才是已修，
所以每条都写了期望结果，免得把成功读成命令失败。

```bash
# G1 已建：应有此文件；`parse-check` 应打印三行 OK 且退出 0
test -f scripts/winvm/verify.ps1 && echo 'G1: entry exists' || echo 'G1: MISSING'
powershell -File scripts/winvm/verify.ps1 parse-check; echo "exit=$?"   # 期望 0

# G2 版本号与快捷方式：期望 0.2.0，且 sbgui 指向 sbgui.exe、sbtui 指向 sbtui.exe
grep -n 'Version=' packaging/windows/sbtui.wxs
grep -n 'Shortcut Id' packaging/windows/sbtui.wxs
# G2 仍未验：这两条期望"无输出"，因为没有任何 runner 跑过 build-msi
grep -n 'sbgui' .github/workflows/release.yml | head -3   # 有（job 已写）；但"跑过"无证据

# G3 已落地：期望"无输出"（模板参数不再被丢弃）
grep -n 'let _ = template' src/subscription/template.rs || echo 'G3: axis wired'
# G3 真核覆盖情况（2026-09-24 后）：两条真核门都已按模板参数化
grep -n 'for (template' tests/clash_mihomo.rs || grep -n 'ClientTemplate::Split' tests/clash_mihomo.rs
grep -n 'SING_BOX_VERSION_PROFILES.len() \* 3' tests/version_profiles.rs   # 5 内核 × 3 模板

# G4 引擎 + TUI 已落地：四条都期望"有输出"
grep -rn 'overrides/' crates/client-core/src | head -2
grep -n 'Override,' crates/sbtui/src/app.rs                    # 第 7 个页签
grep -n 'ClearOverride' crates/sbtui/src/input.rs              # O 清空（缺它时用户删不掉 sha256 命名的文件）
grep -n 'Page::Overrides' crates/sbgui/src/state.rs            # GUI 同页（2026-09-24 落地）

# G6 期望"无输出"
git grep -n tray-icon -- crates || echo 'G6: dep gone'

# G7 仍开放：看计数是否降到 0
git grep -c '\.note(' -- crates | awk -F: '{s+=$2} END {print "G7 note() calls:", s}'

# G9 仍是人工滑带：期望命中（表还在），配合 CI 带检查
grep -n 'SING_BOX_VERSION_PROFILES' src/subscription/profile.rs | head -1

# R19：拒绝文案分两计数（期望"0 行不受支持"与"2 行无法解析"同时出现）
grep -n '不受支持' crates/client-core/src/subscription.rs | head -2
# R20：minimal 的 Clash 工件不该再要 geo 数据库。别用 grep 判（源码里 GEOIP 仍出现在
# 注释与 standard 分支里）——用这条门本身：
cargo test -p sbctl --lib minimal_rule_profile_names_no_rule_cdn 2>&1 | tail -3   # 期望 ok

# G8/G15：Windows 平台行为，只有真跑过 verify.ps1 all 才有证据
ls .scratch/winvm 2>/dev/null || echo 'G8: no real-machine evidence collected yet'
# 跑之前必须先指认虚机：不设 WINVM_VMX 时期望**报错并列出两台候选**（脚本刻意不猜）
echo "$WINVM_VMX"; ls .scratch/win11-vm/../../.scratch/reshoot-override.sh 2>/dev/null
```

2026-09-24 的实测结果与逐条修复过程见 [code-review-2026-09-24.md](code-review-2026-09-24.md)。
