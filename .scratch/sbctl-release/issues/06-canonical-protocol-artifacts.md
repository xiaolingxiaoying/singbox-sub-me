# 完善五协议三格式的 canonical node 生成与事务替换

Status: needs-triage
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

- [ ] 五种协议逐一生成合法服务端配置和客户端节点。
- [ ] 三种格式中的节点集合、host、port、credential、TLS 字段来自同一 canonical model。
- [ ] listener port 在 TCP/UDP 两侧按数字全局唯一且位于 `10000–65535`。
- [ ] Proxy credential 不能读取订阅；Subscription credential 不出现在节点认证字段中。
- [ ] sing-box check 失败时既有 active config 和 artifacts 保持不变。
- [ ] 并发读取 artifact 只能看到完整旧版或完整新版。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0002、ADR-0007、ADR-0018
