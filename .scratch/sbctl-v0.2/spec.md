# sbctl v0.2：发布阻断修复 + 目标功能补齐

Status: ready-for-agent

日期：2026-09-23
来源：对 `refactor/structure`（HEAD `a5fe586`）的全量代码分析 + 目标文档《sing-box sub project 计划》+ `docs/ubuntu-22-vps-e2e-report-2026-09-22.md` 的 No-Go 结论。

## 目标

把当前代码推进到可发布、可部署、且覆盖目标文档三项产品要求的 v0.2：

1. 服务端 `sbctl`：修复阻断发布的 P0；补齐「节点分享链接展示」「mihomo 嗅探模板」「模板可配置度」「最新稳定内核一致性」四项差距。
2. TUI `sbtui`：补齐「覆写配置文件内容」与「显示入站规则」两个硬缺口，并给出 Windows/Linux 真机证据。
3. GUI `sbgui`：功能对齐 TUI，修掉已证实的 UI Bug（P0 两个、P1 若干），纳入发布矩阵与 CI 门禁。

已确认的口径（2026-09-23）：

- 优先级：先修 P0 发布阻断，再补功能，最后扩发布矩阵。
- sing-box 版本口径：最新稳定 minor + 之前 4 个 = 5 个（与现状 1.10–1.14 一致）；不新增 1.9。
- GUI 纳入 Release 资产；Windows 主要交付 `sbgui.exe` + `sbtui.exe` + `ly.exe`。
- 生产发布密钥由维护者配置，步骤见工单 04。

## 基线事实（代码证据）

- 当前分支 `refactor/structure` 领先 `origin/master` 73 个提交，未推送；`master` 为其祖先，可 fast-forward。
- Rust 门禁最近一次全绿：fmt 0、clippy 0、`cargo test --workspace --features sbctl/test-signing` 331 项通过（结构重构报告）。
- 订阅矩阵：9 条路由、sing-box 1.10–1.14 profile、mihomo 两版、Shadowrocket、URI/index/QR 已实现；真核 CI 覆盖 sing-box 1.10.7/1.11.15/1.12.25/1.13.21/1.14.1 与 mihomo 1.19.30。
- 发布阻断（VPS 报告 §7）：独立 sing-box 更新错误提交快速崩溃候选且不回滚（P0）；`sbctl.service` 的 `Sockets=` 位于错误 section（P1）；生产签名未配置（发布配置阻断）。
- 客户端：`crates/client-core` 是唯一控制面；TUI 无配置覆写与入站视图；GUI 有 7 张开放工单（`.scratch/sbgui-progressive-workspace/issues/`）。

## 阶段与工单索引

| 阶段 | 工单 | 说明 | 阻塞 |
| --- | --- | --- | --- |
| 0 基线 | `issues/01-branch-merge-and-version-baseline.md` | 合并 master、版本 0.2.0、推送、CI 绿 | — |
| 1 P0 | `issues/02-singbox-update-stable-health-rollback.md` | S1：稳定健康观察 + 自动回滚 + 回归 | 01 |
| 1 P0 | `issues/03-systemd-unit-sockets-section.md` | S2：unit 无效配置 | 01 |
| 1 P0 | `issues/04-production-release-keys.md` | S3：生产公钥/签名 secret（维护者） | — |
| 1 P0 | `issues/05-acceptance-singbox-rollback-regression.md` | S4：验收脚本补故障回滚与旧断言 | 02 |
| 1 P0 | `.scratch/sbgui-progressive-workspace/issues/01-connections-poll-flake.md` | G1：连接页轮询停摆 | 01 |
| 1 P0 | `.scratch/sbgui-progressive-workspace/issues/05-connections-columns-truncation.md` | G4：连接列截断（并入 G1 轮次） | 01 |
| 2 服务端 | `issues/06-node-share-links.md` | S7：节点分享链接展示 | 01 |
| 2 服务端 | `issues/07-mihomo-sniffer-template.md` | S8：mihomo sniffer | 01 |
| 2 服务端 | `issues/08-client-template-configurability.md` | S9：策略组/规则集/DNS/TUN 可配置 | 01 |
| 2 服务端 | `issues/09-dynamic-singbox-version-profiles.md` | S10：profile 动态化 | 01 |
| 2 服务端 | `issues/10-official-core-download-verification.md` | S5/S6：内核版本一致性与下载校验 | 01 |
| 2 服务端 | `issues/11-subscription-userinfo-total-semantics.md` | S11：`total` 语义定案 | 01 |
| 3 客户端核心 | `.scratch/client-config-override/spec.md` | T1：覆写配置文件（core + TUI） | 01 |
| 3 客户端核心 | `.scratch/client-inbound-view/spec.md` | T2：入站规则视图 | 01 |
| 3 客户端核心 | `.scratch/sbgui-progressive-workspace/issues/02-engine-text-english.md` | G5：事件码 + 英文渲染 | 01 |
| 4 TUI | `.scratch/client-config-override/issues/02-tui-override-ui.md` | T1 的 TUI 部分 | T1-core |
| 4 TUI | `.scratch/verification-environments/issues/03-windows-vm-pipeline.md` | T5：Windows 真机证据 | 01 |
| 5 GUI | `.scratch/gui-completion/spec.md` | G6/G7/G8 | 03, G1 |
| 6 发布 | `issues/12-reacceptance-and-release.md` | 发布与 VPS 重验 | 02–05 |

