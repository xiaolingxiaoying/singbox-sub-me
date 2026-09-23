# S4：验收脚本补故障回滚回归与旧断言修正

Status: ready-for-agent
Type: task
Blocked by: 02

## 事实

- VPS 报告 §10.4：`tests/acceptance/verify.sh` 中旧 rollback 路径断言需要修正。
- `tests/acceptance/verify-real.sh:84-96` 对 IP fallback 显式禁用 4 个 TLS 协议，
  与 `docs/subscription-modes-testing.md:75-83` 的五协议能力不一致。
- `systemd-analyze verify` 无警告尚未成为断言（工单 03）。

## 动作

1. 修正确认订脚本中的 rollback 路径断言。
2. 在系统级验收中加入坏候选故障注入：桩 `check=0 / run=1`，断言更新失败、
   旧二进制摘要恢复、`sing-box.service` 稳定 active、`NRestarts` 不增长、
   订阅路由与五协议端口恢复。
3. 增加 unit 语法断言：`systemd-analyze verify` 对 sbctl 自有 unit 输出为空。
4. IP fallback 验收切换为自签证书 + `--protocol-sni`，覆盖五协议（与文档一致）。

## 验收

- Docker 三发行版验收全部通过，且新增断言在故意注入坏候选时会失败。
- 本地候选验收与 Release workflow 的 acceptance job 使用同一脚本。
