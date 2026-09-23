# WSL 门禁脚本（先拷贝再构建）

Status: ready-for-agent
Type: task

## 契约

新增 `scripts/dev/wsl-gate.sh`：

```bash
SRC=${SRC:-/mnt/c/Users/ranly/Documents/ChatGPT/singbox-sub-me}
DST=${DST:-$HOME/ws/singbox-sub-me}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$HOME/ws/target}
```

1. `tar` 单向同步（排除 `target*`、`.git`、`.reference-*`、`.scratch`、`dist`、`node_modules`）；
2. 在 `$DST` 上运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、
   `cargo test --workspace --features sbctl/test-signing`；
3. 输出每一步耗时，失败即退出；
4. 支持 `SKIP_CLIPPY=1`、`ONLY='-p client-core'` 等窄化参数，供日常快速迭代。

从 PowerShell 调用示例：

```powershell
wsl -d Ubuntu-22.04 -u root -- bash /mnt/c/.../scripts/dev/wsl-gate.sh
```

## 验收

- 首次运行输出「synced to …」并在 WSL 本地盘编译；二次运行不重复传输。
- 全量门禁与 GitHub Actions `test` job 的 Linux 口径一致。
- 脚本本身进入仓库并通过 `shellcheck`（若可用）。
