# 结构重构与现状合并分析 · 2026-09-21 ~ 09-22

Status: resolved
Type: task

把 2026-09-19 的只读审查、本轮结构重构的认证结果、以及重构之后新暴露的问题合并到一份文档里，作为后续工单的唯一入口。逐条问题不在这里修，只在这里定坐标。

## 一、结构重构已完成

分支 `refactor/structure`，60 个提交，未推送。最后一次全量门禁（HEAD `a63b535`）：

| 门禁 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings`（Windows） | 0 |
| `cargo test --workspace --features sbctl/test-signing`（Windows） | 331 passed / 0 failed |
| 同测试（WSL Ubuntu） | 329 passed / 0 failed（本轮未重跑，见"三"） |

拆分结果（行数取拆分后模块总和，入口文件只留骨架）：

- `src/main.rs` 2997 → 94，其余进 `src/cli/`
- `src/subscription.rs` 3962 → `mod.rs` 40 + 8 个子模块，re-export 保持调用方零改动
- `tests/cli.rs` 5237 → `main.rs` 9 + 9 个用例文件 + `fixture.rs`
- `crates/sbgui/src/main.rs` 4212 → 201 + theme/state/app/overlay/chrome/components/pages×8/lang
- `crates/sbtui/src/lib.rs` 2105 → 143
- `config.rs::validate` 251 → 5 行分派 + 4 个校验段；`controller.rs::apply` 185 → 84 行
- 最长生产函数 321 → ≤200；GUI/TUI 重复的显示辅助函数收敛到 `client-core::format`

纯移动的证据是行多重集 diff；CLI 表面证据是 23 个 `--help` 输出逐字节一致。

## 二、2026-09-19 审查的六条发现，当前状态

原始记录见 `../project-review-20260919/report.md`。**本轮没有复核过任何一条**，下表只标注与重构是否有交集：

1. [P1] 生产验证公钥对应的签名私钥已在仓库公开 —— 与重构无关，属发布信任链，见 `../sbctl-release/`。
2. [P1] 以固定端口的响应判断本次子进程启动成功 —— `client-core/src/core.rs` 现在的注释是"Each launch gets a fresh local endpoint and secret"，且 `runtime_config` 会写入随机地址与密钥。**疑似已被覆盖，但需要按原始场景重验**，不能凭注释结案。
3. [P1] 不同订阅档案会覆盖或删除同一份缓存 —— 拆分成 `subscription/{profile,artifacts,…}` 后逻辑未改，状态不变。
4. [P2] 旧版订阅回退在实际导入路径中不会执行 —— 同上，纯移动，状态不变。
5. [P2] 崩溃重启上限和指数退避被短暂启动成功清零 —— 同上，状态不变。
6. [P2] UI 无法观察长操作的 busy/starting 状态 —— 部分相关：GUI 现在有 `starting` 参与空状态判断（`pages/connections.rs`），但共享命令通道的阻塞语义未改。

## 三、重构之后才看得见的问题

结构拆开后，页面第一次能被逐页截图验证，暴露出四类此前无法观察的缺陷。GUI 侧的逐条工单在 `../sbgui-progressive-workspace/issues/`，其中最高优先级是连接页刷新抖动。

方法论上的两条教训，记在这里以免重犯：

- `curl` 打到一个没监听的端口，经过代理时仍然 exit 0（代理会回一个错误响应）。判断"请求是否真的在跑"要看内核自己的日志，不能看客户端退出码。
- 容器里的 DNS 拿到的是宿主 `clash-verge`/`verge-mihomo` 的 fake-IP 段（`198.18.0.0/15`）时，Docker 出网会整体失败（apt 502、cargo `SSL_ERROR_SYSCALL`）。这是宿主代理状态，不是仓库问题。

## 四、环境遗留

- 仓库根目录有一个未跟踪的 `cache.db`：sing-box 的 `cache_file.path` 是相对路径时，会解析到进程的当前目录，而容器里那正是 `/src` 绑定挂载。`seed-demo.sh` 现在把路径钉进数据目录，不会再产生；旧文件可删。

## 五、2026-09-22 复核更正

合并前的分支审查引用本文时说"仍未结案的表述已经过期"。这句话不准确：第二节开头写的是**本轮没有复核过任何一条**，它记录的是"未验证"，不是"未修复"。审查方把它读成了后者，又用"代码里已经改了"替换了"验证过"。今天按原条目重查一遍，结论如下。

分支事实（今天量的，覆盖"一"里的快照数字）：HEAD `c220f48`，比本地 `master` 多 65 个提交、比 `origin/master` 多 68 个，**仍未推送**；`master` 是 `refactor/structure` 的祖先，`git merge --ff-only` 可用；工作区干净。门禁今天复跑：fmt 0、clippy 0、Rust 332 项通过。

| 条目 | 今天能拿出的证据 | 还不能说什么 |
| --- | --- | --- |
| 2 固定端口判成功 | `core.rs:37` 起每次启动取新端点与密钥；`core.rs:51` 在改配置之前先用 `TcpListener::bind` 探 mixed 端口；`wait_ready` 盯子进程而不是端口回声 | 全仓 `grep` 不到覆盖这条的测试（`tests/` 里没有 restart/endpoint 相关命名）。原场景（上一次残留内核占住固定端口）没被重放过 |
| 5 短暂成功清零重启计数 | `controller.rs:1160` 只在 `elapsed() >= STABLE_RUN`（60 秒）后清零；另一处清零在 `apply` 的 `StartCore` 分支（`:456`），且被 `!core_running && !starting` 拦住，属用户显式启动 | 同样没有测试。`crates/client-core` 里凡提到 `restart_attempts` 的只有实现文件本身 |
| 3 缓存互相覆盖 / 4 旧订阅回退 / 6 长操作阻塞 | 今天没有复核 | 维持"未验证"，不要写成已修 |
| 1 生产签名私钥 | 未变，属发布信任链，只有维护者能做 | v0.1.26 仍是开发密钥签名，见 `../sbctl-release/` |

所以第二节那张表的"疑似已被覆盖"两行，今天的判定是：**代码路径确实换了，但回归测试这一半仍然缺**。要结案就得补能红的用例，不是补注释。

另外，GUI 侧的工单已从 6 条增到 7 条（`../sbgui-progressive-workspace/issues/`），其中 03、06 已按维护者裁决结案，07 是原型去 CSS Grid 之后留下的两栏分配偏差。
