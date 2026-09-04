# 命令实现原理与一次完整安装流程

## 一、`bash <(wget -qO- URL)` 是如何实现的

这是一条经典的 **“流式执行远程脚本”** 命令，没有下载到磁盘，也没有拼接 `|` 管道给子进程，而是用**进程替换（process substitution）**。

### 逐段拆解

```bash
bash <(wget -qO- https://raw.githubusercontent.com/yonggekkk/sing-box-yg/main/sb.sh)
```

| 片段 | 含义 |
| --- | --- |
| `wget -qO- <url>` | `-q` 静默；`-O-` 把下载内容直接写到 **stdout**，而不是存成文件 |
| `<( ... )` | **进程替换**：Bash 把括号里命令的 stdout 接成一个类似文件的对象（通常是 `/dev/fd/N` 管道），并把该路径作为参数传给外层命令（这里是 `bash`） |
| `bash <(...)` | 让一个**新的 Bash 实例**把该“文件”当作脚本参数执行，真正主进程由 `bash` 接管 |

### 它与 `curl | bash` 的区别

- `curl ... | bash` 用管道把 curl 的 stdout 交给 bash 的 stdin，两者是**并行子进程**。
- `bash <(wget ...)` 是进程替换，`bash` 接收到的是一个**已存在的、代表脚本的 fd**，一次性读取执行。效果等价，但写法上更“看起来像在用一个文件”。
- 无论哪种，`sb.sh` 都是**在当前 shell 的派生进程中执行**，脚本内 `cd`、`exit`、`export` 等对当前交互 shell 无影响（`exit` 只会结束这个 bash 子进程）。

> `sb.sh` 内部 `[[ $EUID -ne 0 ]] && ... && exit`（`sb.sh:15`）正是靠这一点退出的：缺 root 时子 bash 直接退出，不影响用户终端。

### 辅以 `curl` 的等价入口

脚本 README 与自身菜单用到两种远程执行方式，效果一致：

```bash
# README 常见（curl 版）
bash <(curl -Ls https://raw.githubusercontent.com/yonggekkk/sing-box-yg/main/sb.sh)

# 脚本自更新 / 快捷方式会用 curl 落盘后执行
curl -L -o /usr/bin/sb --retry 2 --insecure https://raw.githubusercontent.com/yonggekkk/sing-box-yg/main/sb.sh  # lnsb(), sb.sh:3834-3838
```

## 二、一次完整安装（菜单 `1` → `instsllsingbox`）的执行流程

`instsllsingbox()` 定义于 `sb.sh:2524-2561`，是安装主控。它按顺序调用以下子例程：

```
instsllsingbox
 ├─ 1. mkdir -p /etc/s-box                          # 安装根目录
 ├─ 2. v6            → 判断纯IPV6/NAT64、决定 endip、ipv 策略
 ├─ 3. openyn        → 问是否关防火墙/开端口 → close
 ├─ 4. inssb         → 下载并安装 sing-box 内核（见 03）
 ├─ 5. inscertificate→ 生成或申请证书（自签或 Acme）
 ├─ 6. insport       → 设定 5 个协议端口 / 随机端口
 ├─ 7. 生成 reality 密钥与 short_id：sing-box generate reality-keypair / rand --hex 4
 ├─ 8. 下载 geoip.db / geosite.db
 ├─ 9. warpwg        → 生成 WARP-Wireguard 出站账户
 ├─10. inssbjsonser  → heredoc 写出 sb10.json / sb11.json 并用 sb.json 生效
 ├─11. sbservice     → 注册 systemd/OpenRC 服务并启动
 ├─12. sbactive      → 校验配置存在
 ├─13. 记录版本号到 /etc/s-box/v（从仓库 main/version 拉取）
 ├─14. lnsb + cronsb → 写入 /usr/bin/sb 快捷方式和每日重启 cron
 ├─15. wgcfgo        → 处理 WARP 相关（按需）
 └─16. sbshare       → 生成并打印 5 个协议分享链接 + 二维码 + 客户端配置
```

### 各步骤细节

#### 1&2. 平台探测与联网策略（脚本开头，非函数内）

- 发行版探测：`/etc/redhat-release`、`/etc/issue`、`/proc/version` → `Centos / alpine / Debian / Ubuntu`；Arch 直接拒绝（`sb.sh:18-44`）。
- CPU 探测：`uname -m` → `aarch64=arm64`、`x86_64=amd64`、`armv7=armv7`（`sb.sh:47-52`），用于拼内核包名。
- 虚拟化：`systemd-detect-virt`，openvz 时会尝试补建 TUN 设备（`sb.sh:119-186`）。

#### 3. 关闭防火墙/开放端口 `openyn`/`close`（`sb.sh:177-209`）

`close()` 会停 firewalld、`ufw disable`、把 iptables 三条链全部 `ACCEPT` 并清空 mangle/F 链，最后 `netfilter-persistent save`。属于“全开”策略。

#### 4. 下载内核 `inssb`（`sb.sh:211-240`）→ 详见 [03-singbox-kernel.md](./03-singbox-kernel.md)

