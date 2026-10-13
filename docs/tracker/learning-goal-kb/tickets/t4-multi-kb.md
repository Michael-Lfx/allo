---
id: t4
title: "多知识库扩展：数组落库 + 预算分摊 + 归属引用 + 前端多选"
type: task
status: open
assignee:
blocked_by: [t2, t3]
part_of: ../map.md
---

# t4: 多知识库扩展：数组落库 + 预算分摊 + 归属引用 + 前端多选

**What to build:** 建课支持选择**多个知识库**（用户原始诉求）。检索侧 `search_bases` 与
t3 的读库面已天然多库（`kb_ids: &[KnowledgeBaseId]`），本票是数据面与输入面的参数扩展：
`learning_courses.source_kb_id` 单列 → 来源库列表；采样预算按库分摊；引用/快照带库归属；
前端单选 → 多选；原文回看按归属解析。

**Blocked by:** [t2 采样](t2-goal-aware-sampling.md)、[t3 写作接库](t3-lesson-live-retrieval.md)
（多库是这两者的参数化，不单独建设）。

**Status:** ready-for-agent

- [ ] **落库**：仿 `cs_agents.knowledge_base_ids` 先例（迁移 019）——数组列
      `knowledge_base_ids TEXT NOT NULL DEFAULT '[]'`（`json_valid` 校验），首个 = 主来源；
      下一号迁移接 072；`id_schema_contract` 的 `learning_courses.source_kb_id` SetNull
      条目退役（参考完整性移应用层过滤：读侧剔除已删库 id）；老数据回填
      （`source_kb_id` → 单元素数组）后旧列删除或保留只读，按实施时点定。
- [ ] **请求面**：`GenerateCourseRequest` 增 `knowledge_base_ids: Vec<KnowledgeBaseId>`
      （兼容单 `knowledge_base_id`，二选一，互斥校验）；逐库核验存在；**选择数量上限**
      （建议 5，按 t2 采样预算倒推）超限 400。
- [ ] **采样分摊**：t2 的目标感知采样按库分摊预算（总量 160KB 不变，按库存量比例 +
      每库保底），跨库路径带来源命名空间（`{库名}/rel`，重名库消歧），去重键排序后拼接
      （`kb:{id_a}+{id_b}`，防乱序重复建课）。
- [ ] **引用归属**：`SourceSpan`/快照条目的库归属字段（t3 已加，本票填多库值）；
      `co_read`/检索路径命名空间与大纲审计的 source 校验同步。
- [ ] **前端**：`CreateCourseDialog` 单选 → 多选（参照 `CsAgentDetailPage` 模式）；
      进度/详情/悬浮条"来源：库A、库B"；`CourseWorkspace` 的"回到知识库"按钮改主来源；
      `LessonSourcePanel` 按 `(kb_id, rel_path)` 解析原文（老课程单库兼容）；i18n 双语。
- [ ] **agent 工具**：`learning_generate_course` 的 `base` 参数支持多库（候选集仍限会话
      挂载集）。

**验收：**

- 两库同名文件（都含 `README.md`/`docs/intro.md`）→ 引用与原文回看各自解析到正确库。
- 一库为空 / 一库无 markdown → 明确报错或按库跳过（口径写入票面 Resolution）。
- 删其中一个来源库 → 课程仍可用（快照+其余库），读侧过滤不外泄悬空 id。
- 乱序提交同一组库 → 被去重槽位拒绝（不重复建课）。
- 老课程（单库、裸路径、老快照）零回归。
- `cargo test -p nomifun-learning -p nomifun-db`、`bun run typecheck`、i18n 门通过。
