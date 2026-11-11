---
id: s6
title: "structure_gen 日志捕获断言——tracing callsite interest 缓存修复"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# s6: 日志捕获测试的 callsite interest 缓存免疫

**What to build:** `learning_graph::structure_gen::tests::
unknown_top_key_passes_in_one_attempt_with_key_dropped` 在全套并行下偶发
挂在日志捕获断言（:669），根因是 tracing 的 **callsite interest 进程级
缓存**：该 `info!` 点首次注册时若当前默认订阅者恰好是别的过滤更严用例，
interest 缓存为 Never，本用例自己的订阅者也收不到。在 `set_default` 后
调用 `tracing::callsite::rebuild_interest_cache()`，把本用例的订阅者
重新登记为当前生效面。

## 法证证据（2026-11-11 验证，实码复核）

- 失败断言 `structure_gen.rs:669`（`captured.contains("structure
  generation warnings (non-rejecting)")`）：失败趟里生成行为断言
  （过门 / 删键 / `calls()==2`）**全过**，仅日志缓冲缺预期行——非行为
  回归。
- 频率：全套并行 1/2 命中（2026-11-11 合并树两趟连跑），单跑必绿；
  合并前一趟绿（低频偶发，样本少）。
- 归属：crate 与合并前逐字节相同，非合并引入。
- 机制缺口：:610-613 注释只论证了「线程局部默认订阅跨 .await 存活
  （current_thread 全程同线程轮询）」，未覆盖 interest 缓存这个
  **进程级**共享面——tracing 在 callsite 首次注册时按当时的默认分发器
  计算 interest 并全局缓存，后续换订阅者不重算。
- 该日志点本体：structure_gen.rs:321 `tracing::info!(warnings = ?warnings,
  ..., "structure generation warnings (non-rejecting)")`，emit 条件
  `!warnings.is_empty()`；生成行为正确时 warnings 非空（删键 findings
  在内），asserts 657-665 已证。

## 实施要点

- [ ] 测试内 `set_default` 之后调用
      `tracing::callsite::rebuild_interest_cache()`，注释点名机制
      （interest 全局缓存 × 并行用例不同过滤面）。
- [ ] 同文件若还有其它 `set_default` 捕获型测试，统一补同款一行
      （逐个确认，不强改非捕获型测试）。
- [ ] 验证：全套并行连跑两次全绿 + 单跑绿。

## 验收

- nomifun-learning 全套 ×2 并行 + 单跑全绿。
- 非生产代码改动（纯 cfg(test) 面）。

## 非目标

- nomifun-ai-agent `acp.rs` 的同款捕获形状（map 后置 #5，留观）。
- tracing 依赖特性面（max_level 系 feature 不动）。
- 该测试断言面的语义（「findings 有行」是 e3 验收原判据，不弱化）。
