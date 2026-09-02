# 上游实现调研：`sing-box-yg` 与 `vps-sub-meter`

调研日期：2026-09-02。本文只依据两项目的公开源码；链接固定到本次读取的 commit，避免 `main` 后续变化改变证据。

## 结论

两个项目值得借鉴的是安装向导、配置/订阅产物的分离，以及把长驻服务交给 systemd 管理的思路。它们都不适合作为新工具的安全或发布架构的直接基础：前者是大而全的 root Bash 脚本，后者依赖前者的文件布局并另启 Python、Caddy、vnStat 等组件。

对于本项目的单人 VPS 场景，上游支持如下取舍：

- 流量口径应明确为**指定默认出口网卡的 RX + TX**，而非 per-proxy 或 per-subscription-user；`vps-sub-meter` 从 sysfs 计数器及基线得出同一口径。
- 每月额度首先只在 `subscription-userinfo` 中展示；上游也只是对超额值做显示钳制，并没有在 sing-box 数据面断流。因此若将来要强制限额，需要独立的 sing-box 规则/计数设计和验收测试。
- HTTPS、认证、订阅令牌、配置提交和升级校验应作为 `sbctl` 的明确边界，而非从上游复制 `curl | bash`、root cron 和可预测的共享 secret。

## `yonggekkk/sing-box-yg`

