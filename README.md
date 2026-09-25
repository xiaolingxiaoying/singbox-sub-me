# sbctl

`sbctl` 是 sing-box 服务端管理工具，用于在 Debian/Ubuntu VPS 上安装和管理 sing-box、启用代理协议并生成私有订阅。

## 功能

- 管理 VLESS Reality、VMess WebSocket、Hysteria 2、TUIC 和 AnyTLS。
- 提供 sing-box、Clash/Mihomo、URI 等客户端订阅。
- 管理 systemd 服务、TLS 证书、流量统计和签名更新，并在更新失败时回滚。
- 安装和运行时需要 sing-box；服务端发布包包含签名校验所需的程序、运行时和安装文件。未完成的 TUI/GUI 客户端不包含在服务端 Release 中。

## 系统要求

- Debian 12 或 Ubuntu 22.04 及以上版本
- amd64 或 arm64 VPS，使用 systemd
- root 权限
- 推荐准备解析到 VPS 的域名。Direct 模式需要公网 TCP 80/443；协议端口须按 `sbctl node` 输出自行在云防火墙和系统防火墙中放行。

## 安装

在 VPS 上下载经过签名校验的安装脚本并运行：

```bash
curl -fL --retry 3 -o /tmp/sbctl-install.sh \
  https://github.com/xiaolingxiaoying/singbox-sub-me/releases/latest/download/install.sh
test -s /tmp/sbctl-install.sh && sudo bash /tmp/sbctl-install.sh
```

如果已登录为 `root`，可将最后一行改成：

```bash
test -s /tmp/sbctl-install.sh && bash /tmp/sbctl-install.sh
```

安装向导会询问订阅模式、域名或 IP、出口网卡和启用的协议。Direct 模式适用于域名已解析到 VPS 的情况；已有 Nginx/Caddy 时选择 External proxy；没有域名时可选择安全性较低的 IP fallback。安装程序不会自动修改防火墙，也不会接管现有 sing-box 或反向代理。

更多安装细节见 [`docs/installation.md`](docs/installation.md)。

## 常用命令

```bash
sbctl menu             # 交互式管理菜单（也可运行 ly）
sbctl status           # 服务状态
sbctl node             # 节点和协议端口
sbctl sub              # 订阅地址
sbctl update           # 更新 sbctl
sbctl sing-box update  # 更新 sing-box 内核
sbctl uninstall        # 卸载并保留备份
```

## 从源码构建

```bash
cargo build --release --locked -p sbctl --no-default-features
```

生成的程序为 `target/release/sbctl`。生产 Release 使用 GitHub Actions 构建和签名；不要直接运行仓库里的 `scripts/install.sh`，它没有生产公钥。开发和验收说明见 [`docs/release-signing.md`](docs/release-signing.md) 与 [`tests/acceptance/README.md`](tests/acceptance/README.md)。

## 许可证

MIT OR Apache-2.0，详见 [`Cargo.toml`](Cargo.toml)。
