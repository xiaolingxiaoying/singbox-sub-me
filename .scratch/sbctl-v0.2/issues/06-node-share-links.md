# S7：显示每个协议节点的分享链接

Status: resolved
Type: task
Blocked by: 01

## 目标（目标文档原文）

「同时显示每个协议节点的协议链接。」

## 现状

- `sbctl node` 只输出协议与端口（`src/cli/commands/status.rs:19-30`）。
- `sbctl sub` / 菜单 / QR / index 页只展示订阅链接与订阅二维码。
- 分享链接只存在于 `/sub/<cred>/uri`、`uri.txt`、`shadowrocket.txt` 工件内，
  必须下载后自行查看。

## 设计约束

- 分享链接含 Proxy credential，属于敏感输出：默认不显示；显式命令才打印。
- 所有输出必须来自 `canonical::nodes()`，不得二次拼装字段。
- 与 `render/uri.rs` 共用同一渲染函数，避免三处实现漂移。

## 动作

1. 新增 `sbctl node --links [--protocol <name>]`：按协议打印分享 URI（明文）+ 中文标注；
   默认（不带 `--links`）保持现状不泄露凭据。
2. `sbctl node --links --qr` 可选打印每个链接的终端二维码。
3. 菜单「节点与协议」增加「查看协议链接」子项，进入前二次确认。
4. index 页增加可折叠「单节点链接」区块（默认折叠，展开才显示；仍走同一订阅凭据路径）。
5. 测试：五协议 × 分享链接与 `render/uri.rs` 逐字节一致；默认 `node` 输出不含
   UUID/password/公钥以外的凭据；`--protocol` 过滤；帮助文本。

## 验收

- `sbctl node --links` 输出 5 条可被 sing-box/v2rayN 解析的 URI。
- 未显式请求时任何命令都不打印节点凭据。
- 单测 + CLI 集成测试通过。

## Comments

已实现：

- `sbctl node --links [--protocol <name>] [--qr]` 显式打印分享链接；保留 `--uri` 兼容别名。普通 `sbctl node` 仍只打印节点摘要。
- 分享链接由 `canonical::nodes()` 与唯一的 `subscription::node_share_link()` 路径生成；每条链接增加中文协议标签。`--protocol` 可过滤协议，`--qr` 需要同时传 `--links`。
- 菜单「节点与协议」中的链接入口在显示凭据前二次确认。订阅 index 的单节点链接移入默认折叠的 `<details>` 区块。
- `tests/cli/subscription_formats.rs` 覆盖五协议 URI 行和 `uri` 工件逐字节相等、五个中文标签、默认节点摘要不泄露节点密码/Reality 私钥/short ID、订阅凭据不混入节点链接、过滤、QR、帮助文本和 index 折叠状态。

验证结果：

- `cargo test -p sbctl --features test-signing --test cli subscription_formats::`：17 passed。
- `cargo clippy -p sbctl --all-targets --features test-signing -- -D warnings`：通过。
- `cargo fmt --all -- --check`：通过。

提交后还需等待 dev-sbctl GitHub Actions 全绿；线上 VPS 和人工客户端流量验收属于总体验证计划的后续项目。
