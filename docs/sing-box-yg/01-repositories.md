# 脚本涉及的仓库清单

以下仓库是从 `sb.sh`（及其引用的脚本、README）中解析出的全部外部依赖。本地已全部拉取到仓库内 `.reference-*` 目录。

## 本体项目

| 仓库 | 角色 | 拉取位置 |
| --- | --- | --- |
| [yonggekkk/sing-box-yg](https://github.com/yonggekkk/sing-box-yg) | **唯一入口**：含 `sb.sh`（VPS 一键安装）、`serv00.sh` / `serv00keep.sh`（Serv00/Hostuno 平台版）、`kp.sh`、`sb.txt`、`version`/`sversion`（版本号）、`sbwpph_amd64`/`sbwpph_arm64`（编译后的 WARP-plus-Socks5/Psiphon 二进制） | `.reference-sing-box-yg/` |
| [yonggekkk/argosbx](https://github.com/yonggekkk/argosbx) | 仅在 README 与主菜单 "9" 提示语里被推荐（"ArgoSBX 一键无交互小钢炮脚本"）。`sb.sh` 本身**不会调用**它 | `.reference-argosbx/` |

## 上游二进制/数据来源（脚本用 `curl`/`wget` 拉取其发布产物，不 clone 源码）

| 仓库 | 被拉取的产物 | 用途 | 拉取位置（源码参考） |
| --- | --- | --- | --- |
| [SagerNet/sing-box](https://github.com/SagerNet/sing-box) | `sing-box-$ver-linux-$cpu.tar.gz`（来自 GitHub Releases `latest` / `vX.Y.Z`） | **代理内核本体**，解压为 `/etc/s-box/sing-box` | `.reference-sing-box/` |
| [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat) | `geoip.db`、`geosite.db`（来自 `releases/download/latest/`） | 分流用的地理规则数据库（客户端配置远程 rule_set 用 `cdn.jsdelivr.net/gh/.../cn.srs`） | `.reference-meta-rules-dat/` |
| [cloudflare/cloudflared](https://github.com/cloudflare/cloudflared) | `cloudflared-linux-$cpu`（来自 `releases/latest/download/`） | **Argo/Cloudflare 隧道**客户端，为 vmess-ws 提供免开放端口/免 IP 的入口 | `.reference-cloudflared/` |

## 其它被 `curl | bash` 执行的辅助脚本仓库

| 仓库 | 被执行的脚本 | 用途 |
| --- | --- | --- |
| [yonggekkk/acme-yg](https://github.com/yonggekkk/acme-yg) | `acme.sh` | 一键申请 ACME 域名/IP 证书（80 端口 http 或 DNS API 校验），用于给 vmess-ws/hy2/tuic/anytls 用域名证书 |
| [yonggekkk/warp-yg](https://github.com/yonggekkk/warp-yg) | `CFwarp.sh` | 管理 WARP（Netflix/ChatGPT 解锁、注册 warp 账户/优选 IP） |
| [teddysun/across](https://github.com/teddysun/across) | `bbr.sh`（路径 `master/bbr.sh`） | 一键开启 BBR+FQ 内核加速 |

### 关于 `acme-yg` / `warp-yg` 的引用点（`sb.sh` 内）

- 证书：`sb.sh:241-310` 的 `inscertificate()`，在用户选择申请 Acme 时执行
  `bash <(curl -Ls https://raw.githubusercontent.com/yonggekkk/acme-yg/main/acme.sh)`
  （另在功能函数 `acme()`，`sb.sh:4023-4026`）。
- WARP：功能函数 `cfwarp()`，`sb.sh:4027-4030` 执行
  `bash <(curl -Ls https://raw.githubusercontent.com/yonggekkk/warp-yg/main/CFwarp.sh)`。

## 内部预编译二进制（打包在本仓库内，不再外拉）

| 文件 | 说明 |
| --- | --- |
| `sbwpph_amd64` / `sbwpph_arm64` | `inssbwpph()`（`sb.sh:4156` 起）直接从本仓库 `main` 拉取的同名文件，用于 WARP-plus-Socks5 / 多地区 Psiphon 代理模式 |

## 仓库之间的调用关系

```
yonggekkk/sing-box-yg (sb.sh)
 ├─ 下载: SagerNet/sing-box      -> /etc/s-box/sing-box   (内核)
 ├─ 下载: cloudflare/cloudflared -> /etc/s-box/cloudflared (Argo 隧道)
 ├─ 下载: MetaCubeX/meta-rules-dat -> /root/geoip.db /root/geosite.db (分流库)
 ├─ 下载: (自身) sbwpph_{amd64,arm64} -> /etc/s-box/sbwpph (Socks5/Psiphon)
 ├─ 执行: yonggekkk/acme-yg/acme.sh   (可选，域名证书)
 ├─ 执行: yonggekkk/warp-yg/CFwarp.sh (可选，WARP 管理)
 ├─ 执行: teddysun/across/master/bbr.sh (可选，BBR 加速)
 └─ 提及(不调用): yonggekkk/argosbx
```

## 总结：为什么有“这么多仓库”

脚本是典型的“一键脚本分发器 + 内核下载器 + 配置生成器”：

- **分发**靠 `yonggekkk/sing-box-yg` 自己（同时托管预编译的 `sbwpph` 二进制）；
- **内核 + Argo 隧道**是 sing-box / cloudflared 的 GitHub Releases 预编译包；
- **分流数据**是 MetaCubeX 的 .db/.srs 规则；
- **证书与 WARP、BBR** 交由辅助脚本（acme-yg / warp-yg / across）完成。
