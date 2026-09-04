# sbctl

[![CI](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/workflows/ci.yml/badge.svg)](https://github.com/xiaolingxiaoying/singbox-sub-me/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/xiaolingxiaoying/singbox-sub-me/blob/master/Cargo.toml)

`sbctl` 是一个使用 Rust 编写的 sing-box 控制面工具，用于在单台 VPS 上部署和管理 sing-box，并生成私有订阅。

项目的目标是保留 sing-box 作为数据面，将协议配置、订阅生成、证书生命周期、流量统计、服务管理和安全更新集中到一个可验证、可回滚的原生程序中。

当前版本已具备从签名发布工件安装、配置五种协议、提供订阅、管理 systemd 服务以及安全更新/卸载的完整闭环。项目面向有 Linux VPS 和 systemd 运维能力的用户；它不会替用户修改防火墙、接管现有代理或管理反向代理。

## 功能概览

- 支持五种 Managed protocol：
  - VLESS Reality Vision
  - VMess WebSocket
  - Hysteria 2
  - TUIC v5
  - AnyTLS
- 每个启用协议使用独立的 Proxy credential 和 Protocol listener port。
- 端口支持手动指定，也支持自动分配。
- 自动端口范围为 `10000–65535`，并统一检查 TCP/UDP 冲突和系统占用。
- 生成三种订阅格式：
  - sing-box JSON
  - Clash/Mihomo YAML
  - URI 文本
- 订阅凭据与协议凭据分离，只接受路径凭据，不接受 query 参数认证。
- 支持 Direct、External proxy、IP fallback 三种订阅模式。
- 支持 VPS 流量统计、自然月/锚定月账期和 `subscription-userinfo`。
- 支持 sing-box 配置检查、原子提交、服务健康检查、更新回滚和可恢复卸载。
- 安装前检测 Existing deployment，不自动接管已有的 sing-box 或 sing-box-yg 部署。

## 五种协议

| 协议 | 传输 | 端口类型 | 主要参数 |
| --- | --- | --- | --- |
| VLESS Reality | TCP | TCP | UUID、Reality key、short ID、SNI、fingerprint |
| VMess WebSocket | WebSocket + TLS | TCP | UUID、WebSocket path、Host、TLS SNI |
| Hysteria 2 | QUIC | UDP | password、TLS SNI |
| TUIC v5 | QUIC | UDP | UUID、password、TLS SNI、`h3` |
| AnyTLS | TCP + TLS | TCP | password、TLS SNI |

高级参数暂时使用 sing-box 的安全默认值，不开放 Argo、Cloudflare 隧道、Psiphon/WARP 分流或复杂的协议调优选项。

## 系统要求

当前 V0.1 目标平台：

- Debian 12 或 Ubuntu 22.04+
- systemd
- amd64；发布流程同时准备 arm64 工件
- 已安装或可从签名 manifest 下载经过校验的 sing-box

当前版本不会自动支持 Alpine、非 systemd 系统、容器环境或 Windows/macOS 服务器。Docker/WSL2 仅用于开发和验收，不是生产部署目标。

## 构建

需要 Rust stable toolchain：

```bash
cargo build --release
```

生成的二进制位于：

```text
target/release/sbctl
```

## 一键安装

在 Debian/Ubuntu VPS 上，首次安装只需一行命令；脚本会先校验发布 manifest 和两个
二进制，再以中文菜单引导选择订阅模式、域名/IP、网卡和协议：

```bash
bash <(wget -qO- https://raw.githubusercontent.com/xiaolingxiaoying/singbox-sub-me/main/scripts/install.sh)
```

脚本默认从最新 GitHub Release 取得与系统架构匹配的 manifest；可通过
`SBCTL_MANIFEST_URL` 固定到指定版本。保留传递 `sbctl install` 参数的非交互入口，适合
自动化部署；交互式安装不会修改防火墙，也不会接管已有 sing-box、sing-box-yg、Nginx 或 Caddy。

安装完成后会安装快捷方式命令 `ly`。直接运行 `ly`（等价 `sbctl menu`，简写 `sbctl m`）
进入 sing-box-yg 风格的全屏彩色菜单，以任务分组选择驱动：

```
 1. 安装与部署
 2. 节点与协议
 3. 订阅中心
 4. 流量与账期
 5. 服务与诊断
 6. 更新与卸载
 0. 退出
```

已安装部署进入主题配置后，回车会保持当前值；流量与账期主题覆盖每月流量上限、本周期流量修正、VPS 刷新时区、客户端显示时区、刷新规则、出口网卡以及对应的订阅服务端口。

命令行方式检查服务并获取订阅地址：

```bash
systemctl status sbctl.service sing-box.service
sbctl status
sbctl sub          # 订阅 URL
sbctl qr           # 订阅 URL 二维码
```

升级 sbctl 与 sing-box（自动拉取并校验最新签名 manifest）：

```bash
sbctl update --check   # 仅显示可用版本
sbctl update           # 实际升级（含回滚点）
sbctl sing-box update  # 仅升级 sing-box 内核
```

卸载（保留备份与配置，`--purge` 连数据一起清除）：

```bash
sbctl uninstall
```

选择 `external-proxy` 时，sbctl 默认监听 `127.0.0.1:2080`，请在 Nginx/Caddy 中将 `/sub/` 反代到该地址。
五个协议端口用 `sbctl node` 查看，并在 VPS 安全组/防火墙中放行；sbctl 不会自动修改防火墙。

## 配置初始化

如果需要先生成配置而不立即安装服务，可以使用 `config init`：

```bash
sbctl config init \
  --mode direct \
  --subscription-host sub.example.com \
  --interface eth0 \
  --protocol vless-reality \
  --protocol vmess-websocket \
  --protocol hysteria2 \
  --protocol tuic \
  --protocol anytls \
  --reality-decoy-sni www.cloudflare.com \
  --vless-port 12001 \
  --vmess-port 12002 \
  --hysteria2-port 12003 \
  --tuic-port 12004 \
  --anytls-port 12005 \
  --sing-box-bin /usr/local/bin/sing-box
```

IP fallback 示例：

```bash
sbctl config init \
  --mode ip-fallback \
  --subscription-host 203.0.113.7 \
  --http-port 2080 \
  --interface eth0 \
  --protocol vless-reality \
  --reality-decoy-sni www.cloudflare.com \
  --vless-port 12001
```

IP fallback 使用明文 HTTP，仅推荐在没有可用域名时使用，并且当前只允许 VLESS Reality。

安装完成后，可使用配置向导修改已有部署。向导会先展示脱敏摘要，确认后才执行原子配置更新；直接回车会保留当前值：

```bash
sbctl config wizard
sbctl config show
sbctl config validate
sbctl config switch-mode --mode external-proxy --listen-port 2080
```

向导还可设置协议监听证书模式（`domain` 或 `self-signed`）、账期策略、IANA 时区、首次锚定重置时间和每月流量上限。`self-signed` 证书只用于协议监听；Direct 订阅入口仍使用 Certbot/ACME 证书。

## 订阅模式

### Direct

sbctl 直接提供 HTTPS 订阅并使用 Certbot/ACME 管理域名证书。该模式需要域名；公网 TCP `80/443` 由 systemd 的 `sbctl-http.socket` 持有，并通过 `LISTEN_FDS` 交给非 root 的 `sbctl` 服务进程按本地端口区分 HTTP-01 与 TLS 订阅。`sbctl` 与 `sing-box` 分别使用独立的无登录服务账户。

证书在加载前校验有效期、SAN、私钥匹配与 SNI；安装时写入 Certbot 的 renewal deploy hook（`sbctl certificate verify`），续期后重新校验并把证书固定到 `sbctl`/`sing-box` 两个服务账户可读的私有副本，下一次 TLS 连接自动使用新证书。续期由 Debian/Ubuntu 的 `certbot.timer`（或手动 `sbctl certificate renew`）触发，首次用 `sbctl certificate obtain --email <EMAIL>` 签发。

Direct 模式需要域名解析到 VPS，且公网 TCP `80/443` 可供 ACME 和订阅入口使用。首次部署后，先完成 Certbot 签发并验证证书，再确认 `sbctl.service` 与 `sbctl-http.socket` 均正常运行。

### External proxy

sbctl 只监听 loopback，由管理员维护的 Nginx、Caddy 或其他反向代理负责公网入口、TLS 和证书。sbctl 不会生成、修改或接管反向代理配置。

### IP fallback

sbctl 在配置的高位 HTTP 端口提供低安全性的 IP 订阅。该模式不使用 IP HTTPS 证书，也不支持 VMess、Hysteria2、TUIC 和 AnyTLS。

## 常用命令

```bash
# 查看部署状态和 VPS 流量
sbctl status
sbctl status --json
sbctl traffic

# 查看协议监听端口，不显示凭据
sbctl node

# 查看、校验配置
sbctl config show
sbctl config validate

# 输出订阅地址
sbctl sub
sbctl sub --format sing-box
sbctl sub --format clash
sbctl sub --format uri

# 轮换订阅凭据（旧订阅 URL 立即失效）
sbctl credential rotate

# Direct 模式证书
sbctl certificate obtain --email admin@example.com
sbctl certificate renew
sbctl certificate verify

# 校验配置并重启服务
sbctl restart --sing-box-bin /usr/local/bin/sing-box

# 管理 sing-box 工件
sbctl sing-box download --manifest /path/to/manifest.json --output /tmp/sing-box
sbctl sing-box install --manifest /path/to/manifest.json --artifact /tmp/sing-box
sbctl sing-box update --manifest /path/to/manifest.json
sbctl sing-box remove

# 检查并执行经过校验的更新
sbctl update --check --manifest /path/to/manifest.json
sbctl update --manifest /path/to/manifest.json

# 卸载；默认保留备份，--purge 才清理 sbctl 持久化数据
sbctl uninstall
sbctl uninstall --purge
```

订阅地址格式为：

```text
/sub/<subscription-credential>/sing-box.json
/sub/<subscription-credential>/clash.yaml
/sub/<subscription-credential>/uri
```

`subscription-credential` 与任何协议的 UUID、password 都不同。订阅响应会包含动态生成的 `subscription-userinfo`，其中的流量统计是整张配置网卡的 VPS traffic，不代表单个协议或用户的流量。

新部署默认使用 America/Los_Angeles 作为 VPS 刷新时区、Asia/Shanghai 作为客户端参考显示时区；需要自定义周期时，可在配置向导中选择 `anchored-month`，并设置 IANA 时区与首次重置时间。菜单中的流量输入按 GiB 处理（兼容 `GB` 后缀，按 1024³ bytes 换算），内部保存精确 byte 数。流量上限目前用于展示和订阅元数据，不会主动阻断 sing-box 数据面。

## 安全边界

- 不自动接管已有 sing-box、sing-box-yg、Caddy、Nginx 或防火墙配置。
- 不使用远程未校验的 `curl | bash` 作为核心运行逻辑。
- sing-box 工件必须经过固定 manifest 的 SHA-256 校验。
- 订阅凭据只放在 URL path 中，拒绝 query 参数凭据。
- 每个协议使用独立凭据，避免共享 UUID 导致权限范围扩大。
- 配置、状态和订阅工件采用原子替换，并在失败时保留已知良好版本。
- 默认卸载保留 root 可读备份；`--purge` 只删除 sbctl 明确拥有的数据。

## 测试

运行 Rust 测试和 Clippy：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Debian/Ubuntu 黑盒验收脚本位于 [`tests/acceptance/run.sh`](tests/acceptance/run.sh)，需要
Docker daemon 和 Linux 发布二进制。脚本会分别启动 Debian 12、Ubuntu 22.04、Ubuntu 24.04 的
systemd 容器，因此 Docker 运行环境必须允许 `--privileged` 和 cgroup 挂载：

```bash
SBCTL_ARTIFACT=/path/to/sbctl-linux-amd64 tests/acceptance/run.sh
```

验收 fixture 的边界和可复用 helper 见 [`tests/acceptance/README.md`](tests/acceptance/README.md)。
WSL2 只属于 Development host，可用于编译、Rust 单测和隔离 root 的 CLI 检查；真实
systemd Debian/Ubuntu VM 或等价环境才属于 Production host 验收依据。

验收使用本地注入的 `sbctl` 发布二进制和容器内的 fake sing-box，不依赖 GitHub
Release 或公网域名；容器仅用于验收，不代表 sbctl 支持容器作为生产部署环境。

也可以使用仓库提供的 Compose 配置启动单个 systemd 验收容器（默认 Debian 12）：

```bash
SBCTL_ARTIFACT=./target/release/sbctl docker compose -f docker-compose.acceptance.yml up -d --build
docker exec sbctl-acceptance systemctl is-system-running
docker exec sbctl-acceptance /usr/local/lib/sbctl-acceptance/verify-bootstrap.sh
docker compose -f docker-compose.acceptance.yml down
```

可通过 `BASE_IMAGE=ubuntu:22.04` 或 `BASE_IMAGE=ubuntu:24.04` 切换验收发行版。
该配置需要 Docker Desktop/Engine 开启 Linux 容器、特权容器和 cgroup 挂载权限；Windows
路径建议使用 WSL 路径执行。生产部署仍应使用 Debian/Ubuntu VPS 上的 systemd。

## 发布与更新

推送 `v*` 标签会触发 GitHub Actions 发布流程，为 `amd64` 和 `arm64` 构建 sbctl，运行 Debian/Ubuntu systemd 验收，并上传 sing-box 工件和按架构区分的签名 manifest。安装器和运行时都会先验证 Ed25519 manifest 签名，再验证每个二进制的 SHA-256；不会信任 manifest 中的未固定 URL 或摘要。

发布后的主机更新会保留回滚点，并在替换前执行候选 sing-box 的配置检查和服务健康检查：

```bash
sbctl update --check
sbctl update
sbctl sing-box update
```

完整的安装、systemd 单元和真实主机验收说明见 [`docs/installation.md`](docs/installation.md) 与 [`docs/release-readiness-and-vps-test-plan.md`](docs/release-readiness-and-vps-test-plan.md)。

## 参考项目

- [sing-box-yg](https://github.com/yonggekkk/sing-box-yg)：五协议配置、端口和节点输出的行为参考
- [vps-sub-meter](https://github.com/xiaolingxiaoying/vps-sub-meter)：sing-box JSON、Clash Meta YAML、订阅服务和流量统计的参考
- [sing-box](https://github.com/SagerNet/sing-box)：实际代理数据面

本项目借鉴参考项目的协议和客户端兼容逻辑，但使用独立的 Rust 控制面、安全凭据模型和可回滚生命周期，不自动导入或接管参考项目的现有部署。

## 当前限制

- 暂不支持 Argo 临时/固定隧道。
- 暂不支持 Cloudflare/CDN 优选 IP 自动化。
- 暂不支持 Psiphon、WARP 分流。
- 暂不提供旧 sing-box-yg 配置自动迁移。
- 暂不提供 Web 管理面板、数据库或多管理员模型。

## 许可证

当前 Cargo 包声明为 `MIT OR Apache-2.0`，详见 [`Cargo.toml`](Cargo.toml)。
