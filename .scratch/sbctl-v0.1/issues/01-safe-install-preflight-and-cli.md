# 01: 安全安装预检与 CLI 骨架

**What to build:** 管理员可运行基础 `sbctl install` 与 `sbctl status` 命令；安装预检只接受受支持的 Debian/Ubuntu systemd 主机，并发现 Existing deployment 后安全退出，不改变主机状态。

**Blocked by:** None (can start immediately).

**Status:** resolved

- [ ] 在受支持主机和不受支持主机上，CLI 返回明确且可操作的预检结果。
- [ ] 发现现有 sing-box 二进制、服务或配置时，安装不修改任何现有部署。
- [ ] CLI 帮助、退出码和错误日志不泄露敏感输入。