源码版本：[`6bd17f0`](https://github.com/yonggekkk/sing-box-yg/tree/6bd17f02e003597f595f71666ed8875bfe1aa2e1)。

### 架构与安装

- README 将入口定义为远程 Bash 进程替换，并宣称生成本地订阅，提供五种协议；脚本支持 `wget` 或 `curl` 入口。[README](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/README.md#L1-L4) [安装示例](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/README.md#L45-L53)
- `sb.sh` 做 CPU/发行版探测，覆盖 Debian、Ubuntu、CentOS 系，但拒绝 Arch；随后直接由系统包管理器安装大量依赖（如 `jq`、`cron`、`socat`、iptables、Python、二维码工具和 Git）。[平台和依赖处理](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L35-L115)
- 脚本抓取 GitHub 的最新 release 并下载 tarball，但该下载段没有对二进制进行 checksum 或签名验证。[下载逻辑](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L212-L235)
- 证书可自签，也可执行另一个远程 `acme-yg` 脚本来走 80 或 DNS API 校验。[证书路径](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L241-L310)

### 配置、服务与订阅

- 配置通过 heredoc 直接生成到 `/etc/s-box/sb10.json`；一个 UUID 同时被 VLESS、VMess、Hysteria2、TUIC 使用，WS path 也由该 UUID 派生。[配置生成](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L400-L528)
- systemd 单元由脚本生成，以 root 身份直接执行 sing-box，并在失败后重启。[服务单元](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L891-L910)
- 订阅 token 默认就是代理 UUID。脚本将配置/链接以软链接暴露给 BusyBox `httpd`，随机选高端口并通过 `crontab @reboot` 重启；此路径没有 TLS、HTTP 认证、订阅头或速率限制。[订阅 HTTP 服务](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L3099-L3177)
- 自更新从 `main` 下载脚本且使用 `--insecure`；另有每日 cron 重启 sing-box。[更新及 cron](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L3814-L3835)
- 卸载会删除整个 `/etc/s-box`，并清空 NAT `PREROUTING` 链，这不应由一个新工具照搬。[卸载处理](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L3909-L3933)

### 可借鉴与避免

可借鉴：互动式首次安装体验、架构/平台探测、按协议选择生成 config 和链接、以及 systemd 生命周期管理。

避免：把 UUID 同时当作代理身份和订阅机密；远程未校验脚本/二进制；以 root 常驻 HTTP 服务、cron 管理关键服务；就地 `sed` 改配置；没有锁、`sing-box check`、原子提交或回滚；以及会影响其他应用的防火墙清理。源码内也没有流量、额度或账号记账实现，因此不能用它作为这些功能的参考。

## `xiaolingxiaoying/vps-sub-meter`

源码版本：[`6cba513`](https://github.com/xiaolingxiaoying/vps-sub-meter/tree/6cba513687a7479ab7a3afe4ff9d02b37d985998)。README 明确要求先运行 `sing-box-yg`，再执行本项目的远程 `auto_setup.sh`，因此两者存在文件路径和服务命名耦合。[README](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/README.md#L1-L8)

### 架构、订阅与 HTTPS

- `auto_setup.sh` 只接受具有 `apt` 的 Debian/Ubuntu 主机；它以 root 运行，并安装 vnStat、Python、curl、OpenSSL、jq、Caddy 等依赖。[前提检查](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L28-L42) [依赖与 Caddy 安装](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L876-L904)
- 它每五分钟从 `sing-box-yg` 的固定路径复制 Clash YAML、sing-box JSON 和 Shadowrocket 文本到服务目录，使用临时文件后 rename，以减少读者看到半份文件的风险。[刷新副本及 timer](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L971-L1038)
- 随机订阅 token 用 `openssl rand -hex 24`（48 个十六进制字符）生成。Python 标准库 `HTTPServer` 仅绑定 `127.0.0.1`，只按精确 token 路由 YAML、JSON、TXT，并将 `subscription-userinfo` 放在响应中。[token](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L845-L848) [HTTP 进程和路由](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1314-L1357) [响应头](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1510-L1522)
- Caddy 在公网端处理证书、精确路由、反向代理和 Basic Auth；同一 token 也可作为 query 参数绕过 Basic Auth。Caddyfile 先 `validate` 再重启，这一点可取，但它仍直接覆写 `/etc/caddy/Caddyfile`（会接管已有 Caddy 实例）。[Caddyfile 生成和认证例外](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1603-L1676)
- 另有 timer 从 Caddy 的内部证书目录复制证书和私钥到 `sing-box-yg` 证书目录并重启 sing-box；这说明两个服务同用 TLS 资产会增加耦合和重启风险。[证书同步](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L230-L395)

### 流量与周期

- 流量基于 `/sys/class/net/<iface>/statistics/{rx,tx}_bytes` 与状态文件中的周期基线；这是整张网卡的统计，包含非代理流量，不能归因到协议或 token。状态/重置由脚本和 systemd timer 管理。[基线重置脚本](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1040-L1312) [实时读取与合计](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1410-L1522)
- 当前实现的计量会在请求订阅时更新 `subscription-userinfo` 的 `upload`、`download`、`total`、`expire`。该端点可在固定自然月、锚定月或固定到期模式下返回空订阅，而不是在数据面阻断旧客户端的连接。[动态响应和到期处理](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1434-L1522)

### 可借鉴与避免

可借鉴：token 使用 CSPRNG；后端只监听 loopback；副本更新先写临时文件再原子替换；状态写入显式设置所有者与权限；并且在应用 Caddy 前做验证。它的周期/计数器回绕代码也说明本项目必须覆盖“主机重启、状态缺失、跨月停机”测试。

避免：依赖固定的 `/etc/s-box/*` 和 Caddy 私有证书目录；重复运行时覆写全局 Caddyfile；同时把 token 放在 URL path 和 query（query 常更容易进入日志、历史和 Referer）；Caddy + Python + vnStat + 多个 timer 的常驻/运维成本；通过远程 shell 入口安装外部依赖。订阅 token 应独立于代理 UUID，且日志必须脱敏。

## 对新工具的具体设计约束

1. 以 Debian/Ubuntu 的明确版本矩阵发布，经 hash/signature 固定版本的发行包；安装器下载后验证，变更配置时使用锁、临时文件、`sing-box check`、原子替换、reload，并保留回滚点。
2. `sbctl daemon` 应以专用非特权用户运行，订阅响应只提供单一、长度足够的随机 path token；不打印完整 token，不采用 query token 旁路认证。
3. 安装时探测默认路由接口并写入配置，允许显式覆盖；账户页和订阅头都标为“VPS 网卡总 RX+TX”。周期状态须有 schema version，使用 UTC 和可验证的跨期逻辑。
4. 不将 Caddy 视为必需组件。若追求最少常驻进程，可让订阅 daemon 自己终结 TLS，而由一个成熟、非长驻的 ACME 客户端负责签发和续期；无论实现选择为何，都不可自行省略 TLS 私钥权限、SNI、HTTP-01 端口占用和续期后的安全 reload。

## 对 Q9–Q14 的源码对照与建议

### Q9：把“流量重置”与“订阅刷新”分开

`vps-sub-meter` 将其清晰地实现为两件不同的事：每五分钟复制上游生成的订阅文件，而每次 HTTP 请求都即时读取当前计数、发送 `subscription-userinfo`，并由独立的每分钟 timer 根据周期基线重置状态。[订阅副本 timer](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L971-L1038) [重置 timer](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1295-L1312) [响应构造](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1434-L1522)

建议将 V0.1 命名定为：**账期重置**（每月某日 00:00 UTC）与**订阅响应**（每个 HTTP GET/HEAD 返回当前生成的 JSON 与响应头）。不要使用“订阅刷新事件”作为一个模糊概念；若日后需要缓存，另加一个可配置的“订阅配置重建”任务。

### Q10：443 的所有权与端口冲突

`sing-box-yg` 的直接入站端口默认从 10000–65535 随机选择，并用 `ss` 检查 TCP/UDP 占用，所以其通常没有把本地 VLESS/Hysteria2/TUIC 放在 443。[端口选择](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L317-L352) 它的 Argo/Cloudflare 输出链接可以显示远端 443，但那不是本机监听端口。[Argo 示例](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L1109-L1116)

上游没有通用的“同一 IP:443 上将 Reality 与 HTTPS 复用”的实现。推荐 V0.1 将 `80/443` 预留给订阅 HTTPS/ACME，让 sing-box 的所有直接入站选择、持久化并验证不冲突的高端口。创建节点前必须检查现有监听端口；不应悄悄抢占 443。

### Q11：订阅格式

两项目当前均提供三种产物：sing-box JSON、Clash/Mihomo YAML 与聚合文本。`sing-box-yg` 本地 HTTP 公开 `sbox.json`、`clmi.yaml`、`jhsub.txt`。[文件公开与 URL 输出](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L3151-L3173) `vps-sub-meter` 也从这三份源文件生成精确路由。[三格式初始化](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L931-L962)

推荐 V0.1 **只发布 sing-box JSON**：它是本项目控制的唯一输出模型，能避免跨格式转换与客户端兼容矩阵。若用户确实需要 Mihomo，再增加一个经过独立测试的 renderer，而不是复用上游的整文件副本。

### Q12：网卡选择

`vps-sub-meter` 先以 `ip route get 1.1.1.1` 找默认路由的 `dev`，失败时才拿第一个非 loopback 接口；它随后要求用户确认、验证该接口存在，并保存名称。[探测和确认](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L767-L804) 这正是适合 VPS 的一次性安装策略。

推荐 `sbctl install` 使用默认路由探测作为建议值，写入 `config.toml`，并提供显式 `traffic.interface` 覆盖。daemon 只能按保存值读取；运行中不自动漂移到另一张网卡，否则账期数字会在网络重配时失真。

### Q13：账期时间语义

上游支持自然月，或以首次日期/时间为锚点的按月周期；短月会将日期收敛到月末。[选项和短月说明](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L671-L765) 它依赖用户选择的时区，并在 Python 和 timer 中分别运算，增加了行为边界。

推荐 V0.1 使用 **UTC、每月 1 日 00:00、无限期账期**；账期开始时间保存为 ISO-8601 UTC，daemon 启动和每次读取状态时均检查是否跨期。这样主机离线也不会漏重置。若未来加“每月 N 日”，只接受 1–28 以消除短月策略和本地时区/DST 的复杂度。

### Q14：权限边界

`sing-box-yg` 的生成服务单元未设置 `User=`，因此 systemd 默认以 root 运行。[服务定义](https://github.com/yonggekkk/sing-box-yg/blob/6bd17f02e003597f595f71666ed8875bfe1aa2e1/sb.sh#L891-L910) 相比之下，`vps-sub-meter` 创建无登录的 `subsrv` 用户，订阅文件/状态为该用户所有，且其 Python server 单元以 `User=subsrv`、loopback 监听运行。[用户和文件权限](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L925-L969) [服务单元](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1545-L1573)

推荐采用后者的最小权限方向，但更严格：`sbctl daemon` 以单独无登录用户运行；配置写入、证书文件安装、创建 systemd unit、重启服务仅由管理员运行的 CLI 完成。若 daemon 自行终结 TLS，则私钥必须只向该服务用户可读；HTTP 监听采用 systemd socket activation 或在 daemon 需要的最低能力范围内绑定 443，不能为方便而整进 root。
