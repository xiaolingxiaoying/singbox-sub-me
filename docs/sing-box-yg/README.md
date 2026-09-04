# sing-box-yg 脚本分析文档集合

本文档集合完整分析一键脚本

```bash
bash <(wget -qO- https://raw.githubusercontent.com/yonggekkk/sing-box-yg/main/sb.sh)
```

回答三个核心问题：

1. 这个命令是如何实现的？
2. 它如何下载/更新 sing-box 内核？
3. 它如何通过 sing-box 内核生成 5 个协议节点，每个节点的具体实现过程是什么？

以及列出脚本涉及的所有仓库。

## 本目录导航

| 文档 | 内容 |
| --- | --- |
| [01-repositories.md](./01-repositories.md) | 脚本涉及的所有仓库清单与各自角色（含本地拉取路径） |
| [02-command-and-install-flow.md](./02-command-and-install-flow.md) | `bash <(wget -qO- URL)` 的原理 + 一次完整安装的逐步流程 |
| [03-singbox-kernel.md](./03-singbox-kernel.md) | sing-box 内核的下载、安装、运行服务、更新/升级/切换机制 |
| [04-five-protocol-nodes.md](./04-five-protocol-nodes.md) | 5 协议节点（vless-reality / vmess-ws+Argo / hysteria2 / tuic / anytls）的生成与每个节点的具体实现 |
| [05-warp-argo-outbounds.md](./05-warp-argo-outbounds.md) | Warp-Wireguard 出站、Argo 隧道、Socks5 代理与域名分流 |
| [06-client-configs-subscription.md](./06-client-configs-subscription.md) | 客户端配置（SFA/SFI/SFW、Mihomo）、本地 HTTP 订阅、Gitlab 订阅 |

## 权威代码来源

- 主脚本：`.reference-sing-box-yg/sb.sh`（已拉取，唯一安装入口）
- sing-box 源码：`.reference-sing-box/`（Go 源码，仅被脚本下载其预编译二进制，本文只用于说明协议字段含义）

> 注意：本集合是**对上游脚本的事实性技术说明**，目的是理解其行为，而不是对其安全性的背书。上游脚本存在若干已被本项目记录在 `docs/research/upstream-analysis.md` 中的风险（root 常驻、远程未校验、UUID 兼任订阅令牌、就地 sed 改配置、无锁/无回滚等），本文档不重复这些安全批评。

## 一句话结论

`bash <(wget -qO- URL)` 用 `wget` 把远端 `sb.sh` 拉下来，经 stdout 管道直接交给当前 shell 执行（进程替换，脚本体代替子进程），随后该脚本在 VPS 上：探测发行版/架构 → 安装依赖 → 从 `SagerNet/sing-box` 的 GitHub Releases 下载对应 `linux-$cpu` 的预编译 tarball 解压为 `/etc/s-box/sing-box` → 自动生成一个 UUID 与一对 Reality 密钥 → 用 heredoc 生成包含 5 个 inbound（vless-reality / vmess-ws / hysteria2 / tuic / anytls）的 `sb.json`（1.10 系列用 `sb10.json`，其余用 `sb11.json`）→ 注册为 systemd/OpenRC 服务 → 按协议拼出 vless/tuic/hysteria2/anytls URI 与 base64 的 vmess URI → 打印二维码 → 另外生成 SFA/SFI/SFW 与 Mihomo 客户端配置。
