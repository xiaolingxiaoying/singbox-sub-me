# sbctl 发布版控制面规格

Status: ready-for-agent

## 依据与目标

本规格综合以下已确定材料：

- `docs/implementation-plan.md`
- `CONTEXT.md`
- ADR-0016：账期状态所有权与流量修正
- ADR-0017：发布、运行时与修正边界
- ADR-0018：上游能力边界

它定义 sbctl 从当前 Rust 原型完善为可发布的单管理员 VPS 控制面所需的行为、接口、边界和验收条件。`CONTEXT.md` 中的术语是本规格的唯一领域词汇来源；`singbox-sub-plan.md` 仅作历史参考，不得覆盖本规格或 ADR 的决定。

## 问题

单一管理员需要在 Debian 或 Ubuntu VPS 上部署和维护 sing-box，并通过私密订阅 URL 获取同一组代理节点的多种客户端格式。控制面必须能够可靠处理 VPS 总流量账期、证书、非 root 运行、发布更新和失败恢复，同时不能接管主机上已有的 sing-box、反向代理、防火墙或其他服务。

## 方案

提供一个 Rust `sbctl` 二进制作为控制面，sing-box 作为数据面。sbctl 管理五种 Managed protocol、三种 Subscription format、Direct HTTPS/External proxy/IP fallback 三种交付模式、按指定网卡 RX+TX 计算的 VPS traffic，以及可验证、可原子提交、可回滚的安装和更新事务。

当前代码已经提供配置、协议生成、三种订阅格式、基础流量累计、CLI acceptance seam、artifact 回滚骨架和部分生命周期逻辑；本规格中的新行为优先修正现有实现与新 ADR 的不一致处，随后补齐真实 systemd、签名 manifest、账期写入边界和发布 gate。

## 用户故事

