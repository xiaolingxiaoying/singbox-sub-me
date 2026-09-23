# 重新验收与发布 v0.2

Status: ready-for-agent
Type: task
Blocked by: 02, 03, 04, 05

## 动作

1. 按 `docs/release-readiness-and-vps-test-plan.md` 执行阶段 1–5。
2. 确认候选版本号、tag、manifest、二进制版本一致（0.2.0）。
3. Release workflow 产出：sbctl amd64/arm64、sing-box、sbtui/sbgui Windows、`install.sh`、
   签名 manifest。
4. 干净 Ubuntu VPS 按 `docs/ubuntu-22-vps-e2e-report-2026-09-22.md` §10 复验：
   安装、Direct HTTPS、五协议真实连接、订阅矩阵与 `subscription-userinfo`、
   坏候选故障注入（失败 → 自动回滚 → 服务稳定 → 订阅恢复）、整机重启。
5. 更新 `docs/release-readiness-and-vps-test-plan.md` 执行记录与 No-Go 报告状态。
6. 测试结束清理 VPS 凭据与临时密钥。

## 验收

```text
坏候选 check=0、run=1
→ 更新命令失败
→ 自动恢复旧二进制
→ sing-box.service 稳定 active/running
→ NRestarts 不增长
→ Subscription route 和五种 Managed protocol 恢复
```

- 报告从 NO-GO 更新为 GO，且每项都有脱敏证据。
