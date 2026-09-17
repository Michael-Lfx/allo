-- 降级兜底持久化（ADR-0008）：质检门未兑现 visual 承诺、修复预算耗尽的节
-- 以 visual=无 纯文字保底落库。此前降级只存在于生成期的 best-effort WS
-- 事件——课程里事后看不出哪些节是降级产物，整课零图表无法解释。
--
-- degraded=1 表示「当前正文是降级纯文字兜底」；单节重写成功兑现承诺或
-- 手动编辑后归 0。visual 列保留原声明不变——它是单节重写的承诺事实源
-- （迁移 051），降级不改变重试时应兑现的目标。
ALTER TABLE learning_lesson_sections ADD COLUMN degraded INTEGER NOT NULL DEFAULT 0;
