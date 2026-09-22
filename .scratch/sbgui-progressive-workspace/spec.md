# sbgui 按 Progressive Workspace 原型重建

Status: claimed

## 目标

把 `crates/sbgui` 的界面与交互对齐 `prototypes/sbgui-progressive-workspace/design-kit/`，功能完整可用。完成与否以 `design-kit/acceptance.md` 的必查项为准，不以"看起来像"为准。

## 验收映射（截至 2026-09-22）

| kit 必查项 | 状态 | 证据 |
| --- | --- | --- |
| 1/2 原型自身构建与测试 | 通过 | 2026-09-22 去 Grid 改版后复跑：`npm run build` exit 0，`npm run test:sites` 6/6 |
| 3 八个目的地全部可达（含 About） | 通过 | `.scratch/sbgui-zh-final/`、`.scratch/sbgui-en-final/` 逐页截图 |
| 4 390×844 无横向溢出 | 不适用 | 生产窗口下限 860×640，窄屏已正式放弃，见 `issues/03-narrow-breakpoints.md` |
| 5 语言按钮在中英之间切换且不丢页面 | 通过 | `.scratch/sbgui-en-final/`、`.scratch/sbgui-rules-en2/` |
| 6 标题栏无设置快捷键 | 通过 | `.scratch/sbgui-zh-final/*.png` |
| 7 节点页分组优先、紧凑行 | 通过 | `.scratch/sbgui-nodes-v1/` |
| 8 停止内核需确认 | 通过 | `.scratch/sbgui-stop-v2/` |
| 9 控制台无 error/warn | 不适用 | 该项针对浏览器原型 |

英文覆盖度存在已知缺口：引擎状态与事件文案仍是中文，见 `issues/02-engine-text-english.md`。

## 验证手段

`scripts/sbgui-shot/`：Docker + Xvfb 无头渲染 Linux 版 sbgui，逐页截图并把内核日志、clash_api 当时的行数一起打进输出。宿主上只允许纯函数单测和编译，任何界面验证都在容器里做。

种子由 `scripts/sbgui-shot/seed-demo.sh` 提供：两份订阅、运行中的内核、10 条路由规则、真实 INFO 日志、以及经混合端口保持打开的慢连接。

## 范围决策

2026-09-22 维护者已裁决三项：

- 窄屏：保持 860×640，acceptance #4 标为不适用（`issues/03`）。
- 概览卡片带：以原型稿为准，kit 措辞收窄为"其他页面不要滥用卡片"（`issues/06`）。
- 英文态引擎文案：走共享层输出机器可读事件码、各界面自行渲染（`issues/02`）。方向已定，实现未开始，acceptance #5 在此之前只算部分达成。

## 已知未完成

- 连接页"内核有连接、界面 0 行"：归因尚未证实，先补时序证据（`issues/01`）。
- 连接表建立时间列显示原始 ISO 串且被截断（`issues/05`）。
- 真实 Windows 只跑过一轮且不完整（`issues/04`）。
- 原型改纯 flex 后，两栏瓜分剩余宽度的容器与 grid 版差 11–16.5px（`issues/07`）。
