# DNS/规则/外部资源/覆写/代理组配置化

Status: resolved
Type: task
Blocked by: 02, 03

## 目标

把 DNS、分流规则、外部资源 URL、延迟探测与代理组模板做成 `DeploymentConfig` 可配置项，并提供服务端覆写机制。

## 交付范围

- `src/config.rs` 新字段（`#[serde(default)]`，缺省值保持升级后的默认行为）+ `validate()`：
  - `client_dns_mode`: `fake-ip`（默认）/ `redir-host`
  - `client_rule_profile`: `standard`（默认，远程 rule-set）/ `minimal`（内置规则）
  - `client_rule_set_base_url`: 默认 jsDelivr + MetaCubeX/meta-rules-dat；可换镜像
  - `client_latency_probe_url`: 默认 `http://aliyun.com/generate_204`（对齐最近提交）
- `src/wizard.rs` + `config init`/`config wizard` CLI 增加对应提问（带默认值，回车保留）。
- 覆写机制：`etc/sbctl/overrides/clash-override.yaml` 与 `sing-box-override.json` 深度合并进对应订阅工件（数组按策略合并：rules/proxy-groups 追加在前，其余键覆盖）；新 CLI `sbctl config override {show,edit,validate,clear}`；validate 做结构校验（YAML/JSON 可解析、合并后 sing-box 侧可跑真核 check）。
- 生成事务纳入 override 读取；override 文件变更后 `sbctl restart` 触发再生成。
- 新 ADR：客户端配置 profile 化与覆写模板（记录决策与供应商中立边界）。

## 验收标准

- [ ] 缺省配置生成结果与不带 override 的升级版工件一致。
- [ ] override 演示文件（把 novixlink-override.yaml 的 chatgpt/x.com 规则收编）合并后规则出现在产物头部。
- [ ] 非法 override（不可解析/类型错误）被 validate 拒绝且不影响现有工件。
- [ ] `cargo fmt/clippy/test` 通过；新增合并/校验单测。

## 相关规格

`.scratch/subscription-upgrade/spec.md`、ADR-0018

## Comments

- 2026-09-14：config 四字段 + validate + wizard ClientTemplate 主题 + override 深度合并（rules 前插）+ config override CLI 已实现；ADR-0021 已写。
- 2026-09-14（收口）：`config override validate` 现在生成合并后工件并用真核 `sing-box check` 校验（`resolve_sing_box_bin` 支持显式/托管/PATH）；新增 override 合并端到端单测（rules 前插、bare 与 URI 工件不受影响）以及非法 override 中止且不修改盘上工件的单测。
