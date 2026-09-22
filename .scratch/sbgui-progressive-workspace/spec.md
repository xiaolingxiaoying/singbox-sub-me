# sbgui 按 Progressive Workspace 原型重建

Status: claimed

## 目标

把 `crates/sbgui` 的界面与交互对齐 `prototypes/sbgui-progressive-workspace/design-kit/`，功能完整可用。完成与否以 `design-kit/acceptance.md` 的必查项为准，不以"看起来像"为准。

## 验收映射（截至 2026-09-22）

| kit 必查项 | 状态 | 证据 |
| --- | --- | --- |
| 1/2 原型自身构建与测试 | 通过 | 原型目录内 `npm run build` / `test:sites` |
| 3 八个目的地全部可达（含 About） | 通过 | `.scratch/sbgui-zh-final/`、`.scratch/sbgui-en-final/` 逐页截图 |
| 4 390×844 无横向溢出 | **未验证** | 见 `issues/03-narrow-breakpoints.md` |
| 5 语言按钮在中英之间切换且不丢页面 | 通过 | `.scratch/sbgui-en-final/`、`.scratch/sbgui-rules-en2/` |
| 6 标题栏无设置快捷键 | 通过 | `.scratch/sbgui-zh-final/*.png` |
| 7 节点页分组优先、紧凑行 | 通过 | `.scratch/sbgui-nodes-v1/` |
| 8 停止内核需确认 | 通过 | `.scratch/sbgui-stop-v2/` |
| 9 控制台无 error/warn | 不适用 | 该项针对浏览器原型 |

英文覆盖度存在已知缺口：引擎状态与事件文案仍是中文，见 `issues/02-engine-text-english.md`。

## 验证手段

`scripts/sbgui-shot/`：Docker + Xvfb 无头渲染 Linux 版 sbgui，逐页截图并把内核日志、clash_api 当时的行数一起打进输出。宿主上只允许纯函数单测和编译，任何界面验证都在容器里做。

种子由 `scripts/sbgui-shot/seed-demo.sh` 提供：两份订阅、运行中的内核、10 条路由规则、真实 INFO 日志、以及经混合端口保持打开的慢连接。

## 未决范围决策

- 概览页保留卡片式指标带，与 kit"避免卡片网格"冲突：见 `issues/06-card-view-vs-kit.md`。
- `window_min_size` 860×640 与窄屏断点 820/560 的取舍：见 `issues/03-narrow-breakpoints.md`。
- 真实 Windows 平台尚未运行过一次：见 `issues/04-real-windows-run.md`。
