---
id: d1
title: "设计定稿：脏库/大教材的课程结构发现与分批展开（含代码现状与生长课复用面）"
type: research
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# d1: 设计定稿：脏库/大教材的课程结构发现与分批展开

## Question

真实用户知识库不保证完整、顺序准确、内容全相关。当前传统建课在大纲阶段只取 40 个文件 / 960KB 的采样快照，`co_read` 又只能读采样清单；105 文件的 Rust 书出现 `sampled 40/105`、缺章、模块倒挂。需要定稿一套能处理脏库/乱序库/大教材的设计，并盘点传统与学习图生长两条链路的代码现状与复用面。

## Resolution

**结论速览（2026-10-21）**：采用五段管线——**清点 → 分诊 → 证据驱动结构 → 分批展开 → 校验/披露**。不再把“全量目录/章节树”当结构真相；文件名/标题只作弱信号。40 文件不再承担结构发现，只作为单批展开/内容接地的预算。学习图生长当前不读库（`learning-goal-kb/t5` 仍 open），g1 只做复用边界，不预裁。

完整调研与源码级机制见 [assets/dirty-kb-and-large-course-research.md](../assets/dirty-kb-and-large-course-research.md)。

### 一、代码现状

**传统知识库建课（现状链路）**

1. `GenerateCourseRequest` → `LearningService::generate_course`（`service/generate.rs`）。
2. `resolve_outline_brief`（`service/generate.rs:175`）：
   - 逐库 `get_base_info`；
   - `sample_bases`（`:210`）→ `generation/sample.rs`；
   - 目标感知采样预算 `40 文件 / 24KB per file / 960KB`（`generation/sample.rs:16,24,26`）；
   - 产出 `OutlineBrief { knowledge_bases, samples, goal, sampling }`（`course_outline/mod.rs:72`）。
3. 大纲引擎 `LiveCourseOutlineAgentEngine`（`agent/course_outline_loop.rs`）：
   - `co_start` 建草稿、`co_read` 读采样文件、`co_patch` 构建、`co_audit` 自查；
   - `co_read` 只接受采样清单路径（`:582`；`:602/:615` 明确拒绝清单外路径）。
4. `assemble_outline_pack` + `import_course_outline` 落 `learning_courses`（`blueprint_json`/`samples_json`/`samples_meta_json`）与 modules/lessons。
5. 课时正文（t3）：`assemble_lesson_grounding`（`service/lesson.rs:531`）读主 source 全文 + `search_bases` 补充；单课预算 24KB/40KB（`generation/lesson.rs:20-22`）。
6. 冒烟：`learning-smoke course`（`cli.rs:405`、`commands/learning_smoke.rs:346`，工作区未提交）已能建库→大纲→课时正文并打印采样/覆盖读数；当前 19 文件教材无负优化，105 文件 Rust 书撞 40 上限。

**学习图生长（现状链路）**

1. `generate_learning_graph_course`（`service/learning_graph.rs:29`）：只收 `description`（目标）+ `proficiency_statement` + `learner_basis`，**不读 KB**；`StructureInput` 只有 `goal/proficiency/basis`（`learning_graph/structure_gen.rs:24`）。
2. 结构生成：`generate_structure_artifacts` 双产物（structure + terrain）+ `gd.*` 契约门 + 门口对账；结构是参考地图。
3. 生长：`kick_growth`（`:996`）→ `growth_body`（`:1218`）→ 教练批次 → `gate_batch`（`:2084`）→ `apply_batch`（`:2343`）；材料来自结构快照、花名册、taught_ledger、概念登记表，**无 KB 通道**。
4. 课时内容：`generate_graph_lesson_content`（`service/lesson.rs:723`）；注释明确“学习图节点不读库”，`excerpts: Vec::new()`（`:779-780`）；grounding 走目标/程度/前置概念/概念面。
5. 边界票 `docs/tracker/learning-goal-kb/tickets/t5-graph-course-kb-boundary.md` 仍 open。

### 二、设计定稿

**阶段 0：清点（Inventory）**

- 枚举全部 KB 文件（v1 为 Markdown 文档面；非 md 暂 out of scope），记录 path/type/size/hash/标题/开头 preview。
- 不判断相关性；产出可复算的 inventory（hash 用于重复/版本识别）。

**阶段 1：分诊（Triage）**

