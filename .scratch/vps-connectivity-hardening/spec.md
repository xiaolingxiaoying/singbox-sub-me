# VPS 连通性与安全加固计划

Status: ready-for-agent

日期：2026-09-13  
范围：对由 `sbctl` 管理的 sing-box 生产 VPS 部署进行可回滚的连通性修复、出口可用性验证和主机安全加固。本文是执行计划，不包含未经确认的线上变更。

## 1. 背景与已验证事实

本仓库的 `sbctl` 是单管理员 sing-box 控制面：它管理五种协议、订阅工件与 systemd 生命周期；配置改动应经过 `sing-box check`、原子替换和健康检查，而不是直接长期手改生成文件。它不会自动修改防火墙，也不会接管非自身管理的既有部署。

2026-09-13 对目标 VPS 的只读检查得到：

| 事项 | 证据 | 结论 |
| --- | --- | --- |
| IPv4 出口 | 生产 VPS（洛杉矶区域） | IPv4 路由和 DNS 可用。 |
| X | `https://x.com` 返回 HTTP 200；sing-box 日志中 `api.x.com:443` 有成功直连 | VPS 到 X 不存在普遍性网络阻断。 |
| ChatGPT | `https://chatgpt.com` 和 `/api/auth/session` 完成 TLS 后返回 Cloudflare HTTP 403，`cf-mitigated: challenge` | 不是 TCP/DNS 断连；需把 IP/反爬风控与代理配置问题分开。 |
| IPv6 | 主机没有 IPv6 地址或默认路由；sing-box 多次尝试连接 Google IPv6 地址并报 `cannot assign requested address` | 已确认的配置缺陷，会造成部分 AAAA 目标失败或明显卡顿。 |
| 服务 | `sing-box 1.12.0` 正在运行；5 个协议 listener 已开放；`sbctl` 占用公网 TCP 80/443 | 不能通过关闭端口或替换配置来“试试”，必须保留订阅和节点功能。 |
| 系统状态 | Ubuntu 22.04、内核 5.15.0-30；117 个待更新、5 个安全更新、系统要求重启 | 需要维护窗口完成补丁和重启验证。 |
| SSH 与防火墙 | root 密码登录允许、公钥登录禁用、UFW 未启用、Fail2ban 未运行 | 互联网暴露面过大，先建立恢复路径后加固。 |

## 2. 问题与目标

### 问题

该 VPS 将没有 IPv6 网络能力的主机用于可解析 AAAA 的代理目标，造成 sing-box 直接向不可用 IPv6 地址拨号。与此同时，ChatGPT 对该出口返回 Cloudflare 挑战，不能被误判为 DNS、端口或 sing-box 单点故障。主机还缺少最小 SSH 与防火墙防护。

### 目标

1. 所有经节点访问的域名在本 VPS 无 IPv6 时稳定使用 IPv4，不再出现 IPv6 拨号失败。
2. 证明 X 和常用 HTTPS 目标在真实客户端可用，并以浏览器实测明确 ChatGPT 是恢复、可挑战，还是出口信誉/服务策略问题。
3. 使 SSH、公开端口、补丁和日志达到单管理员 VPS 的最低安全基线。
4. 任何失败均可回滚到当前已运行的配置；不丢失订阅凭据、协议凭据或现有节点端口。

### 非目标

- 不把 WARP、Argo、Psiphon、第三方 SOCKS 或 IP 伪装作为绕过 ChatGPT 服务政策或风控的方案。
- 不自动关闭防火墙、清空 iptables/nftables 规则，也不接管无关的 Nginx/Caddy 服务。
- 不修改 ChatGPT 账户、验证码、地区或平台侧策略。
- 不在计划阶段直接升级系统、重启服务、轮换密码或写入 VPS 配置。

## 3. 相关项目学习结论与设计约束

### 可复用的 sbctl 能力

