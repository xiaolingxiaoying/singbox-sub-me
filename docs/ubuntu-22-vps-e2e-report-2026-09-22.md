# Ubuntu 22.04 VPS 端到端测试报告（2026-09-22 UTC）

## 1. 结论

本次测试结论为 **No-Go**：当前候选版本不得发布。

候选版本在 Ubuntu 22.04 真实 systemd VPS 上完成了安装、Direct HTTPS、五种 Managed
protocol、订阅认证、流量任务、服务重启和整机重启验证；但是故障注入发现，独立
sing-box 更新路径会把一个启动后立即退出的候选错误地判定为更新成功，并且不会自动应用已经
创建的 rollback point。该问题会使 `sing-box.service` 进入持续重启循环，违反
[ADR-0003](adr/0003-verified-and-reversible-lifecycle.md) 的可逆更新要求和
[ADR-0014](adr/0014-release-gates.md) 的发布门禁。

另外还确认了两个发布阻断项：

1. Direct 模式生成的 `sbctl.service` 把 `Sockets=` 写在 `[Unit]`，Ubuntu 22.04 systemd
   会报告未知键并忽略；
2. 本次候选未编入生产发布公钥，无法验证 Authenticated release manifest。

## 2. 测试范围

### 2.1 已覆盖

- Ubuntu 22.04 amd64 真实 Production host；
- 真实 systemd 启动、停止、自动启动和 socket activation；
- Direct subscription mode；
- TCP 80/443 监听和 HTTP-01 challenge；
- HTTPS Subscription route、Subscription credential 和错误请求边界；
- sing-box、Clash/Mihomo、URI、总览页及二维码路由；
- VLESS Reality、VMess WebSocket、Hysteria2、TUIC v5、AnyTLS；
- `sbctl` 和 sing-box 的非 root 运行身份；
- VPS traffic 命令和 Accounting reset timer；
- 默认卸载、purge、重新安装和失败安装回滚；
- sing-box 本地候选更新及故障回滚注入；
- 服务重启和 VPS 整机重启恢复；
- 候选程序对生产 manifest 的信任锚检查。

### 2.2 未覆盖或未形成发布证据

- External proxy subscription mode 的真实 Nginx/Caddy 链路；
- IP fallback subscription 的本轮真实 VPS 重装验证；
- 真实移动端或桌面 GUI 导入操作；本轮使用同版本 sing-box 客户端逐协议建立代理连接；
- Certbot 向 ACME CA 重新申请证书；使用的是 VPS 上已有且有效的证书，并验证了证书状态、
  pinned certificate 和 HTTP-01 challenge；
- GitHub Actions 生成的正式 signed release artifact；本轮候选是本地 Docker 构建，不是
  Release workflow 产物；
- Debian 12 和 Ubuntu 24.04 的本轮重新验收。

## 3. 安全与脱敏规则

本报告不记录以下内容：

- VPS IP；
- root 密码；
- Subscription host 的真实域名；
- Subscription credential；
- Proxy credential、UUID、Reality 私钥、TLS 私钥；
- 完整 Subscription URL。

命令中的占位符含义如下：

| 占位符 | 含义 |
| --- | --- |
| `<VPS_IP>` | 测试 VPS 的公网地址 |
| `<SUBSCRIPTION_DOMAIN>` | 解析到测试 VPS 的 Subscription host |
| `<SUBSCRIPTION_CREDENTIAL>` | 配置中的 Subscription credential |
| `<CANDIDATE>` | 当前提交构建出的 Linux amd64 `sbctl` |

测试使用的临时 SSH 公钥、候选副本、故障桩、客户端配置和测试脚本在测试完成后均已删除。
由于 root 密码曾通过交互渠道提供，仍应在测试后轮换。

## 4. 测试对象与环境

### 4.1 源码候选

