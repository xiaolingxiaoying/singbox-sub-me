# S8：mihomo 订阅补嗅探模板

Status: resolved
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

## Answer

已实现并按真实 mihomo 接受的配置语法收敛。最初提议的 `sniff: { HTTP/TLS/QUIC: ... }`、`override-destination` 以及错误示例中的 `domain`/`dns` 不能直接照搬：用 CI 固定的 mihomo v1.19.30 真核探针验证后，工件采用 `sniffer: { enable: true, sniffing: [http, tls, quic] }`；不设置 `override-destination`，也不从订阅覆盖客户端自己的 `tun` / `dns-hijack`。

两份 Clash 工件共享 `clash_sniffer()`；单测锁定嗅探只出现一次、两个文件字段一致。`mihomo-profiles` CI job 用固定真核加载所有 Clash 工件通过。订阅指南记录受支持字段和探针结论。

验证证据：`cargo test -p sbctl --features test-signing` 的单测（202 passed）；真实 Mihomo CI job 通过（run [36125841380](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36125841380)）。当前提交的完整 CI run [36129918694](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36129918694) 仍在运行，另含相同 Mihomo 真核 job。
