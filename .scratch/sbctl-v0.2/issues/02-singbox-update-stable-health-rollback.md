# S1 P0：独立 sing-box 更新的稳定健康观察与自动回滚

Status: resolved
Type: task
Blocked by: 01

## 现象（VPS 报告 §6.13 / §7 P0）

`sbctl sing-box update --artifact <BROKEN>`：候选 `check` 返回 0、`run` 立即退出 1。
命令返回成功、二进制已被替换、rollback point 已创建但未应用；5 秒后
`ActiveState=activating / SubState=auto-restart / NRestarts=21`，数据面全部协议中断。

## 根因

`src/lifecycle.rs:139` 的 `restart_sing_box_service` 只做一次
`systemctl is-active --quiet`。`Type=simple` 服务在进程 fork 后短暂 active，
`Restart=on-failure` 又会自动拉起，单次探测容易落在短暂 active 窗口。
完整安装/双服务重启已实现 `wait_for_stable_activation`（3 次 × 1.1s），独立更新路径没有复用。

## 动作

1. `restart_sing_box_service` 改为 `restart` + `wait_for_stable_activation(root, "sing-box.service")`。
2. 回滚路径（`install_candidate_sing_box` 与 `apply`）恢复旧二进制后必须再次执行稳定观察；
   回滚后的观察失败要保持显式警告。
3. 新增 Unix 回归测试：systemctl 桩模拟「restart 后第一次 is-active 成功、随后失败」，
   候选二进制内容带崩溃标记；断言：
   - 更新命令失败（exit code 2，stderr 含 `service health check failed`）；
   - `usr/local/bin/sing-box` 恢复为已知良好内容；
   - 回滚点存在；
   - 服务桩状态回到 stable，且连续 3 次 `is-active` 成功。
4. 测试不得拖慢既有成功路径超过 2.2s/次重启。

## 验收

- 新测试在未修复代码上必然失败（单次探测会误判成功），在修复后通过。
- 既有 `tests/cli/update_release.rs` 全部保持通过。
- VPS 报告 §10 的复验命令在真实 systemd 上通过（由工单 12 执行）。

## Comments

2026-09-23 修复完成：

- `src/lifecycle.rs::restart_sing_box_service` 现在 `restart` 后复用
  `wait_for_stable_activation`（3 次 × 1.1s），回滚路径同样走稳定观察。
- 新增 `tests/cli/update_release.rs::standalone_sing_box_update_rolls_back_a_candidate_that_crashes_on_start`，
  配 `tests/cli/fixture.rs::write_systemctl_restart_race_fixture` 模拟
  「restart 后第一次 is-active 成功、随后进入崩溃循环」。
- 反向验证（WSL Ubuntu-22.04）：临时回到旧实现后，测试失败且命令输出
  `code=0 stdout="本地 sing-box 候选 更新完成，已通过配置检查与服务健康检查…"`，
  与 VPS 报告 §6.13 的现象一致；恢复修复后 `cargo test --test cli update` 13/13 通过。
- 真实 systemd 的最终复验由工单 12 在干净 VPS 上执行。
