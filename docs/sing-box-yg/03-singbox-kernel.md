# sing-box 内核的下载、安装、运行与更新

sing-box 内核是整个脚本唯一的数据平面。脚本不做任何源码编译，全部来自 **SagerNet/sing-box 的 GitHub Releases 预编译 tarball**。

## 一、下载并安装内核：`inssb()`（`sb.sh:211-240`）

### 1. 版本选择

```bash
green "使用哪个内核版本？"
yellow "1：使用目前最新正式版内核 (回车默认)"
yellow "2：使用之前1.10.7正式版内核 (支持geosite分流、IP优选级切换，无Anytls协议)"
readp "请选择【1-2】：" menu
if [ -z "$menu" ] || [ "$menu" = "1" ] ; then
  sbcore=$(curl -Ls https://github.com/SagerNet/sing-box/releases/latest | grep -oP 'tag/v\K[0-9.]+' | head -n 1)
else
  sbcore='1.10.7'
fi
```

- 通过 `GitHub releases/latest` 页面的重定向标签 `tag/vX.Y.Z` 提取最新正式版。
- 1.10 系列是个**特例**：支持 geosite 分流 + IP 优先级切换，但**没有 anytls**。

### 2. 下载 & 解压

```bash
sbname="sing-box-$sbcore-linux-$cpu"    # cpu ∈ amd64 / arm64 / armv7
curl -L -o /etc/s-box/sing-box.tar.gz -# --retry 2 \
  https://github.com/SagerNet/sing-box/releases/download/v$sbcore/$sbname.tar.gz
tar xzf /etc/s-box/sing-box.tar.gz -C /etc/s-box
mv /etc/s-box/$sbname/sing-box /etc/s-box
rm -rf /etc/s-box/{sing-box.tar.gz,$sbname}
```

要点：
- tarball 内是一个以 `sing-box-<版本>-linux-<cpu>` 命名的目录，核心二进制在此目录下。
- 解压后把 `sing-box` 挪到 `/etc/s-box/`，清理压缩包与目录。
- **无 checksum / 签名校验**（脚本直接 `--retry` 下载，不做 sha256 或 GPG 校验）。

### 3. 安装确认与版本记录

```bash
chown root:root /etc/s-box/sing-box
chmod +x /etc/s-box/sing-box
blue "成功安装 Sing-box 内核版本：$(/etc/s-box/sing-box version | awk '/version/{print $NF}')"
sbnh=$(/etc/s-box/sing-box version 2>/dev/null | awk '/version/{print $NF}' | cut -d '.' -f 1,2)
```

`sbnh` 取的是 `major.minor`（如 `1.10` / `1.11`）。它是全局关键标志，决定：

- 用 `sb10.json` 还是 `sb11.json`；
- 是否启用 anytls（`[[ "$sbnh" != "1.10" ]]`）；
- 配置里 wireguard 是旧 `outbounds[].type=wireguard` 还是新的 `endpoints[]` 结构。

## 二、内核如何被运行：`sbservice()`（`sb.sh:879-913`）

### systemd 版本（Debian/Ubuntu/CentOS）

```ini
[Unit]
After=network.target nss-lookup.target
[Service]
User=root
WorkingDirectory=/root
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW
ExecStart=/etc/s-box/sing-box run -c /etc/s-box/sb.json
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=10
LimitNOFILE=infinity
[Install]
WantedBy=multi-user.target
```

- 以 **root** 运行（需要 CAP_NET_ADMIN 建 TUN 出站 / 发包）。
- `run -c /etc/s-box/sb.json`：sing-box 以服务模式读取配置启动全部 inbound。
- `Restart=on-failure` 让崩溃后自动拉起。
- 之后 `systemctl enable/start/restart sing-box`。

### OpenRC 版本（alpine，`command -v apk` 分支）

写 `/etc/init.d/sing-box`：

```sh
command="/etc/s-box/sing-box"
command_args="run -c /etc/s-box/sb.json"
command_background=true
pidfile="/var/run/sing-box.pid"
```
`rc-update add sing-box default` + `rc-service sing-box start`。

### 其它运行/管理函数