- `sbctl` 生成五种协议节点、订阅与客户端工件；其配置/工件更新以 `sing-box check`、原子替换和失败恢复为边界。
- 数据面以专用非 root 账户运行；Direct 订阅端口 80/443 由 systemd socket activation 管理。
- `sbctl system status` 与 `sudo sbctl system bbr` 已提供独立、幂等的 BBR+FQ 系统调优，不需要引入 sing-box-yg 的远程 BBR 脚本。
- 端口变更在 sbctl 中遵循共享 TCP/UDP 命名空间、检查占用、配置验证和健康检查的事务流程。

### 不照搬 sing-box-yg 的部分

- sing-box-yg 的 WARP 出站、Argo、Psiphon/SOCKS、供应商专属分流是外部服务依赖；本项目 ADR-0018 明确保持供应商中立。
- 上游直接下载未校验内核、root 常驻、清空防火墙和裸 HTTP 无认证订阅，与本项目的签名发布、非 root、路径凭据和保留宿主机策略相冲突。
- 上游的 `sniff`、VMess WebSocket early-data、QUIC 参数可作为未来独立产品优化，但不应混入本次 IPv6/安全事故修复。

## 4. 用户故事

1. 作为管理员，我要在变更前保留可验证的配置与访问恢复路径，以便失败时不失去 VPS 控制权。
2. 作为节点使用者，我要让没有 IPv6 的 VPS 只选择 IPv4 目标，以便 AAAA 记录不再导致网站打不开。
3. 作为节点使用者，我要验证 X、Google 类 HTTPS 目标和订阅在真实客户端的结果，以便区分服务器故障与客户端故障。
4. 作为管理员，我要以独立的浏览器验收 ChatGPT，以便识别 Cloudflare/出口信誉问题而不盲目改协议。
5. 作为管理员，我要以 SSH 密钥和最小端口规则管理 VPS，以便减少 root 密码暴露带来的入侵风险。
6. 作为管理员，我要在补丁更新和重启后自动/手动验证服务健康，以便不让安全维护中断订阅与节点。
7. 作为维护者，我要使 IPv4-only 的行为成为 sbctl 可持续生成的配置，而不是下一次更新会覆盖的手工修改。

## 5. 实施计划

### 阶段 A：变更前保护与基线（P0）

**前置条件**

- 管理员可使用 VPS 控制台或保留一个独立 SSH 会话。
- 已记录当前配置、systemd 单元、端口、订阅 URL 的脱敏摘要和最近 sing-box 错误日志。
- 已确认 `/etc/sing-box/config.json` 与两个 systemd 服务的实际所有者均为 sbctl；若存在手工覆盖层，先停止并说明所有权，不能覆盖。

**动作**

1. 在主机本地创建带 sudo 的日常管理员账号并安装其 SSH 公钥；用一个新的终端验证该账号可 sudo 登录。
2. 保存 sbctl 的当前部署配置、配置工件和服务单元的受限备份；不得把私钥、UUID、密码、订阅凭据提交到本仓库或日志。
3. 记录 `sbctl status`、`sbctl config validate`、`systemctl is-active sbctl.service sing-box.service`、监听端口和当前内核网络参数。

**验收**

- 新账号的公钥 SSH 和 sudo 成功；旧 root 会话仍保持可用。
- 配置校验和两个服务健康检查成功。
- 回滚材料已存在且权限仅管理员可读。

### 阶段 B：修复 IPv6 选择（P0）

**设计决定**

为没有 IPv6 地址/路由的部署新增或启用一个明确的“出站地址族策略”配置。该策略必须由 sbctl 生成最终 sing-box 配置，并对所有 direct 出站域名稳定选择 IPv4；不可依赖客户端碰巧只解析 A 记录。

在 sing-box 1.12 当前兼容窗口内，可以使用 IPv4-only 地址解析策略完成过渡；实现时同时记录对新 DNS/route 模型的迁移路径，避免依赖将被移除的遗留字段。最终字段与放置位置以候选配置经过本机 `sing-box check` 的结果为准。

