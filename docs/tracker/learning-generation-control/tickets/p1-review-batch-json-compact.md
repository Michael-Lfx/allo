---
id: p1
title: "评审 batch_json pretty→紧凑序列化——入参省 20-30%"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# p1: 评审 batch_json 紧凑化

**What to build:** `concept_review` 装配评审 user 消息的
`serde_json::to_string_pretty(batch)`（learning_graph/mod.rs:1494 附近）改
`serde_json::to_string`。单点、零语义差（JSON 解析面等价）；教练 prompt
若有同款 pretty 先不动（控影响面，另票）。

**验收：**
- 评审装配单测绿；若有提示词金样快照断言需同步
- `cargo test -p nomifun-learning` 基线

**病历依据：** 2026-11-09 真机 growth 入参 33,156 字符；评审单调用
12-20KB 区间（审计已知「击穿 480s」的输入大头），pretty 空白占 20-30%。

## Resolution

已关（2026-11-10）：commit `cc34a5def`，并回 feat/learn0928。batch_json 紧凑化（:832 边界未动），红绿样双钉紧凑形；952 Rust 全绿。
