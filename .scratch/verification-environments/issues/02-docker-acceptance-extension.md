# Docker 验收扩展（含 GUI 截图门禁）

Status: ready-for-agent
Type: task
Blocked by: sbctl-v0.2/issues/02

## 动作

1. 服务端验收：按 `sbctl-v0.2/issues/05` 扩展 `tests/acceptance/verify.sh`（回滚故障注入、
   unit 语法、IP fallback 五协议）。
2. 把 `tests/acceptance/run.sh` 的工件挂载改为先复制进 WSL 本地目录再绑定，减少 `/mnt/c` IO。
3. 新增 `scripts/dev/docker-gui-smoke.sh`：对当前工作树跑 `scripts/sbgui-shot/shot.sh` 的
   8 页 × 2 尺寸 × zh/en，输出到 `.scratch/gui-smoke/<timestamp>/`，空图或失败即非零退出。
4. CI 增加一个可选 job（或本地门禁）运行 docker-gui-smoke；至少先在本地作为发布门禁。

## 验收

- 一条命令完成三发行版验收 + GUI 截图冒烟。
- 截图文件名包含页面与尺寸，manifest 记录每张图的行数/状态。
- 证据目录可用于发布说明。
