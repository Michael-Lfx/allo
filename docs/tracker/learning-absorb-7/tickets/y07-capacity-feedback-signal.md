---
id: y07
title: "节点容量反馈信号——capacity_note 落库 + 三选一留痕（零结算零置位）"
type: task
status: closed
assignee:
blocked_by: [y04]
part_of: ../map.md
---

# y07: 节点容量反馈信号

**What to build:** 把生成时已有的 `capacity_note`（节点意图超载自报）从
「只打日志」升级为**内容元数据落库**；再给学习者一个 per-user / per-node
三选一反馈（接受现状 / 预算加长 / 拆成两节），latest-wins 留痕，课程详情显示
「N 节超载」聚合。**零结算零置位**：不写 attempts/review/xp/progress/FSRS，
不触发调度/门禁/完成判定；**不把用户反馈注入共享课时生成**（多用户串味）。

## 现状（2026-11-04 实码核实）

- `generation/mod.rs:280` `LessonOutput.capacity_note`；`lesson_draft.rs:412`
  同字段。发布尾 `service/lesson.rs:2077-2095` 只
  `tracing::warn!("over_capacity")`，**不落库**；`LessonView`
  （`models.rs:2545-2565`）无此字段；`CourseDetail`（:2520-2533）无聚合。
- 现有节点级「太简单」=`LessonStatus::Skipped`（`models.rs:1834-1837`），
  会改 satisfied/就绪集并触发补货（`service/progress.rs:23-31,130-140`）——
  **有置位**，与本票「零置位」相反，不能复用。
- 节级自由文本反馈 `GenerateLessonRequest.feedback`（`models.rs:295-304`）
  只用于当次重写，不留痕（`service/lesson.rs:1817`）。
- 课程内容是共享资产：`learning_lessons` 无 `user_id`，进度按 enrollment
  （迁移 015:45-67；`lesson_draft.rs:217-218` 明确「不含用户个人进度」）。
  user-scoped 表先例：`learning_custom_questions`（迁移 070:122-155；
  id 契约 `id_schema_contract.rs:919`）。
- `est` 同时是内容尺寸、XP 基数与水位（`settle.rs:10,121`；
  `xp.rs:291-333`）；反馈**不得改 est**。

## learnhub 参考（main@22d016d，8a97133 #194）

- `src-tauri/src/domain/learner_state.rs:26-206`：`EstFeedbackChoice`
  `{accept|extend|split}`；条目 `{choice, at}`，按 `node → 最新` 存；可选节
  + serde default 容忍旧档；同键覆盖写、最新为准。
- `src-tauri/src/pages/node-detail/ui/capacity-feedback.tsx:16-114`：仅
  `NodePlanSummary.capacity_note` 非空才呈卡；三选项 + 行内失败可重按；
  缺自报 = 零假信号；查询失败静隐。
- `app/course_domain.rs:846-891`：课程聚合 `overloaded_nodes` = 活跃节点 ∩
  sidecar `capacity_note` 非空；前端「N 节超载」零则零版面。
- `engine/contentgen/facade.rs:346-375,1130-1136`：有留痕才在 plan 面注入
  「学习者曾选择…」一行，write 面零注入，无留痕骨架逐字节不变。
- **不吸收**：`extend→重生成` / `split→拆节` 自动执行（allo 无 est 修改
  API，会留死信）；把 feedback 注入共享课时生成（allo 课程多用户共享）。

## 实施要点

### A. Schema（编辑 070 基线，排在 y04 之后）

- [ ] `learning_lessons` 增列：
  `ALTER TABLE learning_lessons ADD COLUMN capacity_note TEXT;`
  （可空；旧行 NULL；重生成时写 `output.capacity_note`，缺席写 NULL 以清旧值）。
