# S10：sing-box 版本 profile 覆盖策略

Status: needs-triage
Type: task
Blocked by: 01

## 现状（2026-09-25 复核）

- `SING_BOX_VERSION_PROFILES` 仍是编译期注册表，当前覆盖 1.10–1.14；版本链接、生成工件、
  profile notes 与字段差异均从这份注册表派生。
- `sing-box-full.json` 并非始终固定取最后一项：重生成时会探测已安装 sing-box，按注册表
  从新到旧选择该内核实际通过 `sing-box check` 的最高档；不可用或都不接受时才回退最新注册档。
- `sbctl status` 会提示已安装内核高于注册表上限，但不会中断服务。
- `sing-box-profiles` CI 从 GitHub `releases/latest` 读取稳定版，并将其与注册表顶部比较；
  每个注册档都必须有真实内核校验，少一个环境变量就失败。上游发布新 minor 时 CI 会阻止
  漂移，但注册表和历史 core 版本仍需维护者显式更新。
- 截至本次复核，上游最新稳定版为 1.14.x，与注册表上限一致。

## 尚未决定的范围

自动把注册表滚动到 `[latest-4 ..= latest]`，并为未研究过的新 minor 复用上一档字段，
会把“CI 验证后发布”改成“按版本号推断字段后先对外发布”。ADR-0020 要求 profile 差异
来自经上游 changelog 核实的字段；因此不能在没有明确决策的情况下直接实施自动猜测。

需要维护者选择：

1. **保持人工维护、CI 硬门禁**：只在确认新 minor 的字段差异并下载真实核心校验后，加入新档；
   CI 已解决静默漂移，但不承诺自动滚动版本带。
2. **改为自动滚动五档**：定义新版本发布到订阅生效之间的审查门、失败回退方式、字段未知时的
   用户标注与兼容承诺，再修订 ADR-0020 和安装/重生成流程。

## 验收（选项 2 时）

- 模拟上游出现 1.15 时，新五档、链接矩阵与 `sing-box-full` 目标同步更新。
- 最新五个 profile 全部用真实核心校验；未知字段不会绕过发布门禁。
- 旧版本移出五档后的兼容行为和迁移说明明确。
- `docs/research/sing-box-client-version-differences.md` 与 ADR-0020 更新。
