---
id: t2
title: "证据驱动的结构发现：检索证据 → 概念图 + 章节 spine → 覆盖/顺序校验"
type: task
status: open
assignee:
blocked_by: [t1]
part_of: ../map.md
---

# t2: 证据驱动的结构发现

**What to build:** 在 t1 分诊结果之上，新增一个**结构发现阶段**：从目标生成多角度查询，对全量来源库检索，汇总证据/候选概念，产出 `concept_graph + chapters` 的 `SourcePlan`；再用 critique/revise 和确定性后处理保证覆盖、顺序、无环、来源锚点。它是后续分批展开的骨架，**不是最终课程大纲**。

**Blocked by:** [t1 清点+分诊](t1-inventory-and-triage.md)。

**Status:** ready-for-agent

## Grill 第一轮定案（2026-10-21）

- **覆盖单位**：文件级只沿用 t1 的角色；`SourcePlan` 的覆盖以**章节/主题（heading/anchor）**为单位，文件是 source anchor 的载体，不是覆盖单位。一个文件可以服务多个章节；一个章节可以来自多个文件。
- **计划权威性**：`SourcePlan` 是权威章节骨架（章节/依赖/覆盖/source anchor）；现有大纲 agent 只在章节内决定 lesson 粒度/标题/purpose，不得自由重排/漏章；合并/拆分章节必须保留 `source_chapters` 映射与理由。
- **失败策略**：`uncertain` 保留为候选；环路/顺序冲突确定性回退（去环/拓扑/原序）+ warning；**只有零章节/零 core 才硬失败**，部分覆盖缺口建课 + 醒目 warning；无 completer/结构发现连续失败 → 回退 40 文件采样路径 + `degraded=true`，不阻断。
- **成本**：检索并发 ≤6、总调用 ≤48；结构发现固定 3 次 LLM（草稿/critique/revise）；>40 文件或预估 >20 次调用时按 t5 的估算/确认门处理。
- **代码落点**：编排与提示词住 `nomifun-learning`（课程域），`KnowledgeService` 做 IO，`LearningCompleter` 做 LLM；`nomifun-ai-agent` 不新增第二套 agent 循环。
- **混合文件（Q9）**：t1 的 `mixed` 文件进入候选；`SourcePlan` 用章节/主题覆盖决定哪些 heading/anchor 进课程，噪声段显式 skipped + anchor；文件级角色只作先验，不整文件排除。
- **计划深度（Q10）**：`SourcePlan` 只到章节 + 概念依赖 + source anchor；不携带 lesson 数/est；lesson 粒度、拆分、est 由 t3 决定。
- **无标题锚点（Q11）**：`source_anchors` 的定位顺序 = heading/anchor → 段落序号/行范围 → path 级并标 `anchor_uncertain=true` + warning；不编造标题、不排除无标题文件。
- **重复/翻译（Q12）**：t1 已按 canonical 规则选一份；`SourcePlan` 只读 canonical，`duplicate_of` 文件不进入 coverage 的 core topic 集。

## 输入/输出

**输入**：`goal`、`KbInventory`、`KbTriageReport`、`KnowledgeService`、`LearningCompleter`。

**输出**：`SourcePlan`（新类型，命名可实施时定）：

```text
SourcePlan {
  goal: String,
  evidence: {
    queries: Vec<String>,
    source_counts: Vec<{ kb_id, kb_name, chunks }>,
    summary: String,
    gaps: Vec<String>,
    candidate_concepts: Vec<String>,
  },
  concept_graph: {
    nodes: Vec<{ id, label, description, weight }>,
    edges: Vec<{ src, dst, relation: depends_on|extends|related, rationale }>,
  },
  chapters: Vec<{
    title,
    summary,
    learning_objectives: Vec<String>,
    covers: Vec<concept_id>,
    prerequisites: Vec<chapter_title>,
    source_anchors: Vec<{ kb_id, rel_path, heading_or_anchor?, snippet?, anchor_uncertain?: bool }>,
    content_type,
  }>,
  coverage: {
    topics: Vec<{
      topic_id,
      label,
      source_anchors: Vec<{ kb_id, rel_path, heading_or_anchor?, paragraph?, anchor_uncertain?: bool }>,
      assigned_chapters: Vec<chapter_title>,
      status: covered|skipped|uncertain,
      reason?,
    }>,
    files: Vec<{ rel_path, role, assigned_chapters, status }>,
    excluded: Vec<{ rel_path, role, reason }>,
    unresolved: Vec<String>,
  },
  warnings: Vec<String>,
  degraded: bool,
}
```

## 实施要点

- [ ] **查询生成**：从 goal 生成 4–8 条短查询，覆盖概述/定义、机制、案例、易错/边界、应用、历史/对比；查询语言跟源材料走；同一查询去重。
- [ ] **全库检索**：对 t1 的 `core/prerequisite/practice` 文件所属库调 `KnowledgeService::search_bases`；并行但限并发（建议 ≤6）、总调用上限（建议 ≤48）；按 `(query × kb)` query-major 排列，预算被砍时不饿死后面的库。
- [ ] **来源均衡**：不按全局 score 排序（不同引擎分数不可比）；每来源内部排序后轮询取样（DeepTutor `_balanced_slice` 口径）；记录每个来源贡献 chunk 数，0 贡献显式 warning。
- [ ] **证据摘要**：把均衡后的 chunk 摘要成 `summary / gaps / candidate_concepts(8-20) / notes(≤5)`；候选概念要原子化、可复用；不要把目录标题直接当概念。
- [ ] **草稿**：一次 LLM 调用同时产出 `concept_graph` + `chapters`；每章 `covers` 至少一个概念、每个概念至少被一章覆盖、章节顺序基本符合依赖、无环；每章尽量带 `source_anchors`（来自检索 evidence 的 kb/path/heading/snippet）。
- [ ] **评审/修订**：第二次调用做 critique，类别固定 `coverage | ordering | granularity | redundancy | missing_anchor | cycle | other`；第三次调用按 fix_hint 修；含糊时优先覆盖与拓扑一致，不引入未被提示的新章节。
- [ ] **确定性后处理**：
  - DFS 去环：丢 `rationale` 最短的 `depends_on` 边（置信度代理），上限 20 次；
  - Kahn 拓扑排序：按章节 `covers` 提升概念依赖到章节级；平局按原序；有环回退原序并 warning；
  - Jaccard 覆盖补齐：未覆盖概念按 `label+description` 与 `chapter.title+summary+objectives` 的 Jaccard 挂到最相关章节；
  - 章节级 mind map（可选）；无章节兜底 Overview。
