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
| 7. 协议和订阅 | 每种模式验证 14 种订阅/二维码路径；用 sing-box 客户端分别测试 VLESS Reality、VMess WebSocket、Hysteria2、TUIC 和 AnyTLS。 | 三种模式的订阅矩阵通过；五种协议由 sing-box 客户端分别成功完成 HTTPS 请求。后续人工导入 Clash Party 后，用户报告 VLESS 节点测速成功，其余四种协议测速超时。VPS 上五个监听端口均存在，sing-box 为 active；从操作者工作站探测三个 TCP 端口均可达。Ubuntu VM 的 Clash Party v1.19.31 日志显示，经 VMess 节点请求 `ping0.cc:443` 时，连接订阅中 VMess WebSocket 服务器端口 62892 超时（`connect error: context deadline exceeded`）；未出现 TLS/WebSocket 握手错误信息。此证据将排查重点移到 VM 到服务器的 TCP 路径或目标地址解析，仍需从 VM 分别探测 VLESS、VMess、AnyTLS TCP 端口并取得 AnyTLS 与 UDP 节点日志。CI 当前 Mihomo 配置加载测试仅执行真实核心 `-t` 配置解析，且固定在 v1.19.30，不能证明实际联网路径或完全覆盖用户使用的 v1.19.31。 |
| 8. 服务端操作 | 运行配置向导改模式、配置重生成、状态/节点、override 校验、系统状态、BBR、QR、流量校正、账期 timer、凭据轮换和交互主菜单。 | 通过。VPS 上 BBR/FQ 已启用时再次运行 `sbctl system bbr` 成功，输出和持久化 drop-in 正确；测试前的 drop-in 已还原，运行值保持 BBR/FQ。凭据轮换后新凭据可用，旧凭据立即返回 404。 |
| 9. 故障与卸载 | 用 `check=0`、`run=1` 的 sing-box 故障桩验证回滚；测试默认卸载、purge 和独立 sing-box 删除；再完成真实 guided 全新安装。隔离 CLI 测试覆盖已签名更新同时替换 sbctl 与 sing-box，并保留回滚点。 | 坏候选被拒绝、旧二进制摘要恢复、服务稳定；卸载保留/清除语义正确。guided 取消无写入，guided 安装及公网订阅、五种协议通过。signed update 成功路径在 `test-signing` 隔离工件上验证通过；未证明生产签名发行流程。 |
| 10. 证书和恢复 | Direct 证书 verify、renew、status；Certbot staging dry-run；恢复快照和候选 sbctl，复核公网订阅与 systemd。 | 通过。VPS 上 `renew` 因现有证书未到续期窗口未重新签发；staging dry-run 通过。隔离 CLI 测试验证 Certbot 替换有效证书后新证书与私钥被固定；SAN 不匹配时续期失败且旧固定副本不变。快照保留，最终运行状态为健康 Direct。 |

订阅矩阵包括 sing-box、版本化 sing-box、Clash、版本化 Clash、URI、Base64 URI、Shadowrocket、索引页和 QR。客户端流量测试使用一次性 root-only 配置；测试后已删除临时配置和客户端进程。

## Actions 证据

- 代码改进：`de5efaa`；Actions `36085917092` 的 Linux 构建和 sbctl 测试通过。该运行的 macOS `sbtui` 快照检查失败。
- systemd 验收 CI：`d5fd927` 首次引入；其验收辅助程序在 Debian 12 上遇到 glibc 基线不兼容后，将验收 job 改为 Ubuntu 22.04 构建。
- Server/UI 验收提交：`fa0b31b`；[GitHub Actions run 36094245350](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36094245350) 全部通过，包括生产 Linux 构建、`test`、Windows 静态检查、macOS `sbtui`、三发行版 `server-acceptance`、sing-box/Mihomo profiles 和 prototype。
- 该运行也确认补充的 `tab-4-macos.snap` 与 macOS runner 实际渲染一致，先前 47 passed、1 failed 的 macOS 快照失败已修复。
- 签名更新与续期测试提交：`021a694`；[GitHub Actions run 36100576893](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36100576893) 全部通过。`tests/cli/update_release.rs` 验证签名更新成功事务，`tests/cli/certificate.rs` 验证 Certbot 续期成功后固定新证书，以及坏 SAN 续期不覆盖旧证书。

## 尚未完成的发布级与端到端验证

1. 仓库没有配置生产 release 公钥和签名私钥。生产候选的 `sbctl update --check` 按预期 fail-closed，未执行 signed manifest 更新；不得把 test-signing 密钥用于生产发布。
2. 本轮 Certbot `renew` 未实际更换尚未到期的生产证书；staging dry-run 通过。未进行新的 production ACME 签发，以免在真实域名上消耗签发额度。
3. Clash Party GUI 已导入当前 Clash 订阅，但用户报告除 VLESS 外的节点测速超时。用户已在 Ubuntu 64-bit VM 安装 Clash Party 并导入配置。现有脱敏日志证明 VMess 节点测速请求期间连接服务器 TCP 端口 62892 超时；还需从 VM 验证三种 TCP 端口可达性，并取得 AnyTLS、Hysteria2、TUIC 的日志后再判断是否为公共网络路径、地址解析或协议配置问题。CI Mihomo job 当前只测生成配置能否解析，测试核心版本 v1.19.30 与 GUI 显示的 v1.19.31 不同，因此其通过结果不覆盖实际节点连通性。
4. 仓库尚未配置受控的生产签名环境，生产签名发行与 VPS 上的生产 signed update 路径未验证。
5. 因此本轮证明了服务端候选的广泛功能和 VPS 兼容性，不构成“绝无缺陷”的保证，也不等同于可发布的 signed release。完成生产发布还需配置受控签名环境，并在产生同一版本 manifest 后验签和实测更新路径。

## 敏感信息与清理

文档和 CI 不保留 VPS 地址、域名、密码、订阅凭据、节点凭据或私钥。临时 VPS 公钥已从 `authorized_keys` 精确移除；root-only 备份按恢复需要保留。工作站 `%TEMP%` 中本轮 SSH 测试密钥文件仍待本机清理（精确路径为 `sbctl-vps-validation-20260925` 和同名 `.pub` 文件）。测试使用的 root 密码应在测试后由 VPS 管理员轮换。
