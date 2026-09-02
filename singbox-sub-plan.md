可以，而且我建议你不要在“全二进制”和“全脚本”里二选一。

对这个项目，最合适的是：

> **核心功能做成 C++ 单二进制，安装/升级/卸载保留少量 Shell 脚本。**

这样既能获得低内存、低运行时开销，又不会把 Linux 系统管理这种本来就适合 Shell 的工作硬塞进 C++。

现有 `sing-box-yg` 已经覆盖了 VLESS Reality、VMess WS、Hysteria2、TUIC、AnyTLS 等节点生成，并支持本地 IP 订阅；它的 README 也明确强调订阅节点本地生成，不依赖第三方订阅转换服务。 而你的 `vps-sub-meter` 当前主要在此基础上补充流量统计、订阅 HTTP 服务、`subscription-userinfo`、鉴权和 HTTPS。

我建议把这两个项目的核心思想重新整理，而不是继续调用 `sb.sh`。

## 一、最终工具形态

比如取一个名字：

```text
sbctl
```

部署后 VPS 上尽量只保留：

```text
/usr/local/bin/sbctl
/usr/local/bin/sing-box

/etc/sbctl/
├── config.toml
├── sing-box.json
├── state.json
└── cert/

/var/lib/sbctl/
└── subscription.cache

/etc/systemd/system/
├── sing-box.service
└── sbctl.service
```

用户平时只需要：

```bash
sbctl install
sbctl status
sbctl node
sbctl sub
sbctl traffic
sbctl restart
sbctl update
sbctl uninstall
```

这样就比现在：

```text
sb.sh
+
auto_setup.sh
+
switch_sb_mode.sh
+
gcp_sub_meter.sh
+
aws-sub-meter.sh
+
vmiss_sub_meter.sh
...
```

清楚很多。

---

# 二、职责必须分开

整个系统最好明确分成两个进程。

```text
                 ┌────────────────────┐
Internet ───────►│      sing-box      │
                 │                    │
                 │ VLESS / HY2 / TUIC│
                 │ AnyTLS / VMess... │
                 └────────────────────┘

                         +

                 ┌────────────────────┐
HTTP/HTTPS ─────►│       sbctl        │
                 │    daemon/server   │
                 │                    │
                 │ subscription       │
                 │ traffic            │
                 │ auth               │
                 │ node export        │
                 └────────────────────┘
```

### `sing-box`

只负责：

```text
真正的代理协议
监听端口
连接处理
TLS / Reality
```

### `sbctl`

负责：

```text
安装 sing-box
生成配置
生成密钥
生成节点
生成订阅
流量统计
subscription-userinfo
配置管理
版本更新
systemd 管理
```

这样以后即使 sing-box 配置格式变化，你也只修改 `singbox/` 模块。

---

# 三、我不建议继续“包装 sb.sh”

最简单的做法当然是：

```cpp
system("bash sb.sh");
```

但我不建议。

因为最终会变成：

```text
你的程序
 ↓
Shell
 ↓
sing-box-yg
 ↓
wget
 ↓
各种脚本
 ↓
配置文件
```

这只是给 Shell 脚本套了一层 C++ 外壳。

维护时依旧会遇到：

- 上游脚本菜单改变
- 输出文字改变
- 目录改变
- 下载 URL 改变
- `printf '3\n8\n...' | sb` 失效
- 难以做可靠错误处理

而 `sing-box-yg` 本身现在已经有许多交互菜单和功能。例如当前 README 中，本地 IP 订阅就是通过其菜单操作生成的。

你的新工具应该直接：

```text
生成 sing-box 配置
```

而不是：

```text
调用另一个生成 sing-box 配置的脚本
```

---

# 四、核心 C++ 项目结构

我建议 C++20：

