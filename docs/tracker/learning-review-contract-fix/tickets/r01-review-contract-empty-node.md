---
id: r01
title: "validate_review 空节点矛盾 + 评审静默降级可观测化 + 违约死因重试"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# r01: 评审契约矛盾修复——让批级意见合法、让降级有信号、让违约有重试

**What to build:** 三件互叠修复：①`validate_review` 对 reject problems 的
`node` 允许空串（批级意见的提示词规定格式），`field`/`issue` 仍必填；
②「评审违约→保守放行」路径补可观测信号（读数/stop_reason 区分「评审缺席
的成功」）；③评审回包违约与评审调用传输失败补「带死因重试恰一次」（与
挂号表/碎片裁决面同律），仍败才降级并在读数标 degraded。

## 现状（2026-11-05 实码 + 真机病历）

- `growth_contract.rs:408` `validate_review`：reject 分支
  `for key in ["node", "field", "issue"]` 三键全要求非空——与提示词
  advice-range 条款（mod.rs ~`:900-905`：「批级意见不点名任何节点，node
  填空串」）**直接矛盾**；i07 波 `a741ee9f3` 引入，潜伏至今（既往真机
  模型未在 reject 里写过批级意见）。
- `mod.rs:1536` 附近：`validate_review` 违约 → warn
  "concept review reply violated its contract — degrading to PASS" →
  **立即**降级（无死因重试）→ `ReviewOutcome::default()`（problems 空、
  读数全零）。
- `mod.rs:1528` 附近：评审调用 Err（传输失败/超时）→ 同款保守放行，
  亦无重试。
- smoke 读数（`growth_run_stats`）：provenance/fragments 的 `degraded`
  只覆盖「结论两次不可用」路径；调用失败/契约违约路径 degraded=false、
  `stop_reason=success`——**评审缺席与评审通过在读数上不可区分**。

## 实施要点

### A. validate_review 空节点对齐（growth_contract.rs）

- [ ] reject problems 循环：`node` 从非空检查中移除（空串=批级意见合法），
  `field`/`issue` 非空与 `field` 词表检查保留；互钉测试与 schema 常量
  （`proposal_schema` 同源装配面如有 review schema）同步。
- [ ] 单测：批级意见（node=""）的 reject 回包过 validate_review；node 仅
  空白的「  」串视为空（合法）——与 trim 语义一致。

### B. 降级可观测化（mod.rs + smoke 读数）

- [ ] 「评审调用失败」与「评审契约违约→保守放行」两路径：findings 通道
  落一条 Degraded 留痕（与挂号表/碎片 degraded_outcome 的 findings 留痕
  同款形状，issue 含「评审缺席/Degraded」），读数面新增可区分信号
  （provenance/fragments 之外的 `review_degraded` 或复用既有 degraded
  位——以最小改动为准），smoke grow 读数行透出。
- [ ] `stop_reason` 或批次 note 区分：评审缺席的成功 ≠ 评审通过的成功
  （最小改动 = 批次 note 追加一句，不动 stop_reason 词表）。

### C. 违约与传输失败死因重试（mod.rs concept_review）

- [ ] `validate_review` 违约 → 带死因重试恰一次（反馈附违约条目文摘），
  仍败才降级——与挂号表/碎片「恰一次带死因 → Degraded」同律。
- [ ] 评审调用传输失败（Timeout/transport）：沿用 y02 失败分流语义——
  传输失败重试恰一次（不占点修预算），仍败降级并在读数标 degraded。
- [ ] 单测：违约一次→重试救回；违约两次→降级留痕；传输失败→重试；
  空节点批级意见不再触发违约。

## Non-goals

- 不动「评审不阻断结构合法批次」的保守放行总语义（只加信号与重试）。
- 不动 problems 其它形状规则与 `REVIEW_PROBLEMS_MAX`。
- 不做评审并发/超时预算重排（480s 包装器保持）。

## 验收

1. `cargo test -p nomifun-learning` 全绿 + 新测全过。
2. 真机 grow 臂 EXIT=0 且：评审 reject（含批级意见）被尊重（打回重裁）
   或违约被重试救回；读数行在评审缺席场景显式标注（不再静默全零）。
3. 病历同形测试：本票病历的两份响应形状（run#1 超时 / run#2 空节点
   违约）钉住。
