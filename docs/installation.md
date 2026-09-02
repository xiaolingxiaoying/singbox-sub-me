# 安装与 sing-box 生命周期

## 首次安装 sbctl

发布 manifest 必须同时包含 `sbctl.url`、`sbctl.sha256`、`sing_box.url` 和
`sing_box.sha256`。GitHub Release 会生成 `manifest-amd64.json` 和
`manifest-arm64.json`。在 Debian/Ubuntu VPS 上，`{arch}` 会自动替换为系统架构：

> 当前仓库是私有仓库。私有 GitHub Release 不能被 VPS 匿名 `curl` 下载；要使用下面
> 的无凭证安装方式，需要将仓库/Release 设为公开，或把 release 文件同步到公开下载站。

```bash
curl -fsSL https://发布地址/install.sh | SBCTL_MANIFEST_URL=https://发布地址/manifest-{arch}.json bash -s -- \
  --subscription-host sub.example.com \
  --reality-decoy-sni www.cloudflare.com
```

bootstrap 脚本只安装并校验 sbctl；它不会接管已有的 sing-box 部署，也不会修改防火墙。

## 独立管理 sing-box

```bash
# 下载并校验 sing-box
sbctl sing-box download --manifest /path/to/manifest.json --output /tmp/sing-box

# 安装已校验的 sing-box
sbctl sing-box install --manifest /path/to/manifest.json --artifact /tmp/sing-box

# 使用本地文件更新；不提供 --artifact 时按 manifest.url 自动下载
sbctl sing-box update --manifest /path/to/manifest.json

# 仅移除 sbctl 标记的 sing-box 服务和二进制，保留配置及订阅数据
sbctl sing-box remove
```

`sing-box update` 会先用候选二进制执行 `sing-box check`，再替换二进制并检查
systemd 服务；失败时恢复 rollback 目录中的旧二进制。完整的 `sbctl update` 仍然
保留同时升级控制面和数据面的能力。
