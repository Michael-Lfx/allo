---
id: j02
title: "课程管理面：现存且启用 + 改名/归档/删除强确认 + 概况"
type: task
status: open
assignee:
blocked_by: []
part_of: ../map.md
---

# j02: 课程管理面

**What to build:** 课程级「现存且启用」读侧横切口径 + 治理写面（改名/归档/删除强确认）
+ 概况读数，前后端一体。

**实现决议:** 三轮 g3（全部）。

**Blocked by:** None。

**Status:** ready-for-agent

- [ ] 迁移：`learning_courses` 加 `enabled INTEGER NOT NULL DEFAULT 1`（append-only 段位）
- [ ] `active_course_ids` 等价单源（`deleted_at IS NULL AND enabled = 1`）；**安排面全量
      排查收束**：到期队列/就绪推荐/教练上下文组装/自由练习抽取/XP 进度尺（后者随 j05）
      ——SQL 逐条加筛；**历史面明确不过筛**（日历/统计聚合）
- [ ] `PATCH /api/learning/courses/{id}` 改名端点：只动 `title`；`learning_goal`/结构
      快照/学习数据不动；标题唯一约束沿用（冲突 = 人面错误；软删墓碑行不阻塞新名）
- [ ] 归档/恢复端点：`enabled` 翻转（数据零删零改）；点名归档课的安排面动作给人面错误
      （「课程已归档，恢复后可继续学习」）
- [ ] **删除语义变更**：`deleteReviews` 连带删卡选项退役（端点参数 + `CourseDeleteDialog`
      勾选删除）；删除 = 墓碑 + 课时/概念网边照旧物理清理，**复习卡与作答记录保留**
- [ ] 强确认重写：逐字输入课程显示名 + 三条如实后果（内容永久移除 / 学习记录保留且各
      安排面不再出现本课 / 不可撤销、想暂停用归档）
- [ ] 概况读数：`CourseSummary` 扩 `enabled/total_lessons/settled_or_completed/due_today/
      total_xp`（total_xp 先行可空，j05 接数据）；列表行 + 详情头区展示（归档徽标 +
      恢复 + 改名入口 + 目标原句悬浮）
- [ ] i18n 双语（新增键全量对称）；`bun run check:i18n` 过
- [ ] 验证：`cargo test -p nomifun-learning`（安排面收窄用例：归档课出队列/历史面不筛）
      + `bun run typecheck`

**Resolution:**（收口时写）