| 项目 | 值 |
| --- | --- |
| Git commit | `a5fe586070738d32505002ae362cacffbd885b6d` |
| Cargo package version | `0.1.26` |
| 候选 SHA-256 | `30ee012704e5f5464f89afca380ca06b3c31c1be8416fc918305f2433a7fea7d` |
| 构建方式 | Docker，Linux amd64 release |
| Rust 镜像 | `rust:1-bookworm` |
| Production public key | 未配置 |

应特别注意：`v0.1.26` 已经存在，本次候选仍报告 `0.1.26`，但源码提交晚于该 tag。这个二进制
只能作为当前代码的 VPS 测试候选，不能作为新的发布附件。

### 4.2 Docker 构建边界

用户要求避免 WSL 文件系统开销，因此构建使用 Docker。为避免只构建服务端时解析 GUI 的
Zed Git 依赖，临时构建上下文只复制根包的 `Cargo.toml`、`Cargo.lock` 和 `src/`，并在容器
中移除临时副本的 `[workspace]` 段。构建使用宿主 Cargo cache 的只读挂载和 `--offline`。

该做法验证了当前服务端源码能够生成并运行 Linux release 二进制，但它不代替 Release
workflow：

- 没有注入 `SBCTL_RELEASE_PUBLIC_KEY_HEX`；
- 不是 GitHub Actions 保存的 artifact；
- 没有和 release tag、signed manifest、`install.sh` 形成同一套候选资产。

因此本报告把它称为“源码候选”，而不是“正式候选 Release”。

### 4.3 Production host

| 项目 | 值 |
| --- | --- |
| OS | Ubuntu 22.04.5 LTS |
| 架构 | x86_64 |
| 内核 | Linux 5.15 系列 |
| init | systemd |
| sing-box | 1.14.1 |
| Subscription mode | Direct |
| 默认路由接口 | `eth0` |
| 公网端口 | TCP 80、443 可达 |

远程操作由 PowerShell 7 调用系统 `ssh`/`scp` 完成。VPS 的 SSH 在测试中多次于
`kex_exchange_identification` 阶段主动关闭新连接，因此使用了单一持久 SSH 会话，并降低了
重连频率。该连接问题没有影响已经建立的会话，也没有发现与 sbctl 服务有关的证据。

## 5. 前置状态与停止点

首次盘点发现 VPS 并非干净环境：

- `/usr/local/bin/sbctl` 和 `/usr/local/bin/sing-box` 已存在；
- `sbctl.service`、`sing-box.service`、`sbctl-http.socket`、
  `sbctl-accounting-reset.timer` 已启用；
- TCP 80/443 已由 systemd socket 和 sbctl 持有；
- `/etc/sbctl`、`/var/lib/sbctl`、`/etc/sing-box` 已存在。

因此测试最初只进行了只读检查，没有覆盖或接管 Existing deployment。在管理员明确确认该 VPS
可中断、可清空后，才继续执行卸载和重装。

只读基线检查结果：

| 检查 | 结果 |
| --- | --- |
| 旧 `sbctl` 版本 | `0.1.26` |
| 旧 sing-box 版本 | `1.14.1` |
| 四个 systemd 单元 | active |
| `sbctl config validate` | 通过 |
| `sbctl status` | 通过 |
| `sbctl node` | 通过 |
| `sbctl sub` | 通过 |
| URI、Clash、sing-box Subscription format | HTTP 200 |
| `subscription-userinfo` | 存在 |
| 总览页、二维码 | HTTP 200 |
| 错误 Subscription credential | HTTP 404 |
| 带 query 的 Subscription route | HTTP 404 |

旧二进制安装时间早于当前提交，且其后服务端已有多项源码变更，因此旧实例健康不被当作当前
候选的发布证据。

## 6. 测试过程

### 6.1 创建可恢复快照

在删除旧部署前，将以下路径归档到 root-only 文件：

```text
/etc/sbctl
/var/lib/sbctl
/etc/sing-box
/usr/local/bin/sbctl
/usr/local/bin/sing-box
/etc/systemd/system/sbctl.service
/etc/systemd/system/sbctl-http.socket
/etc/systemd/system/sbctl-accounting-reset.service
/etc/systemd/system/sbctl-accounting-reset.timer
/etc/systemd/system/sing-box.service
```

