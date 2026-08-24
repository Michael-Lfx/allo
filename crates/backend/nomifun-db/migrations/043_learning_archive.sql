-- 复习卡片归档：不删除数据，但不再出现在复习队列与到期计数中。
--
-- 归档语义 = 暂停（suspend）：卡片及其 FSRS 排期数据完整保留，可在问题
-- 管理中恢复（unarchive）。归档列存 UTC 毫秒时间戳，NULL 表示未归档。
-- 队列查询、到期计数、打卡统计与日历聚合均需排除已归档行；问题管理视图
-- 以 state = 'archived' 展示并支持恢复。
--
-- 课程题归档在 learning_review_items（复习项即卡片）；自建题归档在
-- learning_custom_questions（卡片即问题本身）。
--
-- v3 contract: 无物理外键、无触发器。

ALTER TABLE learning_review_items
    ADD COLUMN archived_at INTEGER;

ALTER TABLE learning_custom_questions
    ADD COLUMN archived_at INTEGER;
