# `dev-sbctl` 服务端验证与交互改进计划

更新日期：2026-09-25  
工作分支：`dev-sbctl`  
服务端代码验证提交：`e5aa7fc2352229a3fee8d77d4dab5842611a6cf6`  
当前工作分支提交：`937588782b8a357061f448c20ac05cc824442bb5`（后续仅更新验收记录）

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
| 1. 改进入口 | `sbctl install --guided` 先运行安装前检查，再一次收集完整配置、显示脱敏摘要并确认后执行事务；取消时不创建部署状态。新鲜安装菜单直接进入同一向导。配置向导提交后重印当前订阅与全部已启用协议的 UFW 放行命令，供管理员核对；不自动修改防火墙。 | 已实现；首次向导、配置变更、取消与凭据脱敏测试通过。 |
| 2. 自动门禁 | CI 构建生产 Linux amd64 二进制；真实 systemd 验收使用隔离的 test-signing 工件，不把测试签名带入生产工件。验收覆盖 Debian 12、Ubuntu 22.04、Ubuntu 24.04。代码验证见 [Actions run 36122703588](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36122703588)，全部通过：workspace Rust tests/Clippy、Windows/macOS 检查、生产 Linux 构建、真实 sing-box/Mihomo profile 校验，以及三发行版 `server-acceptance`；该运行也验证严格 HTTP 431 oversized-header 断言。之后的文档提交在 [Actions run 36124229773](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36124229773) 再次全部通过，包括完整 workspace tests、生产构建、profile 校验和 Debian/Ubuntu systemd acceptance。验收辅助二进制改由 Ubuntu 22.04 构建，兼容 Debian 12 的 glibc。 |
| 3. 候选安装 | 用 Actions 构建产物在现有 Direct 部署上升级 sbctl；执行重启、配置检查、节点与订阅检查。 | 通过。CI run `36108915661` 构建的生产 Linux amd64 工件 SHA-256 为 `3dd2d829b39d00812baee6c0f48002f64c78fbe1a2d19a0757f9d68bd1a69dce`，已在 VPS 上校验摘要后原子替换旧版；旧二进制另存 root-only 回滚副本，摘要为 `48e2a566e587264e380164442b502ea0cb0c7196927c2b2cf291910793fb4efd`。新程序报告 `sbctl 0.2.0`；`sbctl status` 正常，`sing-box.service` 和 `sbctl.service` 均为 active。替换 CLI 二进制无需重启代理服务。 |
| 4. Direct | 检查 80/443 socket activation、HTTPS 订阅、索引和 QR、全部订阅格式、错误凭据和 query 边界、HTTP-01 webroot、证书状态及服务账户。 | 通过。真实公网 HTTPS 返回 200 和 `subscription-userinfo`；坏凭据和带 query 的路径返回 404。 |
| 5. External proxy | 用配置向导切换到 loopback 监听，并临时安装 Nginx 验证 TLS 反代到 sbctl；完成后移除 Nginx 及测试配置。 | 通过。loopback 监听未占用公网 80/443；真实 HTTPS 反代订阅返回 200 和流量头。Nginx 已卸载。 |
| 6. IP fallback | 向导切换到高位 HTTP 端口，从 VPS 和外部测试机分别请求订阅。 | 通过。公网端口返回 200；坏凭据和 query 返回 404。 |
| 7. 协议和订阅 | 每种模式验证 14 种订阅/二维码路径；用 sing-box 客户端分别测试 VLESS Reality、VMess WebSocket、Hysteria2、TUIC 和 AnyTLS。 | 三种模式的订阅矩阵通过；五种协议由 sing-box 客户端分别成功完成 HTTPS 请求。后续人工导入 Clash Party 后，用户报告 VLESS 节点测速成功，其余四种协议测速超时。VPS 上五个监听端口均存在，sing-box 为 active。Ubuntu VM 的 Clash Party v1.19.31 日志显示，经 VMess 节点请求 `ping0.cc:443` 时，连接 VMess WebSocket 服务器端口 62892 超时（`connect error: context deadline exceeded`）。VPS 的 UFW 为 active 且默认拒绝入站；当前节点五个端口中当时只有 VLESS TCP 36065 有 allow 规则。内核日志在截图对应时段记录到来自 VM 的四次 TCP SYN 被 UFW 丢弃，目的端口均为 62892；sing-box 没有对应 VMess 入站连接。这证明此次 VMess 超时由 VPS 主机防火墙规则遗漏导致，而非客户端握手参数。已为 VMess TCP 62892、AnyTLS TCP 36559、Hysteria2 UDP 32896、TUIC UDP 58714 添加带协议注释的 UFW 规则，VLESS 36065 原规则保留；修改前的 UFW v4/v6 用户规则备份在 VPS root-only 目录 `/root/ufw-before-sbctl-node-ports-20260925T071100Z`。修改后从操作者工作站探测三个 TCP 节点端口均成功；用户随后确认 Ubuntu VM 的 Clash Party 中五个节点测速均通过，测速目标 `ping0.cc:443` 的 HTTPS 探测也随测速成功。普通浏览器访问任意站点尚未单独验证。CI Mihomo job 仅执行真实核心 `-t` 配置解析，已将核心更新到 GUI 对应的 v1.19.31，但配置解析不能替代网络连通性验收。 |
| 8. 服务端操作 | 运行配置向导改模式、配置重生成、状态/节点、override 校验、系统状态、BBR、QR、流量校正、账期 timer、凭据轮换和交互主菜单。 | 通过。VPS 上 BBR/FQ 已启用时再次运行 `sbctl system bbr` 成功，输出和持久化 drop-in 正确；测试前的 drop-in 已还原，运行值保持 BBR/FQ。凭据轮换后新凭据可用，旧凭据立即返回 404。 |
| 9. 故障与卸载 | 用 `check=0`、`run=1` 的 sing-box 故障桩验证回滚；测试默认卸载、purge 和独立 sing-box 删除；再完成真实 guided 全新安装。隔离 CLI 测试覆盖已签名更新同时替换 sbctl 与 sing-box，并保留回滚点。 | 坏候选被拒绝、旧二进制摘要恢复、服务稳定；卸载保留/清除语义正确。guided 取消无写入，guided 安装及公网订阅、五种协议通过。signed update 成功路径在 `test-signing` 隔离工件上验证通过；未证明生产签名发行流程。 |
| 10. 证书和恢复 | Direct 证书 verify、renew、status；Certbot staging dry-run；恢复快照和候选 sbctl，复核公网订阅与 systemd。 | 通过。VPS 上 `renew` 因现有证书未到续期窗口未重新签发；staging dry-run 通过。隔离 CLI 测试验证 Certbot 替换有效证书后新证书与私钥被固定；SAN 不匹配时续期失败且旧固定副本不变。快照保留，最终运行状态为健康 Direct。 |