- 规则先处理高置信：LICENSE/CONTRIBUTING/CHANGELOG/配置/空文件/导航页 → `noise`；翻译布局（microsoft/parallel/suffix）→ 配对/分流；同 hash/近似 hash → `duplicate` 候选。
- 剩余文件交 LLM 分诊，输入目标 + README/目录树 + 每文件标题/preview/类型；输出：
  `core | prerequisite | practice | noise | duplicate | uncertain` + confidence + reason。
- 40 文件/批；不确定 → 保守保留为 `uncertain`，**绝不静默丢内容**；每个排除项必须有 reason。
- 产出 `KbTriageReport`（总文件、各角色计数、排除清单+理由、uncertain、degraded/deterministic 标记）。
- 无 provider → 规则分诊降级：非噪声全进 core/prereq，披露 `degraded`，不阻断建课。

**阶段 2：证据驱动结构（Evidence Structure）**

- 从目标生成 4–8 条多角度查询（概述/机制/案例/易错/应用/对比）；对**全量来源库** `search_bases` 并行检索。
- 按来源均衡取样（不同引擎分数不可比），去重/裁剪；记录每个来源贡献 chunk 数；无贡献的来源显式披露。
- 摘要 → themes/strong evidence/gaps + 8–20 个候选概念 + notes。
- 一次草稿同时产出 `concept_graph`（前置→依赖）+ `chapters`（`covers`/`prerequisites`/`source_anchors`/目标）。
- 专门 critique：`coverage / ordering / granularity / redundancy / missing_anchor / cycle / other`；按 fix_hint revise。
- 确定性后处理：DFS 去环（丢 rationale 最弱边）、Kahn 拓扑排序（平局按原序，有环回退）、Jaccard 覆盖补齐、章节级 mind map；无章节兜底 Overview。
- 标题/路径只作 seed/tie-break；不把目录当结构真相。
- 大课程在 proposal/spine 两处确认（带成本估算）；小课程可自动 + 告警。

**阶段 3：分批展开（Batched Expansion）**

- 按 spine 的模块/章节展开；每批只加载该章 `core/prerequisite/practice` 文件（或按需检索），40 文件/批。
- 输出截断/形状错误 → 批拆半；单文件仍失败 → `h1/文件名` 兜底一课；网络/看门狗错误不重试，交给断点续跑。
- 长文件按 H2/H3 + 字数拆（目标每课 3000–8000 字），<1000 字与邻课合并；章节标题忠实原书，不发明“第 N 章”。
- 保留 t3 课时正文检索（主 source 全文 + 40KB 补充）；每个 lesson/block 带 source anchor；检索错误文本不当证据。
- 合并按 spine 顺序确定性排序；每个文件的最终去向（成课/前置/练习/跳过）进报告。

**阶段 4：校验/披露（Validation）**

- 覆盖报告：总文件、core 覆盖、prerequisite 去向、practice 归属、noise/duplicate 排除、uncertain、每章/每课 source anchor。
- 顺序不确定显式 warning；允许教学法重组但带 `source_chapters` 与理由。
- 失败隔离：单批失败拆半；单章失败不拖垮全课；provider 级连续失败断路器；已完成产物保留。
- 成本护栏：清点/规则分诊零 LLM；LLM 分诊按批；结构 1 草稿 + 1 critique + 1 revise；展开按模块计费；每阶段 token/调用上限；缓存/可复现。

### 三、生长课复用面（交给 g1 裁决，不在本票预裁）

**能直接复用的（低风险，不触碰 t5 边界）**

- 清点/分诊的纯函数与类型（`KbInventory`/`KbTriageReport`）——图课程将来接库时直接复用。
- 结构化输出的“批拆半/单文件兜底”工具——传统大纲批次与图教练批次都可复用；图生长当前有 `GROWTH_MAX_ATTEMPTS=3`，但没有通用的 JSON 截断拆半工具。
- 覆盖/不确定报告的形状——图结构已有 `gd.*` 契约与门，但可借“每个 core 文件/概念去哪了”的披露口径。

**必须先过 `learning-goal-kb/t5` 的（触碰边界）**

- 图课程 `StructureInput` 是否接收 KB 证据 brief（结构参考，不供内容）。
- 图课时内容是否改走 t3 的 grounding（当前 `excerpts: Vec::new()`）。
- 图节点/课时是否增加 `source_anchors`（schema/迁移/契约变更）。
- 教练批是否允许按目标检索的库片段（“库 → 教练”新增供给通道）。

