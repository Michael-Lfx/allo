---
id: f1
title: "评审挂号表支持批外合法概念锚定——声明/历史批概念不再打死挂号表"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# f1: 评审挂号表支持批外合法概念锚定

**What to build:** 生长评审的 term_provenance 挂号表放行「registry 已存在且本批
复用」的概念锚——m2 声明铸下的概念被生长批复用时，评审不再两次同死降级。
附带不可满足反馈短路。

## 法证证据（2026-10-30 真课验收，e2e2/server.log）

- `06:46:35 provenance validation failed — retrying the review once with the
  death cause attempt=1 reason=已教行「每存满一年，多…」`
- `06:47:19 provenance table unusable twice — degraded pass death=挂号表不可用:
  行 concept 「百分数改写为小数」不在批内`
- 该概念是 **m2 声明刚铸进登记表的**（source=basis-declaration）。生长批
  defnear **正确复用**了它（登记表 8 条 = 声明 4 + 生长 4，零重复；5 节点只
  铸 4 条）——死的是**评审挂号锚定**：挂号行要求概念 ∈ 本批铸造，复用的
  registry 概念永远不满足。模型被带死因重试一轮 = **不可满足的反馈目标
  （卡壳）**，随后降级 pass（m3 fail-open 兜住，但本批评审锚定全废 + 白烧
  一轮）。

## allo 现状（落点提示，file:line 需实现会话复核）

- 评审挂号表 = m3 落的 term_provenance 校验，在生长评审路径
  （`crates/backend/nomifun-learning/src/learning_graph/` 评审面 + 重试环，
  06:46 两条 WARN 的发射点即执法处——grep「挂号表不可用」「provenance」）。
- 评审输入装配（教练上下文/评审 user 消息）当前携带：批内候选集（m4 复用
  通道 j04 落的 NominationPlan/registry_entries 只认活跃别名）——**没有**
  「本批复用的 registry 存量概念」的 ULID 表。
- 降级语义：两次同死 → degraded pass（m3 fail-open，保留不动）。

## learnhub 参考（实现会话先读）

- 挂号表原main：learnhub #169/#176 term_provenance（评审输出锚定批内铸造）。
- **本碰撞是吸收缝，learnhub 侧是否已有解未核实**——实现第一步读他们的
  评审输入装配：声明概念（ADR-0032 铸的）在生长评审输入里如何呈现、挂号行
  是否允许锚定非本批概念。若无解（他们声明块可能从不进生长批），则本票是
  allo 首创缝补，域中立自研 + 票面记档。

## 实施要点

- [ ] 评审输入携带「批外已知概念表」：本批复用（gate 复用裁决命中）的
      registry 概念，带 canonical + concept_id，注明来源（declaration/
      历史批）——挂号行可锚定到这些 ULID。
- [ ] provenance 校验放行：挂号行概念 ∈ 批内铸造 ∪ 批外已知概念表 =
      合法锚；仍拒绝「批内都不存在的凭空概念」（j04 的编造静默丢弃语义
      不变）。
- [ ] 不可满足短路（belt）：死因匹配「不在批内」且该概念在 registry 中
      存在 → 跳过重试直接降级（不再让模型空转一轮）；日志注明「批外锚定
      未启用/未命中」。
- [ ] 红绿样：声明概念被复用批的评审 → 非降级通过 + 挂号行锚定 registry
      ULID；凭空概念 → 仍拒；同死两轮 → 立即降级（不烧第三轮）。

## 验收

- `cargo test -p nomifun-learning`（+ai-agent 若涉）全绿；零迁移。
- 真机判据：声明 G1+G2 后再生长含声明覆盖块的批 → 日志**零
  「provenance table unusable」**、评审非降级、挂号行锚定 registry 概念。

## 非目标

- 声明覆盖块要不要进生长批（产品雾区，见 e3 Resolution）。
- 降级语义本身（fail-open 保留）。
- 释义级近义检测（雾区⑤）。
