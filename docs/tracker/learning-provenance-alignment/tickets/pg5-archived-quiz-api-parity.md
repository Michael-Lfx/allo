---
id: pg5
title: 归档课的按课 quiz 读数仍到期、直连 start 仍可开考——聚合面已筛，按课面没筛
type: task
status: open
assignee:
blocked_by: []
part_of: map.md
---

## Question

learnhub #278（2026-11-26，票面自述「来源: allo t1 的同构面」）发现：
教练页检测进度对归档课仍显示到期，与「归档课不触发检测」相左。本仓的
聚合读数（quiz overview）已筛 `enabled`，但**按课读数与 start 入口没筛**：
`GET /courses/{id}/quiz/progress` 对归档课返回 `due=true`；直接
`POST /courses/{id}/quiz/start` 也能开考。当前 UI 恰好用 `enabled` 藏了
入口，所以用户看不见；API 语义仍自相矛盾，任何直连/未来消费面都会踩。

## 已核验事实（2026-11-26 对码）

- 按课读数不筛启用态：`crates/backend/nomifun-learning/src/service/xp.rs:450-459`
  `SELECT 1 FROM learning_courses WHERE course_id = ? AND deleted_at IS NULL`
  ——归档课照回 `due=true`；路由 `routes.rs:161-163`。
- 聚合读数筛：`xp.rs:492-495` `AND c.deleted_at IS NULL AND c.enabled = 1`；
  测试 `service/tests.rs:11830-11846`「归档/墓碑/零结算不出列」。
- 开考只看到期：`service/quiz.rs:220-230` 只查 `progress.due`，无
  `require_live_graph_course`/`enabled` 守卫。
- UI 现状（无症状）：图工作区 `LearningGraphWorkspace.tsx:173`/`:1074`
  `!zeroNodes && detail.course.enabled`；传统课
  `CourseWorkspace.tsx:289` `{course.enabled && <QuizEntryCard …>}`；
  首页横幅用 overview；归档卡只给恢复
  （`CourseCard.tsx:216-228`）。
- 本仓无 `coach_guide`/「检测进度」符号（与 learnhub 的呈现面不同）；
  本票只收 API 语义一致性。

## 影响面（2026-11-26 对码）

### 生产面

| 文件 | 触点 | 改动性质 |
| --- | --- | --- |
| `service/xp.rs` | `quiz_progress` 的存在性查询（`:455-465`） | 补 `AND enabled = 1`（或显式判 enabled） |
| `service/quiz.rs` | `quiz_start` 的 due 门（`:220-230`） | 先走存活守卫（归档 409 / 墓碑 404） |
| `service/learning_graph.rs` | `require_live_graph_course`（`:1892-1917`） | 复用或抽同口径 helper |
| `routes.rs` | `quiz_progress`/`quiz_start`（`:928-948`） | 零改（服务层收口） |

### 测试面

- `service/tests.rs`：`quiz_progress` 读数族（`:11672-11705`）、
  overview 口径（`:11817`）、`archived_course_cards_leave...`
  （`:4321`）、`quiz_start` due/supersede 族（`:11876+`）。
- 新增：归档课 `quiz_progress` due=false；`quiz_start` 409；恢复后
  due 回来；墓碑课 404。
- 前端：零改（UI 已按 `course.enabled` 隐藏入口）。

### 不受影响

- **聚合 overview**：已筛 `enabled`（`xp.rs:492-495`），不重复改。
- **复习卡/到期队列**：`tests.rs:4321` 已覆盖，不动。
- **归档触发策略**：ADR-0017 口径不变。
- **DB/迁移**：零迁移；只改查询/守卫。

### 风险

- **归档课的历史读数**：选择「due=false 保留行」而不是「整行不出」，
  恢复后按自然时间回来（与 learnhub #278 的推荐同形）。
- **直连 API 客户端**：归档课 start 从「可能成功」变 409，是行为收紧；
  当前 UI 不受影响，需在票面留一句兼容说明。


## 方案（建议）

1. 按课 `quiz_progress` 查询补 `AND enabled = 1`（`deleted_at` 已有）；
   归档课返回 `due=false`，进度行可保留（恢复后照自然时间回来）。
2. `quiz_start` 入口加存活守卫：归档 → 409「课程已归档，恢复后可
   继续」；墓碑/非图课 → 404。复用 `require_live_graph_course`
   （`service/learning_graph.rs:1892-1917`）或同口径查询。
3. 测试三态可逆：归档前 due>0 → 归档后按课 due=false / start 409 →
   恢复后 due 回来（与 `tests.rs:4321` 的聚合可逆测试同形，扩按课面）。

## Acceptance

- 单测：归档课 `GET /courses/{id}/quiz/progress` `due=false`；`start`
  返回 409；恢复后 due 恢复；墓碑课 404。
- 既有聚合测试不翻；`cargo test -p nomifun-learning` 全绿。
- 无 UI 改动（UI 已筛）；无 DB 迁移。

## Out of scope

- 归档课的复习卡/到期队列语义（`tests.rs:4321` 已覆盖，不动）。
- 教练页/检测进度的呈现面（本仓无此面）。
- 归档触发策略本身（ADR-0017 口径）。
