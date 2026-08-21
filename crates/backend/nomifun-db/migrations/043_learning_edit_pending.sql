-- 复习卡片待编辑标记：刷卡时不打断心流，标记"待编辑"并可选附一句描述，
-- 供日后打开编辑对话框时找回当时的编辑思路。
--
-- edit_pending_at 存 UTC 毫秒时间戳，NULL 表示未标记；edit_note 为选填
-- 描述（可为 NULL）。编辑保存成功即清除两列；重新标记会覆盖旧描述。
-- 课程题标记在 learning_review_items（复习项即卡片）；自建题标记在
-- learning_custom_questions（卡片即问题本身）。
--
-- v3 contract: 无物理外键、无触发器。

ALTER TABLE learning_review_items
    ADD COLUMN edit_pending_at INTEGER;

ALTER TABLE learning_review_items
    ADD COLUMN edit_note TEXT;

ALTER TABLE learning_custom_questions
    ADD COLUMN edit_pending_at INTEGER;

ALTER TABLE learning_custom_questions
    ADD COLUMN edit_note TEXT;
