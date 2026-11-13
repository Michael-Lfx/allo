-- 删除收尾存量背填（t3 R2，ADR-0017 §4）：运行时 delete_course 已在删除
-- 事务内收尾（active 回合废弃、死卡打 archived_at、墓碑 enabled = 0），
-- 本迁移对存量一次性执法——070 以来任何「已删过课」的用户库都留有同构残
-- 影：墓碑课（deleted_at IS NOT NULL）上的 active 幽灵回合（消费面惰性无
-- 害但无界累积），activity 行已删、永不复服务的死卡（与归档卡、在学卡在
-- 行上不可区分），以及 enabled 仍为 1 的旧墓碑（墓碑统一形态未及——双状
-- 态位冗余未收敛，半抄过滤只看 enabled 的查询面会把旧墓碑当在学供给）。
-- 账本面不动：xp_events / quiz_rounds 历史（completed
-- 与既有 abandoned）/ review_log / review_events / enrollments / progress
-- 原样保留——回望历史面「账本永远保留」是设计语义（ADR-0017 §2）。
-- 三条 UPDATE 天然幂等：abandoned 非 active 不再命中、archived_at IS
-- NULL 过滤不重打标、enabled = 0 对已清零墓碑零副作用，重放零副作用。
-- v3 contract: 无物理外键、无触发器。

-- ① 墓碑课的 active 回合废弃：supersede 同款形态（abandoned + finished_at，
--    answers/report/earned 保持空——形态规则单源在 Rust
--    validate_round_shape，ADR-0015）。按课程键定，全用户一并废弃。
UPDATE learning_quiz_rounds
SET status      = 'abandoned',
    finished_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000
WHERE status = 'active'
  AND course_id IN (
    SELECT course_id FROM learning_courses WHERE deleted_at IS NOT NULL
  );

-- ② 死卡打 archived_at（070 清理段同款孤儿识别形态：NOT EXISTS 反查
--    activities）；archived_at IS NULL 过滤保证幂等、不覆盖手工归档时间。
--    列现成、全读面已过滤 archived_at IS NULL → 前瞻口径零行为变化，只让
--    数据自描述（「死卡」成为可查询状态而非推断）。
UPDATE learning_review_items
SET archived_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000,
    updated_at  = CAST(strftime('%s', 'now') AS INTEGER) * 1000
WHERE archived_at IS NULL
  AND NOT EXISTS (
    SELECT 1 FROM learning_activities a
    WHERE a.activity_id = learning_review_items.activity_id
  );

-- ③ 墓碑统一形态背填（运行时 t3 R1③ 同款：enabled 随 deleted_at 一并
--    清零，ADR-0017 §3）——旧墓碑 enabled 缺省 1，双状态位冗余在此收敛。
UPDATE learning_courses
SET enabled = 0
WHERE deleted_at IS NOT NULL;
