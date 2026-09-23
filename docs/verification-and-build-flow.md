# 构建与验证流程（合并后）

> 仓库状态：主分支 `main`，`sbctl` 版本 `0.2.0`。
> 关键提交：合并提交 `234c82f`（把 `refactor/structure` 并入主线）、整理提交 `b55beeb`（文档归档 + 修复 poll 测试调用点）。
> 备份：`%TEMP%\opencode\singbox-merge-backup\main-repo-all.bundle` 与 `clone-repo-all.bundle`（`git bundle verify` 通过）。

本流程把验证分成四条腿。一条变更算"已验证"，取决于它触及的面：渲染字节/轮询/生命周期跑 L1+L2，服务端边界与 systemd 跑 L3，GUI 像素跑 L3 的截图腿，GUI/TUN/系统代理等**平台行为**必须跑 L4。

| 腿 | 环境 | 能证明 | 不能证明 |
| --- | --- | --- | --- |
| L1 | Windows 宿主 | 编译、单测、CLI 集成、签名信任边界 | systemd、TUN、DPI、CJK 渲染 |
| L2 | WSL Ubuntu 22.04 | Linux 编译与全量测试（除 GUI） | systemd、真实内核字段接受度 |
| L3 | Docker（debian12 / ubuntu22.04 / ubuntu24.04） | systemd 安装/更新/回滚、socket 激活、订阅 HTTP、孤儿回收、TUN 布线、GUI 无头截图 | Windows 平台行为 |
| L4 | VMware Windows 11 guest | GUI/TUI 真机、TUN 提权、注册表系统代理、DPI、wintun | 生产发布门禁 |

---

## 1. L1 — Windows 宿主

```powershell
cargo fmt --all -- --check
python -m unittest discover -s scripts -p 'test_*.py'
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --features sbctl/test-signing
cargo test -p sbctl --no-default-features --test release_trust
```

- `--features sbctl/test-signing` 是**必需**的：`tests/cli` 声明了 `required-features`，不加会被静默跳过。
- `release_trust` 必须**不带** `test-signing` 运行，它证明生产构建拒绝开发签名密钥。

## 2. L2 — WSL Ubuntu 22.04

### 2.1 关键前置：cargo 不在非登录 PATH

WSL 里 `cargo` 位于 `~/.cargo/bin`，而 `bash script.sh`（非登录 shell）不会加载它，直接调用脚本会报 `cargo: command not found`。两种解法：

```powershell
# 方式 A：显式导出（推荐，避免登录 PATH 里的空格/括号）
wsl -d Ubuntu-22.04 -- bash -c 'export PATH="/home/ly/.cargo/bin:$PATH"; bash /mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me/scripts/dev/wsl-gate.sh'
```

`scripts/dev/wsl-gate.sh` 会先把 Windows 树 `tar` 同步到 WSL ext4（`~/ws/singbox-sub-me`，排除 `target*`/`.git`/`.reference-*`/`.scratch`/`dist`/`node_modules`/`.zcode`/`.kilo`/`.qoder`），再在 `CARGO_TARGET_DIR=~/ws/target` 下跑 fmt + clippy + 全量测试；默认 `--exclude sbgui`（GUI 需要 X11/fontconfig/vulkan 开发库，`INCLUDE_GUI=1` 才构建）。

> 复制到 ext4 再构建是为了避开 `/mnt/c` 的 IO 与 CRLF 开销。不要在 `/mnt/c` 上直接跑全量测试。

### 2.2 真实内核矩阵（`#[ignore]`，需要真实二进制）

```bash
# WSL 内，把 5 个 sing-box 放到 ~/bin 后
SING_BOX_BIN_1_10=~/bin/sing-box-1.10.7 \
SING_BOX_BIN_1_11=~/bin/sing-box-1.11.15 \
SING_BOX_BIN_1_12=~/bin/sing-box-1.12.25 \
SING_BOX_BIN_1_13=~/bin/sing-box-1.13.21 \
SING_BOX_BIN_1_14=~/bin/sing-box-1.14.1 \
SBCTL_UPSTREAM_LATEST=v1.14.1 \
  cargo test --test version_profiles -- --ignored --nocapture

MIHOMO_BIN=~/bin/mihomo cargo test --test clash_mihomo -- --ignored --nocapture
```

CI 固定版本：sing-box `1.10.7 / 1.11.15 / 1.12.25 / 1.13.21 / 1.14.1`，mihomo `v1.19.30`。

## 3. L3 — Docker 验收

### 3.1 在 WSL 构建三个 Linux release 工件

```bash
export PATH="/home/ly/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=/home/ly/ws/target
cd ~/ws/singbox-sub-me
cargo build --release -p sbctl --no-default-features
cargo build --release -p sbtui
CARGO_TARGET_DIR=/home/ly/ws/target-fixtures cargo build --release -p sbctl --features sbctl/test-signing
```

