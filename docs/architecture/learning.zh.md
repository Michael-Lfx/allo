# 学习域（Learning）

> **最后维护：** 2026-10-15 · 核对基准：分支 `feat/learn0928`（二轮 + 三轮吸收 j01 起，
> 前基准：学习图吸收 learnhub ADR-0010） ·
> 文档性质：现行架构文档（基于源码逐项核对）

[`nomifun-learning`](../../crates/backend/nomifun-learning/) 是构建在知识库之上的
领域无关课程引擎：从绑定的知识库取样/经描述生成课程、按需生成课时内容、
学习图（beta，概念网生长模型）与课程练习，并用 FSRS 算法调度复习。前端入口
是 `/learn` 与 `/learn/:id`（别名 `/settings/learn`）。

> 学习图的知识结构产物（structure/terrain 双产物的 gd.* 机器契约、快照存储与
> 重试纪律）单独立契：[`docs/contracts/learning-structure-artifacts.md`](../contracts/learning-structure-artifacts.md)。
> 生长批产物的 ng.* 条款与教练上下文九块的构成见本文与 `learning_graph` 源码。

## Crate 构成

领域依赖刻意收敛为 `nomifun-knowledge` + `nomifun-db` + `fsrs`。模块：
`generation`（legacy 一次性管线 + 提示词常量）/ `scheduler` / `service` /
`routes` / `models` / `state` / `learning_graph`（概念网生长域核：档位、
就绪判定、结构门、教练单次 LLM 调用、结构生成门面 `structure_gen` 与契约
`structure_contract`/`growth_contract`/`structure_view`）/ `course_outline`
（大纲草稿 + 范围分析 `scope`）。

与 agent 引擎的关系（ADR-0009 后的现行形态）：**两条**生成流（课程大纲 /
课时内容）由两轮 agent 循环驱动（`CourseOutlineAgentEngine` /
`LessonContentAgentEngine`，实现位于
[`nomifun-ai-agent`](../../crates/backend/nomifun-ai-agent/) 的
`course_outline_loop.rs` / `lesson_content_loop.rs`，共享 `loop_core.rs`），
由 `nomifun-app/src/services.rs` 在启动时注入；引擎缺席时回退到
`generation/` 的一次性管线（测试与直连场景）。**学习图生长不再走 agent
循环**：批次只有 ≤7 个节点，教练是 `LearningCompleter` 上的单次调用
（上下文 + 结构门 + 一次打回重裁 + AI 概念评审），全部闭环在本 crate
（ADR-0009；旧 `learning_graph_loop.rs` 已删除）。

## 课程生成管线

`course_kind` 两种课程：

- **traditional** —— 知识库取样或描述生成蓝图（agent 循环 + fallback），
  大纲（modules/lessons）导入即落库；课时正文在学习者打开该课时
  时按需生成（`content_generated` 列支撑幂等），分节落
  `learning_lesson_sections`（ADR-0002）；
- **learning_graph**（beta）—— **先成结构地图，再从概念网生长**（ADR-0009 /
  ADR-0010）：建课单步向导 = 学习目标（必填）+ 程度声明/基础声明（可选）；
  后台先跑**知识结构生成**（双产物 structure + 前置地形，gd.* 契约门 +
  门口对账 + ≤3 轮带条款反馈重试 → `learning_courses.structure_json` 单字段
  快照原子落列，重跑覆盖），随后首生长接续（结构失败不阻断生长——结构对生长
  不供内容、不做判定）。依赖由**概念网**表达（节点 `teaches`/`assumes` 概念 ×
  档位 know<apply<teach），没有先修边、没有锁定态——结构门保证任何发布的节点
  当下就能学。教练按批次生长：就绪存量 < 3（节点完成时自动检查）或手动按钮
  触发，每批 ≤7 个新节点，目标把就绪补到 7；教练批走 json 接缝 + ng.* 条款
  门（shape `validate_proposal` + DB 语义 `validate_batch`）+ AI 概念评审
  （语义兜底，不可用保守放行），≤3 轮带条款报告重试；每批落
  `learning_growth_batches` 出生档案。读侧投影 `structure_view` 纯函数
  （零 IO 零 LLM，课程内作用域）支撑前端结构图 Tab。**终点锚与罗盘已整体
  退役**（迁移 071；方向感由结构投影承担）。概念本身登记在全局跨课程的
  `learning_concept_registry`（canonical + 别名联合唯一，生长按需铸名）。
  **别名闭环（j04 = 二轮 g3 机制 + 三轮 g2 数据形态）**：别名是结构化记录
  `{name, definition?, attestation?, status}`（退役名不占名池）；铸名门带
  复用通道——定义归一化直定宿主 + defnear 近邻候选集（trigram/近形/词面
  补位）+ 概念评审扩权裁决「复用|新铸」（复用指向候选集 concept_id，机械
  核验禁编造，无新 LLM 调用）；new_aliases 专职通道给既有概念提名新名
  （必须裁复用否则整批打回指点改走 mints）；登记单点
  `alias_records::register_alias_tx` 是全仓唯一合法事后加名路径（宿主非
  废弃/活跃占池/幂等/复活四保证）；**层收缩原则**：登记表只存判定所需的
  定义与名字事实，教学材料归内容层。

