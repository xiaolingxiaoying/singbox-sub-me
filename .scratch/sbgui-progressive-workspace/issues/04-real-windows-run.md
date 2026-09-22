# sbgui 只在真实 Windows 上跑过一轮，且不完整

Status: ready-for-human
Type: task

## 现象

本仓库的 GUI 目标平台是 Windows（GPUI 的 Windows 后端）。2026-09-22 已经在加密的 `Windows 11 x64` 虚机里真机跑通过一次：`vmrun -vp <口令> start … gui` → guestops 投 `sbgui.exe` → `runProgramInGuest -interactive` 跑 `.scratch/win11-vm/shot-guest.ps1` → 取回 PNG。图在 `.scratch/win11-vm/shots-win/`（7 张，访客内 `CopyFromScreen` 抓的 DWM 合成画面，裁剪用 `DwmGetWindowAttribute`）。脚本与日志在同目录。

但那一轮的覆盖面不够，`.scratch/win11-vm/manifest2.txt` 记录的是：dashboard / subscriptions / rules / logs / settings 五页 OK，**proxies 与 connections 两页 `EXITED` 且 stderr 为空**，About 页当时还没进脚本。两页退出的原因高度可疑是访客只有 4 GB，连续起 7 个 sbgui 时可用内存掉到 800–1300 MB，内核启动即退——单独重跑又正常。

## 影响

无头 Linux 渲染能证明布局、文案和逻辑，证明不了平台；真机这一轮证明了窗口能在 DWM 下起来并渲染，仍然没证明：字体回退与 DPI 缩放的完整覆盖、窗口拖拽与关闭请求、系统代理的实际写入、TUN 权限、以及 `kill_on_drop` 之外的孤儿内核回收。也就是说"sbgui 可以用了"这句话目前还不成立——不是因为一次都没跑过，而是因为跑过的那一次缺了三页，且缺的平台语义没被测到。

## 为什么交给人

需要维护者授权开机并提供 `Windows 11 x64` 虚机的磁盘加密口令（口令不写进任何记录，需要时向维护者取）。宿主上 `sbtui` 的数据目录里有一个真实内核，不要在那里启动被测程序；`sbgui` 的数据目录是惰性的。脚本已在 `.scratch/win11-vm/`。

## 下一步

1. 给 `shot-guest.ps1` 补上 About 页，并把每页之间加 sleep + 启动失败重试，避开访客内存见底的那条路径。
2. 一次跑满 8 页、`manifest` 里不允许出现 `EXITED`，再谈平台语义（系统代理写没写进注册表、退出后有没有残留 sing-box 进程）。
