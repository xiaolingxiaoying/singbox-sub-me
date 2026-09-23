# S4：验收脚本补故障回滚回归与旧断言修正

Status: resolved
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

## Comments

2026-09-23 完成并在 Debian 12、Ubuntu 22.04、Ubuntu 24.04 的 systemd 容器中通过：

- `verify.sh`：失败更新的回滚点断言从旧路径 `var/lib/sbctl/rollback` 修正为
  `var/backups/sbctl/rollback`。
- `verify-real.sh` 新增坏候选故障注入：桩 `check=0 / run=1` →
  `sbctl sing-box update --artifact` 必须失败、旧二进制摘要恢复、服务 active、
  4 秒后 `NRestarts` 不再增长、订阅路由仍 200。
- `verify-real.sh` Direct 分支新增 `systemd-analyze verify` 断言：五个 unit 不得出现
  `Unknown key name`。
- IP fallback 分支去掉四个 `--disable-protocol`，改用 `--protocol-sni www.bing.com`，
  断言启用五协议且 URI 同时含 `vless:// vmess:// hysteria2:// tuic:// anytls://`。
- 新增 `scripts/dev/build-acceptance-artifacts.sh`：在 WSL 内构建并回传
  release 与 test-signing 两套 Linux 工件，供 `tests/acceptance/run.sh` 使用。
- 证据：本机 Git Bash + Docker Desktop 运行 `run.sh`，三个发行版均打印
  `real sbctl acceptance passed on <distro>`。
