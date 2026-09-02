# 06: VMess WebSocket 与 Hysteria2 节点

**What to build:** 管理员可在域名部署中启用 VMess WebSocket 与 Hysteria2；每个节点有独立端口与 Proxy credential，并出现在全部 Subscription format 中。

**Blocked by:** 05: Direct subscription mode 与 Certbot 证书.

**Status:** resolved

- [ ] 两种节点的 TLS、传输和认证参数与公共主机及证书身份一致。
- [ ] 生成的 sing-box 配置通过检查，且加入节点不会改变既有 VLESS Reality 节点。
- [ ] JSON、YAML 与 URI 表示可被代表性客户端解析，并保留独立凭证。
