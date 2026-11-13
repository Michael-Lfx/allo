# t3: 删除收尾——active 回合废弃 + 死卡 archived_at 打标 + 071 存量背填（B3）

## Status

待施工（2026-11-13 开票；真库实见幽灵回合 + 死卡无标）。

## 已核验事实（2026-11-13 对库 + 对码）

- 用户删三门课时，图课程留有一个 `status='active'` 的进行中检测回合
  （真库实见：round 01a1cf08 active 挂墓碑课）。`delete_course` 不做任何
  收尾：active 回合不废弃、死卡不打标。该幽灵回合现消费面惰性无害
  （quiz.rs:414 按 user+course 查询，课删后不可达），但每门「删课时正测
  到一半」的课都会留一个，无界累积。
- 14 张死卡（activity 行已删）无任何结构标记——与归档卡、在学卡在行上
  不可区分，全靠读面自觉 JOIN 课程表（这正是 t1 泄漏的温床）。
  `learning_review_items.archived_at` 列现成，所有队列/计数查询已过滤
  `archived_at IS NULL` → 打标对现有读面零行为变化，只让数据自描述。
- 废弃动作有现成合法形态：quiz.rs:411 supersede 路径同款
  `validate_round_shape("abandoned", Some(now), None, None, None)` +
  `SET status='abandoned', finished_at=?`。
- 存量背填：dev 库与任何已删过课的用户库都有同构存量（死卡 + 幽灵
  active 回合），需迁移一次性背填；孤儿识别 SQL 与 070 迁移清理段同款
  模式（NOT EXISTS 反查 activities）。

## Terms

- **R1（删除事务内收尾）**：`delete_course` 事务内追加两条：
  ① 该课程 `status='active'` 的 quiz_rounds → `abandoned`（supersede
  同款形态）；② 该课程 enrollment 下、activity 行将被删的
  review_items → `archived_at = now`（在内容清理 SQL 之前打标，用
  同一子查询定位）。
- **R2（迁移 071 存量背填）**：append-only 迁移——
  ① 挂墓碑课（`deleted_at IS NOT NULL`）的 active 回合废弃；
  ② `NOT EXISTS (SELECT 1 FROM learning_activities a WHERE
  a.activity_id = r.activity_id)` 的 review_items 打 `archived_at`。
  不动账本面（xp_events/quiz_rounds 历史/review_log/review_events/
  enrollment/progress）。
- **R3（边界澄清，防过修）**：回望历史面**不过滤不打标**——记忆
  Panel3+4（memory.rs:167 诚实校准）、xp_events 全局统计、review_log
  保留墓碑课贡献是「账本永远保留」的设计语义；t1 只修**前瞻可行动面**
  （负荷预报/状态分布/due_count/日历分桶）。本票与 t1 合起来把这条
  边界写成注释留在两处查询旁。
- **R4（测试）**：删课正测一半的课 → active 回合变 abandoned、卡带
  archived_at；due_reviews/quiz 锚/概念推进读数不变（回归）；迁移测试
  （nomifun-db）+ 背填幂等。

## Acceptance

- `cargo test -p nomifun-learning` + `cargo test -p nomifun-db` 全绿；
  新断言绿。
- 真机：删课后再查库——无 active 幽灵回合、死卡带 archived_at；
  记忆面板历史曲线不回退（回望面保持）。

## Resolution

（关票时回填）
