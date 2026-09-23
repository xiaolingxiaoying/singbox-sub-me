# 11: 可恢复卸载与受限清理

**What to build:** 管理员可安全卸载 sbctl；默认操作保留 root 可读备份，`--purge` 仅清除 sbctl 明确拥有的数据。

**Blocked by:** 09: 五协议安装与服务生命周期.

**Status:** resolved

- [x] 默认卸载停止并移除 sbctl 服务与二进制，同时保存配置、凭证和状态备份。
- [x] `--purge` 删除范围限于 sbctl 创建并拥有的持久数据。
- [x] 两种卸载均不修改既有 sing-box 部署、其他服务、反向代理配置或防火墙规则。