保留的恢复快照：

```text
/root/pretest-state-20260922T155422Z.tar.gz
```

文件权限为 `0600 root:root`。归档中包含旧配置和私密材料，不应复制到仓库、CI artifact 或
公开工单。

### 6.2 默认卸载

执行：

```bash
/usr/local/bin/sbctl uninstall
```

验证结果：

- 服务和托管二进制被移除；
- `/etc/sbctl/config.toml` 保留；
- `/var/backups/sbctl` 中存在 root-readable backup；
- 默认卸载没有删除持久配置。

结果：**通过**。

### 6.3 Purge

执行上传到 root 私有目录的候选：

```bash
<CANDIDATE> uninstall --purge
```

验证结果：

- `config.toml`、ownership marker、sing-box 配置、托管二进制和 systemd unit 均已删除；
- `/etc/sbctl` 空目录可能保留；
- 一次失败安装回滚后，`/var/lib/sbctl/.operation.lock` 可能保留。

空目录及 `.operation.lock` 不是部署残留。操作锁必须保留原 inode，避免仍持锁的事务与后续
事务锁住不同 inode。后续 preflight 也不把只有操作锁的目录视为 Existing deployment。

结果：**通过**。

### 6.4 测试脚本校正记录

测试期间遇到三次属于测试调用方式的问题，均在扩大测试前停止并核对源码语义：

1. 初始脚本错误地要求 `/etc/sbctl` 目录完全不存在；实际 purge 契约只要求托管配置和状态
   被删除；
2. 初始脚本直接从 `/root` 调用候选，未先复制到 `/usr/local/bin/sbctl`，导致生成的 unit
   找不到 `ExecStart`；正式 bootstrap 会先安装已验证的管理二进制；
3. 持久 SSH 会话分配了 TTY，安装程序进入 Managed protocol 逐项确认并在 180 秒后超时；
   正式 bootstrap 是非交互调用。改为从 `/dev/null` 提供 stdin 后行为与 bootstrap 一致。

三次失败均触发了 fresh-install rollback。检查确认没有留下配置、unit、运行服务或 sing-box
二进制；只保留了允许存在的空目录或操作锁。这些事件不计为产品缺陷，但证明了失败安装的
清理路径有效。

### 6.5 按 bootstrap 契约安装

关键顺序如下：

```bash
install -m 0755 <CANDIDATE> /usr/local/bin/sbctl

/usr/local/bin/sbctl install \
  --mode direct \
  --subscription-host <SUBSCRIPTION_DOMAIN> \
  --interface eth0 \
  --reality-decoy-sni <REDACTED_SNI> \
  --sing-box-bin <VERIFIED_SING_BOX> \
  </dev/null
```

实际测试还复用了测试前已配置的五个 Protocol listener port，避免改变云侧已有端口策略。

安装后验证：

| 检查 | 结果 |
| --- | --- |
| Installed sbctl SHA-256 与候选一致 | 通过 |
| `sbctl.service` | active |
| `sing-box.service` | active |
| `sbctl-http.socket` | active |
| `sbctl-accounting-reset.timer` | active |
| `sbctl.service` 用户 | `sbctl` |
| `sing-box.service` 用户 | `sing-box` |
| `sbctl config validate` | 通过 |
| `sing-box check -c /etc/sing-box/config.json` | 通过 |
| `sbctl status`、`node`、`sub` | 通过 |

结果：**通过**。

### 6.6 Subscription route 与认证边界

在 VPS 内使用正确 SNI 将 Subscription host 解析到 `127.0.0.1`，验证 socket-activated
HTTPS：

```bash
curl --resolve '<SUBSCRIPTION_DOMAIN>:443:127.0.0.1' \
  'https://<SUBSCRIPTION_DOMAIN>/sub/<SUBSCRIPTION_CREDENTIAL>/uri'
```

结果：

