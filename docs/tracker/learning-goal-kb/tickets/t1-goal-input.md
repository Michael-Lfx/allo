---
id: t1
title: "目标输入：知识库流多行文本框（课程名/目标二合一）"
type: task
status: closed
assignee: zhangyj
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

- [x] `GenerateCourseRequest` 增 `goal: Option<String>`（归一限长，参照学习图 topic 的
      200 字上限与 `optional_statement` 的 500 归一；trim 后为空 = None）；知识库流缺省
      时主题回退库名（兼容旧直连客户端与 agent 旧调用），**不报错**。
- [x] UI：`CreateCourseDialog` 知识库 Tab 增多行输入框（`Input.TextArea`，autoSize，
      参照描述流的尺寸），标签与占位符走 i18n 双语（`zh-CN`/`en-US` 对称）；**UI 必填**
      （空则提示，不出请求）；提交体带 `goal`。与领域标签并列，语义保持独立。
- [x] 落库：复用 `learning_courses.learning_goal` 列（学习图课程同列承载同类语义）或
      070 后的新列——按实施时的迁移号决定（下一号接 072）；`CourseSummary`/详情 DTO
      暴露该字段，TS 镜像同步。
- [x] 进快照与上下文：目标文本进 `samples_json` 旁的目标字段（或独立列，t3 检索要读），
      进 `LessonGenerationContext`（传统课时的模型目标字段现为空位）。
- [x] 大纲 scope 主题改从 `goal` 取（`service/course_outline.rs:44` 现取"库名+描述"），
      缺省仍回退库名+描述；课程名默认即目标文本（`set_meta` 的 title 校验不变）。
- [x] agent 工具 `learning_generate_course` 知识库分支增 `goal` 参数（`nomi-agent/
      learning_tools.rs`），工具描述更新；无库分支（description）语义不变。
- [x] `learning-smoke` 建课入口接受目标参数（或文档注明缺省行为）。

**验收：**

- 输入短文本"零基础学 Python 数据分析"→ 课程标题即该文本；输入多行长目标 → 完整文本
  落库、标题截断展示口径明确。
- 不输入目标（旧客户端路径）→ 行为与现状**完全一致**（主题=库名，回归）。
- 教程课程（资产包直导）不受影响（不经该校验）。
- `cargo test -p nomifun-learning -p nomifun-ai-agent` 全绿；`bun run typecheck` 与
  i18n 门通过（现有 typecheck 存量错误除外）。

## Resolution

**结论速览**：目标 = 知识库流多行文本框的整体文本（UI 必填、后端 `Option` 宽容）；课程名
默认即该文本（单行折叠 + 60 字截断）；落库复用 `learning_courses.learning_goal`（048 既有列，
不新增列）；目标贯通 `OutlineBrief.goal` → 大纲 scope 主题 / 蓝图提示词 / 课时生成上下文 /
agent 工具参数；无目标路径（旧客户端、描述流）逐字回退现状。

- 归一与限长：`GenerateCourseRequest.goal` + `goal_text()`——trim 后空 = `None`，超长按
  `COURSE_GOAL_MAX_CHARS = 2000` 字符截断（整体文本要完整落库，限长只挡滥用式输入）。
- 标题口径（`goal_course_title()`）：连续空白折叠为单行 + `COURSE_GOAL_TITLE_MAX_CHARS = 60`
  字符加省略号；目标短时标题即原文（不引入 LLM 派名）。装配处（`assemble_outline_pack`）与
  大纲草稿（`OutlineDraft::new` 预置 `meta.title`）同口径；模型自拟标题最终也被装配处收敛
  回目标文本。课程详情标题悬浮显示完整目标（截断口径可见）。
- 落库/暴露：`OutlineSnapshot.learning_goal` → `learning_courses.learning_goal`
  （NOT NULL DEFAULT ''，空串即 None）；`CourseSummary.learning_goal` 与 TS
  `CourseSummary.learning_goal` 同步。
- 上下文与提示词：`LessonGenerationContext.goal`（传统课时从课程行读出，预渲染「课程目标」
  段；学习图节点仍在 `graph.goal`，不重复承载）；蓝图提示词加 `Learner goal (this text IS
  the course title; design to it)`；agent 简报（`brief_user_text`）首段载目标与标题口径；
  scope 主题优先取目标（`SCOPE_TOPIC_MAX_CHARS` 截断），缺省回退库名+描述（旧路径零变化）。
- agent 工具：`learning_generate_course` 增 `goal` 参数，工具描述声明"标题即该文本、不要另起
  标题"；描述流忽略该参数（简报已是目标，语义不变）。
- `learning-smoke`（票面末项）：本树**没有** `nomicore learning-smoke` 无头入口
  （`nomifun-app/src/commands/` 无 learning 子命令），按票面"或文档注明缺省行为"落实——目标
  参数的无头入口是 HTTP 建课端点与 agent 工具；缺省（不传 goal）行为即上文的回退路径。
- UI：知识库 Tab 顶部多行 `Input.TextArea`（autoSize 3–6 行）+ 说明行；空目标提示不出请求；
  `goal` 随提交体下发；i18n 双语（`courseGoal*` / `goalRequired`）并配双语言断言测试
  （`ui/src/renderer/services/i18n/learningGoalLocales.test.ts`）。

**验收对照（测试）：** 「零基础学 Python 数据分析」→ 标题即该文本 + 列落库
（`service::tests::kb_goal_flows_into_brief_title_and_course_row`）；多行长目标完整落库 + 标题
截断（`models::goal_tests::goal_text_keeps_multiline_text_and_bounds_length` /
`goal_course_title_collapses_to_one_line_and_truncates`）；无目标路径零回归（同服务级用例的
legacy 分支 + `models::goal_tests::goal_absent_keeps_the_legacy_kb_flow_valid`）；教程课程走
`import_course(pack)`（`outline = None`）→ `learning_goal` 默认空串，不经该输入面。
门：`cargo test -p nomifun-learning` 184 passed（基线不回归）、`-p nomifun-ai-agent` 1115
passed、i18n 生成器 `--check` 过、`ui typecheck` 仅存量 60 错（videoCanvas 等）且学习/i18n
文件干净。