三条生成流的修复循环以更高的推理档位运行（`REPAIR_REASONING_EFFORT`，
learnhub「修复轮升思考档」的等价物）；生成循环保持 provider 默认档。

课时内容支持断点续跑（失败/超时后草稿在 TTL 内存活，重试经
lesson_id→draft 映射定位草稿接续，迁移 050 的承诺；ADR-0003）与单节
重写（`POST /lessons/{id}/sections/{key}/rewrite`，确定性单节管线按落库
的 `visual` 承诺质检，迁移 051；ADR-0003）。图课程节点的内容上下文带
assumes 概念 + 跨课程教师节点摘要 + 下游禁止清单（概念网口径）。

可视化承诺的逃逸口治理（ADR-0008）：`ls_set_document` 单篇契约只保留在
修复循环（生成必须走分节，单篇形状没有可视化承诺）；概念/例题节全部声明
`visual=无` 时审计出 `visual_none_heavy` warning（软监督，不设硬配额）；
发布帧与 completed 事件披露 visual 声明分布；降级兜底的节带 `degraded`
标记落库（迁移 053），前端持久可见并提供 AI 重写入口，手动编辑即视为
接管并清除标记。

四类长任务（学习图生长/结构生成、大纲建课、课时内容生成）的进行中状态/取消
统一走 `generation_tasks` 长任务注册表（j03：键 `{kind}:{course}:{node|-}`、
状态机 running→done|failed|cancelled 终态幂等、同键并发拒绝、注册序排序）：
快照命令 `GET /api/learning/generation-tasks`、统一取消
`POST /api/learning/generation-tasks/{key}/cancel`（存量 `/generate/cancel`
转发注册表）。取消 = 每任务一枚 `CancellationToken`——growth/structure 循环
在检查点轮询 + 全部 LLM 调用经 `CancellationCompleter` `select!` 真中断
（中断 = 断 HTTP，批事务原子落库 = 零落库；结构快照写前检查 = 零快照）；
大纲 agent 循环经 `CancellableProvider`（token 化）逐请求边界与流中生效；
课时内容循环未接 token（ADR-0004 显式差异：断开请求即中断，远程取消如实
拒绝）。begin/finish 观测旁路 JSONL 落 `observation/learning-tasks.jsonl`
（写失败降级）。生长循环的重试机制（二轮 t1）：同指纹早停（连败两次即停、
调用数 2 非 3）+「未点名被改」机械守卫（豁免早停 + 原地重试恰一次不占轮
预算）+ 重裁出口容量定向打回（`ng.batch.redraft_capacity`）；smoke 增
`attempts_used`/`stop_reason` 读数。

## 提示词纪律方法论

教练批（COACH_SYSTEM）、结构分解与概念评审提示词的条款增改遵循三条贯穿性方法论
（learnhub 二轮 #85 十七提交沉淀，病历出处见
`docs/tracker/learning-absorb-2/assets/r1-learnhub-round2-findings.md`）：

