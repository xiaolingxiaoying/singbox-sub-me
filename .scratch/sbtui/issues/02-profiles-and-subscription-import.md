# 订阅档案与导入

Status: resolved
Type: task
Blocked by: 01

## 交付范围

- 档案模型（profiles.toml）：名称、订阅 URL、更新时间、缓存文件、激活态；增删改/切换激活。
- URL 归一化：粘贴任意 sbctl 订阅链接（含 qr/index 链接剔除）→ 抽取凭据并改写为 `/sub/<cred>/sing-box-full.json`；非 sbctl 的 sing-box JSON 直链/本地文件也支持。
- 导入方式：设置页内输入框粘贴；本地文件选择（手动输入路径）；URI 列表导入（转 outbounds）。
- 下载订阅（reqwest, rustls，超时与镜像设置），缓存到 `cache/`，失败保留上次缓存并提示。
- 定时自动更新（可配置间隔，默认 0=关闭）。

## 验收标准

- [ ] 粘贴 `/sub/<cred>/clash.yaml`、`/sub/<cred>/uri.txt`、`/sub/<cred>/sing-box-full.json` 均归一化到 sing-box-full。
- [ ] 订阅下载成功写入缓存并显示节点数；断网时回退缓存。
- [ ] URI 列表能转为等价 outbounds（vless/vmess/hy2/tuic/anytls）。
- [ ] `cargo fmt/clippy/test` 通过（归一化/URI 转换单测）。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：URL 归一化、订阅下载（镜像前缀）、sing-box JSON/Base64 URI 解析转换、档案增删激活已实现（单测覆盖）。
- 2026-09-14（收口）：新增 `f` 本地文件导入、`e` 修改选中档案链接、`Delete` 二次确认删除；`update_subscription` 在下载失败且存在缓存时显式回退并提示，不再报错中断。
