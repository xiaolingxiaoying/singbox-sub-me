# Serein 风格方案的 GPUI 可实现性映射

这三套方案只使用当前项目所依赖的 GPUI 能稳定表达的界面能力。网页是视觉选择器，不是要求生产代码运行浏览器或 CSS。

## 共用约束

| 视觉/交互 | GPUI 实现方式 | 当前项目证据 |
| --- | --- | --- |
| 横向、纵向和换行布局 | `div().flex().flex_col().flex_wrap()` 与嵌套行列 | `Sbgui::render`、`dashboard` 已使用 |
| 固定侧栏与弹性内容区 | 固定 `w(px(...))` + `flex_1()` + `min_w(px(0.0))` | 主窗口框架已使用 |
| 纯色表面、描边、圆角 | `bg`、`border_1`、`border_color`、`rounded` | 全应用通用面板已使用 |
| 阴影 | `shadow_sm()` 或 `shadow(Vec<BoxShadow>)` | 当前固定版本的 GPUI `Styled` 支持 |
| 图标 | `svg()` + data URI / 内嵌几何 | 当前 `icon()` 已使用 |
| 流量曲线 | `gpui::canvas` + `PathBuilder` + `paint_path` | 当前 `traffic_chart()` 已使用 |
| 纵横滚动 | `overflow_y_scroll()` / `overflow_x_scroll()` + `ScrollHandle` | 页面、连接表与日志已使用 |
| hover、点击和切换状态 | `.hover()` + `.on_click()` + `Context::notify()` | 按钮、开关和导航已使用 |
| 弹窗和浮层 | `absolute()` + `occlude()` + 全窗口遮罩 | 退出确认弹窗已使用 |
| 文本层级与等宽数字 | Segoe UI Variable + Cascadia Mono/等宽字体 | 可通过 `font_family` 在局部设置 |

## 三套方案的生产布局

### 静谧工具

- 左侧 216–236px 固定导航，右侧 `flex_1`。
- 四项指标用两个嵌套的 flex 行；窄窗口切为两行或一列。
- 流量图与节点卡使用 `flex` 比例布局。
- 最接近当前代码，改造成本最低。

### 深空控制台

- 顶部导航 + 主区/右侧状态轨的两列 flex。
- 深色主题只替换 token，不依赖混合模式或透明滤镜。
- 实时亮点为静态状态点；如果后续需要闪烁，可由定时 `notify()` 驱动，但不作为设计成立的前提。

### 晶格工作台

- 所谓“晶格”不依赖 CSS Grid；生产代码使用多行嵌套 flex 和明确比例。
- 错位卡片效果使用 GPUI box shadow，不使用伪元素或浏览器滤镜。
- 顶部导航在窄窗口内水平滚动，内容卡片转为纵向排列。

## 明确排除

- `backdrop-filter`、玻璃模糊和背景采样；
- CSS Grid 才能成立的自动排版；
- 复杂渐变、混合模式和遮罩；
- 必须依赖 CSS transition/keyframes 的状态或反馈；
- DOM、浏览器布局测量或第三方 Web 组件。

网页底部的方案选择器属于评审工具，不进入 GPUI 生产实现。
