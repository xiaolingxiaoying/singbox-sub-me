# 09: 五协议安装与服务生命周期

**What to build:** 管理员可完成一次交互式安装，默认选择全部五种 Managed protocol，安装经过验证的 sing-box，并得到可启动、重启和回滚的 sbctl 与 sing-box 服务。

**Blocked by:** 03: VPS traffic 计量与月度周期; 05: Direct subscription mode 与 Certbot 证书; 06: VMess WebSocket 与 Hysteria2 节点; 07: TUIC v5 与 AnyTLS 节点; 08: 外部反代订阅模式.

**Status:** resolved

- [x] 安装向导默认启用五协议但允许明确取消选择，并输出所有必需 TCP/UDP 端口而不更改防火墙。
- [x] 安装和重启先验证 sing-box 配置；失败时不启动候选服务。
- [x] `status`、`node`、`sub`、`traffic` 与 `restart` 通过生成的 systemd 服务报告和操作真实部署。

## Comments

- Implemented the interactive five-protocol installer, generated sbctl/sing-box systemd units, lifecycle status/restart commands, and CLI acceptance coverage. `cargo test` and Clippy pass.
