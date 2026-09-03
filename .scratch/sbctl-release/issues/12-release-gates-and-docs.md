# 完成真实 systemd release gates、文档和安装脚本收口

Status: needs-triage
Type: task
Blocked by: 04, 05, 07, 09, 10, 11

## 目标

在真实 Debian/Ubuntu systemd 主机或等价 VM 上完成发布验收，并同步所有面向用户和维护者的文档及 bootstrap 行为。

## 交付范围

- 真实非 root sbctl/sing-box、socket activation、ACME/HTTPS、UDP listener、timer persistence。
- 账期、correction、订阅三格式、签名更新、回滚、卸载和 Existing deployment 不变性端到端验收。
- `README.md`、`docs/installation.md`、`singbox-sub-plan.md` 历史标记及相关领域说明更新。
- `scripts/install.sh`、`scripts/generate-manifest.sh` 与 Rust 验证契约一致。
- 发布 gate 结果和已知限制记录。

## 验收标准

- [ ] Debian/Ubuntu systemd gate 验证双非 root 服务、socket 持有 80/443、证书续期、新连接和 UDP 协议监听。
- [ ] 验证跨月停机恢复、每分钟 timer 去重、traffic correction 和所有 `subscription-userinfo` 字段。
- [ ] 验证签名 manifest 安装、候选失败回滚、默认卸载备份和 `--purge` 范围。
- [ ] 验证不修改既有 Nginx、Caddy、iptables、NAT 和手工 sing-box deployment。
- [ ] 文档明确五协议、三格式、RX+TX VPS traffic、monthly limit 仅展示、socket activation、root 边界和行为兼容边界。
- [ ] 所有自动化测试通过，并明确 WSL2 不能单独作为生产 release gate。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0014、ADR-0018
