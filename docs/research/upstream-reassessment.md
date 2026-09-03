# 上游源码级复评：`vps-sub-meter` 与 `sing-box-yg`

调研日期：2026-09-03  
调研方式：将两个公开仓库克隆到 `.scratch/upstream/`，对安装脚本、生成配置、服务单元、订阅服务、计量/定时任务、证书、更新和卸载逻辑进行源码阅读。本文只新增研究记录，不修改当前项目业务代码。

## 固定的源码版本

| 项目 | 本地源码 | 本次复核 commit |
|---|---|---|
| `xiaolingxiaoying/vps-sub-meter` | `.scratch/upstream/vps-sub-meter-retry` | [`6cba513687a7479ab7a3afe4ff9d02b37d985998`](https://github.com/xiaolingxiaoying/vps-sub-meter/tree/6cba513687a7479ab7a3afe4ff9d02b37d985998) |
| `yonggekkk/sing-box-yg` | `.scratch/upstream/sing-box-yg-retry` | [`9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc`](https://github.com/yonggekkk/sing-box-yg/tree/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc) |

用户给出的第二个项目入口：

```bash
bash <(wget -qO- https://raw.githubusercontent.com/yonggekkk/sing-box-yg/main/sb.sh)
```

该命令本质上是在本机 root shell 中下载并立即执行远程 `sb.sh`；仓库 README 同时展示了等价的 `curl` 入口。[README](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/README.md#L1-L4)

## 结论摘要

两个上游项目验证了以下功能需求确实存在：交互式安装、多协议配置生成、HTTPS 订阅、`subscription-userinfo`、网卡总流量统计、证书续期/同步和 systemd 生命周期管理。

但它们的安全边界不同于 sbctl：

- `sing-box-yg` 是一个以 root 为中心的大型 Bash 安装/管理脚本，远程执行、动态下载、cron、iptables 和多个可选组件交织在一起。
- `vps-sub-meter` 是依赖 `sing-box-yg` 文件布局的附加计量/订阅层，采用 Caddy + Python + vnStat/sysfs + 多个 systemd timer。
- 两者都没有发布者签名校验；`sing-box-yg` 下载 sing-box 和 cloudflared 时没有验证 checksum/signature。
- `vps-sub-meter` 的订阅后端可用专用用户和 loopback 监听，但公网鉴权由 Caddy 完成，并保留了把 token 放进 query 参数、绕过 Basic Auth 的兼容路径。

因此此前五项决策仍然成立，而且源码复评使它们更有必要：socket activation、manifest 签名、非 root、脱敏错误日志、真实 systemd 发布验收都应保留为发布约束。

## 1. 安装入口与安装事务

### `sing-box-yg`

`sb.sh` 首先探测发行版、架构和虚拟化环境，并直接通过 apt/yum/apk 安装 `jq`、`cron`、`socat`、iptables、Python、二维码工具、Git 等依赖；在 Debian/Ubuntu 路径中还显式安装 `systemctl` 相关包。[依赖安装](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L35-L115)

默认内核版本来自 GitHub `releases/latest` 页面解析，随后下载对应 tarball 到 `/etc/s-box/`、解压、移动二进制并设置为 `root:root`。下载段没有校验发布签名或 SHA-256。[内核下载](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L212-L235)

安装过程中还可能关闭/清空 firewalld 和 iptables 规则、停用既有 httpd/apache2，并创建多个 systemd/cron 任务。这类行为说明“安装成功”不能只等价于主服务 active；必须验证对宿主机既有服务和网络策略的影响。[网络与防火墙处理](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d11b5ff0c314bc/sb.sh#L167-L193)

### `vps-sub-meter`

`auto_setup.sh` 要求 root 和 Debian/Ubuntu/apt，安装 vnStat、Python、Caddy、OpenSSL、jq 等依赖，并启动 vnStat、Caddy。它假设上游已经生成 `/etc/s-box/` 下的若干订阅文件，然后复制到 `/var/lib/subsrv/`。[前提与依赖](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L28-L42) [安装步骤](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L876-L969)

复制任务使用临时文件后 `mv`，避免订阅读取到半份文件；这是值得保留的原子更新习惯。[订阅副本与 timer](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L971-L1038)

不过整体安装仍是多阶段直接写入 `/etc`、创建 unit、启动服务，且会覆盖全局 `/etc/caddy/Caddyfile`（虽会先备份）。这不是一个完整事务：中途失败可能留下部分用户、文件、timer 或配置。[Caddy 配置写入](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1590-L1674)

### 对 sbctl 的影响

1. 安装器应采用“staging -> 校验 -> 原子提交 -> reload/start -> 验收”的事务模型，并在每个失败点测试恢复。
2. 不应把 `latest`、远程 shell、动态 URL 或未认证 manifest 作为信任根。
3. 安装器必须检测已有端口、systemd 能力、Caddy/反向代理和防火墙状态，不能静默接管宿主机资源。

## 2. 协议配置与凭据模型

`sing-box-yg` 的默认生成路径同时配置 VLESS Reality、Vmess WebSocket、Hysteria2、TUIC，较新内核还配置 AnyTLS；监听地址通常为 `::`，端口在 `10000-65535` 中随机选择，并以 `ss` 检查 TCP/UDP 占用。[端口选择](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L319-L354) [配置生成](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L400-L528)

它为多种协议生成同一个 UUID，并将其同时作为 VLESS/VMess 身份、Hysteria2 password、TUIC password；WebSocket path 也由 UUID 派生。[统一 UUID](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L416-L419) [协议用户字段](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L440-L526)

这种设计很方便生成链接，但违反凭据分离：代理凭据泄露时也会泄露订阅路由线索；订阅 token 不应复用协议 UUID。当前项目应继续将“节点凭据”和“订阅凭据”建模为两个独立的秘密。

另一个注意点是：上游的高端口选择与当前 sbctl 的 Direct 入口不同。上游通常让 sing-box 占用高端口，把 80/443 留给 CDN、Argo 或 Caddy；因此它不能证明“sing-box 直接拥有 80/443”这一实现路径。[Argo/代理输出](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L1091-L1128)

### 对 sbctl 的影响

- 保留 socket activation 决策：上游的高端口做法是另一种产品拓扑，不是对 Direct HTTPS 需求的替代。
- 在节点生成器中显式区分 `proxy credential`、`subscription credential`、Reality private key 和 TLS private key。
- 生成配置后必须执行 schema/协议校验和端口冲突检查，不能只检查 JSON 是否能写入磁盘。
- 只支持一个受控的订阅格式仍是较小的 V0.1 范围；上游同时维护 JSON、YAML、TXT 会扩大兼容矩阵。

## 3. 订阅服务、认证与错误行为

### `sing-box-yg` 的订阅暴露

脚本将生成的 `sbox.json`、`clmi.yaml`、`jhsub.txt` 等文件通过一个随机端口的 BusyBox `httpd` 暴露，并用 cron/@reboot 维持该进程。源码中没有看到 HTTPS、独立订阅 token、请求超时、速率限制或结构化错误策略；本地 HTTP 服务的保护更多依赖 URL 的不可预测性。[订阅 HTTP 服务](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L3099-L3177)

### `vps-sub-meter` 的订阅暴露

该项目生成 48 位十六进制 token，将 Python `HTTPServer` 绑定到 `127.0.0.1`，按精确 path 映射到 YAML、JSON、TXT 文件，并添加 `subscription-userinfo` 响应头。[token 生成](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L845-L848) [Python 路由与响应](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1314-L1522)

公网入口由 Caddy 处理 HTTPS 和 Basic Auth。但 Caddyfile 另设 `?token=TOKEN` 规则，命中后跳过 Basic Auth，以兼容不支持 Basic Auth 的客户端；token 因而同时出现在 path 和 query。query secret 更容易进入访问日志、shell history、浏览器历史或上游 Referer。[Caddy 路由与认证](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1607-L1649) [输出的访问 URL](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1793-L1885)

Python 服务读取订阅源失败时仍返回 200，并给出占位错误 body；计量读取失败时也记录后继续生成响应。该行为利于客户端“不断订阅”，但会把内部故障伪装成有效订阅，难以让监控发现。[读取失败处理](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1488-L1508)

### 对 sbctl 的影响

1. 继续采用“无效路径/凭据对外统一 404，内部故障按类别脱敏记录并返回适当 5xx”的决策；不能照搬上游的错误 200。
2. 不采用 query token 旁路认证；订阅秘密只放在 path，日志中不记录完整 path/token。
3. 订阅服务需要请求读取上限、超时、并发/连接上限和速率控制。Python `HTTPServer` 可作为功能原型参考，但不应成为生产抗滥用模型。
4. `subscription-userinfo` 必须由订阅服务自己的状态模型产生，而不是把 HTTP 200 当成源文件健康证明。

## 4. 流量统计与账期

`vps-sub-meter` 的计量核心读取 `/sys/class/net/<iface>/statistics/rx_bytes` 与 `tx_bytes`，并以状态文件保存周期基线；安装阶段通过默认路由探测网卡，也允许用户确认/覆盖。[网卡探测](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L767-L804) [基线处理](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1040-L1312)

源码的实时订阅响应主要按网卡发送字节计算使用量，并在 `subscription-userinfo` 中提供 `download`、`total`、`expire`；它是整张网卡口径，不能归因到某个协议、节点或订阅用户。状态缺失、计数器回绕和跨月由独立 reset 脚本/timer 处理。[实时统计与响应头](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1410-L1522)

项目提供自然月、锚定月等更复杂选项，并依赖配置时区和 systemd timer；这使短月、DST、停机错过 timer、主机重启后的计数器回绕成为必须覆盖的边界。[账期选项](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L671-L765)

两个上游没有实现按订阅 token 或按协议的数据面强制限额。`total` 只是客户端显示额度，不能被理解为 sing-box 已经阻断流量。

### 对 sbctl 的影响

- 推荐 V0.1 固定为“指定默认出口网卡 RX+TX、UTC 自然月、每月 1 日 00:00”，并把网卡、基线、cycle key 和 schema version 持久化。
- 明确文档语义：这是 VPS 网卡总流量，不是每个用户的精确用量；不宣称 `subscription-userinfo` 能执行限流。
- 订阅读取应与基线重置解耦；daemon 读状态，独立任务写状态，避免请求路径修改账期状态。
- 必须测试设备重启/计数器回绕、状态缺失或损坏、跨月停机、网卡不存在和磁盘写失败。

## 5. 证书与 HTTPS

`sing-box-yg` 默认会生成长期自签证书，也可以再次执行远程 `acme-yg` 脚本申请域名/IP 证书；协议配置直接引用 `/root/ygkkkca/` 或 `/etc/s-box/` 下的证书和私钥。[证书生成与 ACME 入口](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L241-L310)

`vps-sub-meter` 让 Caddy 处理公网 HTTPS，再通过每日 systemd timer 从 Caddy 私有证书目录复制证书和私钥到 sing-box-yg 的证书目录；证书变化时重启 sing-box。同步脚本会检查证书有效期、域名、公钥/私钥匹配，并用临时文件替换。[证书同步脚本](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L230-L395)

这种组合有两个启示：临时文件替换和证书/私钥匹配检查值得采用；但 Caddy 私有目录、外部脚本和“证书变化即重启另一个服务”会造成组件耦合，续期失败也需要独立告警。上游不能证明 sbctl 自己终结 HTTPS 时的 ACME 生命周期已经闭合。

### 对 sbctl 的影响

- Direct 模式仍需要自己的证书状态、过期诊断和续期验收；不能假设外部 Caddy 一定存在。
- 私钥必须由最小范围的服务账户读取，安装/续期提交必须原子替换，并在提交前验证证书、私钥和域名匹配。
- 续期成功后优先通过 socket-activated服务的安全 reload/重新读取生效；必须测试旧连接、新连接和续期失败三种情况。
- ACME HTTP-01 的 80 端口所有权必须在设计中明确，不能同时让 Caddy、sbctl 和 Certbot 竞争。

## 6. systemd、cron、更新与卸载

### 服务和定时任务

`sing-box-yg` 生成的 systemd unit 以 `User=root` 运行，并授予 `CAP_NET_ADMIN`、`CAP_NET_BIND_SERVICE`、`CAP_NET_RAW`；服务失败自动重启。它还为 Argo、更新、订阅 HTTP 等场景混用 systemd、cron 和 `@reboot`。[sing-box unit](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L891-L910) [cron/Argo 管理](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L2415-L2514)

`vps-sub-meter` 则为订阅副本、流量基线和证书同步分别生成 service/timer，并让订阅后端以 `subsrv` 用户运行、只监听 loopback。[订阅 unit 与 timer](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1018-L1038) [后端 unit](https://github.com/xiaolingxiaoying/vps-sub-meter/blob/6cba513687a7479ab7a3afe4ff9d02b37d985998/auto_setup.sh#L1545-L1588)

结论不是“所有 cron 都有问题”，而是关键生命周期应有单一权威：长驻进程、更新、证书续期和账期重置都应有可查询状态、失败日志和明确 owner。cron 的 `@reboot` 适合作为兼容性补丁，不适合作为生产服务的主要监督机制。

### 更新

上游自更新会从 GitHub `main` 下载新脚本，并在部分路径使用 `--insecure`；内核升级同样从 release 下载后直接替换 `/etc/s-box/sing-box`，没有 manifest 签名和回滚协议。[更新逻辑](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L2525-L2553) [升级与替换](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d35d11b5ff0c314bc/sb.sh#L3760-L3835)

### 卸载

`sing-box-yg` 卸载会删除 `/etc/s-box`、脚本和多个 root 文件，并刷新/清空 NAT `PREROUTING` 链；这可能影响同一主机上的其他应用，属于不可接受的默认卸载范围。[卸载逻辑](https://github.com/yonggekkk/sing-box-yg/blob/9e8b710c191d0cfd43f50f3d11b5ff0c314bc/sb.sh#L3909-L3933)

### 对 sbctl 的影响

1. 继续以 systemd 为服务生命周期权威，timer 仅承担明确的周期任务；每个 unit 都要有 `User=`, `Group=`, 目录权限和 hardening 说明。
2. sing-box 使用独立非 root 账户；由于当前协议监听器使用高端口，不需要继承上游的 root/capability 组合。
3. 更新必须固定版本、验证签名 manifest、验证 artifact digest、原子替换，并在失败时恢复前一版本配置和二进制。
4. 卸载只能删除 sbctl 自己登记的文件、unit、用户和目录；不得清空全局 iptables/NAT，不得删除用户未纳管的证书、Caddy 或其他服务。

## 7. 对当前五个决策的复评

| 决策 | 源码证据带来的判断 | 结论 |
|---|---|---|
| socket activation | `sing-box-yg` 选择 root + capability；`vps-sub-meter` 把公网 443 交给 Caddy。两者都没有提供“非 root sbctl 自己拥有 80/443”的安全实现。 | 保留。由 systemd 持有 80/443，再把 socket 交给非 root sbctl。 |
| manifest 签名 | 两个项目都从 GitHub latest/main 或外部脚本下载内容；发现 checksum/signature 信任链缺失。 | 保留并提高优先级。固定版本 manifest 必须有发布者签名，SHA-256 只是完整性检查。 |
| 非 root | `sing-box-yg` 的 sing-box unit 明确 `User=root` 并拥有多项 capability；`vps-sub-meter` 的 `subsrv` 证明订阅后端可用专用账户运行。 | 保留。sbctl daemon 与 sing-box 数据面分别使用专用非 root 账户。 |
| 错误日志 | 上游要么直接暴露简单 HTTP 文件服务，要么在源文件失败时返回 200 占位内容；query token 也会增加秘密进入日志的风险。 | 保留。外部认证失败统一 404；内部故障用脱敏结构化日志和适当 5xx。 |
| 真实发布验收 | 上游依赖发行版、systemd、端口、ACME、Caddy、cron、网卡 sysfs 和云防火墙；理想化 fake fixture 无法覆盖这些交互。 | 保留并具体化。必须在真实 Debian/Ubuntu systemd 环境验证非 root 启动、socket 交接、TLS、timer、更新回滚和卸载隔离。 |

## 最终建议的实现边界

V0.1 可以吸收上游的三个可取点：默认路由网卡探测、订阅产物原子替换、证书/私钥匹配检查。其余部分应保持 sbctl 的独立边界：独立凭据模型、固定版本签名发布、非 root 服务、systemd socket activation、单一可审计的订阅协议、UTC 账期状态机，以及只删除自身纳管资源的回滚/卸载流程。

WSL2 Ubuntu 22.04 LTS 仍只作为编译、单元测试和模拟 CLI 的开发环境。因为上游实际依赖 systemd、低端口/UDP、ACME、sysfs 网卡计数器、Caddy 和云防火墙，发布结论必须来自真实 Debian/Ubuntu systemd 主机或等价的真实虚拟机验收，而不是 `code .` 成功、Rust 测试通过或 WSL2 中一次性启动成功。