```text
sbctl/
├── CMakeLists.txt
│
├── src/
│   ├── main.cpp
│   │
│   ├── cli/
│   │   ├── cli.cpp
│   │   └── commands.cpp
│   │
│   ├── core/
│   │   ├── config.cpp
│   │   ├── state.cpp
│   │   └── filesystem.cpp
│   │
│   ├── singbox/
│   │   ├── installer.cpp
│   │   ├── config_generator.cpp
│   │   ├── service.cpp
│   │   └── version.cpp
│   │
│   ├── node/
│   │   ├── node.cpp
│   │   ├── vless.cpp
│   │   ├── hysteria2.cpp
│   │   ├── tuic.cpp
│   │   ├── anytls.cpp
│   │   └── vmess.cpp
│   │
│   ├── subscription/
│   │   ├── subscription.cpp
│   │   ├── singbox_json.cpp
│   │   ├── clash_yaml.cpp
│   │   └── uri.cpp
│   │
│   ├── traffic/
│   │   ├── traffic.cpp
│   │   └── sysfs.cpp
│   │
│   ├── http/
│   │   ├── server.cpp
│   │   ├── router.cpp
│   │   └── auth.cpp
│   │
│   └── crypto/
│       ├── uuid.cpp
│       ├── random.cpp
│       └── reality.cpp
│
├── scripts/
│   ├── install.sh
│   └── uninstall.sh
│
└── systemd/
    ├── sbctl.service
    └── sing-box.service
```

---

# 五、核心功能 1：安装并运行 sing-box

这部分 C++ 可以完成大多数工作。

安装过程：

```text
检测 CPU
 ↓
amd64 / arm64
 ↓
获取 sing-box release
 ↓
下载
 ↓
SHA256 校验
 ↓
解压
 ↓
安装到 /usr/local/bin/sing-box
 ↓
生成配置
 ↓
sing-box check
 ↓
创建 systemd service
 ↓
启动
```

不要自己实现代理协议。

也就是说：

```text
你的程序 = control plane
sing-box   = data plane
```

这是最合理的职责划分。

甚至 sing-box 官方从 1.14 开始已经有 API service，用于观察和控制运行实例；未来你甚至可以逐渐通过 API 与运行中的 sing-box 交互。

---

# 六、第一版不要一次支持五种协议

虽然 `sing-box-yg` 当前支持：

- VLESS Reality Vision
- VMess WS
- Hysteria2
- TUIC v5
- AnyTLS 


但你自己的第一版，我建议：

```text
V0.1
├── VLESS Reality
└── Hysteria2
```

然后：

```text
V0.2
├── TUIC
└── AnyTLS
```

最后才考虑：

```text
VMess WS + TLS/CDN
```

因为前几个基本是：

```text
VPS
  ↕
client
```

而 VMess WS / CDN / Argo 会引入：

```text
域名
Cloudflare
证书
WebSocket
CDN
Tunnel
```

系统复杂度会突然大很多。

---

# 七、节点采用统一内部模型

这个设计很重要。

不要让：

```text
sing-box 配置
Clash 配置
分享链接
```

分别生成自己的随机参数。

应该先生成一个：

```cpp
struct Node {
    Protocol protocol;

    std::string name;
    std::string server;

    uint16_t port;

    std::string uuid;

    TLSConfig tls;
};
```

比如 VLESS：

```cpp
struct RealityConfig {
    std::string server_name;
    std::string private_key;
    std::string public_key;
    std::string short_id;
};

struct VlessConfig {
    std::string uuid;
    std::string flow;

    RealityConfig reality;
};
```

然后：

```text
                Node model
                    │
        ┌───────────┼─────────────┐
        ↓           ↓             ↓
sing-box server   URI share   subscription
config
```

这是重构以后非常关键的改进。

---

# 八、节点生成

例如：

```bash
sbctl node create vless-reality
```

程序：

```text
随机 UUID
↓
随机端口
↓
生成 Reality private/public key
↓
生成 short-id
↓
取得 VPS IP
↓
生成 server config
↓
写入 sing-box.json
↓
sing-box check
↓
restart
```

然后：

```bash
sbctl node
```

输出：

```text
VLESS Reality

Address: 1.2.3.4
Port: 44321
UUID: ...
SNI: www.cloudflare.com

vless://....
```

二维码只是 UI 功能。

**第一版完全不要为了二维码再引入 Python pip。**

以后需要时可以链接一个小 QR Code C/C++ 库。

---

# 九、核心功能 2：订阅服务

这部分直接由 `sbctl daemon` 提供。

例如：

```text
https://sub.example.com/s/abc123
```

HTTP：

```http
GET /s/abc123
```

