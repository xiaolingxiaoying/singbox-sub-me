# 连接页 0 行：不是偶发，也不是抓图竞态——轮询在启动后不久就停了

Status: resolved
Type: task

## 根因（2026-09-23 结案）

`poll()` 在 `.await` **之前**就推进了三个刷新槽位的时间戳（`controller.rs:1012`、`:1016`、`:1024`），
而驱动 `poll` 的是 `controller.rs:283-286` 的内层 `tokio::select!`：

```rust
tokio::select! {
    command = command_rx.recv() => command,
    _ = self.poll() => { self.publish(&shared); continue; }
}
```

命令一到达就赢过 `poll()`，其 future 被丢弃——**时间戳已经前移，数据一条没取**，该槽位要等满
`CONNECTIONS_EVERY`（2s）/ `TRAFFIC_EVERY`（1s）/ `PROXIES_EVERY`（3s）才会再被尝试。连接、流量、
内存由同一个 `poll()` 喂，所以三者一起停在零，而内核完全健康。这解释了"多等反而必然复现"。

**"下一步"里的第 2 条猜错了方向**：`watch_core_exit()`（`:1138`）返回 true 只在子进程真的退出时
发生，且那条路径会 `schedule_restart()`，不是稳态冻结源。真正的原因是取消，不是早退。

## 修复

每个槽位改为**请求跑完之后**打时间戳，且打的是完成时刻而非本轮开始时刻（慢内核下不会立刻重复
请求）。配套把 `auto_update_due()` 改成无副作用的纯谓词——它是这条路径里最长的请求，最容易在半
途被丢弃，原来却在 `due` 判定里就把 `last_auto_update` 写掉了。

新增两条 `client-core` 测试：
- `a_cancelled_poll_does_not_consume_the_connections_slot`：慢速 clash_api 桩供 3 条连接，把
  `poll()` 驱动到请求在途时丢弃，再跑一次，断言仍取到 3 条。**修复前红**（`active_connections: 0`），
  修复后绿。
- `a_failed_refresh_still_consumes_its_slot`：守护"延后打戳不要把不可达内核变成每 tick 重试"。

## 证据（L4 Xvfb harness，同一镜像同一种子）

`DEMO_CORE=/src/.scratch/sbgui-demo-bin/sing-box PAGES=connections`，sing-box 1.14.1：

| 轮次 | 代码 | harness 探针 | 截图 |
|---|---|---|---|
| `.scratch/sbgui-issue01-baseline/` | 无修复 | `api rows: 3` | **0 条活动连接**、侧栏徽标 0、下载 0 B/s 上传 0 B/s |
| `.scratch/sbgui-issue01-r1/` | 有修复 | `api rows: 3` | **3 行**、徽标 3、192.3 KiB/s / 237 B/s |
| `.scratch/sbgui-issue01-r3/` | 有修复 | `api rows: 3` | **3 行**、徽标 3、流量在动 |
| `.scratch/sbgui-issue01-r4/` | 有修复 | `api rows: 3` | **3 行**、徽标 3、192.3 KiB/s / 237 B/s |

四轮探针读数完全相同（`api first nonzero row at +1s`、`api rows: 3`），唯一差异是
`controller.rs` 的修复——A/B 用 `git stash push -- crates/client-core/src/controller.rs` 做
单文件回退，因果确定。**修复后 3/3 渲染出行，结案标准满足。**

`-r2` 那一轮目录为空：`shot.sh` 在容器内 `cargo` 拉依赖时遇到
`spurious network error (3 tries remaining): SSL connect error`（同一警告也出现在成功的轮次里，
但没有成功的轮次靠缓存活了下来）。这是 harness 的网络脆弱性，不是产品缺陷；单独记一条待办：
`scripts/sbgui-shot` 应为该失败留下非空日志并以明确信息退出，而不是静默产出空目录。

## 遗留（harness 自身，不阻塞结案）

