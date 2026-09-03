# 完善配置向导、配置切换与 credential rotate

Status: needs-triage
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

- [ ] 用户取消、输入非法值或摘要未确认时，现有部署完全不变。
- [ ] 空输入在既有部署中保留当前值，新部署使用 UTC 和安全默认值。
- [ ] 模式、端口、网卡、timezone、DST 和协议前置条件在提交前全部校验。
- [ ] 配置变更通过 artifact/check/health transaction 后才替换运行配置。
- [ ] rotate 后旧 Subscription URL 返回 404，新 URL 可用；Proxy credential 不变。
- [ ] 交互和非交互路径均不打印完整 credential、私钥或密码。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0002、ADR-0007、ADR-0018
