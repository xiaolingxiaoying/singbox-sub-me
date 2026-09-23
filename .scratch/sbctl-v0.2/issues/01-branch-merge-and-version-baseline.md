# 合并分支、统一版本基线

Status: ready-for-agent
Type: task

## 目标

把 73 个提交的结构重构与订阅升级特性从 `refactor/structure` 落到 `master`，并让版本号与发布 tag 一致。

## 事实

- `refactor/structure` HEAD `a5fe586`，比 `origin/master` 领先 73 个提交；`master`（本地 `9cbef8a`，领先 origin 3）是其祖先，`git merge --ff-only` 可用。
- `Cargo.toml` 版本 `0.1.26` 与已发布 tag `v0.1.26` 相同，但源码晚于该 tag（VPS 报告 §4.1）。

## 动作

1. 提交工作树中的 `docs/ubuntu-22-vps-e2e-report-2026-09-22.md` 与 `docs/release-readiness-and-vps-test-plan.md`（No-Go 记录）。
2. `Cargo.toml` 版本升 `0.2.0`；检查 `Cargo.lock` 同步。
3. fast-forward `master` 到 `refactor/structure`，推送 `origin/master`。
4. 等待 GitHub CI 绿；失败则先修 CI，不带病打 tag。

## 验收

- `origin/master` 包含 a5fe586 之后的所有提交与本次修复。
- `cargo metadata` 报告版本 `0.2.0`。
- GitHub CI（fmt / python unittest / clippy / workspace test / release_trust / prototype / windows-static / sbtui-macos / sing-box-profiles / mihomo-profiles）全绿。
