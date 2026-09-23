# 三环境验证流水线（WSL / Docker / VMware Windows）

Status: ready-for-agent

## 目标

把验证流程做成可重复、可审计的脚本，保证测试不影响 Windows 宿主机。

## 约束（来自用户）

- WSL 构建：先把文件移动到 WSL 文件系统再构建，减少 IO、提升速度。
- VMware Windows 11 客户机：用户名 `Test`，密码只经环境变量传入，不写入仓库。
- WSL Ubuntu：sudo 密码只经环境变量或 `wsl -u root` 传入，不写入仓库。
- Docker：Docker Desktop，需 `--privileged` + cgroup 挂载。

## 工单

- `issues/01-wsl-gate-script.md`
- `issues/02-docker-acceptance-extension.md`
- `issues/03-windows-vm-pipeline.md`

## 验收

- 三个脚本都能一条命令跑通，并在 `.scratch/` 下产出脱敏日志与截图证据。
- 任一脚本执行完后，Windows 宿主机的服务、注册表、网络配置与执行前一致
  （用 `sc query`/注册表快照/`Get-NetIPConfiguration` 对比验证）。