- [ ] 新表：
  ```sql
  CREATE TABLE learning_lesson_capacity_feedback (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      feedback_id TEXT NOT NULL UNIQUE,   -- ULID/36 位
      user_id     TEXT NOT NULL,
      lesson_id   TEXT NOT NULL,
      choice      TEXT NOT NULL,          -- accept|extend|split，Rust 单源
      note        TEXT NOT NULL DEFAULT '',
      created_at  INTEGER NOT NULL,
      updated_at  INTEGER NOT NULL,
      UNIQUE (user_id, lesson_id)
  );
  CREATE INDEX idx_learning_lesson_capacity_feedback_lesson
      ON learning_lesson_capacity_feedback (lesson_id);
  ```
  无 DB CHECK（ADR-0015）；`choice` 写侧严格、读侧宽容 Unknown。
- [ ] `id_schema_contract.rs`：`PRODUCT_TABLES` 加表；identity
  `(learning_lesson_capacity_feedback, feedback_id)`；`user_id` → `users`
  （Cascade）、`lesson_id` → `learning_lessons`（Cascade），补索引与迁移测试。

### B. 写侧

- [ ] `persist_lesson_output`（`service/lesson.rs:2077`）：在同一事务里
  `UPDATE learning_lessons SET capacity_note = ?`；保留 `over_capacity`
  tracing（观测面不删）。
- [ ] `LessonView`（`models.rs:2545`）增 `capacity_note: Option<String>`
  （`skip_serializing_if` 可省）；`LessonView` 构造处（`lesson_view`）
  读取列。`lesson_detail`（:479-497）额外查当前用户反馈并填
  `capacity_feedback: Option<CapacityFeedbackView>`；其他构造点填 None。
- [ ] 新 DTO `CapacityFeedbackChoice { Accept, Extend, Split }`（serde
  kebab）+ `CapacityFeedbackView { choice, at }`。
- [ ] `LearningService::record_capacity_feedback(user_id, lesson_id, choice)`
  - 先按 `lesson_detail` 同路径校验课程存在 + 用户 enrollment（
    不存在/未选课按现有 404 口径）；
  - `INSERT ... ON CONFLICT(user_id, lesson_id) DO UPDATE SET choice=,
    updated_at=`（latest-wins；`feedback_id` 首写生成、覆盖不改）；
  - 返回 `CapacityFeedbackView`；空/未知 choice → 400。
- [ ] 路由 `POST /api/learning/lessons/{id}/capacity-feedback`，body
  `{choice}`；响应 saved view。**不新增 GET 路由**（随 lesson detail 读回）。

### C. 读侧聚合与 UI

- [ ] `CourseDetail` 增 `overloaded_lessons: usize`（或
  `capacity: Option<CourseCapacityView>`）：课程内活跃课时
  `capacity_note IS NOT NULL AND trim(capacity_note) <> ''` 计数；纯读、
  零持久化；0 不显示。
- [ ] 节点/课时页：`LessonStudy.tsx`（或节点面板）在
  `lesson.capacity_note` 非空时呈卡——文案显示自报原因 + 三选项；已选过
  则回显选中态（**补 learnhub「重载即失忆」缺口**）；提交失败行内
  `role=alert` 可重按；无自报不呈卡。
- [ ] 课程详情/学习记录顶部显示「N 节超载」，0 时零版面。
- [ ] i18n zh-CN/en-US 双写（选项、说明、错误、聚合标签），
  `bun run gen:i18n` + `bun run check:i18n`；双主题走语义 token。

### D. 零结算零置位（硬边界）

- [ ] 反馈写路径**不得**调用 `update_lesson_progress`、不写
  `learning_attempts` / `learning_review_events` / `learning_review_log` /
  `learning_xp_events` / `learning_progress` / FSRS 任何列；不改
  satisfied/READY_TRIGGER；不发 WS phase。
- [ ] 不把 feedback 注入 `lesson_draft` / `lesson_content_loop` 的共享内容
  生成；不用于 `est`、XP 基数、水位。若未来要做「个人预算」另立票，并明确
  不进 settle。
