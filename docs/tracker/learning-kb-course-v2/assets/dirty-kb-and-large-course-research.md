# 大教材知识库的课程生成：同类方案调研与 allo 建议

日期：2026-10-21
范围：learnhub `docs/research/` 的同类深拆（只作二手资料，不把 learnhub 自身实现当方案）+ 网络上的成熟范式与课程生成研究。

## 一句话结论

成熟产品不靠“把采样上限调大”解决大教材，也不假设知识库是干净、有序、全相关的教材。它们先把**清点/分诊**和**结构发现**拆开：清点全部文件、判相关性/角色、剔除噪声；再用**内容证据 + 概念/章节骨架 + 覆盖与顺序校验**建立课程结构；最后按章/模块分批展开、按课时检索原文。40 文件上限之所以还在，是因为 allo 把“结构发现”压在同一次“全量大纲采样”里；要动的是管线形状，不是常量。

## 1. 问题复述

allo 当前传统课程：

- 大纲阶段：`sample_bases` 先取 40 个文件 / 960KB 的快照，交给大纲 agent；`co_read` 只能读这 40 个文件（`generation/sample.rs:16,24`；`course_outline_loop.rs:602,615`）。
- 课时正文阶段：t3 才按需 `search_bases` 检索全量来源库，并读主 source 全文（`service/lesson.rs:101,203`）。
- 因此 105 文件的 Rust 书只能进 40 个文件；没进大纲的章节不会产生课时，课时检索再强也补不回来。

这不是“按需调用没做”，而是**按需调用只做在内容层，结构层仍是单次有界快照**。

### 1.1 前提修正：库不一定是干净教材

上一版把“全量目录/章节树”当成结构真相，这个前提对真实用户库不成立。真实库可能是：

- 文件名不按阅读顺序（`0.md, 1.md, 10.md, 11.md, 2.md…`）；
- 标题层级缺失、混用、重复，或根本没有标题；
- 同一主题混着无关文件、会议记录、收据、网页剪藏、LICENSE/配置/脚本；
- 同一内容的多个版本、翻译副本、重复文件；
- 书的前言/目录/出版页/推广页与正文混在一起；
- 网页抽取残留导航、关注引导、相关阅读；
- 代码文件、notebook、图片与正文混放；
- 有些文件是“相关但不需要教”的背景材料，有些是“必须先补”的前置材料。

因此：**路径/标题只能当弱信号，不能当结构真相**。成熟做法是“清点 → 分诊 → 证据驱动的结构发现 → 覆盖/顺序校验 → 分批展开”，并在每一步显式披露“跳过了什么、为什么、哪些不确定”。

## 2. learnhub 研究文档里的同类做法

### 2.1 DeepTutor：Spine 先行的“活书”

来源：`docs/research/deeptutor-deep-dive.md` §B2；上游 <https://github.com/HKUDS/DeepTutor>（Apache-2.0，v1.6.13）。

- `BookEngine` 生命周期：`create_book`（KB/笔记本/聊天选择等四源输入）→ `IdeationAgent` 出 `BookProposal`（用户确认）→ `SpineSynthesizer` 出**章节树 Spine + 概念图 ConceptGraph**（用户确认，含成本估算）→ 页壳 + 后台队列**逐页编译**（`book/engine.py:17-36`）。
- 块生成以 `SourceChunk` 检索为 grounding；不是把整本书塞进一次生成。
- `textbook_struct/` 是**纯确定性、零 LLM** 的教材结构抽取：从 MinerU layout.json 重建章节树（`chapter_rebuild.py:1-11`），并用“页眉从不说谎”补章界。
- 知识库侧：多引擎 RAG；系统 prompt 只放**≤20 个文档的一行摘要 manifest**，全量清单走 `kb_files` 工具（`manifest.py:29-41`）——**清单 + 工具**，而不是全量正文。

对 allo 的映射：DeepTutor 的“Spine（全量章节树）→ 逐页编译（按需检索）”几乎就是 allo 需要的“全量结构索引 → 分章展开 → 课时检索”。它的成本估算/用户确认/可暂停续编也值得借用。

### 2.2 LookatStudy：分批做结构设计 + 截断自愈

