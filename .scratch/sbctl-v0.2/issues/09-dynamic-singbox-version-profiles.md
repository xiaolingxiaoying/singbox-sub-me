# S10：sing-box 版本 profile 动态化

Status: ready-for-agent
Type: task
Blocked by: 01

## 现状

`src/subscription/profile.rs:136-189` 是常量注册表（1.10–1.14），
`sing-box-full.json` 固定取最后一项；内核升到 1.15 时不会新增 `sing-box-1.15.json`，
也不会更新 full 的版本语义。CI 真核矩阵同样手工枚举（`.github/workflows/ci.yml:83-96`）。

## 动作

1. 引入「最新稳定 minor」来源：配置项 + 安装/更新时探测（`src/update.rs::fetch_latest_official_sing_box_version` 已有）。
2. profile 集合 = `[latest-4 ..= latest]`，按字段差异开关（`typed_dns`、`route_rule_actions`、
   `supports_anytls`、`supports_store_dns`）从模板派生；无法确认字段差异的新 minor 默认按最新模板生成并标注。
3. 当安装的内核版本高于注册表时，`sing-box-full.json` 取实际内核 minor，其余 4 个保留。
4. CI 真核矩阵改为脚本从 `profile.rs` 导出的版本列表生成，避免手工枚举漂移。
5. 测试：1.15 出现时的派生单测（模拟版本）；现有 1.10–1.14 逐字节回归不变。

## 验收

- `cargo test --test version_profiles -- --ignored` 仍通过全部五个真核。
- 单测证明 latest 变化时 profile 集合与 full 路由同步变化。
- 文档 `docs/research/sing-box-client-version-differences.md` 记录动态化规则。
