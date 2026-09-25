# S10：sing-box 版本 profile 覆盖策略

Status: resolved
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

## 决策：保持人工核实与 CI 硬门禁

不自动把注册表滚动到 `[latest-4 ..= latest]`，也不为未研究过的新 minor 复用上一档字段。
ADR-0020 要求 profile 差异来自经上游 changelog 核实的字段；按版本号推断后自动对外发布会绕过
这项兼容性审查。用户选择由维护者判断，基于这一安全与兼容性约束保留当前策略：新 minor 发布时，
维护者先研究 schema、加入字段 profile 与真实核心 pin，再由 CI 验证并放行。

## 验收

- 新 minor 与已发布注册表不一致时，`sing-box-profiles` CI 失败，阻止静默版本漂移。
- 每个注册 profile 都必须通过真实对应版本 sing-box 核心校验。
- 尚未审查的新 minor 不加入订阅 profile；加入时更新字段研究与真实核心 pin。

## Comments

2026-09-25：用户授权代理自行选择，采用上述人工维护策略。CI run `36140709525` 在
`0ddd1a870d7a8ae7ed43f34b2d427648de7bc334` 上验证注册表顶部跟踪最新稳定版，并对全部注册
profile 执行真实 sing-box 核心检查，结果 success。