来源：`docs/research/lookatstudy-non-ui-deep-dive.md` §5；上游 <https://github.com/Kaiji-Z/LookatStudy>（MIT，v0.40.0）。

5 步导入管线（`import-job-service.ts` 单一入口）：

1. 清点：永远现拉，产出 readme + fileList + fullTree + treeHash（无 LLM）。
2. 文件角色分类：规则预处理 + LLM 判 original/practice/skip/translation，**40 文件/批**。
3. 标题大纲：每文件 H1/H2/H3 + 分段字数（**无 LLM**）。
4. 结构设计：LLM 分 section/lesson/world，长文件按 anchor 拆，附属段不独立成课，**≤40/批**。
5. 落库：按标题序号截正文、图片内联、同步写库、完整性验证。

关键工程点：

- **截断自愈**：JSON 被 token 上限掐断/形状不对 → 批拆半各调（输出体量指数缩小），二分到单文件仍失败 → h1/文件名规则兜底一课，**绝不抛、绝不丢内容**（`import-llm-service.ts:727-768`）。
- **章节标题忠实原则**：沿用原书章名；原书无编号体系时不得发明“第 N 章”（学习者要靠标题回原书）。
- 长文件拆分：每课目标 3000–8000 字，H2>8000 且有 H3 按 H3 拆，<1000 字与邻课合并。
- KC 懒生成：首次点课时才生成摘要 + 3–7 个知识点，不提前全量烧钱。

对 allo 的映射：**结构设计可以分批做，不必一次看完全书**；先有确定性标题大纲，再按批让 LLM 设计结构，截断时拆半而不是丢内容；章节标题要忠实原书。

### 2.3 OpenMAIC：两阶段“大纲 → 逐场景”

来源：`docs/research/openmaic-deep-dive.md` §A1、§A10；上游 <https://github.com/THU-MAIC/OpenMAIC>（MIT）。

- Stage 1 大纲：输入需求 + 可选文档，单次调用出 JSON `{courseTitle, outlines[]}`；Stage 2 逐场景生成内容与导演动作，可对单条大纲独立重试/upsert。
- 文档原料有预算：`bundle.ts` 每文档基准 1500 字符、预留 40% 比例、词边界截断；大纲 prompt 里还把资源约束写成硬数字（interactive ≤1–2、PBL ≤1、每场景 1–3 分钟）。
- 它的“大纲”仍是**一次调用看截断后的文档**，扁平顺序、无 DAG；对 100+ 文件教材并没有真正解决覆盖问题。可借鉴的是两阶段形状与失败隔离，不是它的文档预算。

### 2.4 StudyMate：教学主干不等于资料目录

来源：`docs/research/studymate-deep-dive.md` §4–§6；上游 <https://github.com/Miaotofu01/Study-Mate>（MIT）。

- curriculum-designer 先读资源清单再设计，明确“**主干不跟资料目录走**”；节点粒度 40 分钟学习单元，`prerequisites[]` 强制位次不倒挂、Kahn 环检测；项目里程碑与普通课同级插入。
- 角色交接用**文件路径**而不是把全文贴进上下文：“上游产出给路径不转述”“搬运不转抄”；产物落 `.stage/<角色>-<节点>/deliver/`，总控验收后原样搬入。
- 这对 allo 是一个反向约束：全量 TOC 是**结构输入**，但课程主干可以按教学法重组；重组必须带来源映射和理由，不能变成“模型随手改章节”。

### 2.5 跨仓库综合

`docs/research/cross-repo-synthesis.md` §4.5 把 LogiNote 的混合检索经验（bigram+BM25+加权融合+短语加成）与 DeepTutor 的引擎契约、索引多版本、kb eval、manifest 合并为检索底座。它没有给出“大教材结构保真”的直接方案，但提供了两个原则：**检索质量要可复算**、**索引计数不可信就不报**。

### 2.6 源码实读：脏库、覆盖、顺序、失败的具体机制

这一节直接读上游源码（浅克隆 HEAD，2026-10-21），不是只看研究文档。

#### DeepTutor — SourceExplorer：先检索证据，再谈结构

`deeptutor/book/agents/source_explorer.py`：

