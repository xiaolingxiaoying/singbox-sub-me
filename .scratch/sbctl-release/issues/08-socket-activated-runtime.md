# 实现 socket-activated Direct HTTPS 与双非 root 服务

Status: needs-triage
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

- [ ] sbctl daemon 不直接 bind 80/443，socket unit 持有两个公网监听。
- [ ] 真实或等价 systemd 中 80/443 均能到达正确 HTTP/TLS 处理路径。
- [ ] `sbctl` 与 `sing-box` 使用不同无登录服务账户，服务不以 root 常驻运行。
- [ ] External proxy 只监听 loopback，IP fallback 只使用配置的高位 HTTP 端口。
- [ ] 超大请求、慢读取、超并发和异常连接均受边界限制且不导致进程失控。
- [ ] 现有 Caddy/Nginx、iptables、NAT 和其他服务未被修改。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0009、ADR-0011、ADR-0012