2026-09-25 的 S11 验收修复已由 Actions run `36133020517` 全部通过（workspace tests、Clippy、Windows/macOS 检查、生产 Linux amd64 构建、sing-box/Mihomo 真核验证、Debian/Ubuntu systemd acceptance）。VPS 当前运行 `sbctl 0.2.0`，SHA-256 `45dacacda646d9b433a6a41455ee7c596accfceb2a23830db0bf59a66776793f`；该生产二进制由 run `36131353579` 构建，后续 `bd7b60d` 仅改验收脚本和文档，未改生产二进制源码。root-only 回滚副本 `/root/sbctl.pre-bd7b60d` 的 SHA-256 为 `3dd2d829b39d00812baee6c0f48002f64c78fbe1a2d19a0757f9d68bd1a69dce`。更新后 `sbctl.service` 与 `sing-box.service` 均 active、`NRestarts=0`、`sbctl config validate` 通过；节点清单有五条，`sing-box-full` HTTPS 订阅返回 200，userinfo 含配置额度 `536870912000` 和 `profile-update-interval=24`。本轮部署未修改 sing-box 配置、UFW 或证书。

2026-09-25 的 S9 客户端模板向导改进提交 `e5aa7fc2352229a3fee8d77d4dab5842611a6cf6` 已由 [Actions run 36146609985](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36146609985) 全部通过。模板仍遵循 ADR-0022 编译期目录边界；向导和状态摘要覆盖模板、DNS、规则档位、镜像 URL 与测速 URL，并对订阅凭据、URL 认证信息、query 和 fragment 脱敏。使用该 CI Linux 工件在 VPS `/tmp` 中只读运行，摘要匹配 `4144187ffe519aaac6d83fdcc41c84d86e7a80ad3ef2767f3a51a737479a0d95`，状态摘要正确，受管服务保持 active 且重启计数未增长；临时工件已清理，生产二进制和服务未替换。S10 已决定由维护者先核实并固定新 sing-box minor 的 schema/profile，再由 CI 硬门禁；新 minor 未审查前不自动滚动发布。上述问题单为 `resolved`。