1. **放宽必配收窄判据**：放宽一个限制时必须同时给出收紧后的判据，否则放宽即失稳。
   病历：learnhub c05a43b 放开对照/对比型合装（用户定调「对照学习确实有效果」）后，
   十批对照 critique 暴露「任意两概念被硬装进对照框」——4fc663b 补「对照限同轴」
   （两个不同概念的首次引入不是对照，是抱团）才收口。
2. **修偏差优先删预设、少加规定**：偏差的根源往往是提示词里一条没人读到的预设，
   删它优于再加一条规定——规定叠加成新僵化。病历：learnhub 1bac1c7 动词场——
   不设动词形态要求（删「全是同型」的隐性预设），只留「同批与相邻批不全是同型」的
   多样谱；allo 的 COACH_SYSTEM 动词条沿用同款写法。
3. **判据只放在能看到信息的层**：判据放在拥有裁决所需信息的那一层，看不到信息的层
   不设判据。病历：learnhub 3edbe5a 概念评审面有意不动——评审输入只有已教概念名
   清单、看不到既有节点内容，结构上无法判断重访是否带来新角度，重访成色归装箱层；
   6cddb21 的 guard 豁免同理（机械层判别不了语义豁免，就把豁免权交给能判别的层）。

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

迁移族：`015_learning_engine.sql`（courses/lessons/progress/attempts/
review_items）、`036` tags、`037` course_jobs、`039` 复习题目级重写、
`040` on-demand 列（blueprint/samples 快照 + purpose + content_generated）、
`042` 打卡 + review_events、`043` 归档、`044` edit-pending、
`050_learning_sections_and_question_kinds.sql`（分节表 + 9 种题型 +
teaching_style）、`051_learning_section_visual.sql`（节 visual 承诺落库）、
`052_learning_review_log.sql`（逐次复习日志 `learning_review_log` +
`learning_attempts.elapsed_ms`）、`053_learning_section_degraded.sql`
（节降级兜底标记落库，ADR-0008）、
`066_retire_graph_dag_and_concepts.sql`（**退役**：旧 DAG 图课程数据与
per-course 概念体系五表全部删除，ADR-0009）、
`067_concept_web_growth.sql`（概念登记表 / 概念网边
`learning_lesson_concepts(role,tier)` / 生长批次）、`068` 批行状态机、
`069` 课程软删墓碑、`070` 课时难度列、
`071_retire_endpoints_compass_structure_snapshot.sql`（**退役**：终点表 + 标记
课时行 + `compass_md`/`compass_updated_at` 列删除；新增
`learning_courses.structure_json` 结构快照列，ADR-0010）、
`076_learning_course_enabled.sql`（现存且启用口径）、
`077_concept_alias_records.sql`（别名结构化记录 + 概念网边 ref_name）、
`078_learning_xp_events.sql`（XP 结算账本 + 复习作答流水补列）、
`079_learning_structure_mounts.sql`（挂载事实层 + findings 留痕，ADR-0012）、
`080_learning_structure_repairs.sql`（修补审计表，ADR-0012）。

HTTP 面（`nomifun-app/src/router/routes.rs` 挂载，实例 owner 保护）：
`/api/learning/courses*`（列表/导入/生成/状态/取消/删除/标签/注册/诊断）、
`/api/learning/courses/{id}/graph/grow|history|concepts`
（生长/学习记录/概念表）、
`/api/learning/courses/{id}/graph/structure`（结构读侧投影：200 恒答、
`structure: null` = 未生成；软删/不存在 404；含挂载三态投影 `mounts`）与
`.../graph/structure/regenerate|mount|repair`（重跑生成 / 挂载回合 /
修补回合，j08/j09；进度走 WS structure_* 事件）、
`/api/learning/xp/reading` 与 `/api/learning/xp/calendar`（XP 读数与日历，
j05）、
`/api/learning/course-jobs*`、`/api/learning/generation-tasks*`（长任务
状态/取消，j03 注册表：growth/structure/structure_mount/structure_repair/
outline/lesson）、
`/api/learning/lessons/{id}`（详情按需加载）、
`/api/learning/lessons/{id}/progress|generate|activities*`、`/api/learning/activities/{id}/attempts`、
`/api/learning/reviews/*`、`/api/learning/checkins/today`（j05 起按 XP 口径
锁定）、
`/api/learning/stats/calendar`、`/api/learning/stats/memory`（记忆健康
四面板：负载预报/卡池状态/真实保留率+预测对照/遗忘曲线）、
`/api/learning/(custom-)questions*`、`/api/learning/tags`。

