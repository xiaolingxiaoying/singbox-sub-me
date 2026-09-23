# 订阅工件的 JSON 键序随构建形态变化，可触发无谓的 sing-box 重启

Status: needs-triage
Type: bug
Found: 2026-09-23，由 Phase 0.2 的工件金标准抓到（`src/subscription/artifacts.rs` 的
`the_generated_artifact_set_matches_the_pinned_goldens`）。

## 现象

同一份 `DeploymentConfig`，只改变构建方式，生成的工件字节就不同：

```text
cargo build -p sbctl            → serde_json 按字典序输出对象键
cargo build --workspace         → 某个 workspace 成员把 serde_json/preserve_order 统一打开
                                  → 按 json! 的插入序输出
```

`cargo tree -e features --workspace -i serde_json` 里能看到
`serde_json feature "preserve_order"`；`-p sbctl` 的树里没有。

## 已经处理的部分

`subscription-uri.txt` / `subscription-base64-uri.txt` / `subscription-shadowrocket.txt`
是 ADR-0021 明确**字节冻结**的工件，而 `vmess://` 的 base64 载荷内部就是 JSON——键序一变，
冻结的字节就变。已在 `src/subscription/render/uri.rs` 用 `vmess_payload()` 手工按字典序
拼装载荷，两种构建形态下产出同一行 base64（已逐字节比对确认与修复前的生产字节完全一致）。

## 仍然存在的风险

`sing-box-server.json` 和各 `sing-box-*.json` / `clash*.yaml` 不在冻结范围内，但它们的字节
**参与 `apply_config_transaction` 的 `artifacts_changed` 比较**
（`src/subscription/artifacts.rs:177-215`）：只要任一工件字节变化，事务就会重启被管服务。

因此：如果有人用 `cargo build --workspace --release` 产出二进制并部署到一台用根包构建的二进制
跑着的 VPS 上，升级会因为纯键序差异重启一次 sing-box，且没有任何配置变化可以解释。
`release.yml:66` 目前用的是根包构建，所以生产路径暂时安全——这是一个"没人踩过"而不是"不存在"的坑。

## 可选的收口方式（需要决策，别默认选第一个）

1. 在根 `Cargo.toml` 显式声明 `serde_json = { version = "1", features = ["preserve_order"] }`，
   让插入序成为唯一形态。代价：**所有 JSON 工件的字节变一次**，升级到该版本时每台 VPS 重启一次
   sing-box（一次性、可解释），换来此后与构建方式彻底无关。
2. 让 `artifacts_changed` 对 JSON 工件按语义比较（解析后比较，忽略对象键序）。代价：多一处
   规范化逻辑，且"字节相同"这一强保证被削弱。
3. 只在文档里写明"发布二进制必须由根包构建"，不改代码。代价：靠人守。

金标准目前对 JSON/YAML 做显式键排序的规范化快照（钉内容），对文本工件钉字节，所以本 ticket
的任一选项都不会让测试失效。

## 影响面

任何 `json!` 构造的对象：`sing_box_server`、`sing_box_full`、`clash`（YAML 侧由 serde_yaml
自己决定顺序，同样需要确认）。
