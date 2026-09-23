# 12: Debian/Ubuntu 发布验收

**What to build:** 管理员可依赖一套隔离的 Debian 与 Ubuntu 黑盒验收，确认 V0.1 的发布工件、安装、五协议订阅、流量、模式切换、更新和卸载满足规格。

**Blocked by:** 10: 显式验证更新与回滚; 11: 可恢复卸载与受限清理.

**Status:** resolved

- [x] CLI 验收覆盖 fresh install、Existing deployment 拒绝、Direct subscription mode、外部反代与 IP fallback subscription。
- [x] 验收检索三种 Subscription format，校验五种 Managed protocol、凭证隔离、`subscription-userinfo`、流量恢复与月度周期。
- [x] 验收覆盖更新回滚、默认卸载、`--purge` 及不修改无关服务和防火墙的安全边界。
