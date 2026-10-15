---
id: j02
title: "课程管理面：现存且启用 + 改名/归档/删除强确认 + 概况"
type: task
status: closed
assignee: zhangyj
blocked_by: []
part_of: ../map.md
---

# j02: 课程管理面

**What to build:** 课程级「现存且启用」读侧横切口径 + 治理写面（改名/归档/删除强确认）
+ 概况读数，前后端一体。

**实现决议:** 三轮 g3（全部）。

**Blocked by:** None。

**Status:** ready-for-agent

- [x] 迁移：`learning_courses` 加 `enabled INTEGER NOT NULL DEFAULT 1`（append-only 段位）
- [x] `active_course_ids` 等价单源（`deleted_at IS NULL AND enabled = 1`）；**安排面全量
      排查收束**：到期队列/就绪推荐/教练上下文组装/自由练习抽取/XP 进度尺（后者随 j05）
      ——SQL 逐条加筛；**历史面明确不过筛**（日历/统计聚合）
- [x] `PATCH /api/learning/courses/{id}` 改名端点：只动 `title`；`learning_goal`/结构
      快照/学习数据不动；标题唯一约束沿用（冲突 = 人面错误；软删墓碑行不阻塞新名）
- [x] 归档/恢复端点：`enabled` 翻转（数据零删零改）；点名归档课的安排面动作给人面错误
      （「课程已归档，恢复后可继续学习」）
- [x] **删除语义变更**：`deleteReviews` 连带删卡选项退役（端点参数 + `CourseDeleteDialog`
      勾选删除）；删除 = 墓碑 + 课时/概念网边照旧物理清理，**复习卡与作答记录保留**
- [x] 强确认重写：逐字输入课程显示名 + 三条如实后果（内容永久移除 / 学习记录保留且各
      安排面不再出现本课 / 不可撤销、想暂停用归档）
- [x] 概况读数：`CourseSummary` 扩 `enabled/total_lessons/settled_or_completed/due_today/
      total_xp`（total_xp 先行可空，j05 接数据）；列表行 + 详情头区展示（归档徽标 +
      恢复 + 改名入口 + 目标原句悬浮）
- [x] i18n 双语（新增键全量对称）；`bun run check:i18n` 过
- [x] 验证：`cargo test -p nomifun-learning`（安排面收窄用例：归档课出队列/历史面不筛）
      + `bun run typecheck`

**Resolution:**

**速览**：全项落地（feat/learn0928，本票单提交）；迁移 076 + 三个端点 + 删除语义
对齐 learnhub + 强确认重写 + 概况读数，前后端一体。

- **迁移 076**（`076_learning_course_enabled.sql`）：`learning_courses.enabled INTEGER
  NOT NULL DEFAULT 1`（append-only；存量行全部回填启用 = 「缺席列视为启用」兼容语义）。
  db 侧新增 `learning_course_enabled_migration.rs` 三测：存量行默认启用 / 归档恢复
  只翻 enabled 不碰墓碑位 / 活跃集单源口径（归档课与墓碑课都不在
  `deleted_at IS NULL AND enabled = 1`）。
- **安排面收束（全量排查后的落点清单）**：①到期队列 `due_reviews` base SQL——
  `LEFT JOIN courses` 改带筛 JOIN（`deleted_at IS NULL AND enabled = 1`），归档课与
  已删课的卡一起出队（删除语义变更的自然推论：强确认后果②「安排面不再出现本课」）；
  已出卡的单卡入口（`review_entry`/`rate_review`）不过筛——作答提交不因归档失败，
  历史面如实落账；②生长入口 `kick_growth`（手动/自动共用管道）补
  `require_live_graph_course` 守卫（此前该入口无存活校验）；③结构重跑
  `regenerate_structure` 走既有守卫（扩 enabled）；④课时内容生成入口
  `generate_lesson_content` 加软删 404 + 归档 Conflict 守卫；⑤「教练上下文组装」的
  收窄 = 入口守卫（归档课到不了教练），**跨课程已教账/taught_ledger/概念表
  other_courses 不过筛**——归档是暂停不是撤销已学事实，筛了会让他课就绪集收缩
  （实施裁决，记入偏差）；⑥就绪推荐 `ready_candidates` 课程内自洽，经入口守卫覆盖；
  ⑦「自由练习抽取」无独立面（复习队列 base 即抽取面，已被①覆盖）；⑧XP 进度尺
  随 j05。**历史面确认未筛**：日历/统计聚合/打卡历史（checkin:303）原样。