**动作**

1. 先在 VPS 的临时路径生成候选配置；不覆盖运行中的 `/etc/sing-box/config.json`。
2. 对候选运行 `sing-box check`，并检查其解析/拨号路径不会选择 AAAA 地址。
3. 对 sbctl 管理源配置做一次原子提交，重启 `sing-box.service` 并使用既有健康检查确认服务 active；失败时恢复阶段 A 的已知良好版本。
4. 从真实客户端分别访问一个具有 A+AAAA 的 Google 目标、X 和订阅地址；同时采集短时间脱敏 sing-box 日志，确认不存在 `cannot assign requested address` 的 IPv6 拨号错误。

**验收**

- `sing-box check` 成功，服务重启后 active。
- 真实代理流量对 A+AAAA 域名使用 IPv4，错误日志不再出现 IPv6 `cannot assign requested address`。
- 五个既有协议 listener 和 sbctl 的 80/443 订阅入口保持原端口可用。

### 阶段 C：出口可用性判定（P0）

**动作**

1. 在阶段 B 后，用真实、已登录或未登录的受支持浏览器经该节点访问 ChatGPT；记录响应类别而非账号内容：正常页面、可完成的 Cloudflare 验证、HTTP 403 挑战、地区/不支持提示或其他错误。
2. 对照服务器侧执行 IPv4 HTTPS 连通性检查，并复核 sing-box 连接日志，确保浏览器流量确实经此出口。
3. 若浏览器仍稳定返回 Cloudflare 403/服务不可用提示，将结果定性为“出口 IP 声誉、IP 历史、地理归属或服务策略待供应商处理”，而非再修改协议、DNS 或端口。
4. 只有在不违反服务条款且业务确有需要时，向 VPS 提供商申请更换具有清晰归属和良好信誉的美国 IPv4；更换后重复本阶段与阶段 B 的验收。

**验收**

- X 的浏览器访问成功。
- ChatGPT 的最终状态有明确归类和证据；若未恢复，形成“更换/复核出口 IP”的供应商工单输入，而不是伪造已修复。

### 阶段 D：SSH、网络与日志加固（P1）

**动作**

1. 在阶段 A 的新管理员账号已验证后，禁用 SSH root 密码登录，启用公钥认证，关闭不需要的 X11 转发，并保留经过测试的恢复方式。
2. 先按当前监听表和 sbctl 部署模式写出端口白名单：SSH 管理端口、sbctl Direct 模式的 TCP 80/443、以及实际启用节点对应的 TCP/UDP listener。先在控制台或第二个 SSH 会话验证规则，再启用 UFW 或等价 nftables 策略。
3. 不为“看起来多余”的 80/443 直接关端口：它们是 sbctl Direct 订阅/ACME 的既有边界，须先确认部署模式。
4. 配置 SSH 失败登录限制（例如 Fail2ban 或等价服务）、journald 轮转与有限保留；将 sing-box 生产日志从 trace/debug 调至 info，避免长时间记录流量目的地。

**验收**

- 新管理员仍能 SSH+sudo；root 密码登录失败。
- 白名单外入站被拒绝，五个协议与订阅的已授权入口仍可从预期网络连通。
- 日志不再长期输出连接级跟踪内容，磁盘使用有可预期上限。

### 阶段 E：补丁、重启与性能维护（P1）

**动作**