### 3.2 把工件复制到 Windows 可访问路径

Docker Desktop 的 WSL 集成**未对本发行版开启**（`docker` shim 会提示 "could not be found in this WSL 2 distro"），所以必须在 **Git Bash** 里跑 `run.sh`，并把工件放到 `/c/...` 路径。

```bash
# WSL 内复制到仓库下（target-linux-acceptance 命中 .gitignore 的 /target-linux*/）
mkdir -p /mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me/target-linux-acceptance
cp ~/ws/target/release/sbctl              /mnt/c/.../target-linux-acceptance/sbctl
cp ~/ws/target-fixtures/release/sbctl     /mnt/c/.../target-linux-acceptance/sbctl-test-signing
cp ~/ws/target/release/sbtui              /mnt/c/.../target-linux-acceptance/sbtui
```

### 3.3 在 Git Bash 运行

```bash
cd /c/Users/ranly/Documents/ChatGPT/singbox-sub-me
SBCTL_ARTIFACT=target-linux-acceptance/sbctl \
SBCTL_TEST_ARTIFACT=target-linux-acceptance/sbctl-test-signing \
SBCTUI_ARTIFACT=target-linux-acceptance/sbtui \
  sh tests/acceptance/run.sh
```

三个变量都是**强制**的。`run.sh` 会对 `debian:12-slim`、`ubuntu:22.04`、`ubuntu:24.04` 各构建镜像，以 `--privileged --cgroupns=host` + `/sys/fs/cgroup` 启动 systemd 容器，依次跑 `verify-bootstrap.sh` → `verify.sh`（限流/订阅/userinfo/统一 404）→ `verify-real.sh`（真实 systemd 三模式）→ `verify-client.sh`（孤儿回收 + TUN 布线）。

已验证输出要点：三发行版 `sbctl acceptance passed` / `real sbctl acceptance passed` / `client acceptance passed`。

### 3.4 GUI 无头截图（可选）

```bash
REPO='C:\Users\ranly\Documents\ChatGPT\singbox-sub-me' \
PAGES='dashboard,subscriptions,proxies,rules,connections,logs,settings,about' \
SIZES='860x640,1440x900' SBGUI_LANG=zh \
  bash scripts/sbgui-shot/shot.sh
```

## 4. L4 — VMware Windows 11 真机

- guest 账号 `Test`，口令**只从环境变量 `WINVM_PASS` 读取，绝不写入仓库/日志**。
- 目标入口 `scripts/winvm/verify.ps1`（`snapshot | revert | gui | tui | collect`）**尚未实现**；现有可复用资产在 `.scratch/win11-vm/`（`shot-guest.ps1`、`run-one.ps1`、`capture-vm.ps1`、`manifest*.txt`、`shots-win/*.png`）。
- 流程：`vmrun revertToSnapshot` 回到干净快照 → 启动 guest → `copyFileFromHostToGuest` 投递二进制与脚本 → `runProgramInGuest -interactive` 跑 GUI 8 页截图 / TUI 冒烟 → 取回 PNG 与日志 → 再次 `revert`。
- 注意：guest 内存建议 6–8GB；4GB 下连续起 7 个 sbgui 曾导致 proxies/connections 页崩溃退出，可分批 4 页。
- `packaging/windows/sbtui.wxs` 目前版本号与快捷方式目标有误（见 `.scratch/gui-completion/issues/03`），修好前 MSI 只能手工构建。

## 5. 已知坑

1. WSL 非登录 shell 找不到 cargo → 显式 `export PATH="$HOME/.cargo/bin:$PATH"`（注意引号，登录 PATH 含空格/括号会导致语法错误）。
2. Docker Desktop 的 WSL 集成未对该发行版开启 → L3 必须从 Git Bash 跑。
3. `/mnt/c` 上编译极慢且 CRLF 视图差异 → 先同步到 ext4 再构建。
4. `cargo clean` 会删掉 `target/` 与 `target-linux*`，下次构建是冷启动。
5. 未跟踪的 `.scratch/proto-flex/` 与 `prototypes/sbgui-progressive-workspace/.vite/` 是本地实验产物，**不要** `git clean -xfd`（会误删被忽略的资源）。

## 6. 本地重型资源位置

已从仓库迁出（均被 gitignore，不影响 git 历史）：

```
C:\Users\ranly\Documents\ChatGPT\singbox-sub-me-local\
├── references\   # 9 个 .reference-*（上游参考源码，含 .reference-sing-box-yg 约 377MB）
├── tmp\          # .tmp-sing-box-yg-research
├── dist\         # 旧的 dist/
└── bin\          # sbctl-linux-amd64
```

`target*` 已 `cargo clean`，仓库目录从约 12GB 降到约 225MB。