当前记录提交 `937588782b8a357061f448c20ac05cc824442bb5` 的 [Actions run 36148995690](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36148995690) 已全部通过：Linux workspace 测试/Clippy/release trust、Windows/macOS 检查、生产 Linux amd64 构建、真实 sing-box/Mihomo profile 校验、原型检查，以及 Debian 12、Ubuntu 22.04、Ubuntu 24.04 systemd 工件验收。此 run 包含文档记录提交，S9 对应的代码验证仍以 `e5aa7fc` 工件和 run `36146609985` 为准。

订阅矩阵包括 sing-box、版本化 sing-box、Clash、版本化 Clash、URI、Base64 URI、Shadowrocket、索引页和 QR。客户端流量测试使用一次性 root-only 配置；测试后已删除临时配置和客户端进程。

## Actions 证据

- 代码改进：`de5efaa`；Actions `36085917092` 的 Linux 构建和 sbctl 测试通过。该运行的 macOS `sbtui` 快照检查失败。
- systemd 验收 CI：`d5fd927` 首次引入；其验收辅助程序在 Debian 12 上遇到 glibc 基线不兼容后，将验收 job 改为 Ubuntu 22.04 构建。
- Server/UI 验收提交：`fa0b31b`；[GitHub Actions run 36094245350](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36094245350) 全部通过，包括生产 Linux 构建、`test`、Windows 静态检查、macOS `sbtui`、三发行版 `server-acceptance`、sing-box/Mihomo profiles 和 prototype。
- 该运行也确认补充的 `tab-4-macos.snap` 与 macOS runner 实际渲染一致，先前 47 passed、1 failed 的 macOS 快照失败已修复。
- 签名更新与续期测试提交：`021a694`；[GitHub Actions run 36100576893](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36100576893) 全部通过。`tests/cli/update_release.rs` 验证签名更新成功事务，`tests/cli/certificate.rs` 验证 Certbot 续期成功后固定新证书，以及坏 SAN 续期不覆盖旧证书。
- UFW 指引和孤儿进程守卫测试修正：生产 CLI 在配置提交后会再次显示当前协议所需的防火墙命令（仅提示，不自动更改防火墙）；Actions run `36107274280` 暴露了客户端孤儿进程测试桩问题，修复后 [GitHub Actions run 36108915661](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36108915661) 全部通过。成功工件已部署到 VPS，见阶段 3。
- 账期遗漏数月后的 reset 回归测试：commit `183b1cc`；[GitHub Actions run 36116097720](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36116097720) 全部通过，包括 Windows/macOS、全量 Rust 测试和 systemd acceptance。
- 时区规格同步：commit `11ab87b`。Run `36116953002` 的测试、生产构建、跨平台检查和 server-acceptance 均通过；sing-box 最新版探测遇到匿名 Releases API HTTP 403。CI 已改为解析 GitHub 官方 `/releases/latest` 的重定向，不跳过最新版本兼容性检查。
- CI 限流修正：commit `ca96b67`；[GitHub Actions run 36119636867](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36119636867) 全部通过，含最新稳定 sing-box v1.14.2 核心 profile 校验。
- 服务端 HTTP 连接边界与服务账户验收：commit `6420a2a`；[GitHub Actions run 36121273005](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36121273005) 全部通过。新增 live-listener 测试覆盖超大头、慢读和超过 32 的并发请求；systemd acceptance 检查 `sbctl`/`sing-box` 的 nologin 账户和实际进程用户。之后将超大头断言收紧为必须返回 HTTP 431，并在 Windows 本机及 [Actions run 36122703588](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36122703588) 通过验证。
- 严格 HTTP 431 验收与文档收口：commit `e009fa8` / `5e4be3b`；[GitHub Actions run 36122703588](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36122703588) 全部通过，严格 431 回归测试、生产构建和三发行版 systemd acceptance 均通过。
- 最新文档提交的回归门禁：[GitHub Actions run 36124229773](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36124229773) 全部通过，含 Linux/macOS/Windows 检查、完整测试、生产构建、真实核心配置校验和 systemd acceptance。
- S9 向导改进及状态摘要脱敏：代码提交 `e5aa7fc2352229a3fee8d77d4dab5842611a6cf6`；[GitHub Actions run 36146609985](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36146609985) 全部通过，VPS 临时工件运行证据见阶段 7 和 S9 工单。
- 最新跟踪记录提交 `937588782b8a357061f448c20ac05cc824442bb5`：[GitHub Actions run 36148995690](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/runs/36148995690) 全部通过，包括 Debian/Ubuntu 三发行版 systemd acceptance；该提交为文档记录更新。