1. 在维护窗口更新 Ubuntu 安全与常规补丁；更新前确认根分区可用空间、订阅/配置备份和控制台访问。
2. 重启以加载新内核；等待系统完全启动后检查网络、SSH、`sbctl.service`、`sing-box.service`、订阅入口及五个协议端口。
3. 使用 `sbctl system status` 评估内核拥塞控制；仅在内核支持且管理员批准时运行 `sudo sbctl system bbr`。这只持久化 BBR+FQ sysctl，不改变 sing-box 配置；TUIC 的协议级 BBR 已与其无关。
4. 为约 719 MiB 内存、当前无 swap 的实例创建 **1 GiB swapfile**：先确认根分区有至少 2 GiB 可用空间，再以 root 创建权限为 `0600` 的交换文件、格式化并启用；将其以 UUID/路径持久化写入 `/etc/fstab`，避免重启后失效。
5. 设置保守的 `vm.swappiness`（建议 `10`）并持久化到独立的 sysctl drop-in；不得覆盖与 sbctl BBR 配置无关的现有 sysctl 文件。
6. 记录启用前后的 `free`、`swapon --show`、磁盘可用空间、`vmstat` 与 OOM 日志；观察一个正常使用周期。Swap 的目的是缓冲瞬时内存压力、降低 OOM 风险，不是吞吐优化，也不能替代 IPv6 修复或 VPS 升配。

**验收**

- 系统无待重启标记，服务在重启后自动恢复。
- `sbctl config validate`、`sing-box check`、服务 health 和真实客户端回归均成功。
- BBR（若启用）显示为 `bbr` + `fq`；未启用时保留现状并记录理由。
- `swapon --show` 显示 1 GiB swap，`/etc/fstab` 在重启后仍自动启用；根分区保留充足可用空间，未出现 OOM 或异常 swap 抖动。

## 6. 测试策略与停止条件

测试优先走对用户真实可见的最高层接口：真实客户端的节点导入/访问、订阅入口、systemd 服务健康和 `sing-box check`。单独的 `curl` 仅作为网络诊断证据，不可替代浏览器或客户端验收。

立即停止并回滚到阶段 A 快照的条件：

- 候选配置未通过 `sing-box check`；
- 服务重启后 inactive 或任一既有 listener 消失；
- 新 SSH 公钥路径未验证就准备关闭 root 密码登录；
- 防火墙候选规则无法明确保留已启用协议/订阅的端口；
- ChatGPT 仍返回策略/信誉拦截，却有人提议通过未评审的 WARP、Argo 或第三方匿名出口“绕过”。

## 7. 后续产品工作（不阻塞 VPS 修复）

为避免下一次 sbctl 更新重现 IPv6 地址族问题，建议单列一个 sbctl 功能任务：

- 配置模型增加可验证的 `outbound_ip_family`（或等价名称），默认按宿主网络能力选择；无 IPv6 时默认 IPv4-only。
- 生成器将该策略写入当前 sing-box 推荐的 DNS/route 位置；旧版本兼容逻辑须明确隔离。
- 覆盖五协议服务端配置、sing-box JSON/Clash/URI 订阅输出、候选 `sing-box check`、原子回滚和真实 systemd 验收。
- 不将 WARP/Argo/供应商专属分流纳入该功能；它们若有需求，应以独立、可选且供应商边界清晰的设计评审立项。

## 8. 参考

- `README.md`：sbctl 的部署模式、服务边界、BBR 命令和防火墙责任。
- `docs/sing-box-yg/03-singbox-kernel.md`：上游内核/服务模型及其未校验、root 常驻风险。
- `docs/sing-box-yg/04-five-protocol-nodes.md`：五协议和客户端字段。
- `docs/sing-box-yg/05-warp-argo-outbounds.md`：WARP/Argo 的外部依赖和分流范围。
- `docs/sing-box-yg/06-client-configs-subscription.md`：客户端工件与订阅分发风险。
- `docs/sing-box-yg-port-plan.md`：可移植与暂缓功能的既有决策。
- `docs/adr/0007-transactional-protocol-port-changes.md`、`docs/adr/0011-socket-activated-direct-https.md`、`docs/adr/0012-sing-box-runs-without-root.md`、`docs/adr/0018-upstream-capability-boundary.md`。

