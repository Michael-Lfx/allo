# t1: 归档课 due_today 计数如实化（P1）

## Status

待施工（2026-11-13 开票；AB 审计坐实 + 用户裁决定口径）。

## 已核验事实（2026-11-13 对码）

- 课程目录/详情的到期徽标对**归档课**显示「N 张到期」，但归档课卡不可
  服务（due_reviews 全口径过滤 `enabled=1`，review.rs:52）——点进去
  队列为空。三处裸奔子查询（只滤卡级 archived_at/edit_pending_at，无
  课程存活条件）：
  - `course.rs:237-241`（`list_courses` due_today）
  - `course.rs:289-293`（`course_detail` due_today）
  - `course.rs:434-449`（`course_detail` due_review_count）
- 口径定性（ADR-0017 修正案两轴）：due 计数=**可服务性计数面**（「现在
  能复习什么」）→ 滤归档；知识供给面（教练/生长/就绪门）全宽含归档
  （用户裁决），**本票不碰供给面任何查询**。
- 删除课不出现在目录（`deleted_at IS NULL` 外层过滤），不在本票范围。

## Terms

- **R1（三处补过滤）**：三处子查询接入课程存活条件——子查询经
  `e.course_id = c.course_id` 与外层关联，补 `AND c.enabled = 1` 即可
  （list/detail 两处 due_today 同款；due_review_count 按 enrollment 直查、
  无 c 别名，接 `learning_courses` join 或等价 EXISTS，以可读性定）。
  效果：归档课徽标显示 0（如实——该课当前无可复习卡）。
- **R2（测试）**：tests.rs 课程目录/详情测试旁新增断言：造课+造卡+归档
  （set_course_archived(true)）→ due_today/无归档过滤的 due_review_count
  = 0；恢复归档 → 计数回来（可逆语义对账）。delete-hardening 的
  共享片段常量若可复用则复用（注意该片段是 JOIN 形态、本处是相关子查询
  ——形态不合就写 `c.enabled = 1` 相关条件 + 注释引 ADR-0017）。
- **R3（不做）**：知识供给面（coach/gate/taught_ledger/ready_partition/
  topology）零触碰（全宽裁决，t2 只加注释）；前端徽标渲染不动
  （0 张到期显示如实）；xp_calendar 未动。

## Acceptance

- 归档课三处计数=0、恢复即回；在学课计数不变（回归）。
- `cargo test -p nomifun-learning` 全绿 + fmt 按包 check 干净。
- 真机：归档课卡片/详情不再显示幽灵「N 张到期」。

## Resolution

（关票时回填）
