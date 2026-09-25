# 完善五协议三格式的 canonical node 生成与事务替换

Status: resolved
Type: task
Blocked by: 01

## 目标

以统一 canonical node model 生成五种 Managed protocol 的 sing-box server config 和三种 Subscription format，并确保工件验证、替换和回滚一致。

## 交付范围

- VLESS Reality、VMess WebSocket、Hysteria2、TUIC v5、AnyTLS。
- sing-box JSON、Clash/Mihomo YAML、URI text。
- 独立 Proxy credential、端口唯一性和 Subscription credential 隔离。
- sing-box check、artifact 临时写入和 atomic rename。
- 协议字段与客户端兼容性 acceptance contract。

## 验收标准

- [x] 五种协议逐一生成合法服务端配置和客户端节点；Linux CI 使用真实 sing-box 核心校验生成配置，VPS/Ubuntu VM 实测五种节点均可完成 HTTPS 探测。
- [x] 三种格式中的节点集合、host、port、credential、TLS 字段来自同一 canonical model。
- [x] listener port 在 TCP/UDP 两侧按数字全局唯一且位于 `10000–65535`。
- [x] Proxy credential 不能读取订阅；Subscription credential 不出现在节点认证字段中。
- [x] sing-box check 失败时既有 active config 和 artifacts 保持不变。
- [x] 并发读取 artifact 只能看到完整旧版或完整新版。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0002、ADR-0007、ADR-0018

## Comments

- 2026-09-25：实现与验收完成。canonical node model、跨 TCP/UDP 端口冲突检查、凭据隔离、check 失败事务保持和并发 artifact 完整性均有单测；Linux CI 的真实 sing-box profile 检查通过。服务器订阅模式矩阵和五协议均在 VPS 上验证，用户随后在 Ubuntu VM 的 Clash Party 中确认五种节点测速全部通过。Actions run `36116097720` 全部通过，含 Linux/Windows/macOS 检查、生产构建和 Debian/Ubuntu systemd acceptance。
