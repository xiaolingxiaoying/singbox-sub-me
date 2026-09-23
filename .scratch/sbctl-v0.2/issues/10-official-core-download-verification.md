# S5/S6：内核版本一致性与官方下载校验

Status: ready-for-agent
Type: task
Blocked by: 01

## 现状

- 一键安装（release `install.sh`）与完整 `sbctl update` 走签名 manifest，
  sing-box 版本被钉为 release 打包版本（默认 1.12.0，`release.yml:151`，
  `scripts/generate-manifest.sh:25`）；README 却承诺「始终最新稳定版」（`README.md:281`）。
- `sbctl install` / `sbctl sing-box update`（无 manifest）走官方最新稳定版，但
  `src/update.rs:231-236,311-328` 只依赖 HTTPS + `sing-box version` 自检，无 SHA/签名。

## 决策点

二选一并写入 ADR：

- A：一键安装改为「安装 release manifest 固定的内核，随后 `sbctl sing-box update` 到最新稳定版」，
  README 修正为两段式描述；
- B：保持 manifest 携带固定内核，README 删除「始终最新稳定版」表述。

推荐 A（更接近目标文档「使用最新的稳定版内核」）。

## 动作

1. 按选定方案改 `scripts/install.sh` / `sbctl update` / README。
2. 官方路径增加完整性校验：优先 GitHub Release API 提供的官方 SHA-256 摘要并在无摘要时显式警告；
   把「无签名」风险写入 ADR 与 README 安全边界。
3. 测试：manifest 安装 + 官方更新两条路径各有 CLI 集成用例（可用假内核桩）；
   新增「官方路径失败时回滚」用例。

## 验收

- README 与服务端实际行为逐条一致。
- 两条更新路径都有失败回滚测试。
- ADR 记录官方路径的信任边界。
