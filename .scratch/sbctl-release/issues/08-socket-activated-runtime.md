# 实现 socket-activated Direct HTTPS 与双非 root 服务

Status: resolved
Type: task
Blocked by: 01, 06

## 目标

将 Direct HTTPS 运行时切换为 systemd socket activation，并以独立非 root 身份运行 sbctl 和 sing-box。

## 交付范围

- `sbctl-http.socket` 的 TCP 80/443 两个 `ListenStream`。
- `LISTEN_FDS` 接收、按本地端口区分 HTTP 与 TLS。
- `sbctl.service` 与 `sing-box.service` 的独立服务账户和最小权限。
- Hyper/Axum bounded HTTP handling：请求大小、读取超时、并发和连接关闭。
- Direct、External proxy、IP fallback 的监听边界。

## 验收标准

- [x] sbctl daemon 不直接 bind 80/443，socket unit 持有两个公网监听。
- [x] 三发行版真实 systemd acceptance 验证 Direct HTTP/TLS 80/443 路由。
- [x] `sbctl` 与 `sing-box` 使用不同 `/usr/sbin/nologin` 服务账户；验收检查 systemd 配置和运行进程用户，服务不以 root 常驻运行。
- [x] External proxy 只监听 loopback，IP fallback 只使用配置的高位 HTTP 端口。
- [x] Hyper HTTP/1 入口对超大请求头、慢读和超过 32 的并发未完成请求有实时 socket 测试；连接按上限关闭。
- [x] 安装和卸载验收检查不接管既有 UFW/Nginx 配置；VPS 测试只为五个 sbctl 节点端口显式添加 UFW allow 规则，未改云防火墙或 NAT 配置。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0009、ADR-0011、ADR-0012

## Comments

- 2026-09-25：运行时和 systemd 验收完成。Actions run `36121273005` 全部通过，覆盖生产构建、Debian 12/Ubuntu 22.04/24.04 systemd acceptance、Linux/Windows/macOS 检查及 profile 校验。验收新增 `/usr/sbin/nologin` 与实际 MainPID 用户断言；`src/subscription/serve.rs` 新增超大头、慢读和并发上限 live-listener 测试。三种部署模式、80/443 Direct socket、非 root 服务、loopback proxy 和 IP fallback 也已在 VPS/acceptance 验证。随后将 oversized-header 测试收紧为必须返回 HTTP 431；Actions run `36122703588` 全部通过，确认了该严格断言及完整系统验收。
