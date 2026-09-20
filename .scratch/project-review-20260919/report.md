# 项目审查记录 · 2026-09-19

## 范围与结论

基于当前工作区（HEAD `e60566a`，包含既有未提交修改）审查服务端 `sbctl`、共享客户端 `client-core`、`sbtui`、`sbgui`、安装与发布脚本、CI、领域文档与部分规格。未修改业务代码，未启动真实代理、修改系统代理或操作生产环境。

项目已具备较完整的产品结构：服务端有配置事务、证书验证、流量账期、订阅矩阵、服务管理和回滚；两个客户端已共享控制层。主要短板集中于发布信任根、客户端运行实例隔离和状态转换。当前工作区尚不宜作为稳定发布候选：严格 Clippy 未通过，且存在下列高优先级问题。

这是一轮全项目结构梳理和重点路径代码审查，不代表逐行穷尽审计或生产验收。

## 发现

### 1. [P1] 生产验证公钥对应的签名私钥已在仓库公开

- 位置：`src/release.rs:26`、`src/release.rs:444`、`scripts/generate-manifest.sh:30`、`.github/workflows/release.yml:153`。
- 发布脚本默认使用已被 Git 跟踪的 `scripts/dev-signing-key.hex`；release workflow 未注入独立生产签名密钥。Rust 验证器和 bootstrap 安装器信任该开发私钥对应的公钥，单元测试也验证了这一对应关系。
- 后果：能替换 manifest 来源或诱导用户指定恶意 manifest 的人，可以生成被接受的任意工件签名。签名无法再证明发布者身份；这不意味着攻击者仅靠该私钥就能改写 GitHub Release。
- 建议：生成独立生产密钥，通过受保护的 CI secret 签名，并轮换 Rust 和安装器的信任公钥。开发密钥只能用于显式测试模式；仅删除当前私钥文件不足以恢复信任。
- 与 ADR-0010 的发布者身份认证要求冲突。

### 2. [P1] 以固定端口的响应判断本次子进程启动成功，会误连其他代理实例

- 位置：`crates/client-core/src/controller.rs:421`；控制地址固定于 `crates/client-core/src/clash_api.rs:11`。
- 触发：另一 sing-box/Clash 实例已经监听 `127.0.0.1:9090`，随后在本客户端启动内核。
- 启动循环只调用 `self.api.alive()`，没有验证该 API 属于刚启动的子进程，也未在确认成功前检查该子进程是否退出。新进程因端口冲突退出时，已有实例仍可让检查成功。
- 后果：界面显示启动成功，切节点、模式、关闭连接等指令可能发往其他实例；可能同时开启指向错误端口的系统代理。之后的崩溃监测只能发现新进程退出，无法撤销已经发给旧实例的操作。
- 建议：启动前拒绝控制端口冲突，并采用实例独有控制地址/认证秘密，结合子进程存活状态验证启动。TUI、GUI 分开数据目录不能隔离同一个监听端口。
- 本项为调用链静态确认，未占用用户实际 9090 端口进行动态试验。

### 3. [P1] 不同订阅档案会覆盖或删除同一份缓存

- 位置：`crates/client-core/src/settings.rs:145`、`crates/client-core/src/controller.rs:675`、`crates/client-core/src/controller.rs:739`。
- `profile_cache_path` 将空格和多种标点统一替换为 `_`，但 `unique_profile_name` 只对原始名称去重。
- 已执行公共函数复现：`a b` 和 `a_b` 都映射到 `cache/a_b.json`。两个名称均可从本地 JSON 文件名合法导入。
- 后果：导入第二档案覆盖第一档案的节点配置；删除其中一个档案会删除另一个仍使用的缓存。名称 `active-config` 还直接映射到内部运行配置 `cache/active-config.json`。
- 建议：使用稳定、唯一的档案 ID 作为缓存文件名，显示名称独立保存；内部运行配置使用独立目录。迁移时检查已有文件冲突。

### 4. [P2] 旧版订阅回退在实际导入路径中不会执行

