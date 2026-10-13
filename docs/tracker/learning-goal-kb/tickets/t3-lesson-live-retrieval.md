---
id: t3
title: "课时写作接回知识库：实时检索 + 全文读取，快照降级为兜底"
type: task
status: closed
assignee: zhangyj
blocked_by: [t1]
part_of: ../map.md
---

# t3: 课时写作接回知识库：实时检索 + 全文读取，快照降级为兜底

**What to build:** 课时内容（节/正文/题目）生成从"冻结的单文件 8KB 前缀"升级为**实时读库**：
按课时标题/purpose 词条经 `search_bases` 检索（已有跨库词频检索，不新建系统），以句柄
（`encode_doc_handle`）读**全文/章节**作为 grounding；快照摘录降级为兜底与对照。**锚定纪律**：
课时主 source 不变，检索只做补读（source 覆盖不到或内容不足时向同目录/相关文件扩读），
检索结果不替换主引用。同期修 `search_bases` 的 CJK 检索行为（`query_terms` 中文整句退化
为无召回）与多引用支持。

**Blocked by:** [t1 目标输入](t1-goal-input.md)（课时上下文的 goal 字段）。t2 可并行。

**Status:** ready-for-agent

**实施前拍板（2026-10-13 裁决）**：词条化 **v1 = 确定性切片**（复用 t2 已落地的
`generation/terms.rs` 纯函数，判据"不引 LLM"以此为准）；目标词条集随 `samples_meta_json`
持久化、建课后稳定复用。**LLM 词条扩张（目标同义词补充）留作后续增强**：确定性切片是
其自然回退，将来加也不改变任何接口。t2 实施记录与本裁决的一致性由本条统一。

- [x] **检索质量修正（前置件）**：`KnowledgeService` 检索侧修 CJK 召回——`query_terms`
      整句匹配失败时对 CJK 段降级为**字符 bigram**（整句命中必然蕴含其二元命中，不丢
      召回）；只动词条生成与打分，返回结构与缓存不动；补中文检索单测（整句查询有召回、
      纯英文行为不变）。
- [x] **课时读库面**：learning crate 拿注入的 `KnowledgeService`，课时生成新增检索步骤
      （课时标题 + purpose 词条 + 目标词条集挑选项）→ top-N 命中 → 读**全文**（
      `read_file(kb_id, rel_path)`；句柄编码是 agent 会话面的事，本面直读即可）。
      检索失败/超时（20s 预算）降级到快照摘录，**不阻断生成**。
- [x] **上下文重构**：`LessonExcerpt` 扩为**多片段**结构（保留 `path`/`text` 兼容位，
      增 kb 归属与来源标记 primary/supplement）；primary = 大纲引用的 source 文件
      **全文**（替代 8KB 前缀）；补充 = 检索扩读片段。**token 预算守卫**（建议 primary
      ≤24KB、补充单片段 ≤8KB、合计 ≤40KB，超预算按相关度截断并在披露里标注）。
- [x] **提示词更新**：节/正文/题目三处 "Cited file excerpt" 改为多片段 grounding 表述；
      `SourceSpan` 增可选 `kb_id` 归属（兼容老课程空值）——`validate_blueprint` 的
      unknown_source 语义保持"必须来自已采样/来源库的真实路径"。
      → 提示词三处已改；**`SourceSpan.kb_id` 与 `validate_blueprint` 未动**：实施导航明确
      把这两处（含课时 source 多引用）划给 t4 多库票，见下方「与票面的偏差」。
- [x] **agent 循环接线**：`lesson_content_loop.rs` 的 grounding 装配同步（含
      `context.excerpt = None` 的历史清理点）；学习图节点路径不受影响（不读库）。
      → 清理点统一收敛为 `LessonGenerationContext::clear_grounding()`（兼容位与多片段
      一起清，不留半清状态）。
- [x] **快照兜底语义**：库不可达/文件被删 → 快照仍可生成（现状能力保留），披露降级。
- [x] 读数：课时生成记命中数/扩读文件数（进程内日志或 WS 事件即可——**本分支无
      learning-smoke**，那是 feat/learn0928 的产物，不新建 CLI）。

**验收：**

- 大文件（8KB 之后）中段的事实出现在课时正文（今天不可能）。
- 建课后修改知识库文件 → 新生成的课时反映修改（陈旧快照问题解除）；已生成课时不变。
- **锚定专项**：检索命中散落多个文件时，课时引用的主 source 不被替换；扩读片段在正文中
  可溯源（库+文件+节）。