- **查询设计**：把已确认的 BookProposal/用户意图变成 4–8 条短查询，要求覆盖“概述/定义、机制、案例、易错点、应用、历史/对比”等不同角度；查询语言跟源材料走，不跟书走（`_QUERY_LANGUAGE_HINT`）。
- **检索**：按 `(KB × query)` 并行检索，`RETRIEVAL_CONCURRENCY=6`、`MAX_RETRIEVAL_CALLS=48`；pair 按 query-major 排列，预算被砍时每个 KB 仍能分到查询，不会饿死后面的库。
- **跨源公平**：`_balanced_slice` 不按全局 score 排序（不同引擎分数不可比），而是每个 source 内部排序后轮询取样；避免某个库的高分把其他来源挤出合成输入。
- **去重/裁剪**：`_dedupe_and_clip` 用 `source::ref::text[:200]` 去重，最多 96 个 chunk；送给摘要的是每源均衡的 24 个 chunk，每个截到 320 字。
- **摘要产物**：`summary`（4–8 句主题/证据强弱/缺口）、`candidate_concepts`（8–20 个候选概念）、`notes`（≤5 条矛盾/注意事项）；同时记录 `coverage`（每个 source 贡献多少 chunk）和 `skipped_knowledge_bases`，无法检索的库显式写进 notes，不伪装成“没有相关内容”。
- **意义**：结构不是从文件名/目录推出来的，而是从“为这个目标检索到了什么证据”长出来的。这对脏库/乱序库天然更稳。

#### DeepTutor — SpineSynthesizer：草稿 → 评审 → 修订 → 确定性校验

`deeptutor/book/agents/spine_synthesizer.py` + `book/prompts/*/spine_synthesizer.yaml`：

- **Draft**：一次调用同时产出 `concept_graph`（节点/边，边方向是前置→依赖）和 `chapters`（每章 `covers` 概念节点、`prerequisites`、`source_anchors`、`learning_objectives`）；要求每章至少覆盖一个节点、每个节点至少被一章覆盖、尽量符合拓扑序、不得有环。
- **Critique**：第二次调用专门找问题，类别固定为 `coverage | ordering | granularity | redundancy | missing_anchor | cycle | other`；要求指出“没被覆盖的节点/章节 prerequisite 与图不一致/环/过宽过窄/重复/有证据但 source_anchors 为空”。
- **Revise**：第三次调用按 fix_hint 修；含糊时优先保证覆盖度与拓扑一致性，不引入未被提示的新章节。
- **确定性后处理**（不靠模型自觉）：
  - `_remove_cycles`：DFS 找环，丢掉“rationale 最短”的那条边（置信度代理），最多 20 次；
  - `_topological_sort`：按章节 `covers` 把概念依赖提升到章节级，Kahn 排序，平局按原始顺序；有环则回退原序；
  - `_ensure_full_coverage`：没被任何章节覆盖的概念，按 Jaccard（标题+摘要+目标 vs 概念 label+description）挂到最相关章节；
  - `_build_chapter_map`：把概念图提升成章节级思维导图，多根时加虚拟根；
  - 没有章节时兜底一个 Overview 章。
- **意义**：覆盖和顺序是**可校验、可修复**的，不是“模型说它对就对”。脏库里的无关文件不会自动成为章节；缺章/乱序会被 critique 和确定性后处理抓出来。

#### DeepTutor — BookEngine / 编译：分批编译、失败隔离、来源披露

`deeptutor/book/engine.py`：

- 生命周期：`create_book → BookProposal（用户确认）→ confirm_proposal → Spine（用户确认，带成本估算）→ confirm_spine → 页壳 + 后台队列逐页编译`；`resume_book` 保留已完成页，`rebuild_book` 从 spine 重来。
- 每本书一个 `_BookRuntime`：`asyncio.Queue` + 单 worker + 按 page id 的 in-flight 表；并发请求同一页会 join 同一次编译，不重复烧钱。
- **断路器**：连续 2 页因 provider 级错误失败就暂停全书，避免把剩余章节磨成半成品；已完成页保留。
- `_source_quality_summary`：记录请求的 KB、实际有 chunk 的 KB、缺失 KB、chunk 数、warnings；如果选了来源但 0 chunk，明确告警。
- 每页编译：`SectionArchitect` 规划 block，按 block 生成、每块落盘、失败重试；`_rag_helpers.optional_rag_lookup` 先读 cached exploration chunks，再回退 live RAG；RAG 报 `error_type`/`needs_reindex` 时不把错误文本当证据；anchors 带 kb/ref/snippet。
- **意义**：大教材不是一次生成，而是“先确认骨架，再逐页编译”；单页失败可重试/可跳过，来源缺失可见。

