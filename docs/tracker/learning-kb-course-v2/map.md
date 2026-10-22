# Map: 学习课程生成 v2——脏库/大教材的结构发现与分批展开

<!-- wayfinder:map -->
<!-- 状态：立票（2026-10-21）。d1 设计已定稿；t1–t5 与 g1 待认领。 -->

## Destination

让「知识库 → 传统课程」在**不干净、不按阅读顺序、不全相关、可能很大**的库上仍然可用：

- 先清点全部文件、分诊相关性与角色，噪声/重复/无关内容显式排除；
- 再用**内容证据 + 概念图/章节骨架 + 覆盖与顺序校验**重建课程结构，不把文件名/目录当结构真相；
- 最后按模块/章节分批展开，按课时检索原文，覆盖/跳过/不确定全部可披露；
- 不再用「40 文件采样」承担结构发现；40 文件只作为单批展开/内容接地的预算。

同时给出学习图生长课程（g1）能吸收复用哪些能力、不能吸收哪些，避免把传统大纲的形态硬塞进生长。

## Notes

- **前情（2026-10-21 真机 smoke）**：真实教材《数据结构思维》19 文件整本目标 19/19 入样、8 模块 35 课、正文 17/17 覆盖，未出现负优化；Rust 书 105 文件撞 `sampled 40/105`，课程只引用 34 文件，第 14 章缺失、15/16 模块倒挂。完整报告在
  `C:\Users\test\Desktop\work\learning-smoke-course-20261021\report.md` /
  `report-rust-stress.md`；设计调研见
  [assets/dirty-kb-and-large-course-research.md](assets/dirty-kb-and-large-course-research.md)。
- **用户前提（2026-10-21 明示）**：真实用户库不保证完整、顺序准确、内容主题相关；可能混入无关文件、翻译副本、重复版本、缺失标题、网页推广尾段。设计不得假设“库即教材”。
- **代码锚点（传统）**：
  - `service/generate.rs:175` `resolve_outline_brief` → `:210` `sample_bases`；`generation/sample.rs:16/24/26` 40 文件/24KB/960KB；`course_outline/mod.rs:72` `OutlineBrief`。
  - `course_outline_loop.rs:582` `co_read` 只读采样清单（`:602/:615` 明确拒绝清单外路径）。
  - `service/lesson.rs:531` t3 实时课时 grounding（主 source 全文 + 检索补充）；`generation/lesson.rs:20-22` 24KB/40KB 预算。
  - `learning-smoke course` 已加（`cli.rs:405`、`learning_smoke.rs:346`，工作区未提交），可直接做 clean/dirty/large 三库冒烟。
- **代码锚点（学习图生长）**：
  - `service/learning_graph.rs:29` 图课程创建只收目标/程度/基础，不读 KB；`structure_gen.rs:24` `StructureInput` 只有 goal/proficiency/basis。
  - `service/lesson.rs:723` 图课时内容路径；`:779` 注释“学习图节点不读库”，`:780` `excerpts: Vec::new()`。
  - `learning_graph.rs:996` kick_growth、`:1218` growth_body、`:2084` gate_batch、`:2343` apply_batch；生长材料 = 结构快照 + 教练上下文 + taught_ledger，无 KB 通道。
  - 边界票 `docs/tracker/learning-goal-kb/tickets/t5-graph-course-kb-boundary.md` 仍 open：图课程是否接 KB 尚未裁决。
- **设计基线**：五段管线（Inventory → Triage → Evidence Structure → Batched Expansion → Validation/Disclosure）；细节与源码级机制见 d1 与 asset。
- **既有边界**：课程结构仅在课程内生效、概念表跨课程；图课程“结构对生长不供内容”的铁律不变；g1 不预设 t5 裁决。

## Decisions so far