| 函数（行号） | 作用 |
| --- | --- |
| `restartsb`（`sb.sh:3785`） | `systemctl stop/start sing-box`（或 rc-service），用于更换配置后生效 |
| `sbactive`（`sb.sh:3950`） | 校验 `/etc/s-box/sb.json` 存在，否则报错退出 |
| `sblog`（`sb.sh:3940`） | `journalctl -u sing-box.service -o cat -f` 看日志 |
| `stclre`（`sb.sh:3795`） | 关/开/重启服务（菜单 6） |

## 三、升级 / 切换 / 指定内核版本：`upsbcroe()`（`sb.sh:3862-3910`）

对应主菜单 8。先做三件事之一拿 `upcore`，然后走与安装几乎相同的下载路径：

```bash
# 1) 最新正式版
upcore=$(curl -Ls https://github.com/SagerNet/sing-box/releases/latest | grep -oP 'tag/v\K[0-9.]+' | head -n 1)
# 2) 最新测试版（-alpha/rc/beta）
upcore=$(curl -Ls https://github.com/SagerNet/sing-box/releases | grep -oP '/tag/v\K[0-9.]+-[^"]+' | head -n 1)
# 3) 手动指定（用户输入）
readp "请输入Sing-box版本号：" upcore
```

```bash
sbname="sing-box-$upcore-linux-$cpu"
curl -L -o /etc/s-box/sing-box.tar.gz -# --retry 2 https://github.com/SagerNet/sing-box/releases/download/v$upcore/$sbname.tar.gz
tar xzf /etc/s-box/sing-box.tar.gz -C /etc/s-box
mv /etc/s-box/$sbname/sing-box /etc/s-box
rm -rf /etc/s-box/{sing-box.tar.gz,$sbname}
chown root:root /etc/s-box/sing-box && chmod +x /etc/s-box/sing-box
sbnh=$(/etc/s-box/sing-box version ... | awk ...)          # 重新判定 major.minor
[[ "$sbnh" == "1.10" ]] && num=10 || num=11                # 切模板
rm -f /etc/s-box/sb.json && cp /etc/s-box/sb${num}.json /etc/s-box/sb.json
restartsb && sbshare                                        # 生效并重新输出配置/链接
```

升级后三个关键副作用：
1. **重新选择 10/11 模板**，因为不同内核的 config schema 不同（wireguard 结构变化 + anytls 有无）。
2. **重启服务**使新二进制生效。
3. **重新生成分享链接/客户端配置**（`sbshare`），因字段可能变（例如 hy2 多端口、anytls 有无）。

## 四、版本查询：`lapre()`（`sb.sh:3849-3860`）

主菜单顶部与升级菜单都会调用，用于展示当前/最新版本：

```bash
json=$(curl -Ls --max-time 3 https://data.jsdelivr.com/v1/package/gh/SagerNet/sing-box)
if echo "$json"|grep -q '"versions"'; then
  latcore=$(echo "$json"|grep -Eo '"[0-9.]+",'|head -n1|tr -d '",')   # 最新正式版
  precore=$(echo "$json"|grep -Eo '"[0-9.]*-[^"]*"'|head -n1|tr -d '",') # 最新 pre
else
  page=$(curl -Ls https://github.com/SagerNet/sing-box/releases)
  ...
fi
inscore=$(/etc/s-box/sing-box version 2>/dev/null | awk '/version/{print $NF}')
```

优先用 jsDelivr 对 `gh/SagerNet/sing-box` 的包元数据，失败则回退解析 GitHub releases 页面。

## 五、`/etc/s-box/v` 与脚本自身版本

`/etc/s-box/v` 记录的是 **sing-box-yg 脚本自身**的版本（非内核），来自：

```bash
curl -sL https://raw.githubusercontent.com/yonggekkk/sing-box-yg/main/version | awk -F "更新内容" '{print $1}' | head -n 1 > /etc/s-box/v
```

主菜单用它和远端 `main/version` 对比，提示“脚本有更新，可选 7”。

## 六、风险提示（与本项目已记录的结论一致）

- 下载内核 **不做校验**（无 checksum/signature），升级路径与安装路径共用这一弱点。
- 以 root 常驻，服务单元直接 `User=root` 并授予 CAP_NET_ADMIN。
- 升级/切换是“替换二进制 → 拷配置 → restart”，无版本回滚保护；若新内核 schema 不兼容、配置错误，需手动处理。
- 这些正是本项目 `docs/research/upstream-analysis.md` 与 ADR（0005、0009、0012）记录为“应避免”的点；`sbctl` 采用非 root、校验清单、原子提交等相反设计。