#### LookatStudy — 导入管线：先分诊，再分批设计，截断自愈

`src/main/services/import-llm-service.ts`：

- **Step 2 文件角色分类**：
  - 规则预分类：LICENSE/CONTRIBUTING/CHANGELOG 等元数据直接 `skip`；检测翻译布局（microsoft/parallel/suffix）并把翻译副本从原文候选分流；suffix 成对翻译做配对，孤儿翻译保守留原文。
  - 剩余文件交 LLM，输入 README + 完整目录树，输出 `original | practice | translation | skip`；40 文件/批。
  - 翻译角色必须给出 `lang` 与 `translates`，且 `translates` 必须指向真实文件（全量集合校验），否则保守当原文——**不丢内容，只丢分类精度**。
- **Step 3 标题大纲**（无 LLM）：每文件 H1/H2/H3 + 每段字数 + 正文开头 preview；这是结构信号，不是最终结构。
- **Step 4 结构设计**：
  - 输入 README（前 4000 字）+ 每文件 `h1 / preview / totalChars / headings[]`；40 文件/批。
  - Prompt 明确处理脏库：无 README 时按文件名/目录/类型设计；无子标题标 `(无子标题)`；网页尾部推广段由模型判正文边界；出版方介绍/宣传页不单独成课；代码文件整体一课；长文件按 H2/H3 和字数拆（每课 3000–8000 字），<1000 字与邻课合并；**章节标题忠实原书，不发明“第 N 章”**。
  - 输出校验：`file` 必须在 validSet（防幻觉文件），`attachImages` 只接受真实孤儿图路径，`world` 由 Step 4 判而不是沿用 Step 2。
  - **截断自愈**：JSON 截断/形状不对 → `StructureParseError` → 批拆半递归；单文件仍失败 → `h1/文件名` 兜底一课；网络/看门狗错误不重试（无意义，交给断点续跑）。
- **落库守卫**：空结构（0 课时）直接报错，绝不落“空课程”；课程行与节点在无 await 的同步段一次写入，取消/失败不留半成品。
- **意义**：脏库不是靠“更聪明的目录解析”解决，而是靠“规则 + LLM 分诊 + 分批设计 + 兜底不丢内容”。这套机制可以直接映射到 allo 的建课管线。

#### OpenMAIC — 文档清洗与预算

`lib/document/transforms/noise-removal.ts`、`lib/document/bundle.ts`：

- PDF 噪声清洗：删除重复页眉/页脚/页码（至少 3 页、覆盖 ≥60% 页面才判重复）；这是版面噪声，不是主题相关性。
- 文档预算：每个文档基准 1500 字符，预留 40% 按比例分配，词边界截断；多文档按 order 拼接，图片按 visionPriority/页码排序。
- 两阶段：大纲 JSON → 逐场景生成；场景可独立重试/upsert。
- **意义**：它能清洗版面噪声，但**不解决主题相关性/乱序**；所以不能把 OpenMAIC 当成脏库方案，只能借它的“清洗 + 预算 + 两阶段”工程形态。

#### StudyMate — 教学主干与资源清单分离

`preset/skills/curriculum-designer/SKILL.md`、`preset/skills/resource-scout/SKILL.md`：

- resource-scout 只收高可信来源（权威教材/官方文档），每条标注服务哪些 node；找不到的写 `## Gaps`，不拿低质来源凑数；取不到完整正文就放弃并记缺口，禁止拼接/转写。
- curriculum-designer 先读资源清单再设计，但明确“**主干不跟资料的目录走**”；按知识依赖建 DAG，节点粒度 40 分钟，`prerequisites` 必须真实存在、位次不倒挂、环检测；项目里程碑作为实验课节点插入。
- **意义**：结构是教学法 + 资源依据的合成，不是知识库目录的复制。对脏库尤其重要：目录只能提供线索，主干要按依赖重建。

