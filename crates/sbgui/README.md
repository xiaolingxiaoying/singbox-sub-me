# sbgui — 桌面 sing-box 客户端

`sbgui` 是基于 GPUI 的 Windows 桌面客户端，与终端客户端 `sbtui` 共用同一个控制面
（`crates/client-core`）。窗口本身只做两件事：渲染引擎发布的 `ClientSnapshot`，以及
把用户操作转换成 `ClientCommand`。因此两个客户端在订阅、节点、系统代理、TUN、流量、
连接与日志上的行为完全一致，不会各自漂移。

## 构建与运行

```bash
cargo build --release -p sbgui
# 产出 target/release/sbgui.exe
```

首次运行会在 `%APPDATA%\sbgui` 下创建 `settings.toml`、`profiles.toml`、`cache/` 与
`core/`。数据目录与 `sbtui` 分开，避免两个客户端争抢同一个 sing-box 进程。

## 页面

| 页面 | 内容 |
| --- | --- |
| 概览 | 内核/系统代理/当前节点/活动连接四张指标卡；语义网络路径；带 sparkline 的实时上下行流量；快速操作 |
| 节点 | 代理组切换、组内节点列表、当前节点标记、单节点测延迟、整组并发测延迟（超时显示为「超时」） |
| 连接 | 目标、主机、网络、命中规则、上传/下载、断开单条或全部连接 |
| 日志 | sing-box 内核日志（按级别着色）与运行事件流 |
| 设置 | 订阅档案激活、内核版本与下载、镜像、流量模式、混合端口、延迟地址、自动化开关 |

## 控制面

`sbgui` 通过 `client_core::ClientController` 驱动引擎：

- `controller.snapshot()`：非阻塞地读取最新 `ClientSnapshot`，窗口每 400ms 轮询一次。
- `controller.send(cmd)`：非阻塞地提交命令（启动/停止内核、切换节点、测延迟、系统代理、
  TUN、订阅更新、内核下载、设置修改、关闭连接等）。
- 引擎在后台以 500ms 的节拍轮询流量、连接与代理组，维护速率历史，并在内核意外退出时按
  指数退避自动重启。

引擎内部依赖 `tokio`，由 `main` 创建一个多线程 runtime 承载；GPUI 只负责界面线程。
