# workspace 改造与 sbtui 骨架

Status: resolved
Type: task

## 目标

建立 `crates/sbtui` 与 ratatui 骨架：Tab 框架、底部状态栏、设置/数据目录、空页面可导航。

## 交付范围

- 根 `Cargo.toml` 增加 `[workspace]`（members = [".", "crates/sbtui"] 或等价写法），确认 `cargo` 全套命令、CI、release.yml 路径不受破坏。
- `crates/sbtui`：ratatui + crossterm + tokio；主循环、5 Tab 骨架（仪表盘/代理/连接/日志/设置，先空）、底部快捷键栏、退出确认。
- 数据目录初始化：`profiles.toml`/`settings.toml`（serde TOML，缺省生成），跨平台目录解析。
- README-sbtui（中文）：构建、运行、快捷键表。

## 验收标准

- [ ] `cargo run -p sbtui` 出现 5 Tab 可切换的界面，q 退出干净（终端状态恢复）。
- [ ] 首次运行自动创建数据目录与缺省 settings.toml。
- [ ] `cargo fmt/clippy/test` 全 workspace 通过。

## 相关规格

`.scratch/sbtui/spec.md`

## Comments

- 2026-09-14：workspace + crates/sbtui + 5 Tab 骨架 + 数据目录 + settings/profiles 已实现；Windows 冒烟（--print-dir、--help）通过。