## 3. 网络上的成熟范式

### 3.1 经典 map-reduce / refine

LangChain 的 `MapReduceDocumentsChain`：逐文档 map（可并行），再 reduce 合并；`RefineDocumentsChain`：逐文档串行 refine，保真度更高但不可并行；`ReduceDocumentsChain` 在文档过多时递归 collapse。  
来源：<https://js.langchain.com/docs/tutorials/summarization/>

适用：把“每章摘要 → 合并成课程骨架”做成 map-reduce；代价是丢细节/串行慢。它是所有层次化方案的基线。

### 3.2 DocumentSummaryIndex：先摘要选文档，再取节点

LlamaIndex `DocumentSummaryIndex` 为每个文档生成摘要并保存其全部节点；查询时**先用摘要选相关文档，再取这些文档的节点**。每个文档一次摘要调用；宽泛命中可能把大量节点灌进上下文。  
来源：<https://developers.llamaindex.ai/python/examples/index_structs/doc_summary/docsummary/>

适用：百级文件的“先选章，再读章”。比 allo 现在的“全量采样”更接近“结构索引 + 按章展开”。

### 3.3 TreeIndex / RAPTOR：层次摘要树

- LlamaIndex `TreeIndex`：节点为叶子，自底向上逐层摘要成树；查询从根到叶，`child_branch_factor` 控制每层展开几个孩子。适合超长文档的粗到细浏览；构建需要逐层 LLM 调用。  
  来源：<https://developers.llamaindex.ai/python/framework/module_guides/indexing/index_guide/>
- RAPTOR（ICLR 2024）：chunk → 嵌入 → 聚类 → LLM 摘要 → 递归成树；检索可用树遍历或“扁平化树”跨层取 top-k。适合长文档的整体理解与多跳；第三方审计提示摘要节点有少量轻微幻觉，构建成本为每层每簇一次调用。  
  来源：<https://arxiv.org/abs/2401.18059>

适用：**无清晰目录/章节**的大语料。对有 `01-` 章节的教材，确定性 TOC 更便宜、更忠实；RAPTOR 可作为目录解析失败时的后备。

### 3.4 GraphRAG：社区摘要 + 全局/局部检索

GraphRAG 把语料切成 TextUnits，抽实体/关系/主张，用 Leiden 做层次社区检测，自底向上生成社区摘要；查询分 Global（社区摘要）、Local（实体邻居）、DRIFT（邻居+社区）、Basic（向量 top-k）。  
来源：<https://microsoft.github.io/graphrag/>

适用：跨文档主题归纳、全局问题；索引成本高，且不保证教材章节顺序。对“按教材结构开课”不是首选，但对“这门书讲了哪些主题、主题之间什么关系”很强。

### 3.5 课程生成研究

- **Instructional Agents**（arXiv 2508.19611）：多智能体端到端生成 syllabus、LaTeX slides、lecture scripts、assessments；四种人机协作模式（Autonomous / Catalog-Guided / Feedback-Guided / Full Co-Pilot）。说明课程生成的主流形态是**角色分工 + 分阶段产物**，不是单次大 prompt。  
  来源：<https://arxiv.org/abs/2508.19611>
- **Toward Automated Curriculum Design**（ITiCSE 2026，DOI 10.1145/3803401.3812038）：分层生成——先高层主题/学习目标，再小节描述与 header，最后按 header 逐段生成全文（生成每段时带前一个 header 的文本保持连贯）；JSON schema 约束 + 迭代改进。  
  来源：<https://doi.org/10.1145/3803401.3812038>
- **CoPaa**（ICIC 2025，DOI 10.1007/978-981-96-9901-8_6）：五模块——内容解析引擎（抽取教材结构）、指令链生成、上一步描述生成（保持连贯）、并行输出（LangGraph）、可控校准（human-in-the-loop）。  
  来源：<https://doi.org/10.1007/978-981-96-9901-8_6>
- **Beyond Search Engines**（arXiv 2412.07422）：LLM 从课程标题生成主题，与 YouTube 播放列表主题对比；覆盖不足/内容过时是课程生成的核心质量轴，GPT-4 生成的 topic 比 YouTube 基线更准。  
  来源：<https://arxiv.org/abs/2412.07422>

