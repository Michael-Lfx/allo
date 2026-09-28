-- 生长治理（ADR-0009 Amendment 1）：批行状态机。
--
-- learning_growth_batches 从"出生档案"升级为"生长任务的持久化载体"：
-- 生长开始即插 pending 行（含 seq 预留），成功填 node_ids 转 applied，
-- 失败留 failed 行（应用重启后 pending 即中断痕迹，下次生长起扫为
-- failed），空手批（idle）删行不残留。历史时间线据此展示"生长中/失败"。
--
-- 既有行全部是已落库的成功批，默认 applied。
--
-- v3 contract: 仅追加列，无物理外键、无触发器。

ALTER TABLE learning_growth_batches ADD COLUMN status TEXT NOT NULL DEFAULT 'applied'
    CHECK (status IN ('pending', 'applied', 'failed'));

CREATE INDEX idx_learning_growth_batches_pending
    ON learning_growth_batches (status, created_at);
