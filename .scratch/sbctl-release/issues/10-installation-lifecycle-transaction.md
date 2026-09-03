# 完善安装、卸载、ownership marker 与失败恢复事务

Status: needs-triage
Type: task
Blocked by: 03, 06, 08

## 目标

把安装、服务启停、配置工件和卸载统一为可验证、可恢复且不接管 Existing deployment 的生命周期事务。

## 交付范围

- 下载验证、账户/目录、配置/工件、unit/socket/timer、daemon-reload、启动和 health check 阶段。
- 所有阶段成功后才写 ownership marker。
- 新安装失败只删除本次创建资源。
- 默认卸载备份；`--purge` 仅删除 sbctl 自有持久数据。
- 配置、证书引用、二进制、unit/socket/timer 和 accounting state 的 rollback point。

## 验收标准

- [ ] Existing deployment 被发现时安装不修改既有文件、服务或配置。
- [ ] 任一安装阶段失败都不会留下误导性的 ownership marker。
- [ ] 启动/健康检查失败会恢复本次事务前的 known-good 状态。
- [ ] 默认卸载停止并删除 sbctl 自有运行资源，但保留 root 可读备份和持久数据。
- [ ] `--purge` 删除范围仅限 sbctl ownership，手工 sing-box/Caddy/Nginx/firewall/NAT 不变。
- [ ] 真实或等价 systemd acceptance 验证 service/socket/timer 生命周期。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0003、ADR-0004、ADR-0017
