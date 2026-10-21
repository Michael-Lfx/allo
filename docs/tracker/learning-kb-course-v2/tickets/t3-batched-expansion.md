---
id: t3
title: "分批展开：SourcePlan → modules/lessons，40/批、截断拆半、单文件兜底、覆盖校验"
type: task
status: open
assignee:
blocked_by: [t2]
part_of: ../map.md
---

# t3: 分批展开

**What to build:** 把 t2 的 `SourcePlan` 展开成可导入的 `Blueprint`（modules + lessons）。每批只处理一个章节/模块，≤40 文件；输出截断/形状错误时批拆半；单文件仍失败用 H1/文件名兜底；长文件按 H2/H3 拆、短文件与邻课合并；标题忠实原书；最后做覆盖校验（每个 core topic/章节有去向）和 source 映射。

**Blocked by:** [t2 证据驱动结构](t2-evidence-driven-structure.md)。

**Status:** ready-for-agent

## Grill 第一轮/第二轮定案（2026-10-21）

- **覆盖单位**：以 t2 的章节/主题 topic 为准；文件级 role 只作先验。每个 topic 的 source anchor 至少落到一个 lesson，或显式 skipped+reason。
- **计划权威性**：按 `SourcePlan.chapters` 展开；章节合并/拆分只能带 `source_chapters` 映射与理由，不允许静默重排/漏章。
- **lesson 粒度（Q10）**：plan 只到章节/概念；lesson 数、拆分、est 由本票按字数/标题自适应（目标每课 3000–8000 字），不要求 plan 预写 lesson 清单。
- **无标题锚点（Q11）**：heading/anchor 优先 → 段落序号/行范围 → path 级并标 `anchor_uncertain=true` + warning；兜底课（H1/文件名）标 `fallback=true`。
- **失败策略**：只有零章节/零核心课才硬失败；部分覆盖缺口建课 + 醒目 warning；单文件兜底课标 `fallback=true`，不伪装成正常设计。
- **成本**：每章一批，≤40 文件；>40 文件或预估 >20 次调用时按 t5 估算/确认门；批内硬上限 + 可取消。
- **持久化**：`SourcePlan`/`SourceCoverage` 进 t4 的单个 `course_source_plan_json` 快照列；t3 先产出内存/DTO 报告。
- **代码落点**：展开编排住 `nomifun-learning`；`assemble_outline_pack`/`import_course_outline` 复用；`nomifun-ai-agent` 不改。
- **共享件**：`structured_batch`（尝试→解析错误→拆半→单文件兜底）在本票抽出，传统先用；图教练批等 g1 的 A/E/F 接入，B/C/D 等 `learning-goal-kb/t5`。

## 实施要点

- [ ] **按章展开**：遍历 `SourcePlan.chapters`，每章加载该章 `source_anchors` 对应的 `core/prerequisite/practice/mixed` 文件的**相关段**（或按需 `read_file`/`search_bases` 取正文）；`mixed` 文件只取该章 topic 的 heading/anchor 段，不把整文件塞进批。一批只放该章文件，避免跨章上下文爆炸。
- [ ] **批预算**：单批 ≤40 文件；单文件正文按现有 24KB 摘录/全文策略，总量按 t2 的章节预算；不要把整本书塞进一次调用。
- [ ] **章节内 lesson 设计**：输入章节标题/目标/摘要 + 本批文件 `h1/preview/totalChars/headings[]`；输出 `section/module + lessons`（title/purpose/source/anchor/est/content_type）。
- [ ] **长文件拆分**：目标每课 3000–8000 字；H2 >8000 且有 H3 → 按 H3 拆；<1000 字与相邻同 world 课合并；代码文件整体一课；网页推广尾段/出版方介绍页不单独成课。
- [ ] **标题忠实**：沿用原书章节/文件 H1 标题，可去掉冗余前缀；原书有编号则保留；原书无编号体系时不得发明“第 N 章”；section 按内容主题命名。
- [ ] **source/anchor 映射**：每个 lesson 至少一个真实 `source`（path + kb_id）；拆分的 lesson 带 `anchor`（H2/H3 完整标题）；无标题时用段落序号/行范围；仍无法定位时 `anchor_uncertain=true` + warning。`source_anchors` 必须能在 t1 inventory 里找到；防幻觉（validSet 校验）。
- [ ] **截断自愈**：
  - `StructureParseError`（JSON 截断/缺 sections/形状不对）→ 批拆半递归；
  - 单文件仍失败 → `h1/文件名` 兜底一课，并记 warning；
  - 网络/看门狗/取消错误 → 不重试，原样上抛，交给断点续跑/任务取消；
  - 空结构（0 课）→ 硬失败，不落空课程。
