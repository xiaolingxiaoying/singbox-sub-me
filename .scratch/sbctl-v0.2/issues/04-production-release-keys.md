# S3：配置生产发布公钥与签名 secret（维护者操作）

Status: ready-for-human
Type: task

## 事实（VPS 报告 §6.12 / §7 发布配置阻断）

- 普通构建必须提供编译期 `SBCTL_RELEASE_PUBLIC_KEY_HEX`，缺失即失败（`src/release.rs:23-48`）。
- Release workflow 已预留注入点（`.github/workflows/release.yml:37,138,173,188`）。
- 2026-09-25 使用 GitHub CLI 核验：仓库变量存在，且与用户提供的 Ed25519 公钥一致；`SBCTL_SIGNING_SEED` 存在于 repository secrets，但 GitHub `release` Environment 尚不存在，因此还未按计划配置为环境级 secret。seed 内容未读取或回显。

## 动作（GitHub 仓库设置，只能由维护者执行）

1. 生成独立生产 Ed25519 密钥对（不得复用 `scripts/dev-signing-key.hex`）：

   ```bash
   # 离线机器上执行；私钥/种子不要进入仓库、CI 日志或工单
   openssl genpkey -algorithm ED25519 -out sbctl-release.pem
   # 32 字节 seed（hex，64 字符）→ GitHub secret
   openssl pkey -in sbctl-release.pem -outform DER | tail -c 32 | xxd -p -c 64
   # 32 字节公钥（hex，64 字符）→ GitHub variable
   openssl pkey -in sbctl-release.pem -pubout -outform DER | tail -c 32 | xxd -p -c 64
   ```

2. Repository variable `SBCTL_RELEASE_PUBLIC_KEY_HEX` = 上一步公钥 hex。
   GitHub → Settings → Secrets and variables → Actions → Variables。
   当前仓库该变量已存在且匹配，无需再次修改。
3. 创建 GitHub `release` Environment，将 `SBCTL_SIGNING_SEED` 配置为 Environment secret。
   GitHub → Settings → Environments → New environment `release` → Environment secrets。
   当前 seed 在 repository secrets；设置环境级 secret 后，维护者可按仓库 secret 列表 UI 删除 repository-level 副本，以遵循最小授权范围。
4. 在本地 WSL Ubuntu 的 Linux 文件系统仓库副本根目录，使用该公钥编译一次：

   ```bash
   export SBCTL_RELEASE_PUBLIC_KEY_HEX='<64 位公钥 hex>'
   cargo build --release -p sbctl
   ```

   将占位文本替换成公钥值，不要包含尖括号。此步骤只使用公钥，不需要 seed；再在受控环境中用生产 seed 文件执行 `sbctl release sign` + `release verify` 自检。
5. 在工单追加脱敏证据：公钥指纹、配置时间、自检输出。

## 验收

- 注入生产公钥构建的 `sbctl update --check --manifest <生产 manifest>` 能通过签名校验。
- 未配置密钥的构建仍然显式拒绝更新，而不是静默降级。

## 禁止

- 不把私钥/种子写入仓库、CI 日志、工单或截图。
- 不复用公开开发密钥作为生产信任根。
