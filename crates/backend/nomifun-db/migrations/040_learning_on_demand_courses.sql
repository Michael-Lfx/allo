-- 按需生成课程（on-demand course generation）。
--
-- 课程生成拆成两段：先秒级产出大纲（blueprint）并立即导入课程骨架，
-- 课时正文与练习推迟到用户实际学习该课时时再生成。为让推迟的课时生成
-- 在生成任务行被删除后仍可用，把 blueprint_json 与 samples_json（采样到
-- 的知识库原文摘录）落到课程行；课时行记录大纲阶段的描述（purpose）与
-- 正文是否已生成（content_generated）。任务行记录 generation_mode 以便
-- 断点恢复 / 重试时正确推断下一阶段。
--
-- v3 contract: 仅追加列，无物理外键、无触发器；既有课时均已具备正文，
-- 故 content_generated 默认 1、generation_mode 默认 'full'，向后兼容。

ALTER TABLE learning_courses ADD COLUMN blueprint_json TEXT;
ALTER TABLE learning_courses ADD COLUMN samples_json TEXT;

ALTER TABLE learning_lessons ADD COLUMN purpose TEXT NOT NULL DEFAULT '';
ALTER TABLE learning_lessons ADD COLUMN content_generated INTEGER NOT NULL DEFAULT 1 CHECK (content_generated IN (0, 1));

ALTER TABLE learning_course_jobs ADD COLUMN generation_mode TEXT NOT NULL DEFAULT 'full' CHECK (generation_mode IN ('full', 'on_demand'));
