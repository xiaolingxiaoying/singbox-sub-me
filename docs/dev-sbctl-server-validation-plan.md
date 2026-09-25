# `dev-sbctl` 服务端验证与交互改进计划

更新日期：2026-09-25  
工作分支：`dev-sbctl`  
服务端代码验证提交：`496aa6b0c8f3316b35bc29aec922495160b08d00`

## 目标

验证 sbctl 服务端的首次安装、日常配置、三种订阅模式、订阅与节点、证书、流量、服务生命周期、更新回滚和卸载。测试在 GitHub Actions 的 Linux 构建产物和一台 Ubuntu 22.04 systemd VPS 上完成。安装向导在确认前不得写入部署配置；取消时不得改变部署状态。

## 验收范围

1. Linux amd64 生产构建、Rust 测试、Clippy、格式检查和 Debian/Ubuntu systemd 验收通过。
2. 新安装向导能完成一套真实 VPS 安装；确认前取消不会创建配置、服务或 sing-box 文件。
3. Direct、External proxy、IP fallback 三种模式的配置提交、systemd 单元变化、订阅端点和错误认证边界可实测。
4. 五种托管协议都能使用生成的客户端配置建立真实 HTTPS 连接。
5. 服务重启、流量校正与账期任务、证书检查与续期、凭据轮换、卸载、独立 sing-box 删除及坏候选回滚都有验证结果。
6. 测试结束恢复原部署；所有凭据、订阅链接、私钥、VPS 地址和域名不得进入仓库、CI 日志或文档。

## 测试前置与恢复

- 先检查主机和服务状态，再创建 root-only 快照；不得默认把既有主机当成空机。
- 快照应覆盖 sbctl 配置和状态、sing-box 配置和二进制、sbctl 二进制及托管 systemd 单元。
- 模式重装前确认订阅服务、协议端口和证书状态；不要修改云防火墙或用户未授权的反向代理配置。
- 完成后验证快照恢复结果、服务健康、Direct 订阅以及 systemd 单元；移除测试用反向代理、故障桩、客户端临时配置和临时 SSH 公钥。

本轮在操作前创建了 `/root/sbctl-pretest-20260925T022700Z.tar.gz`，权限为 `0600 root:root`，SHA-256 为 `979f1e1f5d29937312ea01330066737a289e34866f01395882e95a42b3ac4c35`。该归档含私密部署数据，只留在 VPS，不应下载或提交到仓库。

## 执行计划与结果