生成/生长进度经 WebSocket 推送：`learning.course-generation` /
`learning.lesson-generation`（学习图帧带 `kind=learning_graph` 与 `event` 细分
名：growth_* 与 structure_started/completed/failed，j08/j09 起
structure_mount_* / structure_repair_* 同通道）。

## 前端

`ui/src/renderer/pages/learning/`：CourseWorkspace（分节 stepper、按需拉取
课时详情）、LearningGraphWorkspace（知识结构状态条 + 就绪 x/7 补充节点 +
四视图：学习 / 学习记录批次时间线 / 概念表 / **结构图**——思维导图、关系
图谱（dagre 分层、assumes=门方向反转）、学习路线（主干子图 + 传递闭包自动
补前导）、矩阵；j08/j09 起结构图带挂载三态段（三态计数/stale 显影/已挂清
单/观察标签）与挂载/修补双入口；投影非权威标注收敛在详情层与页脚）、
CreateCourseDialog（学习图单步向导：目标 + 可选程度/基础声明）、
ReviewSession、CourseJobTable、XpPanel（今日 XP 卡 + 进度尺 + 84 格 XP 日
历，j06 打卡退役接管）、MemoryHealthPanel（复习横幅下的记忆健康折叠条）、
QuestionManager、LearningModelSelector 等；hooks `useCourseLearning` /
`useReviewSession` / `useCourseJobs` / `useCourseCreation` /
`useCheckinStatus`（j05 起 XP 口径）。

## 真机冒烟（无头）

`nomicore learning-smoke`（`nomifun-app/src/commands/learning_smoke.rs`，
wayfinder 票 s1 + j05/j07/j08/j09 扩档）：与应用同接线（DB pool →
LearningService + set_generation_dependencies + 课时/生成通道注入；provider
凭据复用 `resolve_provider_config` 现链），缺省从零临时 SQLite（`--db` 可
指开发库）。八档：

- `structure`：结构生成双产物过 gd.* 契约门 + 门口对账 + 落列，落列后重读
  重验（失败非零退出）；
- `grow`：生长批 ×N（批间确定性学习者替身程序化完成节点驱动再生长）；
  判据 = 首批 assumes 全空（空登记表机械地板）+ 每批就绪即发布 + 铸名不撞；
- `json`：json_object 能力读数（读数面非门禁，唯一硬判据 = 全灭 0/n；
  探测调用直调内层 completer，不入档案）；
- `loop`：结构 → 首批 → 教练循环 ×N 端到端合龙，EXIT=0 判据；
- `content`（j07）：单节点正文生成走真实管线，读数 = 字数/小节数/题目数/
  首现与用名标注（读数面非门禁）；
- `xp`（j05）：教新 + 复习两线真实结算 → 读数/日历快照打印（读数面非门禁）；
- `mount`（j08）：落位 + hash 锚定 + 条数一致 + **重跑幂等**（第二次必须
  skipped，否则 EXIT=4）；
- `repair`（j09）：留痕驱动回合 + **盘面复验**（磁盘重读独立跑全量 gd.*）
  + **级联标记**（结构已改时挂载必须判 stale，否则 EXIT=3）。

运行必挂 `nomifun_ai_agent::ArchivingCompleter`（每次调用的
prompt/response/耗时落 `<data-dir>/observation/learning-smoke/<run>/`
JSONL）；生产运行不挂（零开销）。无凭据临时库在任何需 LLM 的档位如实非
零退出。

## 相关但独立的 harness

[`nomi-coding`](../../crates/agent/nomi-coding/)（编码完成策略 / 验证门 /
todo 续跑）属于 agent 引擎侧，由 `nomi-agent` 在 `task_profile=coding` 时安装，
见 [agent-engine.zh.md](agent-engine.zh.md)，与课程引擎无关。