## 9. 执行记录

执行日期：2026-09-13

- [x] 创建仅 root 可读的变更前快照：`/root/sbctl-backups/prechange-20260913-1120/`。
- [x] 对五个 sing-box 入站应用 `ipv4_only`，候选与生效配置均通过 `sing-box check`，重启后五个 listener 和 sbctl 的 80/443 订阅入口均为 active。
- [x] 将 sing-box 日志级别调整为 `info`，降低连接级追踪日志暴露。
- [x] 创建 1 GiB `/swapfile`，写入 `/etc/fstab`，设置并持久化 `vm.swappiness=10`；重启后已验证自动启用。
- [x] 启用 UFW：默认拒绝入站、允许出站；仅放行 SSH、TCP 80/443、现有三个 TCP 节点端口和两个 UDP 节点端口。
- [x] 安装并启用 Fail2ban；`sshd` jail 已正常运行。
- [x] 应用 Ubuntu 更新并重启；运行内核已由 `5.15.0-30-generic` 更新为 `5.15.0-191-generic`，SSH、sbctl、sing-box、Fail2ban、Swap 和 UFW 均在重启后通过健康检查。
- [x] 创建 sudo 管理账号 `vpsadmin` 并安装本机已有 RSA 公钥；服务器已启用 `PubkeyAuthentication`。
- [ ] 完成 SSH 最终加固：本机的 `gcp` 私钥受口令保护而当前 SSH Agent 未运行，非交互验证无法完成签名。需由管理员手动以该私钥登录一次 `vpsadmin` 并验证 sudo；成功后才能禁用 root 密码登录、关闭密码认证和 X11 转发。
- [x] 将 IPv4-only 策略写入 sbctl 的生成模型并发布部署（v0.1.22/v0.1.23，2026-09-13）：
  - 生成器为五个入站写 `domain_strategy`、为 vless-reality 握手写 `tls.reality.handshake.domain_strategy`；宿主无 IPv6 路由时自动应用（UDP `connect` 路由探测），`ipv4_only` 持久化字段保留为双栈主机上的显式强制项。
  - 生成器固定输出 `log.level=info`（v0.1.23）：0.1.22 首次 regenerate 曾把手工补丁的 info 日志级别抹回 sing-box 冗余默认，已按阶段 D 意图修复。
  - 修复 master CI 预存红测试：base64 URI 工件断言未先解码（`carries the new canonical node field` 自该工件加入断言列表起恒假）；real acceptance 订阅 curl 补 `--retry` 消除启动竞态。master CI 自 v0.1.20 起首次全绿。
  - 发布流水线（tag → 构建 amd64/arm64 → systemd 验收 → 签名 manifest → GitHub Release）v0.1.22、v0.1.23 均绿；VPS 经 `sbctl update` 升至 0.1.23 并 `sbctl regenerate` 后，生效配置由生成器产出（log=info、五入站 + 握手 `ipv4_only`），工件与 active 配置一致，REALITY 端到端复验通过。
  - **部署验收（2026-09-13，NovixLink 生产 VPS）**：`sbctl config validate`/`node`/`sub` 通过；四种订阅格式经真实 443 入口（socket activation）均 200 且带实时 `subscription-userinfo`，base64（`/sub/<cred>/uri.txt`）解码出 5 个节点，错误凭据 404，clash 工件含新 proxy-groups/routing；五协议用订阅工件本机回环逐一端到端（vless-reality/vmess-ws/hysteria2/tuic/anytls）访问 x.com 全部 HTTP 200（0.17–0.20s）、chatgpt.com TLS 完成返回 403 挑战；`sbctl restart` 后双服务恢复、7 个监听套接字齐全、日志零错误。
  - **v0.1.24（2026-09-13）**：按管理员提供的 Clash Party 覆盖模板重写 clash 订阅分组与规则——`🌍选择代理节点`（select：`♻️自动选择`/DIRECT/五节点）+ `♻️自动选择`（url-test gstatic 204、interval 300、tolerance 50）；规则改为 DOMAIN-SUFFIX×7 → 选择组、`GEOIP,LAN,DIRECT`（吸收原四条私网 IP-CIDR）、`GEOIP,CN,DIRECT`、`MATCH,🌍选择代理节点`；DNS nameserver 的 `#sbctl-proxy` 片段同步改名。已发布部署并在 443 入口实拉验证（YAML 解析通过，2 组 10 规则）。
  - **v0.1.25（2026-09-13）**：`🌍选择代理节点` 增加 `url: http://connect.rom.miui.com/generate_204`（interval 300）——DIRECT 成员此前在客户端延迟测试中恒超时，因组测试回退到国内直连不可达的 gstatic；`♻️自动选择` 保留 gstatic 以便节点选择继续衡量国际可达性。REJECT 延迟测试必然失败属策略语义（掐断连接），非故障。已发布部署并实拉验证。
  - **v0.1.26（2026-09-13）**：应用户要求把选择组测试地址换成 `http://aliyun.com/generate_204`。该地址 301/302 重定向，已用 mihomo v1.19.13 实测确认重定向响应被计为成功探测（`/proxies/DIRECT/delay` 返回真实延迟），VPS 侧亦可达。GLOBAL/独立 DIRECT 卡片的测试地址属客户端全局默认值，订阅不可控，建议用户在 Clash Party 设置中同步修改。已发布部署并实拉验证。
  - 提醒：sing-box 1.12 对旧式 `domain_strategy` 字段报弃用警告（1.14 移除），届时需迁移到 `domain_resolver` 新模型（见第 7 节）。