1. 作为单一管理员，我希望在受支持的 Debian 或 Ubuntu systemd VPS 上安装 sbctl，以便用一个控制面维护 sing-box。
2. 作为单一管理员，我希望安装前发现 Existing deployment 时操作被拒绝，以便既有服务和文件不被静默接管。
3. 作为单一管理员，我希望交互式安装默认启用五种 Managed protocol，以便新部署具备完整协议覆盖。
4. 作为单一管理员，我希望逐项禁用不需要的 Managed protocol，以便不暴露多余监听端口。
5. 作为单一管理员，我希望每个 Enabled protocol 都有独立的 Protocol listener port 和 Proxy credential，以便协议之间相互隔离。
6. 作为单一管理员，我希望 Subscription credential 与所有 Proxy credential 分离，以便节点凭据不能读取订阅。
7. 作为单一管理员，我希望分别配置 Subscription host、Proxy host 和 Reality decoy SNI，以便交付地址、代理地址和 Reality 伪装身份不混淆。
8. 作为单一管理员，我希望选择 Direct subscription mode，以便 sbctl 直接提供 ACME HTTP-01 和 HTTPS 订阅。
9. 作为单一管理员，我希望选择 External proxy，以便已有反向代理继续拥有公网 80/443，而 sbctl 只监听 loopback。
10. 作为单一管理员，我希望在没有可用域名时使用 IP fallback subscription，以便通过明确标注低安全性的高位 HTTP 端口获取订阅。
11. 作为客户端用户，我希望获得 sing-box JSON、Clash/Mihomo YAML 和 URI text，以便使用不同客户端导入同一组节点。
12. 作为客户端用户，我希望三种 Subscription format 的节点、端口、凭据和 TLS 字段一致，以便格式切换不会改变连接目标。
13. 作为单一管理员，我希望订阅响应带有与当前账期一致的 `subscription-userinfo`，以便客户端展示 VPS traffic、限额和下一次重置。
14. 作为单一管理员，我希望 VPS traffic 按指定网卡 RX+TX 统计，以便结果对应 VPS 总流量而非某个协议或用户。
15. 作为单一管理员，我希望安装时自动探测默认路由网卡并允许显式覆盖，以便适配不同 VPS 网卡命名。
16. 作为单一管理员，我希望使用 Natural-month reset，以便账期在所选 accounting timezone 的每月 1 日 00:00 开始。
17. 作为单一管理员，我希望使用 Anchored-month reset 并选择首个重置日期、时间和时区，以便账期按我的月度计划开始。
18. 作为单一管理员，我希望锚定日为短月不存在时收敛到月末，以便 29、30、31 日设置在所有月份都可运行。
19. 作为单一管理员，我希望在 First reset instant 之前看到合法的 Pending first reset 和零使用量，以便未来锚定时间不会被误报为故障。
20. 作为单一管理员，我希望不存在或含糊的 DST 本地时间被拒绝，以便账期不会因时钟歧义而错误切换。
21. 作为单一管理员，我希望主机重启、counter rollback、停机跨月和状态恢复都保留正确历史累计，以便计数器变化不会丢失已用量。
22. 作为单一管理员，我希望用 `sbctl traffic set-used --bytes <TOTAL>` 修正总已用量，以便修复总量而不伪造 RX/TX 方向。
23. 作为单一管理员，我希望用 `sbctl traffic set-used --rx <BYTES> --tx <BYTES>` 修正方向统计，以便精确恢复两个方向的 VPS traffic。
24. 作为单一管理员，我希望流量修正显示当前账期、实际值、目标值和下一次重置，并在锁内原子提交，以便修正可审计且不会与 reset 竞争。
25. 作为系统运行时，我希望只有 accounting reset task 和显式修正命令写入 accounting state，以便 traffic/status 读取和订阅请求保持只读。
26. 作为单一管理员，我希望状态缺失、损坏或 schema 不兼容时得到脱敏 503，而不是 200 占位订阅，以便知道部署不可用且不会收到虚假数据。
27. 作为单一管理员，我希望无效路径、query credential 和错误 credential 都统一得到 404，以便认证失败不泄露授权细节。
28. 作为单一管理员，我希望完整 Subscription credential 不出现在日志、错误和诊断中，以便降低 URL 泄露风险。
29. 作为单一管理员，我希望 sbctl 和 sing-box 使用不同的无登录服务账户，以便控制面和数据面的权限相互隔离。
30. 作为单一管理员，我希望 Direct HTTPS 的 80/443 由 systemd socket 持有，并由同一个 sbctl 进程按本地端口区分连接，以便 daemon 不需要 root 或通用低端口 capability。
31. 作为单一管理员，我希望证书加载前验证有效期、SAN、私钥匹配和 SNI，以便错误证书不会导致不透明的运行时故障。
32. 作为单一管理员，我希望 Certbot timer 负责续期且 deploy hook 触发验证和安全 reload，以便续期后新 HTTPS 连接使用新证书。
33. 作为单一管理员，我希望配置、订阅工件、unit/socket/timer、二进制和账期状态都能原子提交和失败恢复，以便并发读取只能看到完整版本。
34. 作为单一管理员，我希望发布 manifest 的签名先于 URL 和 SHA-256 被信任，以便下载源和工件摘要不能被未授权修改。
35. 作为单一管理员，我希望候选 sbctl 和 sing-box 在替换前分别健康检查和配置检查，以便失败更新自动恢复上一版本。
36. 作为单一管理员，我希望 manifest 声明 sing-box 兼容矩阵，以便不兼容的候选版本在安装前被拒绝。
37. 作为单一管理员，我希望安装成功后才写 ownership marker，以便失败安装只清理本次创建的资源。
38. 作为单一管理员，我希望 `status --json` 和 `diagnostics` 展示服务、socket、证书、账期、工件及最近失败原因，以便维护无需手工检查文件。
39. 作为单一管理员，我希望 `credential rotate` 立即使旧订阅 URL 失效并生成新 URL，以便泄露的 Subscription credential 可被立即撤销。
40. 作为单一管理员，我希望卸载默认保留 root 可读备份，且 `--purge` 只删除 sbctl 自有资源，以便清理操作可恢复且不伤及主机其他服务。
41. 作为发布维护者，我希望 release gate 在真实 systemd、非 root、签名 manifest、证书和失败恢复验收完成后才通过，以便单元测试通过不被误当作生产支持。

## 实现决策

### 1. 领域模型与持久化

- `DeploymentConfig` 必须持久化 `subscription_mode`、主机字段、接口、五种协议选择与凭据、端口、monthly traffic limit、accounting policy、accounting timezone、anchored reset 配置和证书引用。
- accounting timezone 默认固定为 `UTC`；显式配置必须是有效 IANA timezone，不能修改 VPS 操作系统时区。
- `anchored_reset_at` 使用 `YYYY-MM-DDTHH:MM`。锚定日允许 1–31；短月按当月最后一天计算。
- `TrafficState` 必须带 schema version、cycle key、interface、baseline RX/TX、accumulated RX/TX、boot ID 和独立的手工修正记录。Total traffic adjustment 不得写成伪造的方向计数。
- 状态缺失或损坏是可诊断的存储错误；Pending first reset 是合法状态，不是错误。
- 账期切换与 Subscription format 生成完全独立；订阅请求不得触发 reset 或写状态。
- 修改 accounting policy、timezone 或 first reset instant 时，必须确认并建立新的 accounting state，旧账期进入历史记录。

