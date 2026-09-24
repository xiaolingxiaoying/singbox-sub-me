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

**先装前置**，否则这一腿会以一种很像产品缺陷的方式失败：`tests/version_profiles.rs` 自 Phase 0.5
起断言 `checked == SING_BOX_VERSION_PROFILES.len()`，缺任何一个内核时报的是
`subscription artifact is unavailable: No such file or directory`，而不是"二进制缺失"。

```bash
# 在 Git Bash 里按路径调用（见 §5 的两条引号陷阱）
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-22.04 -- \
  bash /mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me/scripts/dev/fetch-sing-box-cores.sh
MSYS_NO_PATHCONV=1 wsl -d Ubuntu-22.04 -- \
  bash /mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me/scripts/dev/wsl-real-cores.sh
```

`fetch-sing-box-cores.sh` 把 5 个固定版本装进 `~/bin` 并写 `~/bin/sb-cores.env`；
它**优先从宿主暂存目录取**（`/mnt/c/Users/ranly/tmp-sbcores`），因为这台机器的 WSL 到 GitHub
不通、而宿主通（实测宿主 302、WSL 连接超时）。`wsl-real-cores.sh` 会先按 `wsl-gate.sh` 的方式
把树同步进 ext4，再跑 version_profiles 与 mihomo 两道真核门。

手工等价物（变量含义照旧）：

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

`clash_mihomo` 现在覆盖 3 模板 × 2 规则档 × 2 工件 = 12 次真核运行；`version_profiles` 覆盖
5 个内核 × 3 模板 = 15 种组合。两条腿都不再只跑默认模板（R13 之前的"这一腿证不了什么"已作废）。

`minimal` 那 6 种组合已不依赖外网：工件里没有 `GEOIP`/`GEOSITE`，`mihomo -t` 0 秒返回。
`standard` 那 6 种**今天在网络不通的情况下也通过了**，这与本文件早前"网络不通会报
`context deadline exceeded`"的记录相矛盾，尚未解释：用一个只含 `GEOIP,CN` 的裸配置在同一个
mihomo 上复测，确实卡在 `Can't find MMDB, start download` 并超时（45 秒被 kill），
而换成内联 `IP-CIDR` 立刻成功。也就是说"下载 geo 库"这条路径真实存在，只是它在工件门上
没有阻塞 `-t` 的退出。结论要分开看：`minimal` 的离线性是**用裸配置 A/B 证出来的**，不是这条门证出来的；
这条门只回答"工件能否被真核解析"。排查线索：`.scratch/probe-mihomo-geo.sh`（裸配置对照）
与 `.scratch/probe-mihomo-gate.sh`（门的计时）。

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
- 入口**已实现**：`scripts/winvm/verify.ps1`，子命令 `parse-check | snapshot | revert | gui | tui | collect | all`；
  历史资产留在 `.scratch/win11-vm/`，正式版本已进 `scripts/winvm/`（`verify.ps1`、`shot-guest.ps1`、`tui-guest.ps1`）。

```powershell
$env:WINVM_PASS = '<客户机口令>'          # 只进环境变量，绝不写进仓库或日志
cargo build --release -p sbgui -p sbtui   # verify.ps1 投递的就是这两个 exe
powershell -File scripts/winvm/verify.ps1 parse-check   # 不需要虚拟机：证明三个脚本都能解析
powershell -File scripts/winvm/verify.ps1 snapshot      # 一次性建立 clean-base
powershell -File scripts/winvm/verify.ps1 all           # revert -> 8 页 GUI -> TUI/注册表 -> 取证据 -> revert
```

- VM 定位四级回退：`-VmPath` → `$env:WINVM_VMX` → `vmrun list` → 常见目录搜索；**搜到多个候选时直接报错**，不猜。
  2026-09-24 实测修正了这条链上的两个真 bug：旧搜索根漏了 `%USERPROFILE%\Documents\Virtual Machines`
  （本机 VM 全在这里，于是自动发现永远找不到），而 `-Filter '*Win11*.vmx'` 既漏掉名字叫
  `Windows 11 x64` 的那台（模式里没有 `Win11` 子串）、又把 `*.vmxf` 组队侧车当成交互机。
  所以本机跑任何一条腿都得先指认：
  `$env:WINVM_VMX = "$env:USERPROFILE\Documents\Virtual Machines\Windows 11 x64\Windows 11 x64.vmx"`；
  同目录另一台 `Win11-sbtui-test` 只能走 VNC，交给这条流水线只会得到"超时"式的假红。
- `Finalize` 对比运行前后的**宿主指纹**（HKCU 代理三键、`netsh winhttp show proxy`、
  `sbctl/sing-box/sbgui/sbtui` 服务状态、PATH 条目数），有漂移就写 `host-drift.txt`。
- 证据落在 `.scratch/winvm/<时间戳>/`（已 gitignore）：`gui/*.png`、`gui-manifest.txt`、`tui.txt`、`verify.log`。
- 流程：`revertToSnapshot` 回到干净快照 → 启动 guest（`checkToolsRunningStatus` 轮询就绪）→
  `copyFileFromHostToGuest` 投递二进制与脚本 → `runProgramInGuest -interactive` 跑
  GUI 8 页截图与 TUI/注册表探测 → 取回 PNG 与报告 → 检查残留进程 → 再次 `revert`。
