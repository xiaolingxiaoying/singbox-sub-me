# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

拥有私有 sing-box 订阅、在 Windows、Linux 或 macOS 终端里管理本地代理的个人用户，以及
在 Windows 桌面使用图形界面的同一批用户。用户需要在不离开终端或窗口的情况下导入订阅、
选择节点、了解连通状态，并在系统代理与 TUN 模式之间安全切换。

## Product Purpose

`client-core` 是共享控制面：订阅、内核生命周期、clash_api、系统代理与设置只实现一次。
`sbtui` 是它的终端界面，`sbgui` 是它的 Windows 桌面界面。两者都管理订阅与 sing-box
内核，并把代理、流量、连接和日志状态集中到一个全键盘界面或桌面窗口中。

## Positioning

服务端生成的完整 sing-box 配置直接被客户端导入；客户端在保留 sing-box 数据面的同时，提供本地核心、节点和系统代理控制。

## Operating Context

用户通常在个人电脑的终端或桌面窗口中短时、高频地查看代理是否工作、切换节点、刷新订阅
和排查连接问题。网络状态与可恢复性比装饰性更重要。

## Capabilities and Constraints

- 保留现有订阅导入、核心下载、节点切换、系统代理、TUN、流量、连接与日志能力。
- 内核内存、运行版本、节点上报延迟与路由规则由引擎统一发布，两个界面一致显示。
- 保留全键盘操作和 Windows/Linux/macOS 终端兼容性。
- 系统代理与 TUN 是会影响网络的操作，必须维持明确状态与可恢复路径。
- 本次界面方向由用户确认：更强视觉仪表盘，优先醒目的运行态与较低的信息密度。

## Evidence on Hand

- 共享控制面：`crates/client-core/src/lib.rs`。
- 终端客户端实现：`crates/sbtui/src/lib.rs`，说明见 `crates/sbtui/README.md`。
- 桌面客户端实现：`crates/sbgui/src/main.rs`，说明见 `crates/sbgui/README.md`。
- 上游差距分析：`docs/research/client-review-and-upstream-gap.md`。
- 客户端功能完成度审查（含 Clash Party / sing-box 对照与复核命令）：`docs/research/client-feature-completeness-review.md`。

## Product Principles

- 先显示网络是否可用，再显示如何操作。
- 所有会改变网络的行为都必须可见、可预期、可撤销。
- 图形化信息服务于判断，不替代精确数值与日志。
- 快捷键是主路径，但界面应当让快捷键可发现。
