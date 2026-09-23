# S8：mihomo 订阅补嗅探模板

Status: ready-for-agent
Type: task
Blocked by: 01

## 现状

sing-box 完整 profile 已生成 `{"action":"sniff"}` 路由规则（`src/subscription/render/singbox.rs:128-152`）；
mihomo 两份 YAML 没有任何 `sniffer` 配置（全仓 `rg sniffer src` 仅命中 `nosniff` 响应头）。

## 动作

1. 在 `src/subscription/render/clash.rs` 的两份工件中生成 `sniffer` 段：
   - `enable: true`；
   - `sniff: { HTTP: { ports: [80, "8080-8880"], override-destination: true }, TLS: { ports: [443, 8443] }, QUIC: { ports: [443, 8443] } }`；
   - 保持 1.18 兼容版与现行版的差异只在既有差异处，不引入新字段差异。
2. 若 mihomo 1.18 不支持某个子键，按版本裁剪并在 profile 注释/文档标注。
3. 用 `tests/clash_mihomo.rs` 的真核 `-t` 验证两份工件（沿用 CI 的 v1.19.30 固定版本）。
4. `src/subscription/render/mod.rs` 的单测补 `sniffer` 存在性与端口断言。

## 验收

- `sbctl sub --format clash` 与 `clash-1.18` 产物在 mihomo `-t` 下通过。
- 文档 `docs/subscription-guide.md` 更新嗅探说明。
- 单测/真核 CI 全绿。