- **d1 已结案（2026-10-21）**：不在 40 常量上做文章；采用“清点 + 分诊 + 证据驱动结构 + 分批展开 + 覆盖披露”的设计。见 [d1](tickets/d1-design-and-code-state.md)。
- **结构真相**：文件名/标题/目录只作弱信号；结构由证据 + 概念图/章节骨架 + 覆盖/顺序校验产生。脏库不能靠“更聪明的目录解析”解决。
- **不做**：把 40 直接调到 105/无限；让大纲 agent 自由浏览文件系统；静默丢弃判不准的文件；用 LLM 摘要替代原文证据。
- **Q1 术语（grill 2026-10-21）**：`SourcePlan`（传统课程蓝图之前的来源结构计划）与 `SourceCoverage`（来源覆盖报告）；`Structure`/`Coverage Gauge` 保留给学习图。
- **Q2 单位与范围**：文件级定角色，章节/主题（heading/anchor）级定覆盖；section 级噪声只处理明显可分离的推广尾段/出版页/导航页。v1 只做 Markdown；翻译副本/同 hash 重复自动排除并给理由；PDF/Word/图片后续立票。
- **Q3 计划权威性**：`SourcePlan` 是权威章节骨架（章节/依赖/覆盖/source anchor）；大纲 agent 只在章节内决定 lesson 粒度/标题/purpose；合并/拆分必须保留 `source_chapters` 映射与理由。
- **Q4 失败策略**：`uncertain` 保留为候选；概念图环/顺序冲突确定性回退 + warning；**只有零章节/零 core 硬失败**，部分覆盖缺口建课 + 醒目 warning；无 LLM 时规则分诊降级（`degraded=true`）不阻断。
- **Q5 成本门**：>40 文件或预估 >20 次 LLM 调用时，建课前展示估算并确认；低于阈值自动跑；所有阶段硬上限 + 可取消。
- **Q6 持久化**：一个版本化 `course_source_plan_json` 列承载 `{version, inventory, triage, plan, coverage}`；t1–t3 先用内存/WS 报告，字段稳定后 t4 迁移（owner 签核）。
- **Q7 代码落点**：`nomifun-learning` 拥有清点/分诊/结构计划编排与提示词；`KnowledgeService` 提供文件/检索 IO；`LearningCompleter` 提供 LLM；`nomifun-ai-agent` 不新增第二套 agent 循环。
- **Q8 生长复用**：A/E/F（共享类型与规则、结构化批次工具、覆盖/披露形状）本波随 t1/t3/t5 落地；B/C/D（证据 brief 进 `StructureInput`、图课时接 t3 grounding、图节点 source anchors/教练批 KB）等 `learning-goal-kb/t5` 裁决后另立票。g1 据此关闭。
- **Q9 混合文件**：新增文件级 `mixed` 角色；有任一相关段就不整文件排除，哪些 heading/anchor 进课程由 t2 的章节/主题覆盖决定；噪声段显式 skipped + anchor。
- **Q10 计划深度**：`SourcePlan` 只到章节 + 概念依赖 + source anchor；lesson 数/拆分/est 由 t3 按字数/标题自适应，plan 只管章节/概念/覆盖/顺序。
- **Q11 无标题锚点**：heading/anchor 优先 → 段落序号/行范围 → 最后 path 级覆盖并标 `anchor_uncertain=true` + warning；不编造标题、不排除无标题文件。
- **Q12 重复/翻译 canonical**：规则选 canonical（非翻译后缀原文优先、README/字符占比判语言、同语言最新/最高质量），其余标 `duplicate_of` 自动排除并给理由；v1 不做逐项用户确认，后续可加覆盖入口。
- **Q13 发布/审阅 UX**：不加独立“生成前审阅”页；大库成本确认弹窗里顺带展示 triage 计数与跳过理由摘要，生成中/详情看完整报告；保留内部 kill switch/legacy fallback，不做用户可见 beta flag。
- **Q14 迁移签核**：现在原则同意单列 `course_source_plan_json`；t1–t3 字段形状冻结后再最终签核并实施 t4。

## Tickets

- [d1](tickets/d1-design-and-code-state.md) 设计定稿 + 代码现状 + 生长课复用面盘点——**已关**（结论进本 map 与 asset）。
- [t1](tickets/t1-inventory-and-triage.md) 全量清点 + 规则/LLM 分诊（core/prereq/practice/mixed/noise/duplicate/uncertain，逐项理由；不确定保守保留）——**已关**（`run_kb_triage` + structured_batch，363 测试绿）。
- [t2](tickets/t2-evidence-driven-structure.md) 证据驱动的结构发现：多查询检索 → 候选概念/概念图 + 章节 spine → critique/revise → 确定性去环/拓扑/覆盖补齐——**已关**（`run_kb_structure` + `SourcePlan` 进 brief，407 测试绿）。
- [t3](tickets/t3-batched-expansion.md) 分批展开 + 截断自愈 + 来源映射/覆盖校验（40/批、拆半、单文件兜底、长文拆分/短文合并、标题忠实）——**已关**（`run_kb_expansion` + `SourceCoverage`，plan 可用时绕过大纲引擎，423 测试绿）。
- [t4](tickets/t4-persistence-and-migration.md) 单个 `course_source_plan_json` 快照列 + 迁移（高风险区，需 owner 签核；t1–t3 形状稳定后做）——**已关**（迁移 069 + `CourseSourcePlanSnapshot` v1 信封 + 同事务落库 + 读侧摘要四态，430 测试绿；owner Q14 签核记录在票面 Resolution）。
- [t5](tickets/t5-api-ui-smoke-docs.md) DTO/WS/UI/冒烟/域文档收口（clean/dirty/large 三库冒烟；双语文案；SourcePlan/SourceCoverage 术语）——**已关**（DTO 分面 + WS triage/source_plan/coverage 帧 + 成本门估算端点 + UI 新步骤条/确认弹窗/来源覆盖卡 + 37 键双语 + ADR-0014/契约/CONTEXT 词条；CLI `--triage-only/--plan-only/--report-json`；真机三库全 EXIT=0、dirty 判据全满足、15/16 倒挂消失且旧截断变显式 skipped+reason；436 测试绿；冒烟移交项与证据在票面 Resolution）。
- [g1](tickets/g1-graph-course-reuse.md) 生长课复用边界——**已关**（A/E/F 并入本波；B/C/D 等 `learning-goal-kb/t5`）。

## Not yet specified

- **无产品级未决**：Q1–Q14 已由用户按推荐确认；字段名、阈值、提示词、fixture 等属实现细节，归各票在实施时定，不再上抛。
- **外部依赖**：g1 的 B/C/D（图课程接 KB 结构参考/课时 grounding/source anchors/教练批）等 `learning-goal-kb/t5` 裁决；t5 裁决前不开图课程 KB 实施票。
- **t4 最终签核**：原则已同意单列；t1–t3 形状冻结后由 owner 做最终迁移签核（见 Q14）。

## Out of scope

- 在本 effort 内裁决“学习图课程是否接知识库”（归 `learning-goal-kb/t5`；g1 只做复用面）。
- 调整学习图生长治理（批次容量、就绪线、评审纪律）——那是生长产品波。
- 修改 `learning-goal-kb` 已关闭的 t1–t4 口径；本 effort 只在其上加“脏库/大教材”层。
- 为脏库写自动清洗/去重的全部格式适配（PDF/OCR/翻译对齐）——v1 只做 Markdown 文档面与规则可覆盖的重复/翻译副本。
