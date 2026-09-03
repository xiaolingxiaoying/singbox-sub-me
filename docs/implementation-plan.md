# sbctl 完善与发布实施计划

## 总结

将当前项目从“功能原型”完善为可发布的 Rust 控制面：

- 保留五种协议和三种订阅格式；
- 吸收 `vps-sub-meter` Shell 脚本中的有效能力；
- 使用指定网卡的 RX+TX 作为 VPS 总流量；
- 支持自然月、锚定月、首个重置时间、时区和已用流量人工修正；
- 完成 socket activation、双非 root 服务、签名 manifest 和真实 systemd 验收；
- 不引入上游的 root 常驻、query token、Caddy 全局接管、iptables 清理或未签名远程更新。

## 关键实现变更

### 1. 账期与流量状态

扩展现有流量和配置模型：

- `natural-month`：每月 1 日 00:00；
- `anchored-month`：按首个重置日期的日、时、分每月重复；
- `accounting_timezone` 使用 IANA 时区，默认 `UTC`，不修改 VPS 系统时区；
- `anchored_reset_at` 使用 `YYYY-MM-DDTHH:MM` 格式；
- 默认 accounting timezone 固定为 `UTC`，只有显式配置才使用其他时区；
- 锚定日允许 1–31 日，短月自动收敛到当月最后一天；
- 首个锚定时间之前不产生有效账期，已用流量显示为 0，下一次重置显示为首个锚定时间。

状态文件需要明确保存：

- schema version；
- cycle key；
- interface；
- baseline RX/TX；
- accumulated RX/TX；
- boot ID；
- 手工修正记录。

流量口径固定为指定网卡的 RX+TX：

- `download = RX`；
- `upload = TX`；
- `total = RX + TX`。

新增管理员修正能力：

```text
sbctl traffic set-used --bytes <TOTAL>
sbctl traffic set-used --rx <BYTES> --tx <BYTES>
```

- `--bytes` 只记录 Total traffic adjustment，不改变 RX/TX 方向值；
- `--rx/--tx` 用于精确修正方向统计；
- 总量修正保存为独立 adjustment，不伪造 RX/TX 方向值；
- 方向修正通过重新计算 baseline 实现，不伪造网卡计数器；
- 允许目标值大于当前计数器，支持 VPS 重启后恢复历史已用量；
- 修改前显示当前账期、当前实际值、目标值和下一次重置时间；
- 修改使用操作锁和原子替换。

账期状态只有以下写入者：

- `sbctl-accounting-reset.service/timer`；
- 管理员显式执行的流量修正命令。

普通 `sbctl traffic`、`sbctl status` 和订阅 HTTP 请求只读。首个锚定时间之前返回合法的 `pending-first-reset` 状态，不视为故障。

不存在的本地时间和 DST 重复的含糊本地时间均拒绝保存，要求管理员重新选择时间。

### 2. 独立账期任务与订阅服务

新增独立的 systemd accounting reset service/timer：

- timer 使用 `Persistent=true`；
- timer 每分钟运行一次，由 cycle key 判断是否需要真正切换账期；
- 周期任务负责建立新账期 baseline；
- `sbctl traffic` 可执行同一套 reconciliation 逻辑；
- 订阅 HTTP 请求只读状态；
- 状态缺失或损坏时返回脱敏 `503`，不返回 HTTP 200 占位订阅；
- 无效路径、query credential 和错误 credential 统一返回 `404`；
- 日志不得记录完整订阅 credential；
- `subscription-userinfo` 的 `upload/download/total/expire` 与当前账期状态一致。

从 `vps-sub-meter` 吸收：

- 默认路由接口探测；
- 用户确认或显式覆盖接口；
- cycle key；
- 首个 anchor 前状态；
- 短月收敛；
- counter rollback 检测；
- `Persistent=true`；
- temp file + atomic rename；
- 已用流量反推 baseline。

明确不吸收：

- 只统计 TX；
- 修改 VPS 系统时区；
- 请求路径写状态；
- 源文件异常时返回 200；
- 使用 `999 TiB` 假装无限额度。

### 3. 五协议与三种订阅格式

保留以下 Managed protocol：

- VLESS Reality；
- VMess WebSocket；
- Hysteria2；
- TUIC v5；
- AnyTLS。

保留全部 Subscription format：

- sing-box JSON；
- Clash/Mihomo YAML；
- URI 文本。

所有格式使用同一组生成节点和独立协议凭据：

- 订阅 credential 与 UUID/password 完全分离；
- credential 只允许出现在 path；
- query 参数永不作为认证旁路；
- 五种协议分别验证服务端配置、客户端输出和字段映射；
- 配置或协议端口变更继续使用锁、`sing-box check`、原子替换和失败回滚。

### 4. Direct HTTPS 与非 root 服务

完成 Direct HTTPS 的 socket activation：

```text
sbctl-http.socket
  ├── TCP 80
  └── TCP 443

sbctl.service
  └── User=sbctl
      接收 systemd 传入的 socket
      处理 ACME challenge 和 HTTPS 订阅

sing-box.service
  └── User=sing-box
      只运行代理数据面
```

