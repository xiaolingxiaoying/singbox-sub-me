# S9：客户端模板向导可用性

Status: resolved
Type: task
Blocked by: 01

## 范围决策（2026-09-25）

沿用 ADR-0022 的编译期模板目录，不开放管理员任意字段或模板文件。管理员可通过向导选择受支持的模板、DNS 模式、规则档位、规则源镜像和测速 URL；结构字段（策略组、规则集、DNS/TUN 内部参数）由已验证的模板负责。

## 现状

- `ConfigurationTopic::ClientTemplate` 已提供上述五个选项，沿用现有值作为默认答案，并在确认后应用。
- URL 输入通过 `DeploymentConfig::validate` 校验，生成内容来自编译期模板；覆写仍走 ADR-0021 的专用校验流程。
- 原提示只列选项名称，未充分说明远程规则的网络依赖、测速 URL 的可达性要求；配置预览/`sbctl status` 也没有显示 DNS 模式与两个 URL。
- `docs/subscription-guide.md` 已说明模板策略、DNS 模式、规则档位和 URL 配置。

## 动作

1. 优化五项向导提示，说明各值的效果、网络依赖和 URL 填写边界。
2. 预览与 `sbctl status` 显示模板、DNS 模式、规则档位、规则镜像和测速 URL；订阅凭据继续脱敏。
3. 更新订阅指南并为预览字段保留回归测试。

## 验收

- 空输入保留当前值；合法选项可修改；无效模板或 URL 会明确报错并重问。
- 变更预览覆盖所有五项客户端设置，且不泄漏订阅凭据。
- 生成结构仍由编译期目录提供，现有 sing-box/mihomo 验证门保持通过。

## 原始需求调整

原票要求把策略组、规则集、DNS/TUN 的任意字段交给管理员编辑；维护者选择沿用目录边界并优化向导，因此这部分已由 ADR-0022 明确排除。本票不验收自定义结构字段；若将来重开该需求，需先修改 ADR-0022 并重新评估跨内核 schema 校验与发布风险。

## Comments

2026-09-25 完成：

- 向导明确每项选项的用途、远程规则依赖及 URL 要求；错误镜像 URL 会提示并重问。
- 预览和 `sbctl status` 显示全部五个客户端设置；URL 认证信息、query、fragment 均脱敏，无效历史 URL 显示占位文本。
- 本地 workspace fmt、clippy、test 全部通过；新回归覆盖预览、错误 URL 重问和 URL 凭据脱敏。
- GitHub Actions run `36146609985`（提交 `e5aa7fc2352229a3fee8d77d4dab5842611a6cf6`）全部成功，含真实 sing-box profile、mihomo 模板与 Debian/Ubuntu systemd 验收。
- VPS 用该 run 的 Linux artifact 实测，SHA-256 `4144187ffe519aaac6d83fdcc41c84d86e7a80ad3ef2767f3a51a737479a0d95` 匹配；新二进制读取实际配置并打印正确摘要，凭据脱敏。四个受管服务 active，`sbctl.service` 与 `sing-box.service` 的 `NRestarts=0`。仅在 `/tmp` 运行后清理，线上二进制及服务未更改。
