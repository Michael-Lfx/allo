# 学习域（Learning）

> **最后维护：** 2026-09-08 · 核对基准：分支 `feat/zyj0907`（commit `ddc22ff71` 之后） ·
> 文档性质：现行架构文档（基于源码逐项核对）

[`nomifun-learning`](../../crates/backend/nomifun-learning/) 是构建在知识库之上的
领域无关课程引擎：从绑定的知识库取样/经描述生成课程、按需生成课时内容、
学习图（beta）与课程练习，并用 FSRS 算法调度复习。前端入口是 `/learn` 与
`/learn/:id`（别名 `/settings/learn`）。

## Crate 构成

领域依赖刻意收敛为 `nomifun-knowledge` + `nomifun-db` + `fsrs`。模块：
`generation`（legacy 一次性管线 + 提示词常量）/ `scheduler` / `service` /
`routes` / `models` / `state` / `learning_graph`（scope 分析、draft 内核、
确定性审计）/ `course_outline`（大纲草稿）。

与 agent 引擎的关系（ADR-0002 之后的现行形态）：三条生成流（学习图 /
课程大纲 / 课时内容）都由**两轮 agent 循环**驱动（生成循环 + 审计门禁驱动
的修复循环）。引擎接缝 trait（`LearningGraphAgentEngine` /
`CourseOutlineAgentEngine` / `LessonContentAgentEngine`）定义在本 crate，
实现位于
[`nomifun-ai-agent`](../../crates/backend/nomifun-ai-agent/)
（`learning_graph_loop.rs` / `course_outline_loop.rs` / `lesson_content_loop.rs`，
共享 `loop_core.rs`），由 `nomifun-app/src/services.rs` 在启动时注入。
学习图生成只走引擎（无 fallback）；大纲与课时在引擎缺席时回退到
`generation/` 的一次性管线（测试与直连场景）。

## 课程生成管线

`course_kind` 两种课程：

- **traditional** —— 知识库取样或描述生成蓝图（agent 循环 + fallback），
  大纲（modules/lessons/concepts）导入即落库；课时正文在学习者打开该课时
  时按需生成（`content_generated` 列支撑幂等），分节落
  `learning_lesson_sections`（ADR-0002）；
- **learning_graph**（beta）—— 描述即学习目标：scope 分析产出大块概念
  清单 → agent 循环用 `lg_*` 工具分批建图（每批 ≤15 操作）→
  `lg_finish` 确定性审计门禁（覆盖/连通/DAG/容量，danger 拦发布）→
  发布为图课程（单隐含模块 + 拓扑序课时 + 前置边表）。节点内容仍按需
  生成，上下文带前置已教节摘要与后代节点禁止清单。

两种生成的进行中状态/取消统一走 `generation_registry`（status/cancel
端点的数据源）；学习图草稿在内存存活 1 小时（TTL），支持续建
（`/courses/generate/resume`，轮次日志注入恢复认知）。

## Agent 入口

agent 可以触发课程生成但不亲自执行生成：接缝
[`nomifun-ai-agent/src/learning_course.rs`](../../crates/backend/nomifun-ai-agent/src/learning_course.rs)
里的 `LiveLearningCourseSink` 实现 `nomi_agent::learning_tools::LearningCourseSink`，
把 agent 工具调用转成后台任务跑同步管线；工具名 `learning_generate_course`
在 `nomifun-app/src/services.rs` 接线。
生成过程本身是 learning job，不占 agent turn。

## FSRS 复习调度

`scheduler.rs` 用 `fsrs::FSRS` + MemoryState 计算到期复习：用户可调目标记忆
保持率、权重与时区偏移；每日 02:00 翻日。复习流：
`reviews/due → answer → rate`，支持 skip/archive/mark-edit。

## 存储与路由

迁移族：`015_learning_engine.sql`（courses/lessons/concepts/prerequisites/
progress/attempts/review_items）、`036` tags、`037` course_jobs、`039` 复习
题目级重写、`040` on-demand 列（blueprint/samples 快照 + purpose +
content_generated）、`042` 打卡 + review_events、`043` 归档、`044`
edit-pending、`048_learning_graph.sql`（course_kind/goal/scope/graph_meta +
课时级前置边表）、`049_learning_sections_and_question_kinds.sql`
（分节表 + 9 种题型 + teaching_style）。

HTTP 面（`nomifun-app/src/router/routes.rs:899` 挂载，实例 owner 保护）：
`/api/learning/courses*`（列表/导入/生成/续建/状态/取消/删除/标签/注册/诊断）、
`/api/learning/course-jobs*`、`/api/learning/lessons/{id}`（详情按需加载）、
`/api/learning/lessons/{id}/progress|generate|activities*`、`/api/learning/activities/{id}/attempts`、
`/api/learning/reviews/*`、`/api/learning/checkins/today`、
`/api/learning/stats/calendar`、`/api/learning/(custom-)questions*`、
`/api/learning/tags`、`/api/learning/concepts`。

生成进度经 WebSocket 推送：`learning.course-generation` /
`learning.lesson-generation`。

## 前端

`ui/src/renderer/pages/learning/`：CourseWorkspace（分节 stepper、按需拉取
课时详情）、LearningGraphWorkspace / GraphDagView、CreateCourseDialog、
ReviewSession、CourseJobTable、CheckinPanel、QuestionManager、
LearningModelSelector 等；hooks `useCourseLearning` / `useReviewSession` /
`useCourseJobs` / `useCourseCreation` / `useCheckinStatus`。

## 相关但独立的 harness

[`nomi-coding`](../../crates/agent/nomi-coding/)（编码完成策略 / 验证门 /
todo 续跑）属于 agent 引擎侧，由 `nomi-agent` 在 `task_profile=coding` 时安装，
见 [agent-engine.zh.md](agent-engine.zh.md)，与课程引擎无关。
