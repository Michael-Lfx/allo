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

**Status:** ready-for-agent

**实施前拍板（2026-10-13 裁决）**：词条化 **v1 = 确定性切片**（复用 t2 已落地的
`generation/terms.rs` 纯函数，判据"不引 LLM"以此为准）；目标词条集随 `samples_meta_json`
持久化、建课后稳定复用。**LLM 词条扩张（目标同义词补充）留作后续增强**：确定性切片是
其自然回退，将来加也不改变任何接口。t2 实施记录与本裁决的一致性由本条统一。

- [ ] **检索质量修正（前置件）**：`KnowledgeService` 检索侧修 CJK 召回——`query_terms`
      整句匹配失败时对 CJK 段降级为**字符 bigram**（整句命中必然蕴含其二元命中，不丢
      召回）；只动词条生成与打分，返回结构与缓存不动；补中文检索单测（整句查询有召回、
      纯英文行为不变）。
- [ ] **课时读库面**：learning crate 拿注入的 `KnowledgeService`，课时生成新增检索步骤
      （课时标题 + purpose 词条 + 目标词条集挑选项）→ top-N 命中 → 读**全文**（
      `read_file(kb_id, rel_path)`；句柄编码是 agent 会话面的事，本面直读即可）。
      检索失败/超时（20s 预算）降级到快照摘录，**不阻断生成**。
- [ ] **上下文重构**：`LessonExcerpt` 扩为**多片段**结构（保留 `path`/`text` 兼容位，
      增 kb 归属与来源标记 primary/supplement）；primary = 大纲引用的 source 文件
      **全文**（替代 8KB 前缀）；补充 = 检索扩读片段。**token 预算守卫**（建议 primary
      ≤24KB、补充单片段 ≤8KB、合计 ≤40KB，超预算按相关度截断并在披露里标注）。
- [ ] **提示词更新**：节/正文/题目三处 "Cited file excerpt" 改为多片段 grounding 表述；
      `SourceSpan` 增可选 `kb_id` 归属（兼容老课程空值）——`validate_blueprint` 的
      unknown_source 语义保持"必须来自已采样/来源库的真实路径"。
- [ ] **agent 循环接线**：`lesson_content_loop.rs` 的 grounding 装配同步（含
      `context.excerpt = None` 的历史清理点）；学习图节点路径不受影响（不读库）。
- [ ] **快照兜底语义**：库不可达/文件被删 → 快照仍可生成（现状能力保留），披露降级。
- [ ] 读数：课时生成记命中数/扩读文件数（进程内日志或 WS 事件即可——**本分支无
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
