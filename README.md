# sbctl

`sbctl` 是一个使用 Rust 编写的 sing-box 控制面工具，用于在单台 VPS 上部署和管理 sing-box，并生成私有订阅。

项目的目标是保留 sing-box 作为数据面，将协议配置、订阅生成、证书生命周期、流量统计、服务管理和安全更新集中到一个可验证、可回滚的原生程序中。

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
- 已安装或可安装经过校验的 sing-box

当前版本不会自动支持 Alpine、非 systemd 系统、容器环境或 Windows/macOS 服务器。

## 构建

需要 Rust stable toolchain：

```bash
cargo build --release
```

生成的二进制位于：

```text
target/release/sbctl
```

## 安装

在 Debian/Ubuntu VPS 上，首次安装只需运行一个脚本；脚本会先校验发布 manifest 和两个
二进制，再以中文菜单引导选择订阅模式、域名/IP、网卡和协议：

```bash
wget -O /tmp/sbctl-install.sh https://raw.githubusercontent.com/xiaolingxiaoying/singbox-sub-me/master/scripts/install.sh
bash /tmp/sbctl-install.sh
```

脚本默认从最新 GitHub Release 取得与系统架构匹配的 manifest；可通过
`SBCTL_MANIFEST_URL` 固定到指定版本。保留传递 `sbctl install` 参数的非交互入口，适合
自动化部署；交互式安装不会修改防火墙，也不会接管已有 sing-box、sing-box-yg、Nginx 或 Caddy。

安装后检查服务并获取订阅地址：

```bash
systemctl status sbctl.service sing-box.service
sbctl status
sbctl sub
```

之后可随时使用 `sbctl menu`（或简写 `sbctl m`）重新进入交互式管理菜单。菜单提供状态、
VPS 流量、节点端口、订阅地址、服务重启和保留备份的卸载操作；重启和卸载会要求确认。

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

## 订阅模式

### Direct

sbctl 直接提供 HTTPS 订阅并使用 Certbot/ACME 管理域名证书。该模式需要域名，并占用公网 TCP `80/443` 用于订阅和 ACME 流程。

### External proxy

sbctl 只监听 loopback，由管理员维护的 Nginx、Caddy 或其他反向代理负责公网入口、TLS 和证书。sbctl 不会生成、修改或接管反向代理配置。

### IP fallback

sbctl 在配置的高位 HTTP 端口提供低安全性的 IP 订阅。该模式不使用 IP HTTPS 证书，也不支持 VMess、Hysteria2、TUIC 和 AnyTLS。

## 常用命令

```bash
# 查看部署状态和 VPS 流量
sbctl status
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
