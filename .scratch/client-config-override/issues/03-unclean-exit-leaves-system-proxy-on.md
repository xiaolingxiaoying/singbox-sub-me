# 非正常退出会把系统代理留在开启态（Linux/macOS 与 Windows 都中）

Status: needs-implementation
Type: bug
Found: 2026-09-24（R25 的 TUI 能力审计），行号以当日 HEAD 为准
Blocked by: none

## 现象

`sbtui` 与 `client-core` 里**没有任何信号处理**：全库 `SIGTERM|SIGHUP|signal::|ctrl_c` 只命中
`crates/client-core/src/core.rs:377` 的 `prctl(PR_SET_PDEATHSIG, SIGTERM)`——那是装在**子进程**
sing-box 上的，作用是"父进程死了内核跟着死"，它不触发客户端自己的清理。

清理路径只有正常退出那一条：`crates/sbtui/src/lib.rs:129-142`（停内核 → 关系统代理 → 还原）。
于是 `kill <pid>`、`systemctl stop`、关终端标签（SIGHUP）、注销会话、WSL 关机
都会**跳过**它。后果不是"进程没退干净"这种洁癖问题：

- 系统代理仍指向 `127.0.0.1:<mixed 端口>`，而那个端口上已经没有监听者 → **用户断网**，
  且不知道原因。恢复要么手动改注册表/GNOME 设置，要么再开一次 TUI 正常退出。
- 内核这一半反而没事（PDEATHSIG 带它走），所以现象是"进程都不在了，代理还开着"，最难猜。

## 要做的

1. 装一个信号/控制台事件处理，把 SIGTERM、SIGHUP、（Windows）CTRL_CLOSE_EVENT / CTRL_C_EVENT
   收敛到**与 `q` 相同的清理路径**，而不是另写一份"简化清理"。
   注意 crossterm 的读取循环是阻塞的：需要一个能被唤醒的出口（self-pipe / 原子标志 +
   已有的 tick 间隔 / `mio` 事件），不要指望"下一次按键"来发现信号。
2. 清理必须**幂等**且**有界**：信号可能连着来（第二次应当直接退，不再等子进程），
   子进程等待要有超时，超时也要先把代理关掉再退。
3. 退出码要能区分"用户按 q"与"被信号打断"（日志里留一行原因），方便复现。

## 验收

- 一条**能在 CI 里跑**的回归测试：起 TUI（或它的清理函数所在的层）→ 发 SIGTERM →
  断言代理状态被还原。当前 `system_proxy` 的测试全是纯函数级（`system_proxy.rs:569` 那条还是
  `#[cfg(unix)]`），所以先决定测在哪一层：**测真信号路径**，不要只测"清理函数本身能被调用"。
- Windows 侧至少要有 `ctrl_c` 的等价测试或明确写清为什么只能真机验（并挂到 G8 的 VM 清单里）。
- 文档：`docs/client-description.md` 里关于退出/代理恢复的说法，要么变成有证据的承诺，要么改措辞。

## 相关但不在本工单

- `orphan_guard.rs` 证明的是内核回收，不是代理恢复（`#![cfg(target_os = "linux")]`）。
- 非 GNOME 的 Linux 代理写入本来就只能提示用户设 env（`system_proxy.rs:432`），
  那种环境下"残留代理"不成立，但也不该靠这条来免除本工单。

## 2026-09-24 落地情况（TUI 半边已修）

`fix(tui)` 4bf4764：`run_app` 的 `tokio::select!` 多一条分支，循环前订阅一次
SIGTERM/SIGHUP，收到就 `break` —— 与被 `q` 打断走的是**同一个**函数出口，清理逻辑没有第二份。
`SIGINT` 故意不接（raw 模式下 Ctrl+C 是按键，接了会把"UI 卡死时唯一出口"堵掉）。
判据抽成 `should_clear_proxy(enabled, kept_on_purpose)` 纯函数，两次变异都判红。

门：`scripts/dev/wsl-signal-exit.sh`，在 Linux 下用 pty 起真 sbtui 再发信号。
**它第一版是假绿**：`pgrep -f "script -qeefc $BIN"` 匹配到 pty 包装器，而 `script` 自己会
接住 SIGTERM 返回 0；是控制组（同一目标发 `SIGKILL` 得到 137）暴露的。
修好后按进程名匹配子进程、打印 `wrapper=` 与 `sbtui=` 两个 pid、两者相同拒跑、轮询子进程消失。
Linux 实测：控制组 137、SIGTERM 0、SIGHUP 0 三例全过；`cargo test -p sbtui` 在
Windows 与 WSL 都是 48 passed。

## 还剩两半（本工单不闭）

1. **GUI 同型缺口，已确认存在**：`system_proxy::disable` 在 sbgui 里只有一处调用
   （`app.rs:467` 的 `choose_exit`），而它只由退出浮层的按钮触发（`overlay.rs:107/:114`）。
   所以任务管理器"结束任务"、注销、关机这三条路都不会恢复代理——与 TUI 修掉的那个是同一个病。
   GUI 没有控制台，`tokio::signal` 那套在这里不适用；要做的是 Win32 侧接
   `WM_ENDSESSION` / `WM_QUERYENDSESSION`（GPUI 的窗口过程不在本 crate 里，需要子类化或
   等上游提供钩子）。**验收必须真机**：VM 里开代理后用 `taskkill /F` 与注销两条路各测一次。
2. **清理要有界且幂等**：现在的实现里子进程回收靠 `PR_SET_PDEATHSIG`，代理恢复是同步一次调用；
   若将来在清理里加等待，必须带超时，且第二次信号要能直接退（不要出现"卡在恢复代理上"）。