### 2. CLI 与配置向导

- 保留并完善 `install`、`config init/show/validate`、`status`、`traffic`、`node`、`sub`、`restart`、`diagnostics`、`credential rotate`、`certificate`、`update`、`uninstall` 和独立 `sing-box` 生命周期命令。
- 交互向导采用“读取已有配置、逐项修改、逐项校验、摘要确认、事务提交”。空输入保留当前值；新部署使用安全默认值。
- 向导覆盖模式、Subscription host、Proxy host、Direct 邮箱、IP fallback 高位端口、五协议及端口、限额、Traffic correction、timezone、账期策略、First reset instant、接口和 External proxy loopback 端口。
- 提交前验证端口范围 `10000–65535`、跨 TCP/UDP 数字唯一、端口占用、网卡存在、主机格式、IANA timezone、DST 本地时间和模式前置条件。
- 非交互安装必须显式提供必要参数，不能从终端或系统状态猜测安全敏感配置。任何摘要、日志和错误不得打印完整 credential、私钥或密码。

### 3. 订阅与协议

- 统一 canonical node model 作为 sing-box server config、sing-box JSON、Clash/Mihomo YAML 和 URI text 的唯一来源。
- Managed protocol 固定为 VLESS Reality、VMess WebSocket、Hysteria2、TUIC v5、AnyTLS。每个 Enabled protocol 使用独立配置和监听端口。
- Subscription credential 仅允许出现在 URL path；query 参数永不认证。代理 UUID/password 只用于代理节点，不可读取订阅。
- 明确路由和 content type；响应动态生成 `subscription-userinfo`，`download=RX`、`upload=TX`、`total` 为当前账期总 VPS traffic，`expire` 为下一次重置时间。
- 节点或配置改变后，三种缓存工件用临时文件和 atomic rename 一次性替换；请求只读取完整工件并只计算当前 header。
- 协议或端口变更必须获取 operation lock，执行 sing-box check，成功后原子替换并 reload/restart；失败恢复上一版本。

### 4. 账期、计数器与写入者

- 流量来源固定为所选 Linux interface 的 sysfs `rx_bytes` 和 `tx_bytes`：`download=RX`、`upload=TX`、`total=RX+TX`。
- accounting reset service/timer 每分钟执行一次，使用 `Persistent=true`；cycle key 决定是否真正切换并避免重复写状态。跨月停机由 persistent timer 补执行。
- counter rollback 或 boot ID 改变时保留既有累计值，并以新计数器作为后续 baseline；不丢弃另一方向仍有效的增量。
- 只有 `sbctl-accounting-reset.service/timer` 和管理员显式 Traffic correction 命令可以写 accounting state。`sbctl traffic`、`status`、`diagnostics` 和 HTTP subscription handler 只读或读取后计算，不写入。
- Total-only correction 作为独立 adjustment 保存，方向值保持原测量值；direction-aware correction 分别设定 RX/TX，并通过 baseline reconciliation 实现，不修改网卡计数器。
- correction 支持目标大于当前计数器，用于 VPS 重启后恢复历史已用量；必须锁定、原子替换，并先展示变更摘要。

### 5. 运行时与证书

- Direct HTTPS 使用 `sbctl-http.socket` 的两个 `ListenStream`（TCP 80、TCP 443）和 `sbctl.service`；通过 `LISTEN_FDS` 接收并按本地端口路由 HTTP 与 TLS。
- `sbctl.service` 以 `User=sbctl` 运行；`sing-box.service` 以独立 `User=sing-box` 运行。root 仅用于管理员生命周期操作。
- HTTP 使用 Hyper/Axum，设置请求大小、读取超时、并发、连接关闭等边界；不继续扩展手写 TCP HTTP 解析。
- Direct 模式的证书由 Debian/Ubuntu Certbot 管理，sbctl 提供 ACME challenge、加载校验和 deploy hook；External proxy 不改写 Caddy/Nginx，IP fallback 使用高位 HTTP。
- 证书加载必须验证有效期、SAN、证书与私钥匹配，以及连接 SNI 与 Subscription host 一致；失败返回脱敏 5xx 并写脱敏诊断日志。
- socket、service、timer、证书路径和权限必须在安装事务中一起写入并通过健康检查；External proxy 仍只监听 loopback。

### 6. 发布、安装与回滚