- [ ] 单测钉住：调用前后上述表计数不变、课时 status 不变、无任务/phase。

## 验收

- 迁移：`cargo test -p nomifun-db`；旧行 NULL 容忍；新表 identity 登记。
- 服务：发布写 `capacity_note`；重生成缺省清 NULL；同用户同节点覆盖
  latest-wins；不同用户互不可见；未选课 404；未知 choice 400；聚合计数。
- 零副作用：一次反馈调用前后 attempts/review/xp/progress/FSRS 计数与课时
  status 全不变。
- UI：有自报才呈卡；三选一提交成功回显；失败行内可重按；「N 节超载」
  0 不占版面；`bun run typecheck` + `check:i18n` + `check:theme`。
- 真机（串行）：生成一课带 `capacity_note` → 发布后课时页呈卡 → 选择后
  重进仍回显 → 课程详情聚合 +1；smoke `EXIT=0`。

## 非目标

- `extend` 自动重生成 / `split` 自动拆节 / 改 `est` / 改 XP/水位。
- 把用户反馈注入共享课时生成（多用户串味）；按用户隔离课程内容。
- ambition / 规划摘要 sidecar / `NodePlanSummary` 全量（learnhub #191 的
  其余部分；等有消费方再立票）。
- 用 `ReviewRating` 充当容量信号，或把 choice 映射到 FSRS。

## 风险

- 课程共享、反馈 per-user：聚合是内容级（不泄露他人选择），个人选择只随
  lesson detail 返回；若产品要展示他人选择，另立票并做隐私评估。
- `capacity_note` 是生成内容元数据，重生成会覆盖；已选反馈保留但可能指向
  旧内容——UI 不据此自动改选，只回显「你上次的选择」。
- `est` 双角色（内容尺寸 + XP 基数 + 水位）是硬红线，任何「预算加长」
  落地都必须走独立字段并明确不进 settle。

## Resolution（2026-11-04）

**实现点**

- Schema（编辑 `070_learning_domain_rebuild.sql` §14，未新增迁移文件）：
  `learning_lessons.capacity_note`（TEXT 可空，旧行 NULL）+
  `learning_lesson_capacity_feedback`（`feedback_id` UUIDv7 唯一业务身份、
  `user_id` → users Cascade、`lesson_id` → learning_lessons Cascade、
  `UNIQUE(user_id, lesson_id)` latest-wins、`note` 默认空串、枚举无 DB
  CHECK）+ `idx_..._lesson` / `idx_..._user` 两条具名索引。
  `id_schema_contract.rs`：`PRODUCT_TABLES` + `feedback_id` 身份（UUIDv7 +
  UNIQUE）与 `NON_REFERENCE_ID_COLUMNS` + 两条逻辑引用；表数契约
  125 → 126；新增迁移测试 4 项（列可空/旧行 NULL、行默认值、身份与唯一键、
  索引与全库契约）。
- 写侧：`persist_lesson_output` 同一事务写 `capacity_note`（`normalize_capacity_note`
  trim；缺席/空白写 NULL 清旧值），`over_capacity` tracing 原样保留。
- DTO/服务/路由：`CapacityFeedbackChoice`（serde kebab
  `accept|extend|split`，写侧严格、读侧宽容未知值）+
  `CapacityFeedbackView {choice, at}` + `LessonView.capacity_note` /
  `capacity_feedback`（后者仅 lesson detail 填本人那一行）+
  `record_capacity_feedback`（choice 空/未知 400；课时不存在/未选课 404；
  `ON CONFLICT(user_id, lesson_id) DO UPDATE`，`feedback_id` 首写生成覆盖
  不改）+ `POST /api/learning/lessons/{id}/capacity-feedback`（body
  `{choice}`；不新增 GET）。
- 读侧聚合：`CourseDetail.overloaded_lessons` = 课程内
  `capacity_note IS NOT NULL AND trim(capacity_note) <> ''` 的课时数（纯读、
  内容级计数，不泄露他人选择）。
