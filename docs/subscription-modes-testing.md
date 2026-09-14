# 三种订阅模式手动联测手册

本文档用于在真实 VPS 上手动验证 sbctl 的三种订阅模式。自动化验收（`tests/acceptance/verify.sh`）已覆盖同样断言，本手册补足「从部署到手机导入成功」的真实链路。

准备工作（三种模式通用）：

```bash
# 在 VPS 上确认服务与端口
sbctl status
sbctl node          # 查看协议监听端口
sbctl sub           # 输出全部订阅链接、二维码链接与总览页链接
```

---

## 1. Direct（生产使用中，回归验证）

前置：域名 A 记录指向 VPS；80/443 已放行；证书已签发。

```bash
# 1. DNS 与证书
dig +short <域名>                       # 应返回 VPS 公网 IP
sbctl certificate status                # 有效期 / SAN / deploy hook

# 2. 订阅自检（把 <凭据> 换成 sbctl sub 输出里的凭据段）
curl -fsS https://<域名>/sub/<凭据>/uri | head
curl -fsS -o /dev/null -w '%{http_code}\n' https://<域名>/sub/<凭据>/index   # 200
curl -fsS -o /dev/null -w '%{http_code}\n' https://<域名>/sub/<凭据>/qr/uri  # 200
```

3. 手机验证：Shadowrocket 扫描 `sbctl qr` 的终端二维码或总览页二维码 → 导入订阅 → 选节点 → 测延迟。

## 2. External proxy（sbctl 只监听 loopback，由 Nginx/Caddy 反代）

在另一台干净 VPS（或与 Direct 共存时用不同 root 与端口）：

```bash
# 1. 安装（sbctl 监听 127.0.0.1:2080，不占用公网端口）
bash <(wget -qO- https://raw.githubusercontent.com/xiaolingxiaoying/singbox-sub-me/master/scripts/install.sh)
# 向导中选择「外部反向代理」模式

# 2. Caddy 反代示例（/etc/caddy/Caddyfile）
<你的域名> {
    handle_path /sub/* {
        reverse_proxy 127.0.0.1:2080
    }
}
# systemctl reload caddy
# 注意：Caddy 需要能访问 2080；保持 sbctl 的 subscription_listen_port 一致。

# 3. Nginx 反代示例
# location /sub/ {
#     proxy_pass http://127.0.0.1:2080;
#     proxy_set_header Host $host;
# }

# 4. 放行协议端口（TCP: vless/vmess/anytls, UDP: hysteria2/tuic）
sudo ufw allow 443/tcp          # 反代入口
sudo ufw allow <vless端口>/tcp
sudo ufw allow <hysteria2端口>/udp
# ...其余端口见 sbctl node

# 5. 自检
curl -fsS https://<域名>/sub/<凭据>/clash.yaml | head
curl -fsS -o /dev/null -w '%{http_code}\n' https://<域名>/sub/<凭据>/sing-box-full.json   # 200
curl -fsS -o /dev/null -w '%{http_code}\n' "https://<域名>/sub/<凭据>/uri?x=1"            # 404（拒绝 query）
```

6. 手机验证：Clash Party 粘贴 `clash.yaml` 链接导入；sing-box (SFA) 导入 `sing-box-full.json`。

## 3. IP fallback（无域名，明文 HTTP 高位端口）

```bash
# 1. 安装时选择「IP 回退」模式，协议证书选 self-signed
sbctl config init \
  --mode ip-fallback \
  --subscription-host <VPS公网IP> \
  --http-port 2080 \
  --interface eth0 \
  --protocol vless-reality --protocol vmess-websocket --protocol hysteria2 \
  --protocol tuic --protocol anytls \
  --reality-decoy-sni www.cloudflare.com \
  --protocol-sni www.bing.com

# 2. 放行端口
sudo ufw allow 2080/tcp            # 订阅端口（明文 HTTP，注意风险）
sudo ufw allow <各协议端口>/tcp|udp

# 3. 自检
curl -fsS http://<VPS公网IP>:2080/sub/<凭据>/uri | head
curl -fsS -o /dev/null -w '%{http_code}\n' http://<VPS公网IP>:2080/sub/<凭据>/index  # 200
curl -fsS -o /dev/null -w '%{http_code}\n' "http://<IP>:2080/sub/<凭据>/uri?x=1"     # 404
```

4. 手机验证：V2rayN / Shadowrocket 粘贴 `uri.txt`（Base64）导入；证书类协议确认客户端「跳过证书验证/allowInsecure」已开启。

---

## 通用验收清单（每种模式过一遍）

- [ ] `sbctl sub` 列出全部链接且都能下载（200）
- [ ] 每条链接的 `qr/<格式>` 二维码能在浏览器打开并被手机扫描导入
- [ ] `index` 总览页能打开，包含全部链接与导入步骤
- [ ] 错误凭据、带 query 参数、未知版本路径均返回 404
- [ ] 导入后客户端能测延迟并连通至少一个节点
- [ ] `sing-box-full.json` 导入后 SFA/SFW 面板可用（clash_api 生效）

## 已知边界

- IP fallback 模式订阅为明文 HTTP，凭据可能被中间人窥探，仅建议临时使用。
- external-proxy 模式下 sbctl 不管理证书；`sbctl certificate status` 会提示由反代负责。
- sbctl 永远不会自动修改防火墙；所有 `ufw` 命令都需要手动执行。
