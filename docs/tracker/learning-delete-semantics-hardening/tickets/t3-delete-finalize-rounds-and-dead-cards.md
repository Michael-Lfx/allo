# t3: 删除收尾——active 回合废弃 + 死卡 archived_at 打标 + 071 存量背填（B3）

## Status

已关（2026-11-13 施工收口，f7ad320e4 经 merge d4b0cf902 合入集成分支；评审修复 29f5475da；整波经 04c110cae 并回 learn0928）。

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

- **R1（删除事务内收尾，2026-11-13 裁决增补 ③）**：`delete_course`
  事务内追加三条：
  ① 该课程 `status='active'` 的 quiz_rounds → `abandoned`（supersede
  同款形态）；② 该课程 enrollment 下、activity 行将被删的
  review_items → `archived_at = now`（在内容清理 SQL 之前打标，用
  同一子查询定位）；③ 墓碑统一形态——课程行 `UPDATE` 同时清
  `enabled = 0`（原只写 `deleted_at`；无恢复路径零风险，半抄过滤的
  查询面从此安全，ADR-0017 §3）。
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

已施工收口（2026-11-13，f7ad320e4 + 评审修复 29f5475da）。

- R1①：新增 `abandon_active_rounds(executor, scoped_user, course_id, now)` 单源助手落 quiz.rs（pub(super)，executor 接池或事务）；**quiz_start 的 supersede 路径同步改走它**（评审核等价：SQL/bind 顺序逐字一致）——废弃形态单源化超出票面字面，评审判定接受。delete_course 事务内对墓碑课调用（课程键定分流）。
- R1②：死卡打标在 `purge_course_content` 之前（activities 在场时定位），同款子查询；R1③：墓碑 UPDATE 同时 `enabled = 0`。三处收尾语句旁注明 R 编号 + ADR-0017。
- R2：迁移 `071_learning_delete_finalize_backfill.sql`（NOT EXISTS 反查形态同 070 清理段）；评审修复 F1 补第三条 UPDATE——**存量墓碑 enabled=0 背填**（Standards 轴硬违规：原 071 漏了它，course.rs 注释承诺失真），头注扩为「三条 UPDATE 天然幂等」。迁移测试 2 例：`tombstone_ghost_rounds_are_abandoned_and_dead_cards_archived`（含正负对照：在学课回合/卡/enabled 原样）+ `backfill_is_idempotent_on_replay`。
- R3：回望面边界注释评审修复 F2 落 memory.rs Panel3+4、xp.rs today_xp。
- R4：`delete_course_finalizes_active_round_and_dead_cards`（TDD 红→绿）；评审修复 F3 升级——真断言而非注释：删课后两课 review_log 行原样保留（digest 输入不变，结构性理由入测试 doc，对照 learning_graph.rs:3109-3117 确认其读面无 archived_at/enabled 过滤）。
- 验证：`cargo test -p nomifun-learning --lib` 965/0；`cargo test -p nomifun-db` 全绿（070 系列基线/lineage 无需跟随）；fmt 按包 check 干净。一次 `quiz_start_supersedes_active_round` 全量跑失败经两次复跑+单跑全绿，判定为共享锁慢机抖动非代码问题。
- 真机复验点：删课后再查库——无 active 幽灵回合、死卡带 archived_at、墓碑 enabled=0；记忆面板曲线不回退（待用户桌面走查；dev 库 3 墓碑 + 1 幽灵回合 + 14 死卡由 071 一次性收编）。
