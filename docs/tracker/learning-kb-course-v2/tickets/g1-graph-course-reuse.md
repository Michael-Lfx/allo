---
id: g1
title: "生长课复用边界：A/E/F 本波共享，B/C/D 等 learning-goal-kb/t5"
type: grilling
status: closed
assignee: zhangyj
blocked_by: [d1]
part_of: ../map.md
---

# g1: 生长课复用边界

## Resolution

**结论速览（2026-10-21 grill 第一轮，用户按推荐确认 Q8）**：

- **本波做 A/E/F**：A 清点/分诊类型与纯规则；E 共享 `structured_batch` 拆半/兜底工具；F 覆盖/披露报告形状。A 并入 t1，E 并入 t3，F 并入 t3/t5；不改图课程行为。
- **B/C/D 推迟**：B 证据 brief 进 `StructureInput`、C 图课时接 t3 grounding、D 图节点 source anchors/教练批 KB——全部等 `learning-goal-kb/t5` 裁决后另立实施票；本 effort 不预裁 t5。
- **不做**：传统 `OutlineBrief`/`Blueprint` 形态、40 文件采样、“章节标题忠实原书”直接搬进图课程。

## Question

传统建课的脏库升级（清点/分诊/证据结构/分批展开/覆盖披露）里，哪些能被学习图生长课程（`course_kind=learning_graph`）直接复用？哪些会触碰“图课程是否接知识库”的边界，必须先由 `docs/tracker/learning-goal-kb/tickets/t5-graph-course-kb-boundary.md` 裁决？

## 现状（源码读后）

- **图课程创建**：`generate_learning_graph_course`（`service/learning_graph.rs:29`）只收目标/程度/基础；`StructureInput { goal, proficiency_statement, learner_basis }`（`learning_graph/structure_gen.rs:24`），**不读 KB**。
- **结构生成**：双产物 structure + terrain + `gd.*` 契约门 + 门口对账；结构是参考地图，不供内容。
- **生长**：`kick_growth`（`:996`）→ `growth_body`（`:1218`）→ 教练批 → `gate_batch`（`:2084`）→ `apply_batch`（`:2343`）；材料 = 结构快照 + 花名册 + taught_ledger + 概念登记表，**无 KB 通道**。
- **图课时内容**：`generate_graph_lesson_content`（`service/lesson.rs:723`）；注释明确“学习图节点不读库”，`excerpts: Vec::new()`（`:779-780`）；grounding = 目标/程度/前置概念/概念面。
- **边界票**：`learning-goal-kb/t5` 仍 open（结构 grounding / 教练批 / 课时内容 / 连贯预算四问）。

## 候选复用面

| 候选 | 内容 | 风险 | 是否触碰 t5 |
| --- | --- | --- | --- |
| A. 清点/分诊类型与纯函数 | `KbInventory`/`KbTriageReport`、规则分诊、翻译/重复检测 | 低（纯工具） | 否 |
| B. 证据 brief 作为结构参考 | 把 t2 的 `EvidenceBrief` 作为 `StructureInput` 的可选参考材料（不供内容） | 中（改变结构生成语义） | **是**（结构 grounding） |
| C. 图课时内容接 t3 grounding | 图课时复用传统 `assemble_lesson_grounding`（主 source + 检索补充） | 中高（内容供给/引用） | **是**（课时内容） |
| D. 图节点/课时 source anchors | 给图节点/课时加来源字段与迁移 | 高（schema/契约/迁移） | **是**（且需数据模型） |
| E. 共享结构化批次工具 | “尝试→解析错误→拆半→单文件兜底”，传统大纲与图教练批共用 | 低（纯工程） | 否 |
| F. 覆盖/披露报告形状 | `SourceCoverage` 的口径用于图结构/生长读数 | 低中（读侧） | 否（只读披露） |

## 建议取向（待 grilling 确认）

1. **先做 A/E/F（不碰 t5）**：
   - 抽 `KbInventory`/`KbTriageReport` 纯类型与规则；传统先用，图后续可 import。
   - 抽 `structured_batch` 工具；传统大纲批次先用，图教练批只在有 JSON 截断问题时接入。
   - 覆盖/披露口径做成通用读侧 DTO/报告形状，不绑定传统 `Blueprint`。
2. **B 只在 `learning-goal-kb/t5` 裁“接结构参考”后做**：
   - 证据 brief 只进 `StructureInput` 的参考段，明确“参考地图、不供内容”；不得把 KB 文本塞进教练/正文。
   - 结构产物仍走 `gd.*` 契约与门口对账；不把传统 `SourcePlan` 直接当图结构。
3. **C/D 只在 t5 裁“接课时/来源”后做**：
   - 图课时复用 t3 grounding 时，保留“前置概念/档位/禁止清单”的图语义；检索只做补充，不替换前置段。
   - D 需要 schema/迁移/契约，单独立票，不塞进本 effort。
4. **如果 t5 裁“不接”**：只保留 A/E/F；图课程继续纯目标驱动，不引入 KB 通道。

## 需要回答

- [ ] 图课程是否需要 KB？如果需要，是结构参考、教练材料、课时内容，还是三者都要？
- [ ] “结构对生长不供内容”的铁律下，证据 brief 进 `StructureInput` 是否仍算“参考地图”？边界怎么写进提示词/契约？
- [ ] 图课时接 t3 grounding 时，前置概念段与 KB 证据的优先级/冲突怎么处理？
- [ ] 图节点/课时是否需要 `source_anchors`？如果需要，schema/迁移/读侧展示的最小集是什么？
- [ ] A/E/F 是否先作为独立小票落地，不等 t5？
- [ ] 共享 `structured_batch` 是否值得抽？图教练批当前是 5 节点/批，是否真的会遇到 JSON 截断？

## 验收

- 产出一张“复用面 × 风险 × 依赖”的裁决表，明确：
  - 现在就能做的共享件；
  - 等 t5 的边界项；
  - 不做/另立票的项。
- 若裁“做”，给出实施顺序与最小改动；若裁“不做”，写明理由与替代方案。
- 不预裁 `learning-goal-kb/t5` 的四问；本票只决定“传统升级里哪些能共享”。
