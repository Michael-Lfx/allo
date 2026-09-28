-- 学习图重构（ADR-0009）第一步：退役旧 DAG 学习图实验与 per-course 概念体系。
--
-- 旧学习图（course_kind='learning_graph'，048 引入）被「概念网 + 生长」模型
-- 取代：先修边表 learning_graph_prerequisites 与全部旧图课程数据一并删除，
-- 不迁移旧实验数据（沿用 048 自身的破坏性更新先例）。
--
-- 传统课程的 per-course 概念体系（015 引入）从未开发完整，随本次重构整体
-- 下线：learning_concepts / learning_concept_prerequisites / 
-- learning_lesson_concepts / learning_activity_concepts / learning_mastery_states
-- 五张表 DROP；自建题的 concept_id 列随之移除（表 rebuild）。概念的未来
-- 唯一落点是全局概念登记表（067 引入）。
--
-- 注意：learning_lesson_progress 的 skipped 状态保留——跳过对传统课时同样
-- 语义自洽。learning_review_events 的卡片历史按契约保留（打卡统计不随卡删）。
--
-- v3 contract: 仅删数据/删表/重建表，无物理外键、无触发器。

-- ── 1. 旧图课程派生数据（按 activity → lesson → course 自底向上）────────

-- 复习日志：卡片（课程题复习项）被删后，其推进历史一并清除——旧图课程
-- 连同学史整体退场，不保留孤儿日志。
DELETE FROM learning_review_log WHERE source = 'course' AND item_id IN (
    SELECT r.review_item_id FROM learning_review_items r
    JOIN learning_activities a ON a.activity_id = r.activity_id
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_question_tags WHERE source = 'course' AND question_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_review_items WHERE activity_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_attempts WHERE activity_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_activity_concepts WHERE activity_id IN (
    SELECT a.activity_id FROM learning_activities a
    JOIN learning_lessons l ON l.lesson_id = a.lesson_id
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_activities WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lesson_sections WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lesson_concepts WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lesson_progress WHERE lesson_id IN (
    SELECT l.lesson_id FROM learning_lessons l
    JOIN learning_modules m ON m.module_id = l.module_id
    WHERE m.course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_lessons WHERE module_id IN (
    SELECT module_id FROM learning_modules
    WHERE course_id IN (SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph'));

DELETE FROM learning_enrollments WHERE course_id IN (
    SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph');

DELETE FROM learning_modules WHERE course_id IN (
    SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph');

DELETE FROM learning_course_tags WHERE course_id IN (
    SELECT course_id FROM learning_courses WHERE course_kind = 'learning_graph');

-- ── 2. 旧图课程行与先修边表 ─────────────────────────────────────────────

DELETE FROM learning_graph_prerequisites;

DELETE FROM learning_courses WHERE course_kind = 'learning_graph';

DROP TABLE learning_graph_prerequisites;

-- ── 3. per-course 概念体系（残余行随表 DROP 一并消失）───────────────────

DROP TABLE learning_mastery_states;
DROP TABLE learning_activity_concepts;
DROP TABLE learning_lesson_concepts;
DROP TABLE learning_concept_prerequisites;
DROP TABLE learning_concepts;

-- ── 4. 自建题移除 concept_id 列（SQLite 无法 DROP COLUMN，标准 rebuild）──

CREATE TABLE learning_custom_questions_new (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    custom_question_id   TEXT NOT NULL UNIQUE CHECK (
        length(custom_question_id) = 36
        AND lower(custom_question_id) = custom_question_id
        AND custom_question_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(custom_question_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id              TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- kind CHECK 以 038 的宽口径为准（三种题型）：重建必须逐字保留既有的
    -- 约束集合，收窄会让 038 之后落库的自建填空题在新表上必然 CHECK 失败。
    kind                 TEXT NOT NULL CHECK (
        kind IN ('single_choice', 'true_false', 'fill_in_blank')
    ),
    prompt               TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json          TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    due_at               INTEGER NOT NULL,
    stability_days       REAL NOT NULL DEFAULT 0.0 CHECK (stability_days >= 0.0),
    difficulty           REAL NOT NULL DEFAULT 5.0 CHECK (difficulty >= 1.0 AND difficulty <= 10.0),
    review_count         INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
    lapse_count          INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
    last_reviewed_at     INTEGER,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    archived_at          INTEGER,
    edit_pending_at      INTEGER,
    edit_note            TEXT
);

INSERT INTO learning_custom_questions_new
    (custom_question_id, user_id, kind, prompt, config_json, due_at, stability_days,
     difficulty, review_count, lapse_count, last_reviewed_at, created_at, updated_at,
     archived_at, edit_pending_at, edit_note)
SELECT custom_question_id, user_id, kind, prompt, config_json, due_at, stability_days,
       difficulty, review_count, lapse_count, last_reviewed_at, created_at, updated_at,
       archived_at, edit_pending_at, edit_note
FROM learning_custom_questions;

DROP TABLE learning_custom_questions;
ALTER TABLE learning_custom_questions_new RENAME TO learning_custom_questions;

CREATE INDEX idx_learning_custom_questions_user_due
    ON learning_custom_questions (user_id, due_at);

CREATE INDEX idx_learning_custom_questions_user_id
    ON learning_custom_questions (user_id);
