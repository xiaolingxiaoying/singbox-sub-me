# 发布就绪与 Ubuntu VPS 测试计划

## 目标

将当前 `master` 从“功能已实现但未完成发布验证”推进到“有可追溯证据的候选发布包”，
并在一台干净的 Ubuntu VPS 上完成受控实测。本计划的完成不等同于立即向现有用户迁移或
接管任何既有 sing-box 部署。

## 当前基线

- 当前工作分支为 `master`，最新提交为 `3940b07`。
- 最新 CI 在 `cargo fmt --all -- --check` 停止；该差异已在 Windows 和 WSL Ubuntu 22.04
  中复现，属于未提交的 Rust 格式化结果。
- 最近一次 Release 的 Linux amd64/arm64 构建成功，但 Ubuntu 22.04 的真实 systemd
  验收在 IP fallback 重装后访问 `127.0.0.1:2081` 时连接被拒绝。
- WSL2 只作为开发主机，不能替代真实 Debian/Ubuntu systemd 主机的发布门禁。

## 范围与原则

- 先修复并验证发布门禁，再部署 VPS；VPS 不是首个调试环境。
- 所有测试从全新、未被 sbctl 或其他 sing-box 管理的系统开始。安装前发现既有部署时，
  必须停止而非接管。
- 测试期间不自动修改云防火墙、安全组、Nginx、Caddy 或其他管理员管理的服务。
- 发布工件、manifest 和签名验证必须来自同一候选版本；不得用旧 `v0.1.0` 工件验证当前代码。
- 所有凭据、订阅 URL、私钥和 VPS IP 均不得写入提交记录、CI 日志或计划附件。

## 阶段 1：恢复开发与 CI 门禁

1. 在 WSL Ubuntu 的 Linux 文件系统（例如 `~/src/`）而非 `/mnt/c` 目录完成验证，避免
   Windows 挂载目录导致的 CRLF 视图差异和 `aws-lc-sys` 编译性能问题。