| 路由 | 期望 | 实际 |
| --- | --- | --- |
| `uri` | 200、非空、含 `subscription-userinfo` | 通过 |
| `clash.yaml` | 200、非空、含 `subscription-userinfo` | 通过 |
| `sing-box.json` | 200、非空、含 `subscription-userinfo` | 通过 |
| `index` | 200、非空 | 通过 |
| `qr/uri` | 200、非空 | 通过 |
| 错误 Subscription credential | 404 | 通过 |
| 正确路径附加 `?x=1` | 404 | 通过 |

从测试工作站使用 PowerShell 验证 TCP 80/443 均可达；通过真实域名和 SNI 访问公网 HTTPS
错误路径得到预期 404，证明公网 TLS 入口可达。

结果：**通过**。

### 6.7 HTTP-01 与证书

在 `/var/lib/sbctl/acme-webroot/.well-known/acme-challenge/` 写入一次性 challenge，使用
Subscription host 和 TCP 80 请求：

```bash
curl --resolve '<SUBSCRIPTION_DOMAIN>:80:127.0.0.1' \
  'http://<SUBSCRIPTION_DOMAIN>/.well-known/acme-challenge/<TEMP_TOKEN>'
```

响应内容与一次性 challenge 完全一致；临时文件随后删除。`sbctl certificate status` 返回
成功，证书能够被非 root sbctl 服务读取。

结果：**通过**。

### 6.8 五种 Managed protocol 真实连接

从 `sing-box.json` Subscription format 取得五个 outbound，在 VPS 上为每个 outbound 生成
一次性 sing-box 客户端配置：

- 只保留一个待测 outbound；
- 创建监听 `127.0.0.1:21001` 起始端口的临时 `mixed` inbound；
- 以待测 outbound 作为 route final；
- 先运行 `sing-box check`；
- 启动临时客户端；
- 通过其 SOCKS5 代理访问 Cloudflare trace HTTPS endpoint；
- 每个协议结束后终止临时客户端并删除配置。

结果：

| Managed protocol | 真实代理请求 |
| --- | --- |
| VLESS Reality | PASS |
| VMess WebSocket | PASS |
| Hysteria2 | PASS |
| TUIC v5 | PASS |
| AnyTLS | PASS |

该检查不仅验证端口监听，还验证客户端配置、TLS/SNI、TCP 或 UDP 传输、服务端 inbound 和
公网出站能够共同完成一次 HTTPS 请求。

### 6.9 服务重启

执行：

```bash
sbctl restart
```

重启后检查：

- `sbctl.service`、`sing-box.service`、`sbctl-http.socket` 均 active；
- HTTPS URI Subscription route 仍然可用；
- 五个 Protocol listener port 均重新监听。

结果：**通过**。

### 6.10 systemd unit 语法

执行：

```bash
systemd-analyze verify \
  /etc/systemd/system/sbctl.service \
  /etc/systemd/system/sbctl-http.socket \
  /etc/systemd/system/sing-box.service \
  /etc/systemd/system/sbctl-accounting-reset.timer
```

sbctl 自有 unit 产生以下警告：

```text
/etc/systemd/system/sbctl.service:7:
Unknown key name 'Sockets' in section 'Unit', ignoring.
```

`systemd-analyze verify` 在该版本 systemd 上仍返回 0，但配置确实被忽略。TCP 80/443 仍能工作，
因为 `sbctl-http.socket` 的 `[Socket] Service=sbctl.service` 完成了 fd 交接，且 service 仍有
`Requires=`/`After=` 关系。

结果：**功能通过、unit 质量门禁失败**。

### 6.11 流量与 Accounting reset

执行：

```bash
sbctl traffic
systemctl start sbctl-accounting-reset.service
systemctl is-active sbctl-accounting-reset.timer
```

结果：

- VPS traffic 查询成功；
- Accounting reset oneshot 成功；
- Accounting reset timer 保持 active。

结果：**通过**。

### 6.12 生产 manifest 信任锚

下载当前公开的 amd64 manifest，并只执行 update check：

```bash
sbctl update --check --manifest <DOWNLOADED_MANIFEST>
```

实际错误：