## 尚未完成的发布级与端到端验证

1. 仓库没有配置生产 release 公钥和签名私钥。生产候选的 `sbctl update --check` 按预期 fail-closed，未执行 signed manifest 更新；不得把 test-signing 密钥用于生产发布。
2. 本轮 Certbot `renew` 未实际更换尚未到期的生产证书；staging dry-run 通过。未进行新的 production ACME 签发，以免在真实域名上消耗签发额度。
3. Clash Party 导入订阅后的节点测速超时已定位到 VPS UFW：默认拒绝入站，仅放行 VLESS 当前端口；内核记录到 VMess 62892/TCP SYN 被丢弃。现已补齐 VMess、AnyTLS、Hysteria2、TUIC 当前端口的 UFW 规则；操作者工作站确认所有 TCP 节点端口可连接，用户随后确认 Ubuntu VM 的 Clash Party 中 VLESS、VMess、Hysteria2、TUIC、AnyTLS 五个节点测速全部通过（目标为 HTTPS 端口 443）。普通浏览器访问任意站点尚未单独验证。
4. 仓库尚未配置受控的生产签名环境，生产签名发行与 VPS 上的生产 signed update 路径未验证。
5. 因此本轮证明了服务端候选的广泛功能和 VPS 兼容性，不构成“绝无缺陷”的保证，也不等同于可发布的 signed release。完成生产发布还需配置受控签名环境，并在产生同一版本 manifest 后验签和实测更新路径。

## 敏感信息与清理

文档和 CI 不保留 VPS 地址、域名、密码、订阅凭据、节点凭据或私钥。临时 VPS 公钥已从 `authorized_keys` 精确移除；工作站 `%TEMP%` 中本轮 SSH 测试私钥和公钥也已删除并确认不存在。root-only 备份按恢复需要保留。测试时使用过的 root 密码仍应由 VPS 管理员轮换。
