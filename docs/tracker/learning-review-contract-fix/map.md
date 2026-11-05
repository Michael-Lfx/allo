# Map: 评审契约矛盾与静默降级修复（r01）

<!-- wayfinder:map -->
<!-- 状态：开图（2026-11-05）。来源 = pctx 波真机验收的法证发现（grow 臂两次
      跑），非本波回归——存量缺陷首次被真机踩响。用户流程：pctx 关票后单独
      施工。 -->
<!-- 基线：feat/learning-pctx-fixes 尖端（关票时点）。 -->

## 病历（run-1793858439491 / run-1793859632428，absorb8-smoke 工作目录）

1. **run#2（绿茶批）**：评审回包 6 problems（#4/#6 为 `node:""` 的批级
   mints 意见——**提示词条款明文规定的正确格式**，t4/#175A「批级意见不点名
   任何节点，node 填空串」）→ `validate_review`（growth_contract.rs
   `for key in ["node", "field", "issue"]` 非空检查，i07 波 `a741ee9f3`
   2026-10-15 引入）判违约 → **无重试直接「degrading to PASS」** → 6 条
   真实挂号缺失问题被吞、批次无评审应用、`stop_reason=success`、
   `degraded=false`、读数全零。
2. **run#1（手冲批）**：评审调用撞 provider SSE 瞬断 + 480s 超时 →
   「评审自身不可用→保守放行」（mod.rs:1528）→ 同样静默应用。

两条路径殊途同归：**评审层任何故障都静默放行且读数零信号**——真机读数
`fragments total=0` 若不查档案会误读为「清单瘦身到零」。

## 票表

| id | 票 | 档 |
| --- | --- | --- |
| r01 | validate_review 空节点矛盾 + 评审静默降级可观测化 + 违约死因重试 | **高** |

实施要点与验收见 tickets/r01-review-contract-empty-node.md。
