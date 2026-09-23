# CI 里没有未用依赖检查，Windows-only 的死依赖因此长期隐身

Status: needs-triage
Type: task
来源：`docs/target-spec-gap-and-verification-plan.md` Phase 0.4。

## 问题

`crates/sbgui/Cargo.toml:27` 声明了 `tray-icon = "0.21"`，全仓源码零使用（唯一的 `tray`
命中就是这一行 Cargo）。它之所以能一直留着，是因为它是 Windows-only 依赖，而 CI 的
clippy/test 主作业跑在 Linux（`ci.yml:13-32`），Linux 构建根本不会解析它的可用性。
`ci.yml:50-60` 的 windows-static 作业只跑 `cargo test --workspace --lib`，也不检查依赖图。

同类问题会反复出现：任何"只在某个 target 下才暴露"的死依赖都没有门守着。

## 建议做法

在 `ci.yml` 增加一个 `unused-deps` 作业，用 `cargo-machete`（GitHub 最新 release 为
**v0.9.2**，资产名 `cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz`，经
`/repos/bnjbvr/cargo-machete/releases/latest` 核实）。

## 落地前必须先做的两件事（不要直接提 PR）

1. **先本地跑一遍看它报什么。** 2026-09-23 尝试在容器里跑时宿主到 GitHub release 资源的
   网络不稳定（HEAD 直接失败），没能拿到输出。一个上线即红的门比没有门更糟。
2. **确认它如何处理 `cfg(windows)` / `cfg(unix)` 下的依赖。** 本仓有多个平台限定依赖：
   `client-core` 的 `winreg` / `windows`（`crates/client-core/Cargo.toml:25-33`）、
   `sbgui` 的 `raw-window-handle` / `tray-icon`、根 crate 的 `libc`（`Cargo.toml:48-51`）。
   如果工具在 Linux runner 上把这些都判为未用，就需要为它们加配置豁免而不是改代码。

## 预期结果

`tray-icon` 被报出来。届时要么删依赖，要么把 close-to-tray 真正实现——按
`docs/target-spec-gap-and-verification-plan.md` Phase 1 的决策，本轮若不做托盘就删依赖。