## 三环境测试与构建流程（强制边界）

### 总原则

- Windows 宿主机只做：编辑、代码生成、`cargo check/test`（进程级、TempDir fixture，不写系统状态）。
- WSL Ubuntu-22.04 做：Linux 全量 Rust 测试、clippy、fmt；源码先 `tar` 同步进 WSL 文件系统再编译（禁止在 `/mnt/c` 上跑全量构建）。
- Docker（Docker Desktop）做：服务端三发行版 systemd 验收、GUI 无头截图、Linux 发布二进制构建。
- VMware `Win11-sbtui-test` 做：Windows 真机 TUI/GUI 运行、TUN、系统代理验证；结束后 revert 快照。
- 真实 Ubuntu VPS 只做：signed release 工件终验（见工单 12）。
- 任何凭据（VM 口令、VPS 口令、签名密钥）只允许经环境变量传入，不写入仓库、日志与工单。

### WSL Ubuntu 流程

```bash
# 一次性：确认真机 WSL 有 rust stable；GUI 依赖只在需要时安装（WSL 内变更）
wsl -d Ubuntu-22.04 -u root -- apt-get install -y libfontconfig1-dev libfreetype6-dev \
  libx11-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libgl1-mesa-dev libegl1-mesa-dev libvulkan-dev libasound2-dev

# 每次：同步 + 门禁（脚本细节见 .scratch/verification-environments/issues/01）
wsl -d Ubuntu-22.04 -u root -- bash /mnt/c/.../scripts/dev/wsl-gate.sh
```

`scripts/dev/wsl-gate.sh` 契约：`tar` 复制（排除 `target*`、`.git`、`.reference-*`、`.scratch`、`dist`）到 `~/ws/singbox-sub-me`，`CARGO_TARGET_DIR=$HOME/ws/target`，依次跑
`cargo fmt --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --features sbctl/test-signing`。

### Docker 流程

服务端验收（三发行版真实 systemd，需 `--privileged` + cgroup）：

```bash
cargo build --release -p sbctl --features test-signing --target-dir target-fixtures
SBCTL_ARTIFACT=<linux release sbctl> SBCTL_TEST_ARTIFACT=target-fixtures/release/sbctl \
  sh tests/acceptance/run.sh
```

GUI 无头截图（Git Bash + Docker Desktop）：

```bash
REPO='C:\...\singbox-sub-me' PAGES='dashboard,...,about' SIZES='860x640,1440x900' \
  SBGUI_LANG=zh DEMO_CORE=/src/.scratch/sbgui-demo-bin/sing-box \
  bash scripts/sbgui-shot/shot.sh
```

### VMware Windows 11 流程

`scripts/winvm/verify.ps1`（新增，契约见 `.scratch/verification-environments/issues/03`）：
revert 快照 → 启动 → `copyFileFromHostToGuest` 投递二进制与脚本 → `-interactive runProgramInGuest` 跑 GUI 8 页截图与 TUI 冒烟 → 取回 `manifest.txt`/PNG/日志 → revert。
宿主机只调用 `vmrun`，不安装、不注册任何服务。

## 验证矩阵

| 层级 | WSL | Docker | Win11 VM | 真实 VPS |
| --- | :-: | :-: | :-: | :-: |
| fmt / clippy / workspace 单测 | 主力 | — | `--lib` | — |
| CLI 集成（含回滚故障注入） | 主力 | — | — | — |
| 三发行版 systemd 验收 | 引擎 | 主力 | — | 终验 |
| sbgui 无头截图 | — | 主力 | 真机 | — |
| sbtui 真机 / TUN / 系统代理 | 冒烟 | — | 主力 | — |
| signed release + ACME + 五协议公网 | — | — | — | 主力 |

## 发布门禁（全部满足才可打 tag）

1. 待发布 commit：WSL 门禁全绿 + Windows `--lib` + Docker 三发行版验收 + GUI 截图冒烟。
2. `docs/ubuntu-22-vps-e2e-report-2026-09-22.md` §10 的 1–9 全部完成。
3. Release workflow 产出同 tag 的 amd64/arm64 sbctl、sing-box、sbtui/sbgui、`install.sh`、签名 manifest。
4. 干净 VPS 复验：坏候选 `check=0/run=1` → 更新失败 → 自动恢复旧二进制 → 服务稳定 active → NRestarts 不增长 → 订阅与五协议恢复。

## 非目标

- 不引入 WARP/Argo/Psiphon 等供应商特性（ADR-0018）。
- 不把容器作为生产部署目标。
- 不重写 GUI 框架；不新增第三套 UI。
