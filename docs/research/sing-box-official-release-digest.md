# sing-box 官方 Release 工件 SHA-256 校验调查

- 调查日期：2026-09-25
- 范围：`sbctl` 无签名 manifest 的官方 sing-box 下载/更新路径（S5/S6）。
- 来源限定：GitHub REST API 官方文档及 SagerNet/sing-box 官方 Release/API。

## 结论

1. **可无 token 读取公开 Release 元数据与资产。** `GET /repos/{owner}/{repo}/releases/latest` 返回最新已发布的完整版本；“latest”排除 draft 和 prerelease。GitHub 明确说明，公开资源可不经认证读取。[Get the latest release](https://docs.github.com/en/rest/releases/releases#get-the-latest-release) [List releases](https://docs.github.com/en/rest/releases/releases#list-releases)
2. Release 资产对象提供 `digest`，格式为 `sha256:<64 位十六进制摘要>`。GitHub API 文档的 release/asset 响应示例均含此字段；**文档示例并不构成每个历史或未来资产都一定有摘要的保证**，实现必须容忍字段缺失、`null` 或格式异常。[Release API 响应示例](https://docs.github.com/en/rest/releases/releases#list-releases) [Release asset API](https://docs.github.com/en/rest/releases/assets#list-release-assets)
3. 对官方最新稳定版 `v1.14.2` 的现场 API 核实中，Linux amd64 资产 `sing-box-1.14.2-linux-amd64.tar.gz` 的 digest 为 `sha256:a684484d7477d1437282ee411f4d131d0340aaad60a7868841ebd5d87dd8a0c6`。该 Release 资产列表里没有单独的 `sha256sum`/checksum 资产；摘要来自 GitHub 的 API `digest` 字段。[v1.14.2 官方 Release](https://github.com/SagerNet/sing-box/releases/tag/v1.14.2) [latest Release API](https://api.github.com/repos/SagerNet/sing-box/releases/latest)
4. `digest` 与下载文件本地计算出的 SHA-256 相等，只能证明下载字节与 GitHub 为该资产报告的摘要一致；它不是作者签名，也不独立证明发布者身份或构建来源。当前直接下载路径仍应在 ADR/README 明确 HTTPS + GitHub 元数据摘要的信任边界。[GitHub release asset API](https://docs.github.com/en/rest/releases/assets)

## 推荐的取用与校验流程

1. 无认证 GET `https://api.github.com/repos/SagerNet/sing-box/releases/latest`，按完整资产文件名（例如 `sing-box-<version>-linux-amd64.tar.gz`）精确选择目标架构；从所选 release 的同一资产对象取 `browser_download_url`、`name`、`size`、`digest`。公开资源无需 `Authorization` 头。[Get the latest release](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)
2. 要求 `digest` 存在且严格符合 `sha256:` 加 64 个十六进制字符；使用 HTTPS 下载选中的 `browser_download_url`，计算本地 SHA-256 并与去掉前缀的摘要做常量字节比较。可用 API 资产下载端点加 `Accept: application/octet-stream`；客户端必须接受 API 返回 `200` 或 `302`。[Get a release asset](https://docs.github.com/en/rest/releases/assets#get-a-release-asset)
3. GitHub `List release assets` 的 `per_page` 默认 30、上限 100。若改用单独的 assets 列表端点，必须跟随分页；优先用 latest release 对象中已嵌入的 `assets[]`，并依然按资产名精确匹配。[List release assets](https://docs.github.com/en/rest/releases/assets#list-release-assets)
4. **摘要缺失、为 `null` 或格式错误时**：不得把它解释为已校验。S5/S6 工单当前建议为该官方路径明确警告并继续既有 `sing-box version` 自检；若调用方要求严格完整性保证，则应拒绝下载/安装并要求签名 manifest 路径。不能静默回退到只依赖 `sing-box version`，因为版本自检不校验下载字节。

无 token 请求示例（API 元数据）：

```sh
curl --fail --silent --show-error \
  -H 'Accept: application/vnd.github+json' \
  -H 'X-GitHub-Api-Version: 2022-11-28' \
  https://api.github.com/repos/SagerNet/sing-box/releases/latest
```

> 现场值仅作为本次调查样本；实现应每次从 API 响应动态读取版本、资产 URL 与摘要，不要钉死上面的版本或 hash。
