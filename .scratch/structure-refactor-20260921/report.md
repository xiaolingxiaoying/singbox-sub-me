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
