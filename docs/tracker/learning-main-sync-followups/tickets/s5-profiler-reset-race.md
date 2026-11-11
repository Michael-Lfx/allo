---
id: s5
title: "nomi-agent profiler 测试竞态——reset 测试与探测测试串行保护"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# s5: profiler 全局计数器测试隔离（既有缺陷，非 CI 红因）

**What to build:** 进程级全局 profiler 计数器被
`profiler::tests::reset_zeroes_every_counter` 在并行下清零，击穿
`horizon::ledger::tests::git_workspace_probe_tracks_a_new_file_and_reports_itself`
的「计数必须增加」断言。给两处加同一把 test-only 锁串行化（本机 Windows
全套并行 3/3 必败 → 修后 3/3 必胜）。

## 法证证据（2026-11-11 验证，实码复核）

- 失败断言 `ledger.rs:456`（"every probe must be reported to the
  profiler"）：`before` 读数与最终断言之间起两次 `git` 子进程（Windows
  上窗口极宽），期间 `profiler.rs:243` `reset_zeroes_every_counter`
  把 `COUNTERS`（:108，进程级 `static [SlotCounters]`）全量清零。
- 复现面：本机并行 3/3 必败；`--test-threads=1` 959 全过；单跑全过。
- 既有缺陷归属：`horizon/ledger.rs` 与 `profiler.rs` 学习分支未碰、与
  origin/main 逐字节相同；Linux CI 绿（git 子进程起得快、窗口窄）——
  但它是 CI 卡点套件（`cargo test -p nomi-agent --lib`），竞态随测试
  数量增长终会咬 Linux，值得现在钉死。

## 实施要点

- [ ] `profiler.rs` 增 `#[cfg(test)]` 共享锁（`static ...: Mutex<()>` +
      `pub(crate)` 取锁助手），注释点名两个消费方。
- [ ] `reset_zeroes_every_counter` 全程持锁；ledger git 探测测试从读
      `before` 起持锁到最终断言止。
- [ ] 验证：本机并行全套连跑三次全绿（修复前同环境 3/3 必败，判据
      对称）。

## 验收

- `cargo test -p nomi-agent --lib` 并行 ×3 全绿。
- 不引入非 test 代码改动（纯 cfg(test) 面）。

## 非目标

- profiler 本体的计数语义（进程级全局 + monotonic 是有意设计，见
  profiler.rs 头注）。
- 其它 nomi-agent 测试的隔离审计（本票只钉已实锤的这一对）。
