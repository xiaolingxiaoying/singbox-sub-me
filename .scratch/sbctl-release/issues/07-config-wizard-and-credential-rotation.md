# 完善配置向导、配置切换与 credential rotate

Status: resolved
Type: task
Blocked by: 02, 06

## 目标

完成安全的交互式配置工作流和 Subscription credential 轮换，不让配置变更绕过验证和事务边界。

## 交付范围

- 读取已有 sbctl 配置、空输入保留当前值、逐项校验和摘要确认。
- 模式、主机、邮箱、IP fallback 端口、五协议、listener ports、limit、账期、timezone、接口和 loopback 端口选择。
- Natural/Anchored 切换及 policy/timezone/first reset 修改时建立新 accounting state。
- `sbctl credential rotate`，旧 URL 立即失效。
- 非交互参数完整性和安全敏感字段 redaction。

## 验收标准

- [x] 用户取消、输入非法值或摘要未确认时，部署状态不变。
- [x] 空输入在既有部署中保留当前值；新部署的 VPS refresh timezone 默认 `America/Los_Angeles`、client display timezone 默认 `Asia/Shanghai`，并使用安全默认值。
- [x] 模式、端口、网卡、timezone、DST 和协议前置条件在提交前全部校验。
- [x] 配置变更通过 artifact/check/health transaction 后才替换运行配置。
- [x] rotate 后旧 Subscription URL 返回 404，新 URL 可用；Proxy credential 不变。
- [x] 交互和非交互路径均不打印完整 credential、私钥或密码。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0002、ADR-0007、ADR-0018

## Comments

- 2026-09-25：实现和 VPS 交互验收完成。配置向导支持读取现值、空输入保留、逐项校验、脱敏摘要确认和事务提交；取消/非法输入不修改部署状态。新部署双时区默认值已与当前代码及配置向导测试对齐。凭据轮换集成测试确认旧 URL 立即 404、新 URL 可用、代理节点凭据不变且输出脱敏。Actions run `36116097720` 全部通过；五协议的 VM Clash Party 测速亦由用户确认通过。
