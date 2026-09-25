# S5/S6：内核版本一致性与官方下载校验

Status: in-progress
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

## 动作

1. 完成 digest 解析与 SHA-256 校验单测，补充官方更新 CLI 集成失败用例；当前新增的
   归档摘要不匹配用例需由 Linux CI 执行，本机 Windows 按平台条件跳过。
2. 检查 GitHub Actions 中全量 CLI 集成测试确实运行该用例，并验证工作树不变、无回滚点、
   无服务重启。
3. 确认成功下载校验用例与摘要缺失/null、格式错误、资产缺失的分支覆盖。

## 验收

- README 与服务端实际行为逐条一致。
- 两条更新路径都有失败回滚测试，官方摘要校验失败不会改动主机。
- ADR-0024 记录官方路径的信任边界。
