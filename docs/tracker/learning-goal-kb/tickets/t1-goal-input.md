---
id: t1
title: "目标输入：知识库流多行文本框（课程名/目标二合一）"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# t1: 目标输入：知识库流多行文本框（课程名/目标二合一）

**What to build:** 传统课程的知识库流增加**多行文本输入**（用户裁决 2026-10-13）：
简单时即课程名称（课程名本身是高度抽象的目标），复杂时是目标描述/长说明；不做形态判断，
整体文本即目标。该文本落库、进快照、进课时生成上下文，作为下游（t2 采样排序、t3 检索、
大纲 scope 与审计）的统一主索引。

**Blocked by:** 无（本图起点）。

**Status:** ready-for-agent

- [ ] `GenerateCourseRequest` 增 `goal: Option<String>`（归一限长，参照学习图 topic 的
      200 字上限与 `optional_statement` 的 500 归一；trim 后为空 = None）；知识库流缺省
      时主题回退库名（兼容旧直连客户端与 agent 旧调用），**不报错**。
- [ ] UI：`CreateCourseDialog` 知识库 Tab 增多行输入框（`Input.TextArea`，autoSize，
      参照描述流的尺寸），标签与占位符走 i18n 双语（`zh-CN`/`en-US` 对称）；**UI 必填**
      （空则提示，不出请求）；提交体带 `goal`。与领域标签并列，语义保持独立。
- [ ] 落库：复用 `learning_courses.learning_goal` 列（学习图课程同列承载同类语义）或
      070 后的新列——按实施时的迁移号决定（下一号接 072）；`CourseSummary`/详情 DTO
      暴露该字段，TS 镜像同步。
- [ ] 进快照与上下文：目标文本进 `samples_json` 旁的目标字段（或独立列，t3 检索要读），
      进 `LessonGenerationContext`（传统课时的模型目标字段现为空位）。
- [ ] 大纲 scope 主题改从 `goal` 取（`service/course_outline.rs:44` 现取"库名+描述"），
      缺省仍回退库名+描述；课程名默认即目标文本（`set_meta` 的 title 校验不变）。
- [ ] agent 工具 `learning_generate_course` 知识库分支增 `goal` 参数（`nomi-agent/
      learning_tools.rs`），工具描述更新；无库分支（description）语义不变。
- [ ] `learning-smoke` 建课入口接受目标参数（或文档注明缺省行为）。

**验收：**

- 输入短文本"零基础学 Python 数据分析"→ 课程标题即该文本；输入多行长目标 → 完整文本
  落库、标题截断展示口径明确。
- 不输入目标（旧客户端路径）→ 行为与现状**完全一致**（主题=库名，回归）。
- 教程课程（资产包直导）不受影响（不经该校验）。
- `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿；`bun run typecheck` 与
  i18n 门通过（现有 typecheck 存量错误除外）。
