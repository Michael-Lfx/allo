# 学习域（Learning）

> **最后维护：** 2026-08-24 · 核对基准：commit `d791691c6` ·
> 文档性质：现行架构文档（新建，基于源码逐项核对）

[`nomifun-learning`](../../crates/backend/nomifun-learning/) 是构建在知识库之上的
领域无关课程引擎：从绑定的知识库取样/补全生成课程、课程与活动练习，并用 FSRS
算法调度复习。前端入口是 `/learn` 与 `/learn/:id`（别名 `/settings/learn`）。

## Crate 构成

依赖刻意收敛为 `nomifun-knowledge` + `nomifun-db` + `fsrs`，**无 agent 依赖**。
模块：`generation` / `generation_job` / `scheduler` / `service` / `routes` /
`tutorial` / `models` / `state`。

## 课程生成管线

两种模式（`CourseGenerationMode`）：

- **sampling** —— 经 `KnowledgeService` 对源知识库 markdown 取样
  （`content_root_for_base` / `sample_base_files`），纯本地组装；
- **blueprint** —— 注入 `KnowledgeCompleter` 驱动模型补全蓝图后再校验导入。

生成以后台任务运行：`CourseJobSource::{Http, Agent}` 区分来源，
状态机见 `CourseJobStatus`；支持 cancel/resume/retry/delete。
首装时 `tutorial.rs` 会经注入的 `KnowledgeService` 建一个教程知识库并挂一门示例课。

## Agent 入口

agent 可以触发课程生成但不亲自执行生成：接缝
[`nomifun-ai-agent/src/learning_course.rs`](../../crates/backend/nomifun-ai-agent/src/learning_course.rs)
里的 `LiveLearningCourseSink` 实现 `nomi_agent::learning_tools::LearningCourseSink`，
把 agent 工具调用转成 `start_course_job(..., CourseJobSource::Agent, session_id)`；
工具名 `learning_generate_course` 在 `nomifun-app/src/services.rs` 接线。
生成过程本身是 learning job，不占 agent turn。

## FSRS 复习调度

`scheduler.rs` 用 `fsrs::FSRS` + MemoryState 计算到期复习：用户可调目标记忆保持率、
权重与时区偏移；每日 02:00 翻日。复习流：`reviews/due → answer → rate`，支持
skip/archive/mark-edit。

## 存储与路由

迁移族：`015_learning_engine.sql`（courses/lessons/concepts/prerequisites/
progress/attempts/review_items）、`036` tags、`037` course_jobs、`039` 复习题目级重写、
`040` on-demand 列、`042` 打卡 + review_events、`043` 归档、`044` edit-pending。

HTTP 面（`nomifun-app/src/router/routes.rs:899` 挂载，实例 owner 保护）：
`/api/learning/courses*`（列表/导入/生成/删除/标签/注册/诊断）、
`/api/learning/course-jobs*`、`/api/learning/lessons/{id}/progress|generate|activities*`、
`/api/learning/activities/{id}/attempts`、`/api/learning/reviews/*`、
`/api/learning/checkins/today`、`/api/learning/stats/calendar`、
`/api/learning/(custom-)questions*`、`/api/learning/tags`、`/api/learning/concepts`。

## 前端

`ui/src/renderer/pages/learning/`：CourseWorkspace、ReviewSession、CourseJobTable、
CheckinPanel、QuestionManager、LearningModelSelector 等；hooks
`useCourseLearning` / `useReviewSession` / `useCourseJobs` / `useCourseCreation` /
`useCheckinStatus`。

## 相关但独立的 harness

[`nomi-coding`](../../crates/agent/nomi-coding/)（编码完成策略 / 验证门 /
todo 续跑）属于 agent 引擎侧，由 `nomi-agent` 在 `task_profile=coding` 时安装，
见 [agent-engine.zh.md](agent-engine.zh.md)，与课程引擎无关。
