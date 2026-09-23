# 订阅页最小窗口下操作列被裁

Status: resolved
Type: task
Blocked by: sbctl-v0.2/issues/01

## 现象

860×640 下订阅表的操作列整体落在窗口右缘之外，删除/更新/设为当前不可点
（`.scratch/sbgui-min/860x640-subscriptions.png`）。根因：表格容器是
`overflow_hidden` 且没有横向滚动，五列的最小轨道宽度之和超过可用内容宽。

## 修复

- 表格外包一层 `overflow_x_scroll`（与连接页同一模式），最小宽 600，任何情况下不再裁切。
- 视口宽 < 940 时进入紧凑布局：隐藏「上次更新」列，名称最小 120、用量最小 130、状态 64。
- 操作列改为固定宽（窄屏 150 / 宽屏 170），表头与数据行因此对齐；此前自适应宽度
  让表头「操作」与行内三个按钮错位。

## 证据

- 修复前：`.scratch/sbgui-min/860x640-subscriptions.png`
- 修复后：`.scratch/sbgui-g2-fix/860x640-subscriptions.png`、`1440x900-subscriptions.png`
  （`scripts/sbgui-shot/`，DEMO_CORE 种子，zh 界面）

## 验收

- 860×640：操作列完整可见，表头与行对齐，无省略号。
- 1440×900：五列布局与修复前视觉一致（固定操作列宽导致的正常位移除外）。
- `cargo clippy -p sbgui --all-targets -- -D warnings` 通过。

## Comments

2026-09-23 修复并复验。复验期间发现截图 harness 的另一个问题（已单独提交）：
Windows 绑定挂载与容器的 mtime 不一致时 cargo 会跳过重建，截图可能仍是旧像素；
`scripts/sbgui-shot/inside.sh` 现在在构建前 touch 工作区源码。
