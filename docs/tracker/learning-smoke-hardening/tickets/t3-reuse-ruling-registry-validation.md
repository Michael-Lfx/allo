---
id: t3
title: "reuse 裁决校验放宽到登记表全量——候选集子集不再误杀正确裁决"
type: task
status: closed
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

**速览**：裁决核验的事实边界从「本提名候选子集」放宽到「活跃登记表全
量」——`name` 仍须在本批提名内，`concept_id` 只要求在
`learning_concept_registry`（`deprecated = 0`）存在；候选集归属降级为 info
日志（`review reuse ruling host outside the nomination candidate set —
accepted (registry-valid)`），不再一票否决。`gate_batch` 把
`registry_concept_ids` 随 `GateOutcome` 下传，零额外查询。评审提示词同步
松绑：concept_id 可用材料里出现过的真实登记表 id（本提名或相关提名候选
均可），编造仍违约；反伪造边界不变。commit `17e1814a3`。

**单测（红绿样）**：

- 新增 `reuse_ruling_accepts_registry_host_outside_the_candidate_set`：登记表
  5 条概念，提名「向量代数」的候选集只含其中 3 条（词面 top-k），裁决指向
  候选集外的「线性组合」。
- 修复前红：提名被铸成新行（`minted = 1`）。
- 修复后绿：不铸新行；宿主 `aliases_json` 落活跃别名「向量代数」（带提名
  定义）；teaches 网边挂宿主、`ref_name = 向量代数`。
- 全量 `cargo test -p nomifun-learning`：330 passed / 0 failed。

**真机证据**：

- 原始误杀（存档 `observation/learning-smoke/run-1792396918900`，call 7）：
  评审 reject 附裁决 `霍纳法 → 01a15330-9aa7-7792-b16e-1634d6dbcdbd`，但
  「霍纳法」的候选集只有函数/向量/随机事件——旧核验把该裁决丢弃；模型是
  在同一材料的相关提名候选里看到的这个真实 id。
- 验收命令（flash，dev db，6 次尝试：run-1792401388594 / -2566347 /
  -3482606 / -3902440 / -5073569 / -6827247）：stderr 中
  `review reuse ruling dropped` 出现 **0 次**。
- **EXIT=0 未达**：6 次尝试分别在第 2–4 轮死于 `ng.redraft.scope`（点修越
  权）等已知 flash 重裁循环——正是 t4 票面记录的缺陷，与本票回归无关。最深
  一次（run-1792405073569）前三轮全绿、第 4 轮才触发；t4 落地后按同一命令
  复跑即可补 EXIT=0 回执。

**口径**：候选集只是评审的可见面，登记表存在即真实；语义正确性仍由评审
负责。