#### 5. 证书 `inscertificate`（`sb.sh:241-316`）

- 无论后续选择，都会**先**用 `openssl` 生成一对 `prime256v1` 密钥与一个 CN=www.bing.com 的自签证书（`/etc/s-box/private.key` + `cert.pem`），有效期 36500 天。
- 若检测到之前用过 acme-yg（`/root/ygkkkca/cert.crt` 存在），询问是否复用；否则询问是否现场执行 `bash <(curl -Ls .../acme-yg/main/acme.sh)`。
- 自定义证书路径被写入 `ymzs()` / `zqzs()` 两组变量（`sb.sh:242-290`），后续 heredoc 直接引用：
  - `zqzs`（自签）：`certificate*=/etc/s-box/cert.pem`，reality SNI `apple.com`，vmess server_name `www.bing.com`，`tlsyn=false`。
  - `ymzs`（域名证书）：`certificatec/p*=/root/ygkkkca/cert.crt|private.key`，`tlsyn=true`，vmess 可用带证书的域名。

#### 6. 端口 `insport`（`sb.sh:359-421`）

- `ports=()` 循环生成 5 个互不重复、未被占用的 10000-65535 随机端口，依次赋给 `port_vm_ws / port_vl_re / port_hy2 / port_tu / port_an`。
- vmess 端口被**特殊处理**：若开 TLS（`tlsyn=true`），从 `2053/2083/2087/2096/8443` 里随机（CDN 优选端口）；否则从 `8080/8880/2052/2082/2086/2095` 随机（`sb.sh:384-397`）。
- `$sbnh == "1.10"`（1.10 系列内核）时**不生成 anytls 端口**（`anport` 跳过）。

#### 7. 统一 UUID 与 Reality 密钥（`sb.sh:417`，`sb.sh:2536-2541`）

```bash
uuid=$(/etc/s-box/sing-box generate uuid)                       # 4 个协议共用同一个 UUID
key_pair=$(/etc/s-box/sing-box generate reality-keypair)        # vless-reality 专用
private_key=$(... awk '/PrivateKey/{print $2}')                 # 服务端私钥
public_key=$(... awk '/PublicKey/{print $2}')                   # 客户端要用的公钥
echo "$public_key" > /etc/s-box/public.key
short_id=$(/etc/s-box/sing-box generate rand --hex 4)           # reality short-id
```

#### 8. 规则库（`sb.sh:2542-2543`）

```bash
wget -q -O /root/geoip.db  https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.db
wget -q -O /root/geosite.db https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.db
```

#### 9. WARP-Wireguard 出站账户 `warpwg`（`sb.sh:3332-3381`）→ 详见 [05](./05-warp-argo-outbounds.md)

#### 10. 生成配置 `inssbjsonser`（`sb.sh:422-877`）→ 详见 [04](./04-five-protocol-nodes.md)

#### 11. 服务 `sbservice`（`sb.sh:879-913`）→ 详见 [03](./03-singbox-kernel.md)

#### 12-16. 收尾

- `sbactive` 检查 `/etc/s-box/sb.json` 存在。
- 用 `curl -sL .../sing-box-yg/main/version` 的首行写入 `/etc/s-box/v`，供主菜单显示“脚本版本号”。
- `lnsb` 把 sb.sh 下载为 `/usr/bin/sb`（可执行快捷方式）；`cronsb` 添加每天 01:00 重启 sing-box 的 cron（`sb.sh:3817-3823`）。
- `wgcfgo`：若 VPS 已走 WARP（`wgcfv4/v6 = on|plus`）则先停 warp 再执行 IP 刷新流程，再重启。
- `sbshare`：`result_vl_vm_hy_tu` → 解析 sb.json 得到各字段 → `resvless/resvmess/reshy2/restu5/resan` 拼出分享链接与二维码 → `sb_client` 生成 SFA/SFI/SFW 与 Mihomo 配置 → 聚合到 `jhdy.txt/jhsub.txt`。

## 三、安装完成后的产物

| 路径 | 说明 |
| --- | --- |
| `/etc/s-box/sing-box` | sing-box 内核二进制 |
| `/etc/s-box/sb.json` | 生效配置（sb10.json 或 sb11.json 的拷贝） |
| `/etc/s-box/sb10.json` / `sb11.json` | 1.10 系列 / 非 1.10 的模板 |
| `/etc/s-box/private.key` + `cert.pem` | 自签 bing 证书 |
| `/etc/s-box/public.key` | vless-reality 的 public key |
| `/etc/s-box/vl_reality.txt` 等 | 各协议分享链接 |
| `/etc/s-box/jhdy.txt` / `jhsub.txt` | 聚合节点订阅内容 |
| `/etc/s-box/sbox.json` | SFA/SFI/SFW 客户端配置 |
| `/etc/s-box/clmi.yaml` | Mihomo/Clash.Meta 配置 |
| `/etc/systemd/system/sing-box.service` | systemd 服务单元 |
| `/usr/bin/sb` | 脚本快捷方式 |
