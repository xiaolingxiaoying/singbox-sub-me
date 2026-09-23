# TUI 的「入站与分流规则」页（G10 的视图半边）

Status: needs-implementation
Type: task
Found: 2026-09-23，Phase 5 的 G10。数据层已落地并被测试覆盖，缺的是视图。

## 已经在仓库里的事实

- `client_core::state::InboundInfo` + `parse_inbounds()`（坏 JSON / 缺字段一律容错），
- `ClientSnapshot.inbounds` 在两处被填：控制器启动时读 `cache/active-config.json`（:226）、
  内核起来后用 `started.config` 重解析（:716）。也就是说**视图要的数据已经在快照里了**，
  不需要再碰 client-core。

## 要改的 10 个点（都是机械改动，一次做完）

1. `crates/sbtui/src/app.rs`：`Tab` 加 `Rules`（放在 `Settings` 之后，index=5，
   这样 `tab-0..4` 的既有金标准不用重编号）；同步 `next` / `previous` / `from_index` / `index`。
2. 删除 `App::show_rules` 字段与初始化（第 3 步之后没有别的使用者）。
3. `crates/sbtui/src/input.rs`：`KeyCode::Char('r') if app.tab == Tab::Logs` 从
   `app.show_rules = !app.show_rules` 改成 `app.tab = Tab::Rules`；再加一条
   `Tab::Rules` 上的 `r` 回 `Tab::Logs`。
4. 新建 `crates/sbtui/src/view/rules.rs`：`draw_rules` + `rules_lines`（把 settings.rs 里的
   `rules_lines` 搬过来，前面加一段「── 入站 ──」：`kind:port（listen） · tag`，
   `port==0` 不显示端口、`listen` 为空显示「全部地址」）。`short_label` 来自 `crate::style`。
5. `view/settings.rs`：删掉搬走的 `rules_lines` 与它的 `rules_lines_render_the_engine_snapshot` 测试
   （随第 4 步迁到新文件，别丢）。
6. `view/logs.rs`：删掉 `if app.show_rules` 的两处分支与 `rules_lines` 的 import，改模块注释。
7. `view/mod.rs`：`mod rules;` + `use crate::view::rules::draw_rules;` + `TAB_TITLES` 由
   `[&str; 5]` 改成 `[&str; 6]` 并加「入站」。
8. `view/mod.rs` 的 `draw` 分发与 `draw_footer` 提示各加一条 `Tab::Rules`。
9. `view/mod.rs` 的渲染脚手架：`frame_for(tab, snapshot, show_rules)` 去掉第三个参数，
   `every_tab_renders_the_published_snapshot` 的列表加 `Tab::Rules`；
   `dense_snapshot()` 里现在写死的 `inbounds: Vec::new()` 换成两条（mixed:2080 与 tun），
   否则新金标准里那一节是空的，等于没测。
10. 跑 `INSTA_UPDATE=always cargo test -p sbtui` 之后**逐行看 diff**：TAB_TITLES 变长会让
    所有 6 个金标准的标签行都动，这是预期的；除此之外的变化都要能解释。

## 为什么这一版没顺手做完

数据层与视图被有意分开提交：`inbounds` 字段只有测试在读、界面还没读它，是一个完整但
未接线的库层改动；把 10 个点摊在最后几个回合里做，风险是留下一个渲染不对、金标准半更新
的工作树。宿主门当前 fmt 0 / clippy 0 / 376 通过。