- 前端：`capacityFeedback.ts` 纯投影（有自报才呈卡 / 回显旧选择 / 聚合 0
  零版面 / 三选项 i18n 键）+ `CapacityFeedbackCard`（三选一、成功后回显、
  失败行内 `role=alert` 可重按、语义色 token）+ `LessonBlock` 内嵌 +
  课程详情顶部「N 节超载」（0 不占版面）；`api.recordCapacityFeedback`；
  zh-CN/en-US 双写 10 键，`gen:i18n` 后 11584 keys。
- 文档：`docs/architecture/learning.zh.md` 新增「节点容量反馈信号（y07）」
  节（数据形状/边界/零结算零置位/HTTP 面）并补迁移与路由清单。

**零结算零置位**

`capacity_feedback_tests.rs` 钉住：一次反馈调用前后 `learning_attempts` /
`learning_review_events` / `learning_review_log` / `learning_xp_events` /
`learning_lesson_progress` / `learning_review_items`（含 FSRS 列合计）计数
不变、课时 status 与 est 不变、任务注册表零新增、WS 零 phase。反馈也不注入
共享课时生成（`generate_*` 不读该表；上下文类型无反馈槽）。

**测试命令与结果**

- `CARGO_TARGET_DIR=… cargo test -p nomifun-learning -p nomifun-db`：
  learning 825 passed / 0 failed（含新增 5 项），db 全部 suites passed
  （含新增 `learning_lesson_capacity_feedback_migration` 4 项）。
- `cargo fmt -p nomifun-learning -p nomifun-db -- --check`：clean。
- `cd ui && bun test src/renderer/pages/learning`：105 passed / 0 failed
  （含新增 `capacityFeedback.test.ts` 8 项 +
  `CapacityFeedbackCard.render.test.tsx` 4 项）。
- `bun run typecheck`：55 条存量错误，学习模块 0 条、无新增。
- `bun run gen:i18n && bun run check:i18n`：up to date（11584 keys）。
- `bun run check:theme`：passed。

**偏差**

- 服务入参 `choice: &str`（而非直接反序列化枚举）：空/未知值要走服务端
  400 口径，而不是 axum `Json` 的 422 反序列化拒绝；枚举仍是 Rust 单源，
  响应/回显经 `CapacityFeedbackChoice` 序列化。
- 聚合口径「活跃课时」= 现存课程下全部课时（课程 `deleted_at` 由详情入口
  过滤；课时无墓碑列）。
- `note` 列按票面建表保留，但当前 API 不写（恒空串）——本票只有三选一，
  自由文本留作未来扩展。
- **评审修复（2026-11-04）**：删除 `feedback_id` 与 `note` 两列。本表无外部
  引用方，`UNIQUE(user_id, lesson_id)` 已是业务身份，代理 UUID 列冗余；
  `note` 恒写空串属 Speculative Generality。表本身保留，表数仍 126；契约
  注册（identity / NON_REFERENCE）与迁移测试同步收缩。
- **评审修复（2026-11-04，可见性）**：`CourseDetail.overloaded_lessons` 聚合
  此前只在传统课程工作区呈「N 节超载」徽标；学习图课程走
  `LearningGraphWorkspace`，同一聚合不可见。该组件已持有 `CourseDetail`，
  直接在标题行复用 `overloadedBadgeCount` 与既有 i18n 键（0 = 零版面），
  不新增查询、不新增死键。
- 真机 smoke（生成带 `capacity_note` → 课时页呈卡 → 重进回显 → 聚合 +1，
  `EXIT=0`）未在本 worktree 执行（需真实 provider 凭据与串行环境）——
  **留波次真机验收**。
- `bun run check:dead-css` / `check:button-layout` 在集成分支尖端已红
  （失败文件均为本票之外的历史文件；`check:button-layout` 另因 Windows
  临时目录 EBUSY），本票文件零新增告警。
