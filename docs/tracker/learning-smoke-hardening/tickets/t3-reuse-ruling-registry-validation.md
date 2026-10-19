---
id: t3
title: "reuse 裁决校验放宽到登记表全量——候选集子集不再误杀正确裁决"
type: task
status: open
assignee: zhangyj
blocked_by: []
part_of: map.md
---

## Question

生长循环对评审 reuse 裁决的机械核验（`learning_graph.rs` rulings 构建处）
要求 `concept_id` 必须出现在**该批提名的 defnear 候选集**内，否则按编造丢
弃。真机实录（run-1792385060145 / run-1792395614887）：评审读的是**全局已
教账**，它给出的正确宿主（如「二进制转十进制」）可能不在近邻候选子集里
——候选集只是评审的可见面之一，不是事实边界。被丢弃的裁决 = 近义铸名继续
走 problems 打回 = 重裁循环。

**修法**：`concept_id` 的有效性校验放宽为「登记表中存在该概念」
（`learning_concept_registry` 全量），候选集归属仅作日志注记不再一票否决。
反伪造边界不变：concept_id 必须真实存在，只是事实源从候选子集换成登记表。

**验收**：
- 单测：ruling 指向候选集外的登记表概念时不再丢弃，落库为宿主活跃别名；
- 真机：深批次 round 3+ 的 `review reuse ruling dropped` 警告消失或仅剩真
  编造（concept_id 不存在）。

## Resolution

（结案时追加）