2. 使用与 GitHub Actions 一致的 Rust stable 工具链运行：

   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   ```

3. 仅当格式、Clippy 和全部测试通过时，才允许推送修复并等待 GitHub CI 绿色。

**通过条件：** GitHub CI 对待发布提交成功，且工作树除预期改动外干净。

## 阶段 2：定位并修复真实 systemd 验收

1. 在 Linux Docker 或临时 VM 运行 `tests/acceptance/run.sh`，覆盖 Debian 12、Ubuntu 22.04
   和 Ubuntu 24.04。
2. 若 IP fallback 重装路径再次出现 `127.0.0.1:2081` 连接拒绝，保留以下诊断证据：
   `systemctl status sbctl.service`、`journalctl -u sbctl.service`、`ss -ltnp`、
   `/etc/sbctl/config.toml` 的脱敏副本，以及安装/卸载步骤的退出状态。
3. 验证卸载后的服务单元、socket、状态目录与后续重装的顺序关系；修复必须由该验收路径
   的自动化回归测试覆盖。
4. 同时确认 Direct 模式的 socket activation、External proxy 的 loopback 监听、IP fallback
   的高位 HTTP 端口，以及 `sing-box.service` 都完成健康检查。

**通过条件：** 三个发行版的发布二进制验收都通过；失败时不生成或不发布候选包。

## 阶段 3：生成候选发布包

1. 从已通过门禁的精确 commit 创建候选 tag，并由 Release 工作流构建 amd64 与 arm64 的
   `sbctl` 工件。
2. 下载指定版本的官方 sing-box 工件，校验摘要，并为每个架构生成包含 sbctl、sing-box
   URL 与 SHA-256 的 manifest。
3. 用发布公钥验证 manifest 签名；安装脚本与 manifest、二进制必须都指向同一候选 tag。
4. 记录 tag、commit SHA、GitHub Actions 运行链接、工件 SHA-256 和 sing-box 版本，作为
   VPS 实测的输入证据。

**通过条件：** Release 的 build、acceptance 与 package job 均成功，候选 Release 附件完整。

## 阶段 4：准备 Ubuntu VPS 测试环境

- 使用全新的 Ubuntu 22.04 或 24.04 amd64 VPS；arm64 VPS 需要使用对应 manifest。
- 确认 systemd 正在运行，并在安装前检查不存在 `/etc/sing-box`、`sing-box.service`、
  sbctl 状态目录或其他 sing-box 管理服务。
- 为 Direct 模式准备已解析到该 VPS 的测试域名，并确保 TCP 80/443 可从公网访问；为协议
  测试按实际启用的节点开放对应 TCP/UDP 端口。
- 为 External proxy 模式预先由管理员配置测试用反向代理；sbctl 只应监听 loopback。
- 使用临时测试凭据和可撤销域名。不要在生产节点、现有订阅域名或已有代理配置上做首测。

## 阶段 5：VPS 分阶段测试

每种模式应使用独立全新 VPS，或在同一专用测试 VPS 上完成证据留存后执行
`sbctl uninstall --purge`，再确认系统中不残留 sbctl 所有的状态。

| 模式 | 核心验证 | 通过条件 |
| --- | --- | --- |
| IP fallback | VLESS Reality、指定高位 HTTP 端口、路径凭据 | 正确路径返回三种订阅；错误或 query 凭据为 404；服务重启后仍可用。 |
| External proxy | loopback 订阅服务与管理员反向代理 | sbctl 不占用公网 80/443；代理后的 HTTPS 订阅正常；五种节点输出与配置一致。 |
| Direct | ACME、80/443 socket activation、非 root 服务 | 证书申请/续期、HTTPS 订阅和 HTTP-01 挑战均成功；`sbctl` 与 sing-box 均以专用非 root 账户运行。 |

三种模式的通用检查：

```bash
systemctl status sbctl.service sing-box.service
sbctl status
sbctl config validate
sbctl node
sbctl sub --format sing-box
sbctl sub --format clash
sbctl sub --format uri
```

还应从至少一个实际客户端导入每种订阅格式，检查连接、TLS/SNI、协议端口与流量
`subscription-userinfo` 是否符合预期。

## 回滚与停止条件

- 任何安装前置检查发现既有部署、端口冲突或证书/签名校验失败时，立即停止，不采取接管或
  手工覆盖措施。
- 服务未健康、订阅认证边界异常、证书续期异常或协议无法连接时，停止扩大测试范围；保留
  脱敏日志和版本信息后回到阶段 2。
- 测试完成但不继续使用时，先执行默认 `sbctl uninstall` 保留备份；仅在确认 VPS 专用于测试
  且已导出所需证据后执行 `sbctl uninstall --purge`。

## 最终发布决策

只有同时满足以下条件，才能将候选版本视为可部署版本：

1. 当前 commit 的 CI、三发行版真实 systemd 验收和 Release 工作流均为成功；
2. 候选工件与已验证的 signed manifest 一一对应；
3. 至少完成一台干净 Ubuntu VPS 的所选订阅模式端到端测试；
4. 所有失败案例都有可重现步骤、脱敏诊断记录和自动化回归覆盖；
5. 没有在测试中接管或破坏任何既有代理、反向代理或防火墙配置。

## 当前执行记录与 TODO

已完成：

- 已拉取远程 `master`，并将 Rust 格式化修复提交为 `1fd1be5`。
- 本地 `cargo fmt --check`、Clippy、完整 Rust 测试和 release 构建均通过。
- 提交 `1fd1be5` 的 GitHub CI 已通过：format、Clippy 和 test job 均成功。

TODO：

- [ ] 在 Windows 上启用 Docker Desktop 的 WSL 集成，并运行三发行版真实 systemd 验收：
  `SBCTL_ARTIFACT=<Linux release binary> sh tests/acceptance/run.sh`，覆盖 Debian 12、Ubuntu 22.04
  和 Ubuntu 24.04。
- [ ] 若验收失败，按本计划保留 `systemctl status sbctl.service`、`journalctl -u sbctl.service`、
  `ss -ltnp`、脱敏后的 `/etc/sbctl/config.toml` 和各安装/卸载步骤退出状态；修复后重新运行
  三个发行版验收。
- [ ] 确认候选版本命名。现有 `v0.1.14` tag 已占用且指向旧提交；建议将包版本更新为 `0.1.15`，
  从通过全部门禁的精确 commit 创建 `v0.1.15` 候选 tag。
- [ ] 运行 Release workflow，确认 amd64/arm64 build、acceptance 和 package job 全部成功；记录
  tag、commit SHA、Actions 链接、工件 SHA-256 及 sing-box 版本。
- [ ] 下载同一候选 tag 的 sbctl、sing-box 和 signed manifest，验证 manifest 签名、工件摘要及
  manifest 中的固定 URL/版本/兼容矩阵一致。
- [ ] 在全新的 Ubuntu 22.04 或 24.04 amd64 VPS 上完成至少一种模式的端到端测试；按条件继续
  验证 IP fallback、External proxy 和 Direct 三种模式，并导入 sing-box、Clash 和 URI 三种
  订阅格式检查连接、TLS/SNI、协议端口和 `subscription-userinfo`。
- [ ] 测试完成后导出脱敏证据，先执行默认 `sbctl uninstall`；确认 VPS 专用于测试且证据已保存后，
  再执行 `sbctl uninstall --purge` 并确认没有残留 sbctl 状态。
