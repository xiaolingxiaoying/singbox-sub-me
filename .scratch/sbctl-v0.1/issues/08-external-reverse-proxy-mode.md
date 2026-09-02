# 08: 外部反代订阅模式

**What to build:** 管理员可让 sbctl 只监听 loopback，并通过自己管理的反向代理公开私密订阅，从而保留已有服务对 80/443 的所有权。

**Blocked by:** 04: VLESS Reality 的 IP fallback 私密订阅.

**Status:** ready-for-agent

- [ ] 外部反代模式不绑定公网 80/443，订阅和流量头在 loopback 后端保持可用。
- [ ] sbctl 不生成、覆写或接管任何 Caddy、Nginx 或其他反向代理配置。
- [ ] 模式切换验证端口冲突并保留最后一个已知良好订阅服务。
