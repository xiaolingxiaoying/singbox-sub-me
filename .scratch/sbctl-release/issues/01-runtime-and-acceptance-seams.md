# 建立真实运行时与 acceptance 测试边界

Status: resolved
Type: task
Blocked by:

## 目标

为发布版实现建立稳定的高层测试 seam，覆盖时间、网卡计数器、命令执行、临时 root 和 systemd fixture，同时保持现有 CLI 行为兼容。

## 交付范围

- 提供可测试的时间与主机文件系统/计数器读取边界。
- 提供隔离 root 下的 systemd、命令和网络 fixture 机制。
- 为 acceptance 测试定义统一的管理员可见入口，不依赖私有 Rust 实现细节。
- 记录 WSL2 仅用于开发/单测，真实 systemd VM 用于生产验收的边界。

## 验收标准

- [x] 现有 `cargo test` 与 `tests/cli.rs` 全部通过。
- [x] 测试可以固定当前时间并构造网卡 RX/TX、boot ID、systemd 和证书 fixture。
- [x] fixture root 下的失败操作不会修改宿主真实服务、文件或防火墙。
- [x] 至少有一个黑盒 CLI acceptance 流程验证配置生成、状态读取和 artifact 完整性。
- [x] 后续 tickets 可以通过该 seam 验证行为，无需新增 mock-heavy 内部接口。

## 相关规格

`.scratch/sbctl-release/spec.md`、ADR-0017、ADR-0018

## Comments

- 2026-09-03：新增 `runtime` seam，提供 `SystemClock`/`FixedClock`、隔离 root 文件读取和 rooted command 执行；fixture 模式拒绝绝对路径、路径逃逸和缺失命令回退。
- 2026-09-03：流量 reconciliation 增加显式 runtime 入口，保留原有 CLI 与 `reconcile_at` 兼容入口。
- 2026-09-03：抽出 `tests/acceptance/fixture.sh`，统一构造网卡计数器、boot ID、systemd shim、证书和卸载 fixture；补充 acceptance 边界文档。
- 2026-09-03：验证通过 `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、27 个库测试、31 个 CLI 测试和全部 acceptance shell 的 `sh -n`。当前 WSL2 无 Docker，未执行真实 systemd 容器验收。
