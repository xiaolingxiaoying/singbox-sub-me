# S5/S6：内核版本一致性与官方下载校验

Status: resolved
Type: task
Blocked by: 01

## 现状（2026-09-25 复核）

- release `install.sh` 和 manifest 更新路径使用固定版本签名 manifest；独立的
  `sbctl install` / `sbctl sing-box update` 无 manifest 路径解析官方最新稳定版。
- README 的最新稳定版表述与无 manifest 安装路径一致；原先关于 `release.yml`
  固定默认 1.12.0 的判断已过时，不再作为本任务前提。
- GitHub Releases API 为部分资产提供 `digest` 字段，但不保证每个版本都有值。
  调查记录见 `docs/research/sing-box-official-release-digest.md`。
- `src/update.rs` 已增加按精确资产名读取 SHA-256、下载后校验的实现；摘要缺失或
  为 null 时明确告警并继续旧的兼容性检查；格式异常、资产缺失或哈希不符时拒绝。

## 决策

保留两条有意区分的信任路径：签名 manifest 固定版本并验证发布者签名；无 manifest
官方直连路径取最新稳定版并使用 GitHub asset digest 做字节完整性检查。GitHub digest
不是发布者签名。此边界写入 ADR-0024 与 README。

## 完成证据（2026-09-25）

- `src/update.rs` 单测覆盖：精确资产匹配、摘要缺失/null、格式错误、缺少目标架构资产、
  匹配/不匹配的归档 SHA-256。
- `tests/cli/update_release.rs::official_sing_box_digest_mismatch_aborts_before_changing_the_host`
  在 Linux Actions 跑完整 workspace 测试时执行通过：摘要不匹配时主机快照不变、没有回滚目录、
  没有触发 systemctl。Windows 本机条件跳过。
- GitHub Actions `36137405665` 全部成功，含 release 构建、全 workspace lint/test、版本 profile
  真核、mihomo 配置验证和 Debian/Ubuntu systemd acceptance。
- VPS `64.81.29.67` 的 CI 构建 `sbctl` SHA-256 为
  `4169087373a413640baf71a092ef0e3bd295669b53a4095182618d4c1a93c3ad`，与下载产物一致。
  官方更新路径实际解析 sing-box `1.14.2`、下载并校验 Release digest、通过配置和服务健康检查；
  `sbctl.service`、`sing-box.service`、HTTP socket、accounting timer 均 active，服务重启计数为 0，
  `sbctl config validate` 成功，sing-box-full 订阅 HTTP 200（6229 bytes）。

## 验收

- README 与服务端实际行为逐条一致。
- 两条更新路径都有失败回滚测试，官方摘要校验失败不会改动主机。
- ADR-0024 记录官方路径的信任边界。
