# 04: VLESS Reality 的 IP fallback 私密订阅

**What to build:** 管理员可生成一个拥有独立 Proxy credential 的 VLESS Reality 节点，并从非 root daemon 的 IP fallback subscription 获取对应 JSON、YAML、URI 三种 Subscription format 与实时流量头。

**Blocked by:** 02: 持久配置与原子状态操作; 03: VPS traffic 计量与月度周期.

**Status:** resolved

- [ ] 生成的 VLESS Reality 配置通过 sing-box 检查，且 Reality decoy SNI 不与公共主机混淆。
- [ ] 三种 Subscription format 均来自同一节点，字段、端口和 Proxy credential 一致。
- [ ] 仅精确路径中的 Subscription credential 被接受；query 凭证被拒绝，日志脱敏，响应包含正确的 `subscription-userinfo`。
