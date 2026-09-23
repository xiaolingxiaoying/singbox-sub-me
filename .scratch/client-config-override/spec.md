# 客户端配置覆写（client-core + TUI）

Status: ready-for-agent

## 目标（目标文档原文）

TUI 必须能「覆写配置文件内容」。

## 现状

- 引擎每次启动自动生成 `cache/active-config.json`，强制改写
  `experimental.clash_api.external_controller`（随机端口 + secret）与
  `route.auto_detect_interface`，并整体替换 `inbounds`（`crates/client-core/src/core.rs:65-83,160-184,549-587`）。
- 唯一「用户提供配置」入口是把本地 sing-box JSON 导入为新档案（`ImportProfileFile`），
  启动时仍被上述强制改写覆盖。
- 没有任何查看/编辑/合并配置的命令或界面。

## 设计约束

- 覆写是每档案（profile）的持久文件，与订阅缓存分离：
  `overrides/<profile-sha256>.json`（与 `settings::profile_cache_path` 同一哈希口径）。
- 合并语义：
  - `rules` 数组前插（与服务端 ADR-0021 一致）；
  - 其余对象深度合并；
  - `experimental.clash_api`（external_controller/secret）与
    `route.auto_detect_interface` 是保留字段，覆写不得修改（控制通道隔离）；
  - `inbounds` 允许覆写，但覆写结果仍需通过最终校验；TUN/系统代理模式切换
    仍可整体替换 `inbounds`（以界面开关为准，并在界面明示）。
- 订阅更新后重新应用覆写；档案删除时一并删除覆写文件。
- 合法性：启动前 `sing-box check`；非法覆写阻止启动并报出具体字段路径。

## 工单

- `issues/01-core-override-model.md`
- `issues/02-tui-override-ui.md`

## 验收

- 覆盖 `dns`/`route.rules`/`outbounds` 后，运行配置为合并结果，且 clash_api 保留字段未被改动。
- 覆写非法时内核不启动，状态行给出字段路径。
- TUI 可查看、编辑、清空、校验覆写；重启内核后生效。
- 测试：core 合并单测（含保留字段、rules 前插、数组替换）、CLI/引擎集成测试。