共同点：**层次化/分阶段生成、结构解析先行、并行或分批输出、人在环校准**。没有一篇把“一次看完全部原文”当作目标。

## 4. 方案对照

| 方案 | 结构覆盖 | 内容接地 | 成本 | 确定性 | 适用 |
| --- | --- | --- | --- | --- | --- |
| 现状：40 文件全量采样 | 只覆盖 40 文件 | 采样全文 | 中 | 采样确定，大纲模型决定结构 | ≤40 文件、结构简单 |
| 直接调大 cap | 覆盖更多，但仍会撞下一堵墙 | 全量塞 prompt | 高，随文件数线性涨 | 输入确定，模型易在长 prompt 丢结构 | 不推荐作为主方案 |
| 仅全量 TOC/清单 + 大纲 | 干净库全章节可见；脏库会把噪声/错序带进结构 | 只给标题/摘要 | 低 | 高（但只对干净库成立） | 干净、有序、全相关的教材 |
| **清点 + 分诊 + 证据驱动结构（推荐）** | **core 全覆盖，noise/duplicate 显式排除** | 证据 chunk + 按章检索 | 中（分诊按批、结构 3 次调用） | 中（证据/覆盖校验；不确定项标出） | **脏库、乱序库、大教材** |
| 分批结构设计（LookatStudy） | 全量，按批处理 | 每批读本批文件 | 中，按批线性 | 中（批边界确定，批内模型决定） | 大文件集、导入型 |
| 两阶段：Spine → 逐页/逐场景 | 全量 spine | 每页/场景检索 | 中高，按页/场景 | 中（spine 可确认/缓存） | 书/课生成（DeepTutor/OpenMAIC） |
| DocumentSummaryIndex | 文档级摘要全覆盖 | 摘要选文档→取节点 | 中（每文档一次摘要） | 中 | 百级文档的“先选章再读章” |
| TreeIndex/RAPTOR | 层次摘要覆盖 | 树检索 | 高（逐层摘要） | 中低（聚类/摘要不确定） | 无目录/超长语料 |
| GraphRAG | 社区摘要覆盖 | 社区/实体检索 | 很高（索引重） | 中低 | 跨文档主题/全局问题 |

## 5. 给 allo 的建议（脏库版）

### 5.0 目标形态：五段管线

1. **清点 Inventory**：枚举全部文件（不只 md），拿路径/类型/大小/哈希/标题/开头 preview；不判断相关性。
2. **分诊 Triage**：规则 + LLM 判每个文件的角色与相关性；输出 `include / prerequisite / practice / noise / duplicate / uncertain` 与理由；不确定的保守保留。
3. **结构发现 Structure synthesis**：目标驱动的检索证据 → 主题/候选概念 → 概念图 + 章节 spine；覆盖/顺序/粒度/环评审；确定性拓扑排序与覆盖补齐。
4. **分批展开 Expansion**：按模块/章节加载对应文件（或检索）设计 lesson；40/批，截断拆半，标题兜底；长文件按 H2/H3 拆，短文件合并。
5. **校验/披露 Validation**：每章每课有 source anchor；覆盖报告能回答“每个文件/主题去哪了”；跳过/不确定/低置信显式披露；成本/调用数可见。

### P0：清点 + 分诊（脏库的前提）

不要先做“全量 TOC”。先做**全量清点 + 分诊**：

- 枚举所有支持的文件，记录 path/type/size/hash/标题/开头 preview；duplicate/near-duplicate 用 hash/minhash 先标出来。
- 规则先处理高置信情况：LICENSE/CONTRIBUTING/CHANGELOG/配置/空文件/导航页 → noise；翻译布局（microsoft/parallel/suffix）→ 配对/分流；同一内容多版本 → 标记候选。
- 剩余文件交 LLM 分诊，输入“目标 + README/目录树 + 每文件标题/preview + 文件类型”，输出：
  - `core`：本课程要教的内容；
  - `prerequisite`：目标需要但不必单独成课的前置背景；
  - `practice`：练习/lab/代码；
  - `noise`：与目标无关、元数据、推广页、空导航；
  - `duplicate`：重复/翻译副本/旧版本；
  - `uncertain`：判不准。
