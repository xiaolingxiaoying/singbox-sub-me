# 03: VPS traffic 计量与月度周期

**What to build:** 管理员可通过 `traffic` 和 `status` 查看一个已配置网络接口的 VPS traffic、Monthly traffic limit、当前 Accounting period 与下次重置时间。

**Blocked by:** 02: 持久配置与原子状态操作.

**Status:** resolved

- [ ] 安装配置可探测默认路由接口并允许管理员覆盖。
- [ ] RX/TX 增量、boot ID 变化、计数器回退、缺失状态和服务重启都产生正确累计值。
- [ ] Natural-month reset 和 Anchored-month reset（包括短月月末）按选择的 Accounting timezone 计算正确。
