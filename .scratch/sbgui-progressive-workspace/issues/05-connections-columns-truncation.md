# 连接表两列被截断，其中建立时间还是原始 ISO 串

Status: resolved
Type: task

## 现象

第一张渲染出表体的截图（`.scratch/sbgui-conn-live/1440x900-connections.png`）暴露两处：

- 建立时间列显示 `2026-09-22T1…`，既是原始 ISO 字符串（未本地化、未相对化），又因列宽不够被省略号截断。
- 命中规则列显示 `ip_is_private=true…`，同样被截断；这一列在 1440 宽度下本应有空间。

## 影响

`acceptance.md` 视觉签核要求"标签在空间允许时不换行、数据值保持稳定"。原始 ISO 串既不稳定（同一页其他时间用的是相对描述）也不可读。

## 下一步

1. 建立时间改成人能读的形式：连接在跑，最有意义的不是"几点开始"而是"跑了多久"，与 `client-core::format::age_label` 的思路一致；绝对时间放进行内详情。
2. 调 `crates/sbgui/src/components.rs` 里连接表的列宽分配（规则列加宽、时间列在改语义后收窄），并用 `scripts/sbgui-shot/` 重截连接页确认两列都不再出现省略号。

## Comments

2026-09-23 完成：

- `client-core::format::connection_start_epoch` 解析 clash_api 的 RFC3339 起始时间（含测试：
  UTC 带纳秒、显式偏移、垃圾输入回落为"刚刚"），中文相对时长继续走 `age_label`；
  `sbgui::lang::connection_age_label` 提供对应的英文措辞。
- 规则列 150→190，建立时间列 100→110，表格最小宽 920→960；截图确认
  `ip_is_private=true => route…` 前缀完整可读、建立时间显示 `0 分钟前`。
- 证据：`.scratch/sbgui-g1-fix-3/1440x900-connections.png`（同批 3/3 运行）。