```text
update failed: no production release public key was configured at build time
(SBCTL_RELEASE_PUBLIC_KEY_HEX)
```

检查前后 `sbctl` 和 sing-box SHA-256 均未改变。

结果：**失败，符合本次未注入生产公钥的构建条件；正式发布阻断**。

### 6.13 sing-box 更新故障注入

#### 目的

验证候选 sing-box 在静态配置检查成功、但作为服务启动后立即退出时，更新事务是否自动恢复
旧二进制并恢复服务。

#### 故障候选

故障桩实现以下行为：

```sh
#!/bin/sh
case "${1:-}" in
  check) exit 0 ;;
  run) exit 1 ;;
  *) exit 0 ;;
esac
```

它能通过更新前的 `sing-box check`，但无法作为 systemd 服务稳定运行。

#### 执行

```bash
before=$(sha256sum /usr/local/bin/sing-box)
sbctl sing-box update --artifact <BROKEN_SING_BOX>
sleep 5
systemctl show -p ActiveState -p SubState -p NRestarts sing-box.service
after=$(sha256sum /usr/local/bin/sing-box)
```

#### 期望

- 更新命令失败；
- 自动应用 rollback point；
- `before` 与 `after` 相同；
- `sing-box.service` 恢复为 `active/running`；
- Subscription route 不受影响。

#### 实际

- 更新命令返回成功；
- 故障候选替换了 `/usr/local/bin/sing-box`；
- rollback point 已创建，但没有被应用；
- `before` 与 `after` 不同；
- 延迟 5 秒后：

  ```text
  ActiveState=activating
  SubState=auto-restart
  NRestarts=21
  ```

- journal 持续出现：

  ```text
  sing-box.service: Main process exited, code=exited, status=1/FAILURE
  sing-box.service: Failed with result 'exit-code'.
  sing-box.service: Scheduled restart job...
  ```

结果：**P0 失败**。

#### 代码关联

`install_candidate_sing_box` 在替换二进制后调用 `restart_sing_box_service`。后者只执行：

```text
systemctl restart sing-box.service
systemctl is-active --quiet sing-box.service
```

`Type=simple` 服务在进程刚启动时会短暂变成 active；`Restart=on-failure` 又会在失败后自动重启，
所以单次 `is-active` 容易落在短暂 active 窗口并错误提交更新。

仓库已经为完整安装和双服务重启实现 `wait_for_stable_activation`：连续三次探测、间隔
1100 ms；独立 sing-box 更新路径没有复用它。

相关位置：

- `src/update.rs`：`install_candidate_sing_box`；
- `src/lifecycle.rs`：`restart_sing_box_service`；
- `src/lifecycle.rs`：`wait_for_stable_activation`。

### 6.14 故障恢复

发现重启循环后立即执行：

1. 停止 `sing-box.service`；
2. 从测试前保存的已验证 sing-box 1.14.1 副本恢复 `/usr/local/bin/sing-box`；
3. `systemctl reset-failed sing-box.service`；
4. 重新启动服务；
5. 等待并复查服务状态与 Subscription route。

恢复结果：

```text
sing-box version 1.14.1
NRestarts=0
ActiveState=active
SubState=running
subscription_after_manual_recovery=PASS
```

### 6.15 整机重启

执行 VPS reboot，启动完成后重新通过 PowerShell 7 SSH 检查：

| 检查 | 结果 |
| --- | --- |
| `sbctl.service` | active |
| `sing-box.service` | active |
| `sbctl-http.socket` | active |
| `sbctl-accounting-reset.timer` | active |
| sbctl 运行用户 | `sbctl` |
| sing-box 运行用户 | `sing-box` |
| URI Subscription route | 通过 |
| sing-box 配置检查 | 通过 |

结果：**通过**。

## 7. 缺陷分级

### P0：独立 sing-box 更新错误提交快速崩溃候选

影响：

- 管理员看到“更新成功”，但数据面随后进入无限重启；
- 所有 Managed protocol 中断；
- rollback point 虽然存在，但需要管理员手动识别并恢复；
- 直接违反发布所要求的失败自动回滚。