- 40 文件/批；不确定 → 保守保留（`uncertain` 进后续人工确认），绝不静默丢内容；每个排除项必须有 reason。
- 产出 `KbTriageReport`（含总文件数、各角色计数、排除清单+理由、置信度、未判项），UI/日志可查；大库可让用户确认/覆盖。
- 意义：先把“哪些文件值得进结构”定下来，后面的结构发现才不会被无关文件污染；也避免把整本无关的库硬做成课。

### P1：证据驱动的结构发现（不把目录当真相）

- **检索证据**：从目标生成 4–8 条多角度查询（概述/机制/案例/易错/应用/对比），对全量来源库 `search_bases` 并行检索；按来源均衡取样（不同引擎分数不可比），去重/裁剪，保留每个来源的贡献计数。
- **主题摘要**：把均衡后的证据摘要成 themes/strong evidence/gaps + 8–20 个候选概念 + 注意事项；明确哪些来源没有贡献证据。
- **概念图 + 章节 spine**：一次草稿同时出 `concept_graph`（前置→依赖）与 `chapters`（每章 `covers`、`prerequisites`、`source_anchors`、目标）；要求每章至少覆盖一个概念、每个概念至少被一章覆盖、尽量拓扑序、无环。
- **评审/修订**：专门评审 `coverage / ordering / granularity / redundancy / missing_anchor / cycle / other`；按 fix_hint 修，含糊时优先覆盖与拓扑一致。
- **确定性后处理**：DFS 去环（丢 rationale 最弱的边）、Kahn 拓扑排序（平局按原序，有环回退）、Jaccard 覆盖补齐（未覆盖概念挂到最相关章节）、章节级 mind map；无章节时兜底 Overview。
- **标题/路径的角色**：只作为 seed/弱信号（例如给候选概念、给章节命名参考、给顺序 tie-break），不作为结构真相；标题缺失时用 preview/内容类型兜底。
- **用户确认**：大课程在 proposal/spine 两处确认（带成本估算）；小课程可自动，但保留“结构不确定/低覆盖”告警。

### P2：分批展开 + 按需检索

- 按 spine 的模块/章节展开，每批只加载该章的 `core/prerequisite/practice` 文件（或按需检索），40 文件/批；批内输出 lesson 时保留 `source`、`anchor`、`source_chapters`。
- 批太大或输出截断 → **批拆半**（LookatStudy `StructureParseError` 路径）；单文件仍失败 → `h1/文件名` 兜底一课；网络/看门狗错误不重试，交给断点续跑。
- 长文件按 H2/H3 + 字数拆分（目标每课 3000–8000 字），<1000 字与邻课合并；章节标题忠实原书，不发明“第 N 章”。
- 保留现有 t3 课时正文检索（主 source 全文 + 40KB 补充）；每个 block/lesson 带 source anchors；检索报错（error_type/needs_reindex）时不要把错误文本当证据。
- 合并按 spine 顺序确定性排序；每个文件的最终去向（成课/前置/练习/跳过）写入报告。

### P3：无结构/千级语料再上层次摘要

- 如果分诊后 `core` 仍无标题/无结构，或文件数到千级：用 `DocumentSummaryIndex`（每文档摘要 → 先选文档再取节点）或 `TreeIndex`/RAPTOR（层次摘要树）；GraphRAG 只在需要跨文档主题归纳时考虑。
- 摘要缓存/可重建；LLM 摘要不能替代原文证据，source anchor 仍要回到原文。

### 护栏

- **不静默丢内容**：每个 exclude 都有 reason；`uncertain` 默认保留并标记；空结构直接报错，不落空课程。
- **覆盖报告**：总文件数、core 覆盖、prerequisite 去向、practice 归属、noise/duplicate 排除、未判项；每章/每课 source anchor。
- **顺序不确定要显式**：拓扑排序/依赖推断有冲突时标 warning，不强行排序；允许教学法重组，但带 `source_chapters` 与理由。
- **失败隔离**：单批失败拆半；单章失败不拖垮全课；provider 级连续失败触发断路器；已完成产物保留。
- **成本护栏**：清点/规则分诊零 LLM；LLM 分诊按批；结构发现一次草稿+评审+修订；展开按模块计费；给每阶段 token/调用上限。
- **可复现**：清点/分诊结果缓存；结构索引/摘要带 hash；同库同目标重跑可对比差异。

