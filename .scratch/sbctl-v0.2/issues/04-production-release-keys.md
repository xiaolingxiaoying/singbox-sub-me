# S3：配置生产发布公钥与签名 secret（维护者操作）

Status: ready-for-human
Type: task

## 事实（VPS 报告 §6.12 / §7 发布配置阻断）

- 普通构建必须提供编译期 `SBCTL_RELEASE_PUBLIC_KEY_HEX`，缺失即失败（`src/release.rs:23-48`）。
- Release workflow 已预留注入点（`.github/workflows/release.yml:37,138,173,188`），
  但仓库尚未配置变量与 Environment secret。

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
3. 创建 `release` Environment，配置 Environment secret `SBCTL_SIGNING_SEED` = 上一步 seed hex。
   GitHub → Settings → Environments → New environment `release` → Environment secrets。
4. 自检：用该公钥编译一次（`SBCTL_RELEASE_PUBLIC_KEY_HEX=<hex> cargo build --release -p sbctl`），
   再执行 `sbctl release sign` + `release verify`（用 seed 文件）。
5. 在工单追加脱敏证据：公钥指纹、配置时间、自检输出。

## 验收

- 注入生产公钥构建的 `sbctl update --check --manifest <生产 manifest>` 能通过签名校验。
- 未配置密钥的构建仍然显式拒绝更新，而不是静默降级。

## 禁止

- 不把私钥/种子写入仓库、CI 日志、工单或截图。
- 不复用公开开发密钥作为生产信任根。