- [ ] **共享批处理工具**：抽出 `structured_batch`（尝试 → 解析错误 → 拆半 → 单文件兜底）；传统大纲批次先用。图教练批按 g1 的 A/E/F 可直接复用该工具；B/C/D（图课程接 KB）等 `learning-goal-kb/t5`，不在本票改图生长行为。
- [ ] **合并**：按 plan 章节顺序确定性合并；同章内按文件标题/字数/anchor 顺序；不按字典序重新洗牌。
- [ ] **覆盖校验（章节/主题级）**：
  - `plan.coverage.topics` 每个 topic 必须 `covered`（至少一个 lesson source/anchor）或 `skipped + reason`；
  - `prerequisite/practice` topic 必须有归属（章节/课时）或显式 skipped；
  - `uncertain` 保留并可见；
  - 文件级 role 只作诊断：core 文件可以有未覆盖的 noise 段，但每个 core topic 必须有去向；
  - 生成 `SourceCoverage`（可先内存/DTO，t4 落库）。
- [ ] **与现有导入的接缝**：复用 `assemble_outline_pack`/`import_course_outline`；`blueprint_json` 继续是最终 modules/lessons 快照；plan 作为伴随字段（t4 落库）。
- [ ] **失败隔离**：单章失败不拖垮全课；已展开章节保留；整课失败时给出结构化错误（哪个章/批/文件）。

## 非目标

- 不生成课时正文/题目（现有 lesson engine + t3 grounding）。
- 不做数据库迁移（t4）。
- 不把 plan 的 concept graph 直接渲染成课程结构（图课程的 B/C/D 等 `learning-goal-kb/t5`；A/E/F 已在 g1 裁定可共享）。

## 验收

- **大教材**：105 文件 Rust 书：所有 20 章都有 lesson 或显式 skipped+reason；每课 source 可回溯；无未解释倒挂；覆盖报告可读。
- **脏库**：混合库：noise/duplicate 不进入 lesson；core/mixed 的相关 topic 全部成课；uncertain 可见；跳过有 reason。
- **混合文件**：一个文件含核心章节 + 会议记录，断言 lesson 只引用核心 heading/anchor 段，会议记录段不进入正文/题目，文件不被整文件排除。
- **无标题**：无 H1/H2/H3 的文件按段落/行范围成课；无法定位时 `anchor_uncertain=true` + warning，不丢内容。
- **截断**：模拟 40 文件批 JSON 截断，断言拆半后覆盖全部文件；单文件失败 → 兜底一课 + warning；空结构 → 错误。
- **长文/短文**：>8000 字 H2 按 H3 拆；<1000 字合并；代码文件一课；标题忠实原书。
- **来源防幻觉**：LLM 返回不存在的 file/anchor → 被 validSet 拒绝，不落库。
- `cargo test -p nomifun-learning` 全绿；`learning-smoke course` 在 clean/dirty/large 三库可跑。

## 备注

- 这一层的“批”是**章节内**的批，不是全库 40 文件采样；不要把 t1/t2 的全量视野又缩回 40。
- 单文件兜底是最后一道防线；兜底课必须在覆盖报告里标记 `fallback=true`，不能伪装成正常设计。
- `anchor` 是正文截取的唯一定位；拆分的 lesson 没有 anchor 时默认取该 H2 起点到下一 H2 之前。
