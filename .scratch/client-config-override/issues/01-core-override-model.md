# core：覆写模型与合并引擎

Status: ready-for-agent
Type: task
Blocked by: sbctl-v0.2/issues/01

## 动作

1. `settings.rs`：新增 `override_path(dir, profile_name)`（hash 与 `profile_cache_path` 一致）；
   删除档案/清缓存时同步删除覆写文件。
2. `core.rs`：`runtime_config` 在订阅原文之后、强制保留字段之前应用覆写：
   - 解析覆写 JSON（错误 → 明确报错，含字段路径）；
   - `rules` 前插；对象深度合并；数组整体替换；
   - 再次强制写入 clash_api 随机端点/secret 与 `route.auto_detect_interface`；
   - 执行 `sing-box check`，失败返回覆写来源。
3. `command.rs`：`SetOverride { profile, contents }`、`ClearOverride { profile }`。
4. `state.rs`：快照发布 `override_summary: Option<String>`（存在与否、字节数、字段数）与
   `override_error: Option<String>`。
5. 测试：合并语义（保留字段、前插、替换）、非法 JSON、字段路径错误信息、档案删除清理、
   `UpdateSubscription` 后重新应用。

## 验收

- 单测覆盖上述 5 类场景；`cargo test -p client-core` 通过。
- 覆写后 `cache/active-config.json` 中 `experimental.clash_api.external_controller`
  仍为随机地址，secret 仍随机。