要求：

- systemd 持有 80/443；
- 两个 `ListenStream` 由同一个服务通过 `LISTEN_FDS` 接收，并按本地端口区分 HTTP 80 和 TLS 443；
- `sbctl` 不直接 bind 低端口；
- `sbctl` 和 `sing-box` 使用不同的无登录服务账户；
- 证书私钥只授予需要读取的服务账户；
- 证书、私钥、域名/SAN 和有效期在加载前验证；
- 续期后通过安全 reload 或下一次连接重新加载；
- 续期由 Debian/Ubuntu 的 `certbot.timer` 负责，sbctl 提供证书校验和 deploy hook；
- 证书错误只返回脱敏 5xx；
- External proxy 继续只监听 loopback；
- IP fallback 保持高端口 HTTP，并明确标记为低安全模式；
- 服务增加请求大小、读取超时、并发和连接关闭限制。
- HTTP 实现使用 Hyper/Axum，不继续扩展手写 TCP HTTP 解析。

### 5. 签名发布与更新

扩展 release manifest：

- manifest 包含固定版本；
- manifest 包含 sbctl 和 sing-box artifact URL；
- manifest 包含 SHA-256；
- manifest 包含 Ed25519 签名；
- 签名覆盖不含 `signature` 字段的 canonical JSON manifest；
- 签名值使用标准 Base64，manifest 使用版本化 schema；
- 客户端内置第一版发布公钥；
- 先验证 manifest 签名，再信任 URL 和 digest；
- 下载文件使用临时文件；
- digest 校验通过后原子替换；
- 候选 sbctl 执行健康检查；
- 候选 sing-box 执行配置检查；
- 服务启动失败恢复旧二进制、配置和状态；
- 保留 rollback point；
- 不支持 latest/main 无签名自动升级。

manifest 还必须声明经过测试的 sing-box 版本兼容矩阵；安装或更新前使用候选二进制执行配置检查，不满足矩阵时拒绝操作。

证书加载前必须验证有效期、SAN、证书与私钥匹配，以及连接 SNI 与订阅域名一致。证书续期由系统 Certbot timer 负责，sbctl 通过 deploy hook 重新验证并安全 reload。

安装事务按阶段执行：下载验证、创建账户和目录、写入配置/工件、写入 unit/socket/timer、daemon-reload、启动和健康检查。所有阶段成功后才写入 ownership marker；失败时只回滚本次创建的资源，不触碰既有服务或文件。

安装脚本必须与 Rust 更新逻辑使用同一套签名验证规则，不能仅由 `jq + sha256sum` 决定信任。

### 6. 文档与领域模型

继续维护：

- [CONTEXT.md](../CONTEXT.md)；
- [0015-selectable-accounting-periods.md](adr/0015-selectable-accounting-periods.md)。

文档必须明确：

- `subscription-userinfo` 只描述 VPS 网卡总流量；
- monthly traffic limit 第一版只展示，不执行断流；
- “账期重置”和“订阅格式刷新”是两个独立概念；
- 五协议与三格式属于当前产品范围；
- Direct HTTPS 由 systemd socket activation 实现；
- root 只用于管理员生命周期操作，不作为长驻数据面权限；
- 上游兼容是行为兼容，不是接管上游文件布局或服务。

同步修正 `singbox-sub-plan.md` 中已经落后的内容，尤其是“第一版只支持 sing-box JSON”“由 Caddy 负责 Direct HTTPS”和“继续依赖上游文件布局”等描述。
该文件标记为历史方案；正式实现以本计划、`CONTEXT.md` 和 `docs/adr/` 为准。

### 7. 交互式安装与配置选择

安装向导采用 `vps-sub-meter` 的“加载已有配置、逐项修改、逐项校验、摘要确认”模式，但所有实际写入仍由 sbctl 事务完成：

1. 启动时读取已有 sbctl 配置；空输入保留当前值，新部署使用默认值。
2. 依次选择并校验：
   - 订阅模式；
   - 订阅主机和代理主机；
   - Direct 模式的域名与证书邮箱；
   - IP fallback 的高位 HTTP 端口；
   - 五种协议是否启用及各自监听端口；
   - 每月流量上限；
   - 当前已使用流量或 Traffic correction；
   - Accounting timezone；
   - Natural-month reset 或 Anchored-month reset；
   - Anchored-month 的 First reset instant；
   - 默认路由网卡或手动指定网卡；
   - External proxy 的 loopback 监听端口。
3. 每一项都在提交前校验格式、范围、端口占用、网卡存在性、时区有效性和账期时间合法性。
4. 展示完整配置摘要，包括流量口径、账期规则、首个重置时间、协议列表、端口和安全警告。
5. 用户明确确认后，才进入安装事务；取消或校验失败不改变现有部署。
6. 非交互安装必须显式提供必要参数，不能隐式从终端或系统状态猜测安全敏感配置。

