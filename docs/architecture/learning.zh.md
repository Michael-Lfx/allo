# 学习域（Learning）

> **最后维护：** 2026-09-16 · 核对基准：分支 `feat/zyj0907`（commit `730181b35` 之后） ·
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
  清单 → agent 循环用 `lg_*` 工具分批建图（每批 ≤15 操作，审计文本携带
  从 findings 派生的「Suggestions」行动清单，下一批必须逐条处理或驳回）
  → `lg_finish` 确定性审计门禁（覆盖/连通/DAG/容量，danger 拦发布）→
  发布前一次独立的单次 LLM 终审（咨询级软门：首次 finish 被意见弹回一次，
  再次 finish 直接发布，意见随 `graph_meta_json.final_review` 落库）→
  发布为图课程（单隐含模块 + 拓扑序课时 + 前置边表）。节点内容仍按需
  生成，上下文带前置已教节摘要与后代节点禁止清单。

三条生成流的修复循环以更高的推理档位运行（`REPAIR_REASONING_EFFORT`，
learnhub「修复轮升思考档」的等价物）；生成循环保持 provider 默认档。

课时内容支持断点续跑（失败/超时后草稿在 TTL 内存活，重试经
lesson_id→draft 映射定位草稿接续，迁移 050 的承诺；ADR-0003）与单节
重写（`POST /lessons/{id}/sections/{key}/rewrite`，确定性单节管线按落库
的 `visual` 承诺质检，迁移 051；ADR-0003）。

可视化承诺的逃逸口治理（ADR-0008）：`ls_set_document` 单篇契约只保留在
修复循环（生成必须走分节，单篇形状没有可视化承诺）；概念/例题节全部声明
`visual=无` 时审计出 `visual_none_heavy` warning（软监督，不设硬配额）；
发布帧与 completed 事件披露 visual 声明分布；降级兜底的节带 `degraded`
标记落库（迁移 053），前端持久可见并提供 AI 重写入口，手动编辑即视为
接管并清除标记。

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

复习调度消费侧（迁移 052 起）：每次真实推进 FSRS 卡都在
`learning_review_log` 落一行（rating 1-4、rating_source auto/self/synthetic、
间隔日数与推进前 S·D·r_pred 快照、review_day）；种卡只落 synthetic 标记行，
统计与优化训练一律排除。**到期门**：到期卡即可推进；当日尚未推进过的卡允许
提前练一次（课程复习会话出示未到期卡）；只有「未到期且当日已推进」的过期
重复只记作答流水（accuracy/诊断），不推进、不落日志、不进打卡。到期队列按
预测回忆率 R 的五百分点分桶升序出示（桶内先易后难），响应逐卡带 `r`，
前端披露「预测回忆 xx%」；「忘记」申报有 5 秒主动回忆门。口径详见
`CONTEXT.md` 与 `docs/research-learnhub-migration.zh-CN.md` §9。

## 存储与路由

迁移族：`015_learning_engine.sql`（courses/lessons/concepts/prerequisites/
progress/attempts/review_items）、`036` tags、`037` course_jobs、`039` 复习
题目级重写、`040` on-demand 列（blueprint/samples 快照 + purpose +
content_generated）、`042` 打卡 + review_events、`043` 归档、`044`
edit-pending、`048_learning_graph.sql`（course_kind/goal/scope/graph_meta +
课时级前置边表）、`050_learning_sections_and_question_kinds.sql`
（分节表 + 9 种题型 + teaching_style）、`051_learning_section_visual.sql`
（节 visual 承诺落库）、`052_learning_review_log.sql`（逐次复习日志
`learning_review_log` + `learning_attempts.elapsed_ms`）、
`053_learning_section_degraded.sql`（节降级兜底标记落库，ADR-0008）。

HTTP 面（`nomifun-app/src/router/routes.rs:899` 挂载，实例 owner 保护）：
`/api/learning/courses*`（列表/导入/生成/续建/状态/取消/删除/标签/注册/诊断）、
`/api/learning/course-jobs*`、`/api/learning/lessons/{id}`（详情按需加载）、
`/api/learning/lessons/{id}/progress|generate|activities*`、`/api/learning/activities/{id}/attempts`、
`/api/learning/reviews/*`、`/api/learning/checkins/today`、
`/api/learning/stats/calendar`、`/api/learning/stats/memory`（记忆健康
四面板：负载预报/卡池状态/真实保留率+预测对照/遗忘曲线）、
`/api/learning/(custom-)questions*`、
`/api/learning/tags`、`/api/learning/concepts`。

生成进度经 WebSocket 推送：`learning.course-generation` /
`learning.lesson-generation`。

## 前端

`ui/src/renderer/pages/learning/`：CourseWorkspace（分节 stepper、按需拉取
课时详情）、LearningGraphWorkspace / GraphDagView、CreateCourseDialog、
ReviewSession、CourseJobTable、CheckinPanel、MemoryHealthPanel（复习横幅
下的记忆健康折叠条）、QuestionManager、
LearningModelSelector 等；hooks `useCourseLearning` / `useReviewSession` /
`useCourseJobs` / `useCourseCreation` / `useCheckinStatus`。

## 相关但独立的 harness

[`nomi-coding`](../../crates/agent/nomi-coding/)（编码完成策略 / 验证门 /
todo 续跑）属于 agent 引擎侧，由 `nomi-agent` 在 `task_profile=coding` 时安装，
见 [agent-engine.zh.md](agent-engine.zh.md)，与课程引擎无关。
