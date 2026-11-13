# t1: 全局计数面墓碑过滤补齐 + 共享过滤片段（A1+B1）

## Status

待施工（2026-11-13 开票；真库实演坐实）。

## 已核验事实（2026-11-13 对库 + 对码）

- 用户删除三门测试课后，14 张复习卡留存但 activity 行已物理删除（死卡，
  永不可服务）。`due_reviews` 队列口径正确（review.rs:52 双过滤 + activity
  JOIN），实视 0。
- 但 4 个用户级计数查询只 JOIN `learning_enrollments`、按 `user_id`
  聚合，不带课程墓碑过滤，14/14 死卡全部计入（真库实演 overdue=14）：
  - `memory.rs:94` 记忆面板 Panel1 负荷预报（含 overdue_count）
  - `memory.rs:133` 记忆面板 Panel2 卡片状态分布
  - `checkin.rs:102` 打卡面 due_count
  - `checkin.rs:309` 打卡日历按日分桶
- 暴露面：`GET /api/learning/stats/memory`（MemoryHealthPanel）、
  `GET /api/learning/checkins/today`、`GET /api/learning/stats/calendar`。
  用户视角症状：面板/日历显示「14 张到期」，点进复习队列为空。
- 同文件不一致即根因实证：`checkin.rs:281` 的课程子查询带
  `deleted_at IS NULL`，102/309 没带——过滤语义 ~20 处手抄无代码单源
  （g3 定了「单源口径」但单源的是语义不是代码）。
- `learning_custom_questions` 用户级、无课程归属，不受删除影响（已核）。
- 其余 review_items 读点（review.rs:750/1351/1472/1522、course.rs:438、
  learning_graph.rs:4120）均有 activity INNER JOIN 或课程作用域兜底，
  不泄漏（已逐点核）。

## Terms

- **R1（共享过滤片段）**：Rust 侧新增共享 SQL 片段常量（如
  `COURSE_ARRANGEMENT_JOIN`，携带
  `JOIN learning_courses c ON c.course_id = e.course_id AND c.deleted_at IS NULL AND c.enabled = 1`
  语义），本票修复点一律使用。存量 ~20 处不强行全量收编（map 观察项，
  边际收益递减）；review.rs:52 的 base 片段可顺手收编（同一语义源）。
- **R2（修活面）**：memory.rs:94（Panel1 负荷预报/overdue）与
  memory.rs:133（Panel2 状态分布）接入 R1 片段——这是仅有的两个**活消费**
  泄漏面（MemoryHealthPanel 渲染负荷预报 day.due_count 与状态分布）。
- **R5（死链清理，2026-11-13 用户裁决：删）**：checkin.rs 两处泄漏
  经消费面复核均为**死读数**，随死链一并删除：
  - `calendar_stats`（checkin.rs:176-250，含 216 读 events、309 分桶）
    → 删函数 + 路由 `/api/learning/stats/calendar`（routes.rs:175）+
    `CalendarStatsQuery`（routes.rs:1018）+ api.ts `getCalendarStats`
    + `CalendarStats`/`CalendarDayStats` 类型（models.rs:3417/3432 及
    ui types.ts 对应）。j05 已拍板打卡按 XP 口径（`xp/calendar` →
    XpPanel），本链是口径迁移遗留体。既有 calendar_stats 测试
    （tests.rs 2195/2220/2285 三例）随函数删除。
  - `CheckinStatus.due_count` 字段（models.rs:2978 段，checkin.rs:102
    计算）→ 删字段 + 计算子查询 + 前端 types.ts 字段（XpPanel 零渲染；
    useCheckinStatus 链不拆，其余字段照常）。checkin_today 既有测试
    （2094-2187）里断言 due_count 的行同步移除。
  - 注意：`xp_calendar`（routes.rs:153/910，XpPanel 在用）**不动**。
- **R3（测试）**：新增断言——造卡 + 删课（软删）后：
  memory_health_stats 负荷预报/状态分布读数归零/回落；且 due_reviews
  口径不变（回归守卫）。`memory_health_stats` 现无测试，一并落首测。
  R5 删除涉及的既有 checkin/calendar 测试同步移除，其余 checkin_today
  测试保持绿。
- **R4（不做，边界）**：回望历史面不过滤——memory.rs:167 记忆 Panel3+4
  诚实校准、xp_events 全局统计、review_log 保留墓碑课贡献是「账本永远
  保留」的设计语义（详细定性与分工见 map C2 观察项）。本票只修前瞻
  可行动面。

## Acceptance

- 真库演算口径：死卡 overdue 计数 14 → 0（面板与队列一致）。
- learning 回归绿 + 新增断言绿；`cargo test -p nomifun-learning` 全绿。
- 真机：删课后记忆面板负荷预报/打卡日历不出现幽灵到期数。

## Resolution

（关票时回填）
