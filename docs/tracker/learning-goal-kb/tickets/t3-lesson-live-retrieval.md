---
id: t3
title: "课时写作接回知识库：实时检索 + 全文读取，快照降级为兜底"
type: task
status: open
assignee:
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

**Status:** ready-for-agent（含一个实施前拍板项，见下）

**实施前拍板（map 的 Not yet specified）**：目标词条化 = LLM 单次调用（建课时一次，
推荐）还是确定性切片。拍板后写入本票 Resolution 并实施。

- [ ] **检索质量修正（前置件）**：`KnowledgeService` 检索侧修 CJK 召回——词条整句匹配
      失败时降级为字符 bigram（或等价的中文切分）；长目标/长课时标题不再必然空召回。
      只动词条生成与打分，返回结构与缓存不动；补中文检索单测。
- [ ] **课时读库面**：learning crate 拿注入的 `KnowledgeService`，课时生成新增检索步骤
      （课时标题 + purpose 词条、可加目标词条集挑选项）→ top-N 命中 → 按句柄读全文。
      检索失败/超时（20s 预算）降级到快照摘录，**不阻断生成**。
- [ ] **上下文重构**：`LessonGenerationContext` 的 `excerpt: Option<LessonExcerpt>` 扩为
      **多片段**（`Vec<LessonExcerpt>`，带 kb 归属与路径）；primary = 大纲引用的 source
      全文（替代 8KB 前缀），补充 = 检索扩读片段；token 预算守卫（单课时调用上限，
      超预算按相关度截断并披露）。
- [ ] **提示词更新**：节/正文/题目三处 "Cited file excerpt" 改为多片段 grounding 表述；
      source 多引用（课时可引多文件）与 `SourceSpan` 的库归属字段（`kb_id`，可空兼容老
      课程）一并落——`validate_blueprint` 的 unknown_source 语义扩展为"必须来自某来源库
      的真实路径"。
- [ ] **agent 循环接线**：`lesson_content_loop.rs` 的 grounding 装配同步（含
      `context.excerpt = None` 的历史清理点）；学习图节点路径不受影响（不读库）。
- [ ] **快照兜底语义**：库不可达/文件被删 → 快照仍可生成（现状能力保留），披露降级。
- [ ] `learning-smoke` 增读数：课时生成读库命中数/扩读文件数（读数面非门禁）。

**验收：**

- 大文件（8KB 之后）中段的事实出现在课时正文（今天不可能）。
- 建课后修改知识库文件 → 新生成的课时反映修改（陈旧快照问题解除）；已生成课时不变。
- **锚定专项**：检索命中散落多个文件时，课时引用的主 source 不被替换；扩读片段在正文中
  可溯源（库+文件+节）。
- **课程式库专项**：primary 读源文件全文 + 按结构序扩展，不因检索结果改写阅读线。
- 源库被删/不可达 → 课时仍可生成（快照兜底），前端披露降级态。
- 中文检索单测：整句查询有召回；`cargo test -p nomifun-knowledge -p nomifun-learning`
  全绿。
