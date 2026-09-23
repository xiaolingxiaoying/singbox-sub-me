# 落地 `ClientTemplate` 模板轴（Phase 2 PR(b)，G1/G3 的第一步）

Status: needs-implementation
Type: task
Found: 2026-09-23，由一次只读定位得出。行号是当时的 HEAD，动手前先复核——
计划正文里的 `singbox.rs:159` / `clash.rs:138` 已经漂移，下面给的是当前锚点。

## 今天的结构内容长在哪（要搬进 `TemplateSpec` 的东西）

`src/subscription/render/singbox.rs::sing_box_full`（25-208）：

- 代理组：节点出站来自 `render/mod.rs:49-108` 的 `client_outbounds`；selector `:50-55`、
  urltest `:56-64`、direct `:65`。
- `dns` 对象 `:71-102`；`dns.rules` `:104-125`；`dns["final"]` `:126`。
- `route.rules` `:149-165`：sniff `:151`、hijack-dns `:153-157`、private `:158`、
  AI 后缀 `:159-162`、CN 直连 `:165`。
- `route.rule_set`：vec 初始化 `:163`，填充 `:166-173`（经 `remote_rule_set` `:219-230`）。
- `final` 出站 `:181`。

`src/subscription/render/clash.rs`：

- 代理组 `clash_proxies:69-101`（selector `:69-85`、url-test `:86-96`、direct `:97-100`）。
- `rules` `:164-218`（AI `:165-167`、Standard 分支 `:174-179`、else `:213-217`）。
- `rule-providers` `:180-208`；`dns` 见 `clash_dns:107-125`（在 `:219` 追加）；
  MATCH/final `:179`、`:216`、legacy `:239`。

## `client_rule_profile` 现在怎么做掉的（G3 那个回归就是这里）

- sing-box：`:113` 门控 `geosite-cn` 的 DNS 规则；`:164` 同时门控 CN 直连规则（`:165`）与
  **全部** `rule_set` 条目。`Minimal` 下 `rule_sets` 为空（`:163`）且**没有内联孪生**——
  整类外部资源连带分流一起消失。
- clash：`:168` 门控 `RULE-SET` 规则与 `rule-providers`（`:180-208`），else 分支 `:213-217`
  退成内置 `GEOIP`。由 `singbox.rs:526-586` 的测试固定。

## 配置侧要动什么

- 字段加在 `src/config.rs:78-79` 旁边，`#[serde(default = "default_client_template")]`。
- 枚举照 `ClientRuleProfile`（`:217-235`）的写法：`#[derive(Default, Deserialize, Serialize)]`
  + `#[serde(rename_all = "kebab-case")]` + `#[default] Standard` + 手写 `Display`
  + `default_client_template()`（参照 `:241-243`）。
- 构造点 `:409`；`apply_options` 的保留逻辑 `:526-528`。
- `summary()`（`:844-886`）**没有**列相邻的 `client_rule_profile`，按惯例不必加。
- **不存在 `sbctl config set` 路径**（`ConfigCommand` 只有 Init/SwitchMode/Show/Validate/Wizard/Override）。
  唯一变更入口是向导主题（`wizard.rs:478-518`，提问在 `:489-494`，解析参照 `parse_rule_profile:868-874`）。
  另外 `--client-rule-profile` 这个 CLI 参数**今天也不存在**，所以 `--client-template` 是净新增面，
  最近的模板是 `InstallOptions:149-190` / `Init:196-244`。

## 第一个"真做完"的最小增量（PR(b)）

1. 新建 `src/subscription/template.rs`：`ClientTemplate { Standard, Global, Split }` +
   `TemplateSpec { groups, rule_sets, inline_rules, dns, sniff, final_group }`；
   **`Standard` 就是今天的字面量**。
2. 配置字段默认 `Standard`，两个渲染器改为从 `TemplateSpec` 读——但**默认路径必须保持 JSON 键序**
   （键序一变金标准就动，而 `stack` 那次已经证明"条件插入保持键序"是可行的做法）。
3. `Minimal` 的行为与 `Global`/`Split` 的更丰富字节**这一步都不改**，这样 `singbox.rs:526`
   那条测试继续成立。

## 判据

- 现有 `the_generated_artifact_set_matches_the_pinned_goldens`（`artifacts.rs:753-781`）一字不动：
  13 份金标准全部走 `pinned_five_protocol_config`（Standard，`artifacts.rs:609` 起）。
- 新增一条：显式 `Standard` == 默认 == 今天的内联字面量。
- 新增一条：`Global`/`Split` 让组数与规则集增长，而四份冻结工件
  （`sing_box` / `uri` / `base64` / `shadowrocket`，`artifacts.rs:357-364`）逐字节不变——
  它们根本不经过 `sing_box_full`/`clash`，所以这条断言几乎免费但必须写下来。
- G3 的内联规则孪生与 G2 的嗅探留到 PR(c)，不要混进这一步。
