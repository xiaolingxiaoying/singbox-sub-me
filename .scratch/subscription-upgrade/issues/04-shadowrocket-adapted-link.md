# Shadowrocket 适配订阅链接

Status: resolved
Type: task
Blocked by: 01

## 目标

提供 `shadowrocket.txt`：面向 Shadowrocket 的 Base64 URI 订阅，参数按 Shadowrocket 的解析习惯适配。

## 前置研究

确认 Shadowrocket 当前版本对 vless(reality)/vmess/hysteria2/tuic/anytls 的 URI 参数要求（哪些参数名 SR 必需、哪些值格式不同），标注支持的最低版本。

## 交付范围

- 新增 `shadowrocket(config, nodes)` 生成函数：以现有 `uri()` 为基础，按研究结果调整参数（如 vmess 的 aid/scy/net/type/host/path、hy2 的 mport/insecure/sni、tuic 的 alpn/congestion_control、vless 的 fp/pbk/sid/type、anytls 支持性与回退说明），节点备注名统一可读格式。
- 输出 Base64（整体），Content-Type text/plain。
- 工件 `subscription-shadowrocket.txt` 纳入生成与事务。
- 若 SR 不支持某协议（如 anytls），该节点跳过并在 index 页说明。

## 验收标准

- [ ] `shadowrocket.txt` 在 Shadowrocket 真机导入成功（用户真机验证，先以单测固定 URI 形态）。
- [ ] 五协议 URI 均含研究确认的必需参数；不支持的协议被跳过并有说明。
- [ ] `cargo fmt/clippy/test` 通过。

## 相关规格

`.scratch/subscription-upgrade/spec.md`

## Comments

- 2026-09-14：shadowrocket.txt 实现百分号编码、anytls 官方 URI 规范、tuic udp_relay_mode；真机导入待用户验证。