### 不建议

- 不要把“文件名/标题顺序”当结构真相：脏库的文件名可能完全不按阅读顺序。
- 不要把 40 直接调到 105/无限：成本/延迟线性涨，长 prompt 更容易丢结构，下一本 300 文件的书还会撞墙。
- 不要让大纲 agent 自由浏览文件系统：不可复现、容易漏章、提示注入面变大。
- 不要用 LLM 摘要替代原文证据：摘要会漂移/幻觉，source anchor 必须能回原文。
- 不要静默丢掉“判不准”的文件：保守保留 + 标记 uncertain，比悄悄漏一章更安全。
- 不要假设所有文件都与主题相关：先分诊，再结构；无关文件不进课程是正确行为，不是“覆盖不全”。

## 6. 验证计划（建议）

准备三类库做 A/B：

- **干净教材**：19 文件《数据结构思维》——验证不回归。
- **脏库**：在干净教材里混入无关文件（会议记录/收据/网页剪藏/License/配置）、翻译副本、重复版本、打乱文件名顺序、删掉部分标题、加入网页推广尾段——验证分诊/结构发现/跳过理由。
- **大教材**：105 文件 Rust 书——验证 40 文件上限下的覆盖与顺序。

对比：

- A：现状 40 文件采样；
- B：清点+分诊+证据驱动结构+分批展开（P0–P2）。

指标：

1. **分诊质量**：core 文件召回率、noise/duplicate 排除准确率、uncertain 比例；是否有静默漏文件。
2. **结构覆盖**：应教主题/章节是否都有模块，或显式“跳过+理由”；无静默缺章。
3. **顺序**：无未经解释的倒挂；不确定顺序有 warning。
4. **来源**：所有 lesson.source 可回溯到真实文件；每章至少一个 source anchor。
5. **正文质量**：抽 2–3 节课时，检查 grounding 术语/公式/例子仍落在原文。
6. **成本/延迟**：分诊/结构/展开三阶段 token、调用数、总时长；与 A 对比。
7. **可复现**：同库同目标重跑，分诊/结构结果稳定或差异可解释；缓存命中。

验收线：脏库的 core 主题全覆盖、noise 不进入课程且排除有理由、无静默缺章；大教材有完整覆盖/不确定报告；干净教材正文 grounding 不差于 A，成本在可接受范围。

## 7. 参考

### learnhub 研究文档（本地）

- `docs/research/deeptutor-deep-dive.md` §A2/A3/B2（上游 <https://github.com/HKUDS/DeepTutor>）
- `docs/research/openmaic-deep-dive.md` §A1/A10（上游 <https://github.com/THU-MAIC/OpenMAIC>）
- `docs/research/lookatstudy-non-ui-deep-dive.md` §5（上游 <https://github.com/Kaiji-Z/LookatStudy>）
- `docs/research/studymate-deep-dive.md` §4–§6（上游 <https://github.com/Miaotofu01/Study-Mate>）
- `docs/research/cross-repo-synthesis.md` §4.5

### 网络

- RAPTOR: <https://arxiv.org/abs/2401.18059>
- LlamaIndex DocumentSummaryIndex: <https://developers.llamaindex.ai/python/examples/index_structs/doc_summary/docsummary/>
- LlamaIndex index guide (TreeIndex): <https://developers.llamaindex.ai/python/framework/module_guides/indexing/index_guide/>
- GraphRAG: <https://microsoft.github.io/graphrag/>
- LangChain summarization (map-reduce/refine): <https://js.langchain.com/docs/tutorials/summarization/>
- Instructional Agents: <https://arxiv.org/abs/2508.19611>
- Toward Automated Curriculum Design: <https://doi.org/10.1145/3803401.3812038>
- CoPaa: <https://doi.org/10.1007/978-981-96-9901-8_6>
- Beyond Search Engines: <https://arxiv.org/abs/2412.07422>