- [ ] **覆盖对账（章节/主题级）**：t1 的每个 `core`/`mixed` 文件按 heading/anchor 拆成 topic；每个 topic 必须：
  - 被某章 `source_anchors` 覆盖，或
  - 显式标记 `skipped` + reason（例如“与目标无关/重复/超出范围”），或
  - `uncertain`（保留可见）。
  文件级角色只作先验；一个文件可以有多个 topic、分属不同章节；`mixed` 文件的相关段必须进入 coverage，噪声段 skipped + anchor。`prerequisite` topic 可挂到前置章/课时；`practice` topic 挂到练习章/课时；`noise/duplicate` 只进 excluded。
- [ ] **无标题/弱标题兜底**：heading/anchor 缺失时按段落序号/行范围建 topic；仍无法定位时用 path 级覆盖并标 `anchor_uncertain=true` + warning；不得让 LLM 编造原文不存在的标题。
- [ ] **计划权威性**：`chapters` 是后续 t3 的权威骨架；章节合并/拆分必须在 plan 里保留 `source_chapters` 映射与理由；不允许大纲 agent 静默重排/漏章。
- [ ] **标题/路径只作弱信号**：可用来 seed 候选概念、章节命名参考、拓扑平局 tie-break；不得因为文件名不按顺序而得出错误章节顺序。
- [ ] **失败/降级**：
  - 无 completer / 结构发现连续失败 → 回退当前 40 文件采样路径，`degraded=true` + warning，不阻断建课；
  - 同指纹早停（复用 `growth_retry` 思路）：连续两次 critique 相同即停；
  - 取消：沿用 `CancellationToken`/generation task 注册面。
- [ ] **预算**：查询/检索/摘要/草稿/评审/修订各自的 token/调用上限；structure plan 生成建议 3 次 LLM 调用（草稿/critique/revise），检索调用独立限流。
- [ ] **缓存/可复现**：查询、evidence、plan 带 hash；同库同目标重跑可对比；未变更的 inventory/triage 可复用。

## 与现有大纲引擎的接缝

- `OutlineBrief` 增加 `source_plan: Option<SourcePlan>`（`serde(default)`，旧客户端/描述流为 None）。
- `LiveCourseOutlineAgentEngine` 的 `co_start` 返回 plan 摘要与章节清单；`co_read` 仍只读**本批/本章**采样文件；`source_plan` 是权威骨架，模型在章节内决定 lesson 粒度/措辞。
- 章节合并/拆分只能发生在 plan 层或由大纲 agent 提出、引擎复核 `source_chapters` 映射与覆盖后才能落；不允许静默重排/漏章。
- 描述流（无 KB）完全不变；旧课程/旧请求无 plan 时走现状。
- 若 plan 与现有 `co_audit` 的 scope 覆盖冲突：以 plan 的章节/主题 coverage 为准，scope 作为附加检查，不重复造两套覆盖门。

## 非目标

- 不写数据库/迁移（t4）。
- 不做最终 lesson 正文/题目（t3 + 现有课时引擎）。
- 不做 embedding/GraphRAG（v1 用 `search_bases` 词频检索；后续增强）。
- 不替换学习图结构生成（B/C/D 等 `learning-goal-kb/t5`；A/E/F 已在 g1 裁定可共享）。

## 验收

- **干净教材**：19 文件《数据结构思维》整本目标：plan 覆盖 17/17 正文章，模块顺序顺书，source anchors 可回溯；与现状 8 模块/35 课对比不劣化。
- **脏库**：t1 的混合库：core/mixed 的相关 topic 全部有章节归属或显式 skipped+reason；noise/duplicate 不进 plan；uncertain 可见；无静默缺章。
- **混合文件**：一个文件同时含核心章节 + 会议记录，断言核心 topic 进章节、会议记录段 skipped + anchor，文件不被整文件排除。
- **无标题文件**：无 H1/H2/H3 的正文，断言 paragraph/行范围 anchor 生效；无法定位时 `anchor_uncertain=true` + warning，仍可成章。
- **大教材**：105 文件 Rust 书：plan 覆盖 20/20 章（或显式 skipped+reason），无未解释倒挂；给出 source_counts/gaps/warnings。
- **无证据**：查询零命中 → gaps 非空、plan 降级或回退，不伪造章节。
- **无环/顺序**：构造含环的概念图，断言去环 + 拓扑排序 + warning；构造前置引用倒挂，断言排序修复或 warning。
- `cargo test -p nomifun-learning` 全绿；`cargo check --workspace` 干净。

## 备注

- 结构发现是“证据 → 结构”，不是“目录 → 结构”；LLM 摘要只作证据压缩，source anchor 必须能回原文。
- 不要在这一步生成 35 节课的完整大纲；plan 只到章节/概念骨架，lesson 粒度留给 t3。
- 若检索成本过高，先对 t1 的 `core` 文件做路径/标题过滤再检索；不要退回“只看 40 文件”。