- **课程式库专项**：primary 读源文件全文 + 按结构序扩展，不因检索结果改写阅读线。
- 源库被删/不可达 → 课时仍可生成（快照兜底），前端披露降级态。
- 中文检索单测：整句查询有召回；`cargo test -p nomifun-knowledge -p nomifun-learning`
  全绿。

## Resolution

**结论速览（2026-10-13）**：传统课时的写作 grounding 已接回知识库——生成时先**实时读库**
（primary = 大纲主 source 的**全文**；补充 = `search_bases` top-N 命中读全文），快照摘录
**降级为兜底**；检索侧 CJK 整句零召回已修（字符 bigram）；预算守卫按 t2 同一把尺做
「超预算按相关度选摘」；读数走 grounding WS 帧 + 进程内日志，前端在生成期间披露降级态。
**不做**（留给 t4 多库票）：`SourceSpan.kb_id`、课时 source 多引用、`validate_blueprint` 语义。

### 落地要点

- **前置件：CJK 检索召回**（`nomifun-knowledge/src/service.rs`）。`query_terms` 对每个查询
  词条里的 CJK 段追加**相邻字符二元**（整句命中必然蕴含其二元命中，只补召回不丢召回）；
  纯英文/含空格查询词条集**逐字不变**，双字中文词与整句同形去重（短词不劣化）；词条总数
  封顶 `MAX_QUERY_TERMS = 64`（防长段落展开成数百次子串扫描）。`score_md` 加权、搜索缓存、
  `search_bases` 返回结构均未动。
- **读库面**（`nomifun-learning/src/service/lesson.rs::assemble_lesson_grounding`）：
  - 查询词 = 课时标题 + purpose 经 `generation/terms.rs::term_slices`（t2 同一切片器）成词条，
    再追加**目标词条集**（拍板项：建课时切好、随 `samples_meta_json` 的 `SampleDisclosure`
    新增 `goal_terms` 持久化，建课后稳定复用；旧课程该字段为空 → 对 `learning_goal` 重切，
    同一纯函数结果一致）；词条以空格连接交给 `search_bases`。
  - primary：`read_file(kb_id, source.path)` 全文（20s 预算内）→ 读不到（库不可达/文件被删/
    读出空）**降级为快照摘录**并计降级；检索结果**永不替换** primary（锚定纪律）。
  - 补充：`search_bases(kb_id, query, LESSON_RETRIEVAL_HITS = 5)` → 逐条 `read_file` 全文；
    主 source 自己命中的条目跳过；单文件读不到只跳过该条。检索失败/超时（`LESSON_RETRIEVAL_BUDGET
    = 20s`）只丢补充片段，**不阻断生成**。
  - 顺序：**相关度决定读什么**（检索选入与预算丢弃按分），**结构决定按什么顺序读**
    （`sort_supplements_by_path` 交付前回到路径字典序，primary 恒在首位）——课程式库专项
    「按结构序扩展、不改写阅读线」。
- **上下文结构**（`lesson_draft.rs`）：
  - `LessonExcerpt`：保留 `path`/`text` 兼容位，新增 `kb_id`/`kb_name`（库归属）、`heading`
    （检索命中章节）、`role`（`ExcerptRole::Primary | Supplement`）；新增
    `role_label()`/`provenance_label()`（库+文件+章节的可溯源标签，渲染与披露共用）。
  - `LessonGenerationContext`：保留 `Option<LessonExcerpt> excerpt` 兼容位，新增
    `excerpts: Vec<LessonExcerpt>`（primary 在前）；读取入口 `grounding()`（`excerpts` 空时
    回退兼容位）+ `clear_grounding()`（agent 循环清理点统一入口）。`samples_json` 的
    `Vec<(String, String)>` 形状与既有读取路径**未动**。
- **预算常量**（`generation/lesson.rs`）：`GROUNDING_PRIMARY_MAX_BYTES = 24KB`、
  `GROUNDING_SUPPLEMENT_MAX_BYTES = 8KB`、`GROUNDING_TOTAL_MAX_BYTES = 40KB`；超限用
  `goal_excerpt`（t2 采样同一把尺，已提为 `pub(crate)` 并补「选摘落空回退文件头」护栏）
  按词条相关度选摘，截断处写 `GROUNDING_TRUNCATION_MARKER`（上下文里明示截断）。