- [ ] 以真实浏览器经节点完成 ChatGPT 验收。服务器侧 HTTPS 到 ChatGPT 仍会收到 Cloudflare challenge/403；这属于出口信誉或平台策略待判定，不应通过 WARP/Argo 等方式规避。

### 补充执行：REALITY 握手 IPv6 修复（2026-09-13 下午）

- **症状**：客户端经 vless-reality 节点访问任何网站均立即 `ERR_CONNECTION_CLOSED`；服务器日志对每个连接报 `TLS handshake: REALITY: failed to dial dest: dial tcp [2606:4700::…]:443: connect: network is unreachable`。
- **根因**：REALITY 握手伪装目标（`www.cloudflare.com:443`）由一个独立于入站 `domain_strategy` 的专用拨号器拨出（sing-box `common/tls/reality_server.go`：`dialer.New(ctx, options.Reality.Handshake.DialerOptions, …)`）。阶段 B 的运行时补丁只给入站加了 `ipv4_only`，握手拨号器仍默认选 AAAA，而本机无全局 IPv6 路由，导致所有 REALITY 握手失败。
- **修复**（事务流程：备份 `/root/sbctl-backups/prechange-20260913-125829/config.json` → 候选配置 `sing-box check` 通过 → 原子替换 → 重启）：在 `/etc/sing-box/config.json` 的 vless 入站 `tls.reality.handshake` 中新增 `"domain_strategy": "ipv4_only"`（sing-box 1.12 中 `InboundRealityHandshakeOptions` 内嵌 `DialerOptions`，该平铺字段直达握手拨号器；1.14 将移除旧式字段，届时需迁移到 `domain_resolver`）。
- **验证**：重启后五个 listener 与 sbctl 80/443 订阅入口均 active；在 VPS 本机用临时 sing-box 客户端实例经 `127.0.0.1:64477` 完整走 REALITY+vision：`https://x.com` 返回 200（0.24s）；`https://chatgpt.com` TLS 完成、Cloudflare 返回 403 `cf-mitigated: challenge`（连接不再被掐断，属上述出口信誉待判定项）；测试期间 sing-box 日志无错误。测试实例与临时文件已清理。
