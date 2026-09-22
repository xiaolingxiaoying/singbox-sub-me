# 纯 flex 原型的两栏分配仍与 grid 版差十几像素

Status: ready-for-agent
Type: task

## 现象

原型去掉 CSS Grid 后，在同一个顶层视口下逐元素比对 rect（`.scratch/proto-flex/gridTop.json` vs `topFlex-a/b.json`，比对脚本 `diff.py`），八个页面里绝大多数元素完全一致，只有"按 fr 比例瓜分剩余宽度"的两栏布局还差一点：

- 概览 `.bottom-cols`：`.subscription-block` 432 → 448.5（+16.5），`.events-block` 681 → 665，事件行整体右移 16.5px。
- 设置 `.settings-layout`：`.settings-detail` 697 → 712（+15），`.settings-list` 439.5 → 424.5。
- 节点 `.strategy-layout`：本视口下与 grid 版一致；在 1440 下的另一轮量到左右各差 11px。

单页偏差 1%–1.5%，文本左对齐、开关右对齐，肉眼看不出；订阅表、连接表、规则表、日志表、关于页、标题栏、侧栏、指标带全部逐像素相同。

## 已知与未知

能确定的是：这类偏差只出现在"两栏瓜分剩余空间"的容器上，且总是**带 padding 的那一栏变宽**，另一栏被挤掉同样的量——`flex-basis: 0` 时 item 的自身 padding+border 构成了它的地板，先扣掉再按 grow 分配；grid 轨道没有这层耦合。

没能确定的是为什么 `.group-nodes`（padding 16px）这一轮与 grid 一致，而结构相同的 `.settings-detail`（padding 20px）差了 15px。动手修之前先把这条量清楚，不要照着猜测改。

## 下一步

1. 若要严格对齐：把 `.subscription-block` 与 `.settings-detail` 的内边距移到内层包裹元素上，让被分配的那一栏自身不含 padding，再用同一套 rect 比对复测。
2. 若接受现状：在 `spec.md` 的验收映射里记一句"两栏分配按 flex 语义，允许 ±16px"，本工单结案。

## 参考

本轮同时确认：`place-items`/`grid-template`/`grid-row`/`grid-column` 在 `src/` 下已为 0；日志页的 `Ⅱ` 文本暂停图标改为 `Pause`（已登记进 `design-kit/icon-manifest.md` 与 `IconSet.jsx`）；`npm run build` 与 `npm run test:sites`（6 项）均通过。
