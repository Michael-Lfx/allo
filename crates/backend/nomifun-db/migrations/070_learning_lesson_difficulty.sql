-- 节点难度（1-5）：教练生长批的深度水位执法面——本批最高难度不得超过
-- 已落库批次数 + 2，形式化/证明类内容（4-5）必须等台阶铺满低档后再长。
ALTER TABLE learning_lessons ADD COLUMN difficulty INTEGER NOT NULL DEFAULT 2;