返回：

```text
HTTP/1.1 200 OK

subscription-userinfo:
upload=...;
download=...;
total=...;
expire=...

Content-Type:
application/json
```

正文是：

```text
sing-box JSON subscription
```

或者：

```text
Clash/Mihomo YAML
```

---

# 十、`subscription-userinfo` 应该成为独立模块

不要让 HTTP 层自己计算流量。

应该：

```cpp
struct TrafficInfo {
    uint64_t upload;
    uint64_t download;
    uint64_t total;
    std::optional<std::time_t> expire;
};
```

然后：

```cpp
std::string make_subscription_userinfo(
    const TrafficInfo& info
);
```

例如：

```text
upload=1073741824;
download=5368709120;
total=107374182400;
expire=1798761600
```

这样：

```text
traffic
   ↓
TrafficInfo
   ↓
subscription HTTP header
```

非常干净。

---

# 十一、流量统计最好直接使用 Linux `/sys`

不需要 vnstat。

读取：

```text
/sys/class/net/eth0/statistics/rx_bytes
/sys/class/net/eth0/statistics/tx_bytes
```

例如：

```cpp
struct Counters {
    uint64_t rx;
    uint64_t tx;
};
```

读取成本非常小。

问题在于：

> 网卡 counter 是系统启动以来的数据，不是你的月度套餐数据。

所以保存 baseline：

```json
{
    "rx_base": 12345678,
    "tx_base": 23456789,
    "period": "2026-09"
}
```

计算：

```text
download =
current_rx - rx_base

upload =
current_tx - tx_base
```

月底：

```text
base = current
```

---

# 十二、但必须处理重启

这是很多简单 `/sys` 流量脚本容易犯的错误。

机器 reboot：

```text
rx_bytes
tx_bytes
```

会重新从较小数值开始。

所以不能仅仅：

```cpp
current - baseline
```

否则会 unsigned underflow。

状态应该设计成：

```cpp
struct TrafficState {
    uint64_t accumulated_rx;
    uint64_t accumulated_tx;

    uint64_t last_rx;
    uint64_t last_tx;

    std::string boot_id;
};
```

Linux：

```text
/proc/sys/kernel/random/boot_id
```

如果 boot ID 变化：

```text
发生 reboot
```

则：

```text
之前累计值保留
+
新 boot 的计数器
```

这样才能做到真正可靠。

这是我认为重写 `vps-sub-meter` 时值得认真改进的地方。

---

# 十三、不需要数据库

对于单 VPS：

```text
SQLite
PostgreSQL
Redis
```

都没有必要。

一个：

```text
/var/lib/sbctl/state.json
```

就足够。

例如：

```json
{
  "traffic": {
    "period": "2026-09",
    "upload": 284738292,
    "download": 918273829,
    "last_rx": 123,
    "last_tx": 456,
    "boot_id": "..."
  }
}
```

用：

```text
tmp
↓
fsync
↓
rename
```

原子更新。

---

# 十四、订阅格式应该至少支持两个

建议：

```text
/s/{token}/singbox
/s/{token}/clash
```

比如：

```text
https://sub.example.com/s/abc/singbox
https://sub.example.com/s/abc/clash
```

还可以：

```text
/s/abc
```

根据 User-Agent 自动判断。

但第一版我反而不建议 User-Agent 自动判断。

显式 URL 更可靠。

---

# 十五、HTTPS 怎么解决

这里我建议第一版依旧用 Caddy。

架构：

```text
                  :443
                    │
                 Caddy
                    │
             127.0.0.1:2080
                    │
                  sbctl
```

只有：

```text
Caddy → HTTPS/ACME
sbctl → HTTP
```

原因很简单：

如果为了少二三十 MB 内存，自己实现：

```text
ACME
TLS
证书续签
challenge
```

开发成本和风险明显不值得。

768 MB VPS：

```text
sing-box
+
Caddy
+
C++ daemon
```

已经足够轻。

以后 V2 再考虑直接 `rustls`……当然你这里是 C++，可以用 OpenSSL/Boost.Asio SSL，但 ACME 仍然需要处理。

---

# 十六、CLI 与 daemon 可以是同一个二进制

这个设计非常推荐。

