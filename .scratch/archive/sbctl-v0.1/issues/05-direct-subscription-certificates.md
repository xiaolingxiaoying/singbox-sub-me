# 05: Direct subscription mode 与 Certbot 证书

**What to build:** 管理员可使用自己的域名让 sbctl 在 Direct subscription mode 下直接提供 HTTPS 订阅，并通过发行版 Certbot 包完成 ACME 签发、续期与安全 reload。

**Blocked by:** 04: VLESS Reality 的 IP fallback 私密订阅.

**Status:** resolved

- [ ] sbctl 仅在 Direct subscription mode 下拥有公网 TCP 80/443，并为 ACME webroot 提供挑战响应。
- [ ] 证书签发、续期和 reload 不依赖 Caddy 或常驻 ACME 进程。
- [ ] 无效域名、占用端口或失败证书操作产生明确错误且保持原有可用服务。

## Comments

- Implemented direct HTTPS delivery, an ACME webroot, and distribution-package Certbot obtain/renew commands. Full test suite passes.
