# sing-box 完整客户端配置与版本 profiles

Status: resolved
Type: task
Blocked by: 01

## 目标

为 sing-box 客户端生成完整可用的配置（不再是裸 outbounds），并按 1.12.0 → 最新稳定版做版本差异化 profile。

## 前置研究

以官方 changelog / 文档为准（后台研究报告落到本文档 Comments 或 `docs/research/`），明确各版本差异：新 DNS server 对象格式、fakeip 写法、`domain_resolver`/`default_domain_resolver`、弃用与移除字段、tun 推荐写法、clash_api/cache_file 字段。禁止凭记忆写字段名。

## 交付范围

- 新增 `sing_box_full_client(config, nodes, version_profile)`，生成：
  - `log`（info + timestamp）
  - `dns`：fake-ip 模式；直连侧 AliDNS（223.5.5.5）、代理侧 DoH（1.1.1.1/8.8.8.8，detour 选择组）；局域网/连通性检测域名直连解析；fakeip 段 198.18.0.0/15
  - `inbounds`：tun（auto_route/strict_route/auto_detect_interface；`ipv6: false` 对齐现有 ipv4_only 偏好）
  - `outbounds`：🚀节点选择(selector，含 ♻️自动选择 + DIRECT + 全节点) + ♻️自动选择(url-test) + 5 协议节点 + direct；延迟探测 URL 走 `latency_probe_url` 配置
  - `route`：rule_set 远程 `.srs`（geosite-cn/geoip-cn，URL 走 `rule_set_base_url`）+ LAN/CN 直连 + AI/x.com 分流（吸收 novixlink-override.yaml 规则）+ final 指向选择组
  - `experimental`：clash_api（127.0.0.1:9090，default_mode rule）+ cache_file（enabled, 存储选择）
- `VersionProfile` 注册表：id（如 `1.12`/`1.13`/`latest`）、标注文本、兼容范围（复用 `src/release.rs::sing_box_compatibility` 的形态）、按版本调整的字段差异。
- `rule_profile = minimal` 时 route 不引用远程 rule_set（内置 LAN/CN 精简域名/IP 规则），满足 ADR-0018。
- 自签证书部署（certificate_mode=self-signed）时客户端 TLS 保持 `insecure: true`。

## 验收标准

- [ ] `sing-box-full.json` 与各 `sing-box-<ver>.json` 工件生成且字段完整；版本间差异与研究报告一致。
- [ ] 每个版本 profile 用对应版本 sing-box 真核 `sing-box check` 通过（测试脚本按版本下载内核；CI 至少覆盖 1.12 与最新）。
- [ ] minimal/standard 两种 rule_profile 都能生成合法配置。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/subscription-upgrade/spec.md`、ADR-0018、docs/sing-box-yg-port-dev.md F5

## Comments

- 2026-09-14：已实现 1.12/1.13/1.14 三个 profile + full；真核验证：1.12.4/1.13.9/1.14.0 分别 check 对应 profile 通过（含 full）。研究结论在 docs/research/sing-box-client-version-differences.md。
