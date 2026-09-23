# 邮箱/证书体验优化

Status: resolved
Type: task

## 目标

降低 VPS 配置的繁琐与遗漏：Certbot 邮箱校验与说明、证书状态可见性、安装后必做清单。

## 交付范围

- wizard：Certbot 邮箱格式校验（非空、含 @、无空白）+ 提示「仅用于 ACME 到期通知」；支持显式确认的免邮箱路径（`--register-unsafely-without-email`，需要用户二次确认）。
- 新增 `sbctl certificate status`：证书路径、SAN 覆盖、有效期、剩余天数、deploy hook 是否在位、下次自动续期时间（certbot timer）。
- `sbctl status` 增加 Direct 模式证书剩余天数（<14 天高亮提醒）。
- 安装/证书签发完成后输出「后续必做清单」：放行 80/443 与各协议端口的具体 `ufw` 命令（按启用协议 TCP/UDP 区分）、DNS 解析自检命令、`curl` 订阅自检命令。
- IP fallback / external-proxy 模式的清单按模式裁剪（无证书步骤，改为反代配置提示）。

## 验收标准

- [ ] 非法邮箱被 wizard/CLI 拒绝并给出示例；免邮箱路径需二次确认。
- [ ] `certificate status` 在 Direct 部署上显示全部字段；证书缺失时给出可执行的修复命令。
- [ ] 安装完成后终端输出完整清单（含逐协议端口与 TCP/UDP 区分）。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/subscription-upgrade/spec.md`、ADR-0009、ADR-0011

## Comments

- 2026-09-14：certificate status 命令、status 证书剩余天数、安装后必做清单（ufw 逐端口）、wizard 邮箱校验已实现。
- 2026-09-14（收口）：新增 `certificate obtain --no-email`（交互二次确认后映射 `--register-unsafely-without-email`）与 `acme_email_is_valid`；wizard 留空邮箱需显式确认；`docs/installation.md` 增补证书状态与后续必做清单；CLI 测试覆盖非法邮箱 / 缺参 / 未确认 / 互斥。真机签发仍待人工验证。
