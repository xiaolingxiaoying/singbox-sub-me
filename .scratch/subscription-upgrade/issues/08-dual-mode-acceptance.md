# external-proxy 与 ip-fallback 打通验证

Status: resolved
Type: task
Blocked by: 01, 02, 03, 05

## 目标

用 Docker systemd 验收自动化覆盖 external-proxy 与 ip-fallback 两种从未实测的订阅模式，并提供真实 VPS 手动联测手册。

## 交付范围

- `tests/acceptance/` 新增 scenario：
  - **ip-fallback**：高位 HTTP 端口部署 → 断言全部订阅格式（含新路由）HTTP 200、`subscription-userinfo` 存在、错误凭据 404、节点 URI 可解析。
  - **external-proxy**：sbctl 绑定 loopback:2080 + Caddy 容器把 `/sub/` 反代到该地址（共享 network namespace 或 host 网络）→ 同样断言全部格式 200/404。
  - 两个 scenario 都验证 `sing-box-full.json` 能被 fake/真 sing-box `check` 通过（以验收环境现有 fake sing-box 边界为准，不能 check 则记录说明）。
- 手动手册 `docs/subscription-modes-testing.md`（中文）：
  - external-proxy：Caddy 反代片段（2–5 行）与 Nginx 等价片段、systemd 注意点、验证 curl 命令
  - ip-fallback：部署步骤、防火墙放行、客户端导入验证清单
  - 每种模式一条「从部署到手机导入成功」的完整 checklist（Shadowrocket/Clash Party/SFA/V2rayN）

## 验收标准

- [ ] 两个 acceptance scenario 在本地 Docker 跑通（Debian 12 基镜像）。
- [ ] 手册步骤可直接照抄执行，命令均可复制。
- [ ] `cargo fmt/clippy/test` 通过；验收脚本不依赖公网域名或 GitHub Release。

## 相关规格

`.scratch/subscription-upgrade/spec.md`、tests/acceptance/README.md

## Comments

- 2026-09-14：verify.sh 扩展为全矩阵断言（含 QR/index/负例），external-proxy 段加入真实 nginx 反代链路；debian:12 与 ubuntu:22.04 验收通过；手册 docs/subscription-modes-testing.md 已写。
