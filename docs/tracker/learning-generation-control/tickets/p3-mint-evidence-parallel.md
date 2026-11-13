---
id: p3
title: "铸名门外部证据检索并行 fan-out——5 次串行 3.1s"
type: task
status: closed
assignee:
blocked_by: []
part_of: ../map.md
---

# p3: 铸名门证据检索并行化

**What to build:** `learning_graph/mint_evidence.rs` 的逐候选串行搜索改
并发 fan-out（join_all/buffer_unordered）。fail-open 语义不变（无提供方/
搜索失败/零结果 = 无证据照常评审，判据条款与评审输出契约零改动）；
off 臂读数显式零不变。

**验收：**
- 单测：并发路径与串行结果等价；fail-open 分支保持
- `cargo test -p nomifun-learning` 基线

**病历依据：** 2026-11-09 真机 5 次检索严格串行（0.7-0.85s 间隔，
共 3.1s）——收益秒级但零风险。

## Resolution

已关（2026-11-10）：commit `92de10db3`，并回 feat/learn0928。join_all fan-out + 按 index 回填，fail-open/去重/stats 口径零改动；混合批表征测试恰 4 次搜索断言；945→952 全绿。