**明确不能直接搬的**

- 传统 `OutlineBrief`/`Blueprint` 的 modules/lessons 形态：图课程是概念网 + 生长，不是大纲导入。
- 传统 40 文件采样：图课程没有“章节采样”概念，硬套会把结构变成库投影。
- 传统大纲的“章节标题忠实原书”：图课程的主干按教学法/依赖生长，不按资料目录。

### 四、票据

- t1 清点+分诊；t2 证据驱动结构；t3 分批展开；t4 持久化/迁移；t5 API/UI/冒烟/文档；g1 生长课复用边界。
- 依赖与验收见各票；t4 为 DB 迁移高风险区，实现前需 owner 签核。

## Addendum（grill 第一轮，2026-10-21）

用户按推荐确认 Q1–Q8，定案如下：

1. **术语**：`SourcePlan`（传统课程蓝图之前的来源结构计划）与 `SourceCoverage`（来源覆盖报告）；`Structure` / `Coverage Gauge` 继续专指学习图。
2. **单位与范围**：文件级定角色（`core/prerequisite/practice/mixed/noise/duplicate/uncertain`），章节/主题（heading/anchor）级定覆盖；section 级噪声只处理明显可分离的推广尾段/出版页/导航页。v1 只覆盖 Markdown；翻译副本/同 hash 重复自动排除并给理由；PDF/Word/图片后续立票。
3. **计划权威性**：`SourcePlan` 是权威章节骨架（章节/依赖/覆盖/source anchor）；大纲 agent 只在章节内决定 lesson 粒度/标题/purpose；合并/拆分必须保留 `source_chapters` 映射与理由。
4. **失败策略**：`uncertain` 保留为候选，不静默丢；环/顺序冲突确定性回退 + warning；**只有零章节/零 core 硬失败**，部分覆盖缺口建课 + 醒目 warning；无 LLM 时规则分诊降级（`degraded=true`）不阻断。
5. **成本门**：>40 文件或预估 >20 次 LLM 调用时，建课前展示估算并确认；低于阈值自动跑；所有阶段硬上限 + 可取消。
6. **持久化**：一个版本化 `course_source_plan_json` 列承载 `{version, inventory, triage, plan, coverage}`；t1–t3 先用内存/WS 报告，字段稳定后 t4 迁移（owner 签核）。
7. **代码落点**：`nomifun-learning` 拥有清点/分诊/结构计划编排与提示词；`KnowledgeService` 提供文件/检索 IO；`LearningCompleter` 提供 LLM；`nomifun-ai-agent` 不新增第二套 agent 循环。
8. **生长复用**：A/E/F（共享类型与规则、结构化批次工具、覆盖/披露形状）本波随 t1/t3/t5 落地；B/C/D（证据 brief 进 `StructureInput`、图课时接 t3 grounding、图节点 source anchors/教练批 KB）等 `learning-goal-kb/t5` 裁决后另立票。g1 据此关闭。

第二轮（Q9–Q14）定案：

9. **混合文件**：新增文件级 `mixed` 角色；有任一相关段就不整文件排除；哪些 heading/anchor 进课程由 t2 的章节/主题覆盖决定；噪声段显式 skipped + anchor。
10. **计划深度**：`SourcePlan` 只到章节 + 概念依赖 + source anchor；lesson 数/拆分/est 由 t3 按字数/标题自适应。
11. **无标题锚点**：heading/anchor 优先 → 段落序号/行范围 → 最后 path 级覆盖并标 `anchor_uncertain=true` + warning；不编造标题、不排除无标题文件。
12. **重复/翻译 canonical**：规则选 canonical（非翻译后缀原文优先、README/字符占比判语言、同语言最新/最高质量）；其余标 `duplicate_of` 自动排除并给理由；v1 不做逐项确认。
13. **发布/审阅 UX**：不加独立“生成前审阅”页；大库成本确认弹窗展示 triage 计数/跳过摘要；生成中/详情看完整报告；内部 kill switch/legacy fallback；不做用户可见 beta flag。
14. **迁移签核**：原则同意单列 `course_source_plan_json`；t1–t3 形状冻结后最终签核并实施 t4。

产品级前沿已空；后续是实现细节与外部依赖 `learning-goal-kb/t5`。
