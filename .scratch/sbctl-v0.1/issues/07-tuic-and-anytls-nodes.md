# 07: TUIC v5 与 AnyTLS 节点

**What to build:** 管理员可在域名部署中启用 TUIC v5 与 AnyTLS；每个节点有独立端口与 Proxy credential，并出现在全部 Subscription format 中。

**Blocked by:** 05: Direct subscription mode 与 Certbot 证书.

**Status:** resolved

- [ ] 两种节点的 TLS、认证和端口参数与公共主机及证书身份一致。
- [ ] 生成的 sing-box 配置通过检查，且加入节点不会改变已生成的其他 Managed protocol。
- [ ] JSON、YAML 与 URI 表示可被代表性客户端解析，并保留独立凭证。
