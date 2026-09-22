# 从未在真实 Windows 上运行过 sbgui

Status: ready-for-human
Type: task

## 现象

本仓库的 GUI 目标平台是 Windows（GPUI 的 Windows 后端），但到目前为止所有视觉与行为证据都来自 Docker + Xvfb + Mesa 软渲染的 Linux 版 sbgui。没有任何一次在真实桌面合成器下打开过窗口。

## 影响

无头渲染能证明布局、文案和逻辑，证明不了平台：字体回退与 DPI 缩放、窗口拖拽与关闭请求、系统代理的实际写入、TUN 权限、以及 `kill_on_drop` 之外的孤儿内核回收，都只在真实平台上才会暴露。也就是说"sbgui 可以用了"这句话目前不成立。

## 为什么交给人

需要维护者授权开机并提供 `Windows 11 x64` 虚拟机的磁盘加密口令（此前给过的三个口令都不是口令本身）。脚本已在 `.scratch/win11-vm/`。另外宿主上 `sbtui` 的数据目录里有一个真实内核，不要在那里启动被测程序；`sbgui` 的数据目录是惰性的。