- 位置：`crates/client-core/src/controller.rs:580`、`crates/client-core/src/subscription.rs:39`。
- 导入先将 `/sub/<credential>/sing-box.json` 归一化为 `sing-box-full.json`；下载失败时，却将已经归一化的 URL 传给只接受 `sing-box.json` 的回退函数。
- 已执行公共函数组合复现：旧链接经过 `normalize_url` 后，`bare_sing_box_fallback_url` 返回 `None`。
- 后果：仅提供旧版精简端点的服务器无法导入，即使解析器已经支持旧格式。单独测试回退函数不能覆盖调用链问题。
- 建议：保留原始来源用于兼容判断，或显式从可信的 sbctl full-profile 路由构造旧端点；增加“full 404、bare 200”的本地 HTTP 集成测试。

### 5. [P2] 崩溃重启上限和指数退避会被短暂启动成功反复清零

- 位置：`crates/client-core/src/controller.rs:423`、`crates/client-core/src/controller.rs:904`。
- 本次未提交修改增加了最多 5 次自动重启限制，但每次 `/version` 成功即把 `restart_attempts` 清零。
- 触发：内核启动后短暂响应 API，随后再次崩溃。每轮都重新从第 1 次、2 秒重试开始，无法触发第 5 次停止条件。
- 后果：持续崩溃的内核无限循环重启，系统代理反复启停。现有测试只验证退避数值函数，没有覆盖生命周期状态转换。
- 建议：达到一段稳定运行时间后再清零，或在时间窗口内累计崩溃次数；补充“启动成功后很快崩溃”的回归场景。

### 6. [P2] UI 无法观察长操作的 busy/starting 状态，命令处理同时被阻塞

- 位置：`crates/client-core/src/controller.rs:206`、`crates/client-core/src/controller.rs:210`、`crates/client-core/src/controller.rs:226`。
- 引擎设置 `busy` 后直接等待完整操作，完成后先清空 `busy` 再发布 snapshot；`starting` 也在启动函数返回前清空。两个 UI 均通过 snapshot 轮询读取状态。
- 后果：下载、启动、测速期间界面仍显示旧状态，依赖 `starting` 的按钮保护不生效。下载最长可等待 600 秒，期间同一循环无法处理 StopCore 等新命令，也无法继续内核监测；事件队列中的 OperationStarted 不能弥补轮询 UI 的快照缺失。
- 建议：操作开始时先发布快照；将耗时操作作为可取消任务执行，让控制循环继续处理停止、退出和运行状态更新。

## 验证结果

| 检查 | 本轮结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo test -p sbctl --lib` | 157 通过 |
| `cargo test -p client-core` | 32 通过 |
| `cargo test -p sbtui -p sbgui -- --quiet` | TUI 11、GUI 3 通过，其余目标无测试 |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 失败：`controller.rs:955` 的 `push_log(&strip_ansi(line))` 触发 `needless_borrows_for_generic_args` |
| 公共函数最小复现 | 缓存名称冲突、内部运行配置名称冲突、旧订阅回退缺失均复现 |

合计 203 项已有测试通过。Clippy 已在第一处错误停止，因此不能据此认定其他代码无 lint 问题。复现源码保存在同目录 `probe.rs`，链接本次构建的 client-core 库执行；首次直接 rustc 链接缺少 Windows 原生库路径，补齐路径后成功。

未执行：Linux CLI 完整集成测试、真实 systemd/Docker 验收、真实 sing-box/mihomo 多版本配置验收、GUI 原生点击与视觉验收、生产部署测试。本轮没有检查远程 CI 的实际运行结果，CI/发布评价来自仓库配置。

## 工程现状与后续顺序

- 结构优点：领域定义和 ADR 较完整；服务端使用统一节点模型、受锁保护的事务写入和失败回滚；共享客户端控制层减少了双 UI 行为分叉。
- 验证基础：CI 配有 Linux、Windows、macOS 检查和真实内核 profile 验证；release 有真实 systemd 容器验收步骤。但 release workflow 未依赖普通 CI 测试结果，GUI 也尚未进入发布工件矩阵。
- 维护成本：GUI 主文件 4202 行，TUI 主文件 2228 行，控制器 1116 行。宜优先抽离状态转换、档案存储和下载任务；服务端大文件还包含大量内联测试，不能仅凭总行数认定需拆分。
- 跟踪准确性：部分规格状态仍是 ready-for-agent/needs-triage，而实现已存在；GUI 改版规格为 claimed。应在验收后同步任务状态，不能把文档状态直接当作功能缺失证据。
- 建议顺序：先轮换发布信任根并恢复 CI 通过；再解决实例识别和缓存隔离；随后处理回退、崩溃状态机与可取消操作；最后补齐系统级验收、发布矩阵和模块拆分。