一个：

```text
/usr/local/bin/sbctl
```

同时承担：

```text
CLI
+
HTTP daemon
```

例如：

```bash
sbctl daemon
```

systemd：

```ini
ExecStart=/usr/local/bin/sbctl daemon
```

用户：

```bash
sbctl status
```

还是同一个文件。

最终发行只有：

```text
sbctl
```

非常舒服。

---

# 十七、Shell 到底保留多少

我推荐只有：

```text
install.sh
```

大约几十到一两百行。

负责：

```text
检测 root
安装 ca-certificates / curl
下载 sbctl
chmod +x
执行 sbctl install
```

比如：

```bash
curl -fsSL https://example.com/install.sh | bash
```

这个安装脚本最终执行：

```bash
/usr/local/bin/sbctl install
```

也就是说：

```text
Shell
只负责 bootstrap

C++
负责真正的软件逻辑
```

这是我最推荐的边界。

---

# 十八、不要把 apt 管理全部写进 C++

例如这种东西：

```cpp
system("apt update");
system("apt install caddy");
```

虽然可以，但我认为不好。

Linux 包管理：

```text
apt
apk
dnf
```

还是 Shell 更自然。

因此你可以做：

```text
scripts/
├── install-debian.sh
└── install-alpine.sh
```

而 C++ 本身完全发行版无关。

这还有一个额外好处：

### 以后 Alpine 就很好支持了。

现在你的 `vps-sub-meter` 明确依赖 `apt`，因此限制在 Debian/Ubuntu。

重构以后：

```text
sbctl binary
        │
    Linux API
        │
 ┌──────┴──────┐
Debian       Alpine
```

就容易很多。

---

# 十九、最终部署资源

你的 768 MB VPS 可以最终变成：

```text
Debian 13 minimal

systemd
├── sshd
├── sing-box
├── caddy
└── sbctl
```

不再需要：

```text
Python
pip
vnstat
jq
cron
复杂 Bash daemon
Docker
Node.js
```

这就是我认为比较理想的状态。

---

# 二十、安装体验

最终用户体验应该是：

```bash
curl -fsSL https://xxx/install.sh | bash
```

然后：

```text
sbctl v0.1

Select protocols:

[1] VLESS Reality
[2] Hysteria2
[3] Both

> 3

Traffic limit:
> 1024 GB

Reset day:
> 1

Subscription domain:
> sub.example.com
```

结束：

```text
✓ sing-box installed
✓ sing-box config generated
✓ VLESS Reality created
✓ Hysteria2 created
✓ Caddy configured
✓ subscription service started

Subscription:

Sing-box:
https://sub.example.com/s/xxxxx/singbox

Mihomo:
https://sub.example.com/s/xxxxx/clash

Traffic:
0 B / 1 TB
```

之后：

```bash
sbctl status
```

：

```text
sing-box          running
subscription      running

VLESS Reality     :44321
Hysteria2         :31452

Traffic
Upload            1.21 GiB
Download          18.35 GiB
Total             19.56 / 1024 GiB

Reset             2026-10-01
```

---

## 我的最终建议

不要做：

```text
一个巨大的 sb.sh
```

也不要做：

```text
一个什么系统操作都自己实现的巨大 C++ binary
```

而是做：

```text
                 项目

        ┌──────────┴──────────┐
        │                     │
     C++20 sbctl          Shell bootstrap
        │                     │
        │                install.sh
        │                uninstall.sh
        │
 ┌──────┼─────────────┐
 │      │             │
管理   订阅          流量
 │      │             │
 │      └──── HTTP ───┘
 │
 ▼
sing-box
```

**运行时核心全部 C++；Shell 只负责安装环境。**

这样既符合你“高性能、低内存”的目标，又不会为了追求“100% C++”把系统维护搞复杂。

而且如果这是你准备长期维护的项目，我建议第一阶段就只做 **Debian + VLESS Reality + Hysteria2 + sing-box JSON 订阅 + `subscription-userinfo`**。等这一条链路完全稳定之后，再逐个加入 TUIC、AnyTLS、Mihomo 和 Alpine。这样项目很容易做成，而不是一开始就复制 `sing-box-yg` 已经积累多年的全部复杂度。