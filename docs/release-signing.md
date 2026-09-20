# 生产发布签名配置与旧密钥迁移

仓库中的 `scripts/dev-signing-key.hex` 是公开测试数据。旧安装器信任它对应的公钥，无法证明发布者身份。新代码的普通构建不再信任该密钥；未配置生产公钥时，manifest 安装和更新会明确失败，不能回退到开发密钥。

## 配置一次新的生产密钥

1. 在可信维护设备上运行 `cargo run -p sbctl -- release keygen --output <仓库外的私有目录>`。命令输出公钥，私钥写入该目录；不要把私钥提交到 Git、聊天、日志或发布工件。
2. 在 GitHub 仓库 Actions Variables 中设置 `SBCTL_RELEASE_PUBLIC_KEY_HEX`，值为命令输出的 64 位十六进制公钥。
3. 创建 GitHub Environment `release`，在其 Secrets 中设置 `SBCTL_SIGNING_SEED`，值为新私钥文件的十六进制内容。建议限制可发布标签及允许使用该环境的维护者。
4. 推送发布标签。构建 job 将公钥编译进普通 `sbctl`，package job 用私钥签名并由该二进制验签。公私钥不匹配、缺少密钥或仍使用公开开发密钥时，流程必须在上传发布工件前失败。

`scripts/prepare-installer.py` 将同一个生产公钥写入发布工件 `install.sh`。仓库里的 `scripts/install.sh` 是未配置的模板，直接执行会失败。README 的安装入口已改为 GitHub Release 的 `install.sh` 工件。

已有旧版本不会自动获得新的信任根；旧 updater 无法验证新 manifest。迁移时需要通过可信渠道取得带新公钥的 `sbctl` 二进制，独立核对发布来源及公钥后手动替换，再使用新 updater。不要把旧开发公钥作为兼容备用公钥保留。仅删除开发私钥文件或重写仓库历史都不能修复已经发布的旧二进制。

## 本地构建

生产构建在编译时提供 `SBCTL_RELEASE_PUBLIC_KEY_HEX`，例如：

```bash
SBCTL_RELEASE_PUBLIC_KEY_HEX='<新的公开验证密钥>' cargo build --release -p sbctl --no-default-features
```

它不是运行时环境变量；已经构建的二进制不能通过修改运行环境改变信任根。没有该变量也可以构建以开发非发布功能，但 manifest 验证会失败。

## 测试与验收隔离

```bash
cargo test --workspace --features sbctl/test-signing
cargo test -p sbctl --no-default-features --test release_trust
```

`test-signing` 是显式测试开关，供 CLI 和签名回滚 fixture 使用。启用该特性的二进制不得发布。普通 `cargo test` 不运行需要该特性的 `cli` 目标；CI 显式运行以上两条命令，分别验证签名业务流程和生产构建拒绝公开开发签名的边界。

systemd 验收分别传入 `SBCTL_ARTIFACT`（生产构建）和 `SBCTL_TEST_ARTIFACT`（独立目录中的测试签名构建）。公开测试签名的回滚场景使用后者；真实服务安装、非 root 启动和卸载使用前者。测试安装器只在验收容器的临时目录注入测试公钥，不会进入发布工件。

本次修复没有生成或配置生产私钥，也没有触发发布；维护者完成上面的密钥配置后才能发布。