| 阶段 | 工作 | 结果 |
| --- | --- | --- |
| 1. 改进入口 | `sbctl install --guided` 先运行安装前检查，再一次收集完整配置、显示脱敏摘要并确认后执行事务；取消时不创建部署状态。新鲜安装菜单直接进入同一向导。 | 已实现并通过 CLI 测试。 |
| 2. 自动门禁 | CI 构建生产 Linux amd64 二进制；真实 systemd 验收使用隔离的 test-signing 工件，不把测试签名带入生产工件。验收覆盖 Debian 12、Ubuntu 22.04、Ubuntu 24.04。 | `test`、生产构建、Windows 静态检查、`server-acceptance` 均通过。验收辅助二进制改由 Ubuntu 22.04 构建，兼容 Debian 12 的 glibc。 |
| 3. 候选安装 | 用 Actions 构建产物在现有 Direct 部署上升级 sbctl；执行重启、配置检查、节点与订阅检查。 | 通过。当前安装候选 SHA-256：`48e2a566e587264e380164442b502ea0cb0c7196927c2b2cf291910793fb4efd`。 |
| 4. Direct | 检查 80/443 socket activation、HTTPS 订阅、索引和 QR、全部订阅格式、错误凭据和 query 边界、HTTP-01 webroot、证书状态及服务账户。 | 通过。真实公网 HTTPS 返回 200 和 `subscription-userinfo`；坏凭据和带 query 的路径返回 404。 |
| 5. External proxy | 用配置向导切换到 loopback 监听，并临时安装 Nginx 验证 TLS 反代到 sbctl；完成后移除 Nginx 及测试配置。 | 通过。loopback 监听未占用公网 80/443；真实 HTTPS 反代订阅返回 200 和流量头。Nginx 已卸载。 |
| 6. IP fallback | 向导切换到高位 HTTP 端口，从 VPS 和外部测试机分别请求订阅。 | 通过。公网端口返回 200；坏凭据和 query 返回 404。 |
| 7. 协议和订阅 | 每种模式验证 14 种订阅/二维码路径；用 sing-box 客户端分别测试 VLESS Reality、VMess WebSocket、Hysteria2、TUIC 和 AnyTLS。 | 三种模式的订阅矩阵通过；五种协议分别成功完成 HTTPS 请求。 |
| 8. 服务端操作 | 运行配置向导改模式、配置重生成、状态/节点、override 校验、系统状态、QR、流量校正、账期 timer、凭据轮换和交互主菜单。 | 通过。凭据轮换后新凭据可用，旧凭据立即返回 404。 |
| 9. 故障与卸载 | 用 `check=0`、`run=1` 的 sing-box 故障桩验证回滚；测试默认卸载、purge 和独立 sing-box 删除；再完成真实 guided 全新安装。 | 坏候选被拒绝、旧二进制摘要恢复、服务稳定；卸载保留/清除语义正确。guided 取消无写入，guided 安装及公网订阅、五种协议通过。 |
| 10. 证书和恢复 | Direct 证书 verify、renew、status；Certbot staging dry-run；恢复快照和候选 sbctl，复核公网订阅与 systemd。 | 通过。`renew` 因现有证书未到续期窗口未重新签发；staging dry-run 通过。快照保留，最终运行状态为健康 Direct。 |

订阅矩阵包括 sing-box、版本化 sing-box、Clash、版本化 Clash、URI、Base64 URI、Shadowrocket、索引页和 QR。客户端流量测试使用一次性 root-only 配置；测试后已删除临时配置和客户端进程。

## Actions 证据

- 代码改进：`de5efaa`；Actions `36085917092` 的 Linux 构建和 sbctl 测试通过。该运行的 macOS `sbtui` 快照检查失败。
- systemd 验收 CI：`d5fd927` 首次引入；其验收辅助程序在 Debian 12 上遇到 glibc 基线不兼容后，将验收 job 改为 Ubuntu 22.04 构建。
- 当前工作流提交：`496aa6b`；[GitHub Actions run 36087995403](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36087995403)。`build-sbctl-linux-amd64`、`test`、`windows-static`、`server-acceptance`、sing-box/Mihomo profiles、prototype 均通过；`server-acceptance` 在三种 Linux 系统上通过。
- 整体 CI 仍为失败：`sbtui-macos` 的 `tab-4-macos` 发布快照断言失败（47 passed、1 failed）。这不是 sbctl 服务端测试失败，但应由对应 GUI 任务单独修复后再要求全工作流绿色。

## 尚未完成的发布级验证

1. 仓库没有配置生产 release 公钥和签名私钥。生产候选的 `sbctl update --check` 按预期 fail-closed，未执行 signed manifest 更新；不得把 test-signing 密钥用于生产发布。
2. 本轮 Certbot `renew` 未实际更换尚未到期的生产证书；staging dry-run 通过。未进行新的 production ACME 签发，以免在真实域名上消耗签发额度。
3. 未使用手机或桌面 GUI 客户端人工导入；五种协议由 sing-box 客户端逐项连接验证。
4. 因此本轮证明了服务端候选的广泛功能和 VPS 兼容性，不构成“绝无缺陷”的保证，也不等同于可发布的 signed release。完成生产发布还需配置受控签名环境，并在产生同一版本 manifest 后验签和实测更新路径。

## 敏感信息与清理

文档和 CI 不保留 VPS 地址、域名、密码、订阅凭据、节点凭据或私钥。临时 VPS 公钥已从 `authorized_keys` 精确移除；root-only 备份按恢复需要保留。测试使用的 root 密码应在测试后由 VPS 管理员轮换。
