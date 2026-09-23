# 发布流水线与真机验收

Status: resolved
Type: task
Blocked by: 01, 02, 03, 04, 05, 06, 07, 08

## 交付范围

- `.github/workflows/release.yml`（或新增 workflow）：sbtui 在 windows/amd64、linux/amd64、macOS（arm64）构建并发布（产物名 `sbtui-<os>-<arch>`，与 sbctl 同 tag 或独立 tag，择一并记录 ADR/说明）。
- CI：workspace 级 fmt/clippy/test 覆盖 sbtui。
- Windows 真机验收清单（用户执行）：导入生产订阅 → 测延迟 → 切节点 → 系统代理 → TUN → 速率/连接/日志。

## 验收标准

- [ ] 推 tag 后 Release 含三平台 sbtui 产物且可运行。
- [ ] Windows 真机按清单全绿。
- [ ] `cargo fmt/clippy/test` 全 workspace 通过。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：release.yml 增加 windows/macos/linux 三平台 sbtui 构建 job 并随 release 上传；首次打 tag 时验证。