修复要求：

1. `restart_sing_box_service` 必须使用与 install/update 一致的稳定观察窗口；
2. 观察期间任一次发现 `inactive`、`failed` 或 `activating/auto-restart` 均应触发恢复；
3. 恢复旧二进制后也必须运行同样的稳定观察；
4. 添加真实或等价 systemd 回归测试：候选 `check` 返回 0，`run` 立即返回 1；
5. 测试必须断言更新命令失败、二进制摘要恢复、服务稳定 active、Subscription route 恢复。

### P1：Direct service unit 中的 `Sockets=` 位于错误 section

影响：

- Ubuntu 22.04 systemd 忽略该配置并在每次 unit 解析时记录警告；
- 当前 socket 仍因 `.socket` unit 的 `Service=` 工作，但 unit 内容与意图不一致；
- `systemd-analyze verify` 无法做到无警告。

修复要求：

- 删除不需要的 `Sockets=`，或将它放到 systemd 支持的正确 `[Service]` section；
- 增加 unit 语法测试，并要求 sbctl 自有 unit 不产生 warning。

### 发布配置阻断：生产签名未配置

影响：

- 普通生产构建无法验证任何 Authenticated release manifest；
- Release workflow 即使完成普通编译，也不能形成可安全更新的发布资产。

修复要求：

- 配置 repository variable `SBCTL_RELEASE_PUBLIC_KEY_HEX`；
- 创建 `release` Environment；
- 配置 Environment secret `SBCTL_SIGNING_SEED`；
- 重新运行 Release workflow，并在本报告相同的 Production host 上验证 signed manifest。

## 8. 最终 VPS 状态

测试结束时 VPS 保留可用候选部署，而不是故障桩：

- 当前 `sbctl`：提交 `a5fe586` 的源码候选；
- sing-box：1.14.1；
- 四个 systemd 单元：active；
- 五种 Managed protocol：监听正常；
- Subscription route：正常；
- 整机重启恢复：通过；
- 测试前 root-only 快照：
  `/root/pretest-state-20260922T155422Z.tar.gz`；
- update rollback point 保留在 `/var/backups/sbctl/rollback/`，权限为 root-only；
- 临时 SSH 公钥、候选上传副本、故障桩、客户端配置和测试脚本：已删除。

## 9. 发布决策

根据 [发布就绪与 Ubuntu VPS 测试计划](release-readiness-and-vps-test-plan.md) 和
ADR-0014，本次结果不满足以下 Release gate：

- 更新失败自动回滚；
- Authenticated release manifest；
- systemd unit 无无效配置；
- 当前 commit 的正式 Release workflow 证据。

因此：

```text
Release decision: NO-GO
```

不得为当前源码创建新的公开发布 tag，也不得把本次本地候选作为 release asset 上传。

## 10. 重新验收条件

完成以下事项后才能重新执行本报告：

1. 修复独立 sing-box 更新的稳定健康检查和自动回滚；
2. 增加快速崩溃候选的自动化回归；
3. 修正 `sbctl.service` 的 `Sockets=` unit 配置；
4. 修正 `tests/acceptance/verify.sh` 中旧 rollback 路径断言；
5. 配置生产发布公钥和签名 secret；
6. 提升 Cargo package version，确保 tag、manifest 和二进制版本一致；
7. 对待发布 commit 运行完整 CI；
8. 运行 Debian 12、Ubuntu 22.04、Ubuntu 24.04 acceptance；
9. 由 Release workflow 生成同一 tag 的 amd64/arm64 二进制、manifest、签名和
   `install.sh`；
10. 在干净 VPS 上重新执行安装、五协议连接、故障更新回滚和整机重启测试。

重新验收至少必须证明：

```text
坏候选 check=0、run=1
→ 更新命令失败
→ 自动恢复旧二进制
→ sing-box.service 稳定 active/running
→ NRestarts 不增长
→ Subscription route 和五种 Managed protocol 恢复
```
