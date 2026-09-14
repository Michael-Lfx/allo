-- 课时分节 + 题型扩展（ADR-0002）。
--
-- 1) learning_lesson_sections：课时内部的节段清单（manifest），是逐节生成
--    管线的进度事实源——status=ready 的节已有正文，支持断点续跑与单节
--    重写；旧课时无节行，读取端回退渲染 learning_lessons.summary（双读）。
-- 2) learning_activities / learning_custom_questions 重建：kind CHECK 从
--    4 种扩到 9 种（新增 multi_choice / numeric / ordering / matching /
--    open_question，题型体系对齐 learnhub）；activities 增加 section_key
--    列（题目绑定来源节，NULL = 跨节综合题「通用」）。
-- 3) learning_courses 增加 teaching_style（讲解风格：standard / socratic /
--    feynman），课程级选择，课时生成时决定节写作提示词变体。
--
-- v3 contract: 无物理外键、无触发器；SQLite 无法修改 CHECK，需要重建表。

-- ── 1. 课时节段表 ────────────────────────────────────────────────────────────
CREATE TABLE learning_lesson_sections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    section_key TEXT NOT NULL CHECK (length(section_key) > 0 AND length(section_key) <= 32),
    lesson_id   TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 节类型菜单（交互节暂缓，见 ADR-0002 妥协清单）。
    kind        TEXT NOT NULL CHECK (kind IN ('concept', 'example', 'demo', 'summary', 'practice')),
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    -- 大纲要点（一句话）；生成前置骨架注入用。
    points      TEXT NOT NULL DEFAULT '',
    -- 节正文（Markdown）。pending 节为空串。
    body_md     TEXT NOT NULL DEFAULT '',
    -- pending：大纲已规划未生成；ready：正文已生成；failed：修复轮耗尽。
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'ready', 'failed')),
    -- 该节自身的重写次数。
    version     INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
    position    INTEGER NOT NULL CHECK (position >= 0),
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    UNIQUE (lesson_id, section_key),
    UNIQUE (lesson_id, position)
);
CREATE INDEX idx_learning_lesson_sections_lesson_id
    ON learning_lesson_sections (lesson_id, position);

-- ── 2. learning_activities：kind 扩到 9 种 + section_key ───────────────────
CREATE TABLE learning_activities_new (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id TEXT NOT NULL UNIQUE CHECK (
        length(activity_id) = 36
        AND lower(activity_id) = activity_id
        AND activity_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(activity_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    lesson_id   TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    kind        TEXT NOT NULL CHECK (
        kind IN ('single_choice', 'true_false', 'reflection', 'fill_in_blank',
                 'multi_choice', 'numeric', 'ordering', 'matching', 'open_question')
    ),
    prompt      TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    -- 来源节的 section_key（如 s2）；NULL = 跨节综合题（通用）。
    section_key TEXT,
    position    INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (lesson_id, position)
);
INSERT INTO learning_activities_new (
    id, activity_id, lesson_id, kind, prompt, config_json, section_key, position
)
SELECT id, activity_id, lesson_id, kind, prompt, config_json, NULL, position
FROM learning_activities;
DROP TABLE learning_activities;
ALTER TABLE learning_activities_new RENAME TO learning_activities;
CREATE INDEX idx_learning_activities_lesson_id
    ON learning_activities(lesson_id, position);

-- ── 3. learning_custom_questions：kind 同步扩到 9 种 ───────────────────────
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
    kind                 TEXT NOT NULL CHECK (
        kind IN ('single_choice', 'true_false', 'reflection', 'fill_in_blank',
                 'multi_choice', 'numeric', 'ordering', 'matching', 'open_question')
    ),
    prompt               TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json          TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    concept_id           TEXT CHECK (
        concept_id IS NULL OR (
            length(concept_id) = 36
            AND lower(concept_id) = concept_id
            AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    due_at               INTEGER NOT NULL,
    stability_days       REAL NOT NULL DEFAULT 0.0 CHECK (stability_days >= 0.0),
    difficulty           REAL NOT NULL DEFAULT 5.0 CHECK (difficulty >= 1.0 AND difficulty <= 10.0),
    review_count         INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
    lapse_count          INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
    last_reviewed_at     INTEGER,
    archived_at          INTEGER,
    edit_pending_at      INTEGER,
    edit_note            TEXT,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL
);
INSERT INTO learning_custom_questions_new (
    id, custom_question_id, user_id, kind, prompt, config_json, concept_id,
    due_at, stability_days, difficulty, review_count, lapse_count,
    last_reviewed_at, archived_at, edit_pending_at, edit_note, created_at, updated_at
)
SELECT id, custom_question_id, user_id, kind, prompt, config_json, concept_id,
       due_at, stability_days, difficulty, review_count, lapse_count,
       last_reviewed_at, archived_at, edit_pending_at, edit_note, created_at, updated_at
FROM learning_custom_questions;
DROP TABLE learning_custom_questions;
ALTER TABLE learning_custom_questions_new RENAME TO learning_custom_questions;
CREATE INDEX idx_learning_custom_questions_user_due
    ON learning_custom_questions (user_id, due_at);
CREATE INDEX idx_learning_custom_questions_user_id
    ON learning_custom_questions (user_id);
CREATE INDEX idx_learning_custom_questions_concept_id
    ON learning_custom_questions (concept_id);

-- ── 4. 讲解风格（课程级） ───────────────────────────────────────────────────
ALTER TABLE learning_courses ADD COLUMN teaching_style TEXT NOT NULL DEFAULT 'standard'
    CHECK (teaching_style IN ('standard', 'socratic', 'feynman'));