- **提示词**：三处（节规划/正文/题目）改为多片段表述——`render_grounding_fragments` 逐片段
  标角色/序号/出处，引导语声明「主引用 = 引用的源文件全文，补充片段 = 库内相关文件」；
  `lesson_content_loop.rs::lesson_user_text` 同口径渲染多片段（引用补充片段事实时注明出处）。
- **读数与披露**：课时生成发 `learning.lesson-generation` 的 `grounding` 帧
  （`primary_live`/`hits`/`supplements`/`degraded`）+ `tracing::info!`；前端
  （`LessonStudy.tsx`）在生成期间显示「来源库不可达，本次生成使用课程快照兜底」（i18n 双语
  `learning.lessonGenSourceDegraded`），新一次生成重置。

### 验收对照（测试）

- 大文件中段事实：`lesson_grounding_reads_the_full_source_past_the_snapshot_prefix`
  （夹具事实在 8KB 之外，快照前缀不含；primary 全文 + 超预算按相关度选摘后事实到位，
  截断标注在上下文里）。
- 实时优先：`lesson_grounding_prefers_the_live_file_over_the_stale_snapshot`（建课后改库 →
  新生成取新内容；第二次调用幂等重入不重生成 = 已生成课时不变）。
- 锚定专项：`lesson_grounding_keeps_the_cited_source_and_traces_supplements`（命中散落两文件
  ——检索分序与路径序刻意相反——primary 不被替换，补充片段带库+文件+命中章节且按结构序交付，
  主 source 不重复作补充）。
- 快照兜底 + 披露：`lesson_grounding_falls_back_to_the_snapshot_when_the_base_is_gone`
  （库根被删 → 照常生成，primary 回到快照，grounding 帧 `primary_live=false`/`degraded=true`）。
- 老课程零回归：`legacy_course_without_goal_still_generates_from_title_purpose_retrieval`
  （无目标/无披露/旧快照形状照常生成，查询词退到标题+purpose）。纯函数级：
  `grounding_fragments_render_roles_and_provenance`、`grounding_budget_truncates_and_marks_over_budget_fragments`、
  `grounding_budget_keeps_mid_file_relevant_facts_when_truncating`。
- 中文检索：`query_terms_adds_cjk_bigrams_and_keeps_ascii_terms_verbatim`（修前红）、
  `search_bases_finds_a_cjk_sentence_query`（整句查询修前零召回）、
  `search_bases_ranks_cjk_filename_and_heading_above_body`（短词不劣化）。

**验证门（实跑）**：`cargo test -p nomifun-learning` **192 passed**（基线 184 + 新增 8）；
`cargo test -p nomifun-ai-agent` 1115 passed 等 15 个 target 全绿；`cargo test -p nomifun-knowledge`
**299 passed / 1 pre-existing failure**（`create_snapshot_source_fetches_and_chains_autogen`：
断言旧 `> **source_url**` 头部而实现已改为 YAML frontmatter——在基线提交 `bacb5fd98` 上同样红，
与 t3 无关，未动）；`bun run gen:i18n` + `check:i18n` 过；`ui` typecheck 与基线一致
（存量 60 错，均在 videoCanvas/videoGeneration 等，学习/i18n/ipcBridge 文件干净）。

### 与票面的偏差

1. **`SourceSpan.kb_id` / 课时 source 多引用 / `validate_blueprint` 未动**：实施导航把它们
   明确划给 t4 多库票；本票只按 t1 既有 `learning_courses.source_kb_id` 解析来源库（单库）。
   前端的降级提示落在 `LessonStudy` 生成进度行。
2. **单节重写（`rewrite_lesson_section`）仍用快照摘录**：票面只列「节规划/正文/题目」三处
   提示词；重写面（ADR-0002 单节管线）保持现状，留给后续按需统一。
3. **目标词条集持久化**落在 `SampleDisclosure.goal_terms`（`samples_meta_json` 内新增
   `#[serde(default)]` 字段），未新增列；旧披露读出为空 → 重切目标文本兜底。
4. **词条化 v1 = 确定性切片**已按拍板执行（复用 `terms.rs`，不引 LLM）。
5. **`learning-smoke` 缺省行为**：本分支无该无头入口，读数走 grounding WS 帧 + 进程内日志
   （票面「或 WS 事件即可」）。
