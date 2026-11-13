---
id: t3
title: "分批展开：SourcePlan → modules/lessons，40/批、截断拆半、单文件兜底、覆盖校验"
type: task
status: closed
assignee: zhangyj
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

## Resolution（2026-10-21 关票）

**交付**（分支 `learn-v2/t3-batched-expansion`，提交 `ebbffe4ec`，已并入集成分支）：

- 编排入口 `run_kb_expansion(knowledge, inventory, triage, plan, goal, completer, cancel) -> Result<Option<ExpansionOutcome>, AppError>`，住 `generation/expand.rs`；`ExpansionOutcome{blueprint, coverage}`、`SourceCoverage` 与 chapter/lesson/file/topic 报告类型经 lib.rs 再导出（t4 落库/t5 报告消费）。
- **按章展开**：章文件 = `source_anchors` 去重保序；≤40/批（超限按锚点序分块）；批输入 = h1/preview/总字数/逐标题字数/相关摘录（24KB/文件，沿用 `GOAL_SAMPLE_MAX_PER_FILE`）；mixed 文件只进该章锚点命中的 heading/paragraph 段（会议记录等不进批）；headingless 段落锚定文件取锚点段；代码为主文件（fence 过半）带「代码为主」信号。
- **截断自愈**（复用 t1 structured_batch）：Parse 错误批拆半（8→4→2/2 递归全覆盖）；单文件批合法空回包 = 模型显式判定无教学价值段（推广尾段），不兜底；单文件解析失败 → H1/文件名兜底一课（`fallback=true` + warning + degraded）；Call 错误不拆半、结构化上抛（章/批/文件上下文）；取消 → `Conflict(CANCELLED_MESSAGE)`；空结构（0 课）→ 硬失败不落空课程；单章零课时 → 章级 skipped+reason 不拖垮全课。
- **防幻觉 validSet**：课时来源必须在章文件清单（t1 inventory 口径），编造路径整课拒收 + warning；锚点标题必须匹配原文（`heading_matches`），对不上拒收锚点值（课时保留、`anchor_uncertain=true` + warning），编造文本不落库。
- **确定性合并**：模块序 = plan 章节序（单一分节沿用章标题 = 标题忠实）；课时按（章锚点序, 标题清单序, 段落序）稳定排序，整文件课时排文件首；跨章同名分节补章名后缀。
- **SourceCoverage**：chapters（modules/lessons/skipped+reason）、lessons（sources/est/content_type/fallback/anchor_uncertain）、files（prerequisite/practice 归属可见，未锚定候选也入台账）、topics（plan 台账 × 课时锚点重对账：covered / skipped+reason / uncertain+醒目 warning）、warnings、degraded。
- **接缝**：`BlueprintLesson` 增可选 `anchor`（serde default，旧快照零回归）；展开课时直接带 SourceSpan 行范围与 kb_id 标注；`assemble_outline_pack`/`import_course_outline` 原样复用（零迁移）；`generate_course` 有可用 plan → 展开直接产 Blueprint（大纲引擎不被调用），展开失败（非取消）→ 回退现行引擎路径（brief 仍带 plan），取消传播给用户；t2 发现的 inventory/triage 透传复用（不重跑 t1）。
- 测试：`cargo test -p nomifun-learning` **423 passed / 0 failed**（基线 407 + 新增 16：expand 14 + 接线 2）；`cargo check --workspace` 干净。票面验收 fixture+scripted completer 全覆盖：大教材（20 章 × 6 文件全章有课、模块序 = 章节序、每课来源可回溯）、脏库（mixed 核心段成课、会议记录 topic 显式 uncertain+warning、文件不整文件排除）、无标题（段落序号锚 + 行范围）、截断（批大小序列 8/4/2/2/4/2/2 全覆盖；单文件兜底一课 + warning；空结构报错）、长文/短文规则与逐标题字数进提示词、代码文件信号+文件名兜底、防幻觉（编造 file/anchor 均拒）、>40 分块（40+5）、接线两路（plan 可用引擎不被调 / 展开失败回退引擎）。

**实施偏差**（不违背票面）：

1. Call 错误（网络/看门狗）在展开层上抛后由 `generate_course` 接线回退大纲引擎路径（degraded 不阻断）——「单章失败不拖垮全课」由章内兜底课与章级 skipped+reason 承担；网络错误属整课级别，由引擎路径整体接手。
2. `est`/`content_type` 为覆盖报告字段（advisory）：蓝图/落库仍走 `assemble_outline_pack` 的 est=10 现状（课时生成时重估），零迁移零 UI 面。
3. 「>8000 H2 按 H3 拆、<1000 合并、标题忠实」以提示词规则 + 逐标题字数输入落地（模型自适应），引擎侧不做二次切分；拆分结果经锚点 validSet 对账兜底，兜底课标题只取原文 H1/文件名（不发明编号）。
4. 票面 `learning-smoke course` 三库冒烟归 t5（沿 t2 先例）；本票以 fixture+scripted completer 覆盖同口径验收。
