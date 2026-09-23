# `concurrent_reads_observe_only_complete_state_versions` 在 Windows 高负载下偶发失败

Status: needs-triage
Type: bug
Found: 2026-09-23，Phase 2 PR(c) 的门禁运行中。

## 现象

`cargo test -p sbctl --lib` 173 项里偶发 1 红，且**只在全量套件的并发压力下**出现：

```text
test config::tests::concurrent_reads_observe_only_complete_state_versions ... FAILED
panicked at src\config.rs:1750:50:
a complete state remains readable: Os { code: 5, kind: PermissionDenied, message: "拒绝访问。" }
```

单点重跑 10/10 通过；随后全量重跑 3/3 通过（同一棵树、同一份代码）。
失败那一次，宿主同时在跑 WSL 的 rsync + cargo 构建。

## 为什么值得管

不是本次改动引入的：`src/config.rs` 未被触碰。但它是**发布门禁**里的一条测试，
症状（PermissionDenied）看起来像生产代码的文件替换有问题，容易被误判成刚提交的改动。
ADR-0003 的"可验证且可回滚"依赖这类测试可信。

## 待查的方向

1. 测试自身的构造：读写并发 + `fs::rename` 覆盖正在被读的文件，在 Windows 上
   共享模式由 Rust std 决定，读者持有句柄时 rename 会以 `ERROR_ACCESS_DENIED` 失败——
   这是 Windows 语义，不是产品缺陷；测试应重试或串行化到 `--test-threads=1`。
2. 若是生产路径：`write_state` 的原子替换在并发读下真的会失败，那才是缺陷。
   判据是把失败点从 `src/config.rs:1750` 的读侧挪到写侧再复现一次。

## 建议动作

先按 1 处理（给该测试加有界重试并断言最终一致），把 2 作为"如果重试后仍然失败才升级"的分支。
不要直接删测试或加 `#[ignore]`。