- Release manifest 使用版本化 schema，含固定版本、sbctl/sing-box artifact URL、SHA-256、sing-box 兼容矩阵和 Ed25519 signature。
- 签名覆盖去除 `signature` 字段后的 canonical JSON；signature 使用标准 Base64。客户端内置第一版发布公钥。
- 客户端必须先验证 manifest signature，再信任 URL/digest；安装脚本和 Rust 更新逻辑共享同一套 canonicalization、签名和摘要规则。
- 下载写入临时文件，digest 通过后 atomic replace；候选 sbctl 执行 health check，候选 sing-box 执行配置检查。
- 更新前保存 sbctl、sing-box、配置、订阅工件、unit/socket/timer、证书引用和 accounting state 的 rollback point。启动失败或健康检查失败时完整恢复并重新验证服务。
- 安装阶段为下载验证、创建账户/目录、写配置和工件、写 unit/socket/timer、daemon-reload、启动、健康检查。全部成功后才写 ownership marker；失败只回滚本次创建资源，不触碰 Existing deployment。
- 不支持 `latest`、`main` 或未签名远程更新；manifest 兼容矩阵不满足时在安装/更新前拒绝。

### 7. 上游能力边界

吸收交互向导、默认网卡探测、可配置账期和修正、短月处理、counter rollback、三种订阅格式、原子工件更新、证书/私钥检查及 systemd 生命周期等行为能力；不复制上游文件布局或服务。

首版不实现固定到期、Basic Auth、上游兼容 URL、WARP/IPv4/IPv6 出站切换、云厂商入口、Caddy 全局管理、Argo/Cloudflare 隧道和自动防火墙修改。

## 测试决策

最高测试 seam 是黑盒 CLI acceptance 加真实或等价 Debian/Ubuntu systemd VM 验收。测试通过管理员可见命令、生成的工件、HTTP 响应、systemd 状态和文件边界验证行为，不依赖内部 mock。纯账期函数测试用于补充边界组合，不替代 acceptance。

必须覆盖：

- 五协议、三格式、独立凭据、端口唯一性和 sing-box 配置检查；
- Direct、External proxy、IP fallback，包含 loopback/public 端口和低安全警告；
- Natural-month、Anchored-month、UTC/非 UTC、首个 reset 前 pending、1–31 日、短月收敛、DST 不存在/含糊时间拒绝；
- RX/TX 映射、首次观察、增量、boot ID、counter rollback、重启、状态缺失/损坏/schema mismatch、停机跨月；
- total-only 与 direction-aware correction、目标大于当前计数器、并发 correction/reset 和原子状态读取；
- 404 统一失败、503 脱敏存储失败、query credential 拒绝、日志 credential redaction、`subscription-userinfo` 一致性；
- socket activation、LISTEN_FDS 端口区分、两个非 root 服务、ACME challenge、证书 SAN/有效期/私钥/SNI、续期后新连接；
- manifest canonical JSON、Ed25519/Base64 签名、URL/digest 先后信任顺序、兼容矩阵、下载失败、健康检查失败和完整回滚；
- 安装 ownership marker 时序、Existing deployment 不变、默认卸载备份、`--purge` 仅清理 sbctl 自有资源，且不改 Nginx/Caddy/iptables/NAT/手工 sing-box；
- release gates 在真实 systemd 主机或等价 VM 上通过，而不是只依赖 WSL2 或 Rust 单测。

当前基线：`cargo test` 已通过现有 22 个库测试和 31 个 CLI 测试；这些结果不代表上述新增真实主机验收已经完成。

## 非范围

- 多管理员、subscriber account、per-user traffic、per-user quota、按月流量上限断流。
- 自动导入、迁移或接管 Existing deployment、上游配置/凭据/服务、Caddy、Nginx、防火墙、NAT 或云厂商部署。
- query token、Basic Auth、上游 URL 兼容路径、User-Agent 自动猜格式。
- root 常驻 sbctl/sing-box、全局低端口 capability、全局 iptables/NAT 清理。
- 未签名或 latest/main 远程更新、自动后台升级、定时强制重启。
- 固定到期模式、WARP/IPv4/IPv6 出站切换、Argo/Cloudflare 隧道、云厂商专用入口。
- 自研 ACME daemon、IP HTTPS 作为常规路径、数据库、Web 管理面板、QR UI。
- 非 Debian/Ubuntu、非 systemd 生产支持，以及仅凭 WSL2 验证得出的生产兼容声明。

## 交付与追踪

本规格位于 `.scratch/sbctl-release/spec.md`，状态为 `ready-for-agent`。后续实现票据应拆分到 `.scratch/sbctl-release/issues/`，每个票据单独编号并保持依赖顺序。实现前应继续以本规格、`CONTEXT.md` 和 ADR-0016/0017/0018 为准；若实现发现矛盾，必须显式提出 ADR 冲突，而不是静默改变边界。
