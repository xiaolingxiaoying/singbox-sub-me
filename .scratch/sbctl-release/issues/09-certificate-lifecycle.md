# 实现证书校验、Certbot deploy hook 与安全 reload

Status: needs-triage
Type: task
Blocked by: 08

## 目标

完成 Direct HTTPS 的证书加载边界和 Certbot 续期后的安全重载。

## 交付范围

- 证书有效期、SAN、私钥匹配和 Subscription host/SNI 检查。
- ACME HTTP-01 webroot 处理。
- Debian/Ubuntu `certbot.timer` 续期与 deploy hook。
- 证书变化后的安全 reload 或下一连接加载。
- 证书错误的脱敏 5xx 和诊断。

## 验收标准

- [ ] 过期证书、SAN 不匹配、私钥不匹配和 SNI 不匹配均在加载前拒绝。
- [ ] ACME challenge 能通过 Direct HTTP 入口完成验证。
- [ ] Certbot deploy hook 重新验证证书，并使后续 HTTPS 连接使用新证书。
- [ ] 证书私钥权限只授予需要读取的服务账户。
- [ ] 证书失败不会泄露路径中的 credential 或私钥内容。
- [ ] External proxy 模式不接管或改写外部代理的证书配置。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0009、ADR-0011、ADR-0017