向导可以提供已保存密码/配置的“保留当前值”选项，但不得打印完整 credential、私钥或密码。默认时区为 UTC，不执行上游脚本中的 `timedatectl set-timezone`。

### 8. 运行维护、轮换和模式切换

- 修改时区、账期规则或首个重置时间时，要求管理员确认并创建新的 accounting state；旧账期进入历史记录。
- `sbctl credential rotate` 立即使旧订阅 URL 失效并生成新 URL，不保留兼容窗口。
- 增加 `sbctl status --json` 和 `sbctl diagnostics`，通过 systemd/journald 展示服务、socket、证书、账期、工件和最近失败原因。
- sbctl 自己生成的工件是唯一权威来源，不周期性复制上游 `/etc/s-box/*` 文件。
- 配置、订阅工件、证书引用、unit/socket/timer、二进制和账期状态统一采用候选版本校验、原子提交、健康检查和失败恢复。
- 第一版拒绝自动导入或迁移上游项目的配置、credential、文件和服务。
- Direct、External proxy、IP fallback 之间切换前先检查目标端口、证书和反向代理条件；切换失败时保持原模式运行。
- 回滚必须恢复上一版本的 sbctl、sing-box、配置、订阅工件、unit/socket/timer、证书引用和账期状态。

### 9. 上游能力边界

纳入核心实现：

- 交互式配置向导；
- 默认网卡探测；
- 流量上限和已用流量设置；
- 时区、自然月和锚定月；
- 首个重置日期和时间；
- 短月处理和 counter rollback；
- sing-box JSON、Clash/Mihomo YAML、URI 三种订阅格式；
- 原子工件更新；
- 证书/私钥匹配检查；
- systemd service、socket 和 timer 生命周期。

保留为后续扩展：

- 固定到期模式；
- Basic Auth；
- 上游订阅 URL 兼容路径；
- WARP/IPv4/IPv6 出站切换；
- 云厂商专用安装入口；
- Caddy 全局管理；
- Argo/Cloudflare 隧道；
- 自动防火墙修改。

明确排除：

- query token；
- root 常驻服务；
- 全局 iptables/NAT 清理；
- 远程未签名更新；
- 自动接管已有部署。

上游兼容只要求协议配置、客户端字段和订阅内容行为兼容，不要求兼容上游文件路径、URL 形状、认证方式或云厂商分支脚本。

## 测试与发布验收

### 单元和 CLI 测试

覆盖以下场景：

- 自然月边界；
- 锚定月首个日期；
- 首个锚定时间之前；
- 1–31 日锚定；
- 28/29/30/31 日短月收敛；
- DST 和非法/不存在的本地时间；
- 时区不修改系统时区；
- 主机重启；
- RX/TX counter rollback；
- 状态缺失、损坏和 schema 不匹配；
- 手工设置总已用流量；
- 手工设置 RX/TX；
- `download=RX`、`upload=TX`；
- 五协议三格式生成；
- query credential 返回 404；
- 错误状态返回 503 且不泄露 credential；
- manifest 签名成功、签名失败、digest 失败；
- canonical manifest 字段、编码、Base64 签名和 schema 版本；
- DST 重复/不存在本地时间被拒绝；
- 更新失败恢复旧文件；
- 并发读取只看到完整 artifact。

### systemd/真实主机验收

在真实 Debian/Ubuntu systemd 主机或等价 VM 中验证：

- `sbctl` 服务以非 root 运行；
- `sing-box` 服务以独立非 root 用户运行；
- socket unit 持有 80/443；
- ACME HTTP-01 challenge；
- HTTPS 订阅；
- certificate/private-key/domain 匹配；
- 续期后新连接使用新证书；
- UDP 协议真实监听；
- 默认出口网卡 RX/TX；
- timer 重启后补执行；
- timer 每分钟运行且同一 cycle key 不重复写状态；
- 跨月停机恢复；
- 手工修正已用流量；
- 签名 manifest 安装和更新；
- 更新失败回滚；
- 默认卸载备份；
- `--purge` 只删除 sbctl 自有资源；
- 不修改现有 Nginx、Caddy、iptables、NAT 和手工 sing-box 部署。

## 已确定的约束

- 五协议和三种订阅格式属于当前版本范围；
- `vps-sub-meter` 作为行为参考，但 sbctl 统一采用 RX+TX；
- 账期时区默认 UTC，可由管理员选择；
- 锚定月允许 1–31 日，短月按月末处理；
- 首个锚定日期允许过去或未来；
- 月流量额度第一版只展示，不强制阻断；
- `sbctl traffic set-used` 是修正当前账期已用量的正式接口；
- Direct HTTPS 使用 socket activation，不给整个 daemon root 或通用低端口 capability；
- 签名算法固定为 Ed25519，第一版使用内置单一发布公钥；
- 不自动接管或迁移已有上游部署。
- 不加入固定到期、Basic Auth、上游 URL 兼容、云厂商专用入口或 WARP/IPv6 出站切换。