`inside.sh:90` 在 curl 子进程尚未写出结果时报 `/tmp/curl.status: No such file or directory`，
导致 `curl:` 这一行打印为空。本轮靠内核日志里的 `outbound/direct` 记录确认请求确实到达代理，
但探针本身该修：先 `: > /tmp/curl.status` 建文件，或在读取前等待子进程结束。

## 原始现象（2026-09-22 上午）

`scripts/sbgui-shot/` 用相同脚本、相同种子、相同镜像跑两次：捕获瞬间 harness 打印的 `api rows:` 都是 `3`，内核日志也确实记录了这三条 `outbound/direct`。但一次截图渲染出 3 行，另一次渲染出"暂无活动连接"，侧栏徽标同步 0。当时记为"偶发"，并猜测是刷新路径静默清空。

## 两个猜测都被证伪

**猜测 A：漏一次轮询会静默显示空态。** 不成立。`controller.rs:1073` 的 `refresh_connections` 在 `Err` 分支只 `note("刷新连接失败: …")`，**保留旧快照**；整表清空只发生在 `stop_core`（`controller.rs:721`）和 `schedule_restart`（`controller.rs:1174`）两处，两者都会把 `core_running` 置假。而截图里内核是"运行中"。

**猜测 B：抓图与 2 秒轮询的时序竞态。** 不成立。给 harness 加了探针之后（`inside.sh`：请求发出后每秒探一次 api，记录首次非零的时刻，再额外等 5 秒 = 至少两个轮询周期，才 `import`），连跑三轮，打印完全相同：

```
api first nonzero row at +1s after the requests were fired
api rows: 3 [('127.0.0.1','8099'), ('127.0.0.1','8099'), ('127.0.0.1','8098')]
```

三帧截图逐张看过，**3/3 都是 0 行**（`.scratch/sbgui-race1/`、`-race2/`、`-race3/`，各 `1440x900-connections.png`）。连接在 +1 秒就已存在，界面有整整 5 秒、至少两次轮询的机会，仍然没有拿到。**这不是偶发，是确定性复现，而且多等反而必然复现。**

## 新线索

- 页头状态行是"内核已启动"，**不是**"刷新连接失败"——说明这段时间里 `refresh_connections` 要么没被调用，要么调用成功且返回 0 条（成功返回 0 不会写 note）。
- 同一帧里 `下载 0 B/s 上传 0 B/s`、侧栏徽标 0、页头"代理 0 · 直连 0"。流量、内存、连接是同一个 `poll()` 函数喂的（`controller.rs:1000`）。三个都停在零，指向**整个轮询循环在启动后不久就不再推进**，而不是连接这一条路径单独坏了。
- 上午那次能拍到 3 行，是在更短的等待窗口里——与"循环跑了几轮之后停掉"一致。

## 下一步（按顺序做，别跳）

1. 先固定证据：在 `poll()` 入口加一次性计数（或 `tracing`），跑同一个 harness 场景，打印每 2 秒是否真的进过一次 `poll`、`core_running` 当时是什么、`api.connections()` 的返回条数。**先确认是"没调用"还是"调用后拿到 0"**，这两者的修法完全不同。
2. 若是"没调用"，重点看 `poll()` 开头的早退路径：`watch_core_exit()`（`controller.rs:1138`）返回 true 时会 `cancel_operation().await; return;`，直接跳过本轮所有刷新；以及调度 `poll` 的那个定时器是不是只在某个 action 的回调里被重新登记（一次早退就可能永久断链）。
3. 若是"调用后拿到 0"，比较 harness 的 python 探针与 `clash_api.rs:343` 的请求差异（端点、密钥、`error_for_status`、base 地址来源），注意 `active-config.json` 每轮都是新生成的随机端点与密钥。
4. 修好之后用现在这套 harness 连跑 3 轮，要求 3/3 都渲染出行，才算结案；单张好图不算。

## 影响

用户在内核确实在跑的时候打开连接页，看到的是"暂无活动连接"。同一条路径还喂概览页的流量数字与内存读数，以及日志页的实时性——如果确认是轮询停摆，这三页一起受影响。
