# 修正只读 traffic/status/subscription 与 subscription-userinfo

Status: resolved
Type: task
Blocked by: 02, 03, 04

## 目标

让正常读取路径严格只读，并统一订阅认证失败、状态故障和流量元数据行为。

## 交付范围

- `traffic`、`status`、`status --json` 的当前 period 报告。
- subscription handler 只读取完整 artifact 和 accounting state。
- `subscription-userinfo` 的 RX/TX/total/expire 映射。
- 无效路径、query credential、错误 credential 的统一 404。
- 缺失、损坏或 schema 不兼容状态的脱敏 503。
- 完整 credential 在日志、错误和诊断中的 redaction。

## 验收标准

- [x] 重复执行读取命令和订阅请求不会改变 accounting state mtime/content。
- [x] `download=RX`、`upload=TX`、`total=RX+TX` 与当前 period 一致。
- [x] pending-first-reset 返回零流量和首个 reset，而非 5xx。
- [x] query 参数 credential 永远不能授权，所有错误 credential/path 均为 404。
- [x] state/artifact 故障返回脱敏 503，不返回 200 占位订阅。
- [x] acceptance 日志和错误输出不包含完整 Subscription credential。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0013、ADR-0016

## Comments

- 2026-09-03：`subscription_response` 从“所有错误统一 404”改为显式判定：`parse_route` 失败（含 query `?`、未知格式、多余 path 段、缺 credential）与 credential 不匹配一律 404；已认证但 artifact 读取或 `traffic::report` 失败（StateMissing/StateStale/StateCorrupt/StateSchemaMismatch、计数器读取、schedule）一律返回空 body 的脱敏 503，并写一条 `redact_secret` 脱敏后的 stderr 诊断。新增 `pub fn redact_secret`，503 诊断与响应均不含完整 credential。
- 2026-09-03：`subscription-userinfo` 的 `total` 从 `monthly_traffic_limit` 修正为当前账期报告总量 `RX+TX(+correction)`，与 spec 决策 3 一致；`download=RX`、`upload=TX`、`expire=next_reset` 语义不变。现有 CLI/acceptance 断言（`total=999/1000`）随之更新。
- 2026-09-03：新增 `sbctl status --json`，输出脱敏的 JSON：`configured`、模式、主机、接口、限额、账期策略/时区、启用协议、服务状态与当前 period traffic（含缺失时 `traffic.error`）；未安装时为 `{"configured": false}`，不包含 credential。
- 2026-09-03：`service_status_entries` 抽成结构化条目，`service_status` 文本视图保持原有格式。
- 2026-09-03：验证通过 `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、63 个库测试、50 个 CLI 测试及全部 acceptance shell 的 `sh -n`；新增 CLI 测试覆盖统一 404、missing/corrupt/schema-mismatch/artifact 四类脱敏 503（诊断不含 credential）、`status --json` 当前 period 与无凭据输出、unmanaged JSON、`redact_secret` 直测与 total-only correction 后 header `total` 的锁定语义；acceptance 新增 `total=RX+TX`、state mtime/content 不变、503 脱敏日志、pending-first-reset 200 与 `status --json` 无凭据检查。
- 2026-09-03：延后项：`diagnostics` 命令、socket/证书/工件详细 JSON 属 ticket-12（release gates）范围；Direct 模式证书加载失败仍走 `serve_tls` 静默跳过，其脱敏 5xx 与 SNI 校验属后续证书生命周期 ticket。