- **三个端点**：`PATCH /api/learning/courses/{id}`（改名，`RenameCourseRequest { title }`
  ，返回更新后的 CourseSummary）+ `POST .../archive` + `POST .../restore`（enabled
  翻转，墓碑行 404）。改名只动 title；撞名 = 409（`deleted_at IS NULL AND
  course_id <> ?` 查重——墓碑不阻塞新名，测试钉住）；空名 400。
- **删除语义变更**：`DeleteCourseRequest`/`delete_reviews` 参数整体退役（模型/路由/
  api.ts/对话框四处）；`delete_course` 恒走「保留账本」路径——复习卡/作答/进度/
  注册行保留（删后测试核对注册行数不变），课时/模块/概念网边/课程标签照旧物理清理，
  课程行落墓碑；`delete_courses_by_title`（教程种子替换流程，非用户删除面）不动。
  教程种子的同名查重本就只看现存课（墓碑不阻塞预置课重导入）。
- **强确认重写**（`CourseDeleteDialog`）：逐字输入课程显示名（trim 相等才可用确认
  按钮，不一致有提示）+ 三条如实后果（内容永久移除 / 学习记录保留且安排面不再出现
  本课 / 不可撤销、想暂停用归档）。
- **概况读数**：`CourseSummary` 扩 `enabled / settled_or_completed / due_today /
  total_xp: Option<i64>`（total_xp 恒 None 等 j05 账本；settled_or_completed 先行
  = completed_lessons，j05 切结算口径——字段先行避免 j05 再改键）；列表/详情 SQL 各
  加 `enabled` 列与 `due_today` 子查询（与到期队列同口径：archived_at/edit_pending_at
  为空 + due_at ≤ now）。列表目录不过筛 enabled——归档课带徽标在列、可恢复。
- **前端**：`CourseCard` 重写（归档徽标 + 恢复主按钮 + 归档动作 + 改名入口 + 概况行
  今日到期/XP + 目标原句标题悬浮）；改名控件提取为 `CourseRenameButton`（卡片与两个
  详情工作区共用）；`CourseWorkspace`/`LearningGraphWorkspace` 头区加归档徽标 +
  恢复按钮 + 改名入口（传统工作区标题目标悬浮已有，沿用）。归档卡的「打开」主按钮
  换成「恢复」——列表层面就不再把归档课当可学入口。
- **i18n**：zh-CN/en-US 对称新增 16 键（归档/恢复/改名/概况/强确认三后果/逐字确认
  提示）；退役 3 键（deleteCourseReviews×3）；`gen:i18n` 重生成（11391 键）。
- **实施偏差与边界记录**：①「教练上下文组装」的收窄解释为入口守卫而非跨课程账本
  过筛（见上，⑤）；②归档课详情仍可打开（读侧视图不拦），生成/生长动作报人面错误
  ——比「详情也 404」更符合可逆暂停语义；③单卡入口不拦作答（队列收窄不拦提交，
  与历史面如实落账一致）；④enroll 不设归档守卫——详情打开即自动注册是现状，
  归档课浏览不算安排动作，队列/生成面已收窄。
- **验证**：`cargo test -p nomifun-learning` **241 全绿**（238 + 新增 3：归档出队/
  恢复回队与已出卡可作答、改名只动名与墓碑不阻塞、归档图课生长报人面错误恢复后
  放行；删除测试扩展注册行保留断言）；`cargo test -p nomifun-db` 458+ 全绿（含新
  迁移测试 3）；`bun run typecheck` 60 错 = 存量基线，learning 路径零新增；
  `bun run check:i18n` 过；`cargo check --workspace` 干净。
