# S2 P1：删除 sbctl.service 中无效的 Sockets= 配置

Status: resolved
Type: task
Blocked by: 01

## 事实（VPS 报告 §6.10 / §7 P1）

Direct 模式生成的 `sbctl.service` 在 `[Unit]` 写 `Sockets=sbctl-http.socket`
（`src/lifecycle.rs:46-55`）。Ubuntu 22.04 systemd 报
`Unknown key name 'Sockets' in section 'Unit', ignoring.`
socket 仍因 `.socket` 单元的 `Service=sbctl.service` 与已有 `Requires=`/`After=` 工作，
但 unit 内容与意图不符，`systemd-analyze verify` 无法无警告。

## 动作

1. 从 `sbctl_unit(direct)` 删除 `Sockets=sbctl-http.socket` 行，保留
   `Requires=`/`After=`。
2. 在 `src/lifecycle.rs` 增加单元测试：`sbctl_unit(true)` 不含 `Sockets=`，
   且包含 `Requires=sbctl-http.socket` 与 `After=sbctl-http.socket`；
   `sbctl_unit(false)` 三者皆无。
3. 验收脚本增加 `systemd-analyze verify` 对 sbctl 自有 unit 无 warning 的断言
   （并入工单 05）。

## 验收

- 单元测试通过。
- Docker 验收中 Direct 安装后 `systemd-analyze verify` 不再输出 Unknown key 警告。

## Comments

2026-09-23 修复完成：

- `sbctl_unit(direct)` 删除 `Sockets=sbctl-http.socket`，保留
  `Requires=`/`After=`。
- 新增 `src/lifecycle.rs::tests::the_direct_service_unit_uses_valid_dependency_keys_only`，
  断言 direct 版含 `Requires=`/`After=` 且任何版本都不含 `Sockets=`。
- `systemd-analyze verify` 的 Docker 断言按工单 05 补进验收脚本。