- `parse-check` 自身做过变异检验（塞进一个语法错的脚本 → 报 PARSE 并退出 1）。
  **但 `all` 这条腿还没真跑过一次**：Windows 平台行为（DPI、DWM、真实注册表、wintun 提权、
  Job Object 回收）目前仍是"未验证"，不是"已通过"。
- 注意：guest 内存建议 6–8GB；4GB 下连续起 7 个 sbgui 曾导致 proxies/connections 页崩溃退出，可分批 4 页。
- `packaging/windows/sbtui.wxs` 的版本号与快捷方式目标已修正（`sbgui`/`sbtui` 各一条），
  但 **MSI 从未在任何 runner 上构建过**：新加的 `build-msi` job 是"写好了、可能红"。
  本机也没装 WiX（装它属于改动宿主环境，未做），所以"CI 能产出可安装 MSI"这句话目前**没有证据**。

## 5. 已知坑

1. WSL 非登录 shell 找不到 cargo → 显式 `export PATH="$HOME/.cargo/bin:$PATH"`（注意引号，登录 PATH 含空格/括号会导致语法错误）。
2. Docker Desktop 的 WSL 集成未对该发行版开启 → L3 必须从 Git Bash 跑。
3. `/mnt/c` 上编译极慢且 CRLF 视图差异 → 先同步到 ext4 再构建。
4. `cargo clean` 会删掉 `target/` 与 `target-linux*`，下次构建是冷启动。
5. 未跟踪的 `.scratch/proto-flex/` 与 `prototypes/sbgui-progressive-workspace/.vite/` 是本地实验产物，**不要** `git clean -xfd`（会误删被忽略的资源）。
6. **管道的退出码不是命令的退出码**：`cargo test 2>&1 | tail -40; echo $?` 报的是 `tail` 的状态，
   `cargo clippy | grep error` 报的是 `grep` 的。本会话两次把红门读成绿门都是这个原因。
   正确做法：把输出重定向到文件、单独取 `$?`，或者 `set -o pipefail`；
   并且报告"绿"的时候要给出可核对的计数（如 `404 行 ok / 0 失败`），不要只报退出码。
7. **从宿主 Git Bash 调 `wsl -- bash -c "…"` 会先被外层展开**：`$v`、`$f` 变成空串
   （我第一次抓内核就下成了 `sing-box--linux-amd64.tar.gz`），且 `/mnt/c/...` 会被 MSYS 改写成
   `C:/Program Files/Git/mnt/c/...`。规则：**多变量逻辑写成脚本文件，按路径调用并加 `MSYS_NO_PATHCONV=1`**。
8. **WSL 到 GitHub 不通、宿主通**（实测宿主 302、WSL connect timeout，与宿主代理把 DNS 指进
   `198.18.0.0/15` 一致）。凡是依赖外网的腿（`fetch` 内核、`mihomo -t` 下 `geoip.metadb`、
   Docker 镜像里的 `apt-get`）都可能因此假红；先判"是不是网络"，判据是**工件字节与金标准是否一致**，
   不是"我觉得像网络"。
9. `#[cfg(unix)]` 的代码在宿主 L1 上**根本不参与编译**——本轮新增的 tun 设备探测就是这样在 L2 才被抓到
   （`is_char_device` 属于 `std::os::unix::fs::FileTypeExt`）。凡是 unix 门，必须至少跑过一次 L2。
10. **`VAR=x wsl -d … -- bash script.sh` 不会把 `VAR` 带进 Linux 侧**：实测 `SRC_REV=HEAD …` 到了 WSL 里是
   `unset`，脚本于是静默走回"同步工作树"分支，构建出的是**另一个 agent 正在编辑的半截代码**（报
   `Tab::Override not covered`），而工件时间线看起来完全正常。规则：开关型变量写进 `bash -c 'VAR=x bash 路径'`
   的内层，并且让脚本自己打印它选中了哪条分支（`build-acceptance-artifacts.sh` 打印
   `exported revision <rev> into <dir>`），核对那行而不是核对"没报错"。

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

## 2026-09-24 晚上的 L3 复跑（含 R31 那次服务端改动）

`SRC_REV=HEAD` 从**已提交**的树导出三个 Linux 产物（`scripts/dev/build-acceptance-artifacts.sh`），
再跑 `tests/acceptance/run.sh`：**12/12 `acceptance passed`，退出码 0**
（debian:12-slim / ubuntu:22.04 / ubuntu:24.04 × verify-bootstrap / verify / verify-real / verify-client）。
这一轮必须重跑的理由写在 R31：金标准测试原来会读主机有没有 IPv6 路由，
而我把那个探测改成了参数 + fixture 显式钉住 `ipv4_only`——生成逻辑的**输出形态**因此可能在
"有 IPv6 的机器"与"没有的机器"之间不同，只有真容器矩阵能证明两端都对。矩阵里跑的正是无 IPv6 的容器。

同日 `cargo test --workspace`：**426 passed / 0 failed**，`clippy --workspace --all-targets
-D warnings` 与 `fmt --all --check` 退出码 0。这条门是今天才发现的漏洞：
G11/G12/G13 全在 `crates/*`，服务端一行没动，却正好落在**唯一一条没人跑的服务端门**的盲区里
——单 crate 的门不等于工作区的门。
