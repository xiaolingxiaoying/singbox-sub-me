# 实现签名 manifest、兼容矩阵与可回滚更新

Status: needs-triage
Type: task
Blocked by: 06, 08, 10

## 目标

将 release manifest 和更新流程升级为先验签、再信任 URL/digest，并在候选运行失败时完整回滚。

## 交付范围

- versioned manifest schema、固定版本、sbctl/sing-box URL、SHA-256、兼容矩阵和 Ed25519 signature。
- 去除 `signature` 字段后的 canonical JSON 签名与标准 Base64 校验。
- 内置首版发布公钥；Rust 更新逻辑与安装脚本共享验证规则。
- 临时下载、digest 验证、候选 sbctl health check、候选 sing-box check。
- 二进制、配置、工件、units、证书引用、state 的 rollback point 和恢复。

## 验收标准

- [ ] manifest 签名失败时不会访问或信任其中的 URL/digest。
- [ ] canonical JSON 字段、编码、Base64、schema version 均有正反例测试。
- [ ] 兼容矩阵不满足时安装/更新在替换前拒绝。
- [ ] digest、候选 health check 或服务启动失败时旧版本、配置、工件和 state 恢复可用。
- [ ] `update --check` 完全只读，不下载、不替换、不重启。
- [ ] `latest`、`main` 和未签名远程更新被拒绝；安装脚本不能绕过 Rust 验证规则。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0010、ADR-0014、ADR-0017
