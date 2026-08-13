-- Fill-in-the-blank: a third objective activity kind, accepted both in
-- generated course lessons and learner-authored custom questions. The blank
-- sits at a relationship-critical spot and carries near-synonym distractor
-- traps in config_json; accepted answers are a JSON array. SQLite cannot
-- alter a CHECK constraint, so both tables are rebuilt with the kind check
-- widened; existing rows are copied verbatim (kind values all stay valid).

-- ── learning_activities: widen the kind CHECK ───────────────────────────────
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
    kind        TEXT NOT NULL CHECK (kind IN ('single_choice', 'true_false', 'reflection', 'fill_in_blank')),
    prompt      TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    position    INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (lesson_id, position)
);
INSERT INTO learning_activities_new (
    id, activity_id, lesson_id, kind, prompt, config_json, position
)
SELECT id, activity_id, lesson_id, kind, prompt, config_json, position
FROM learning_activities;
DROP TABLE learning_activities;
ALTER TABLE learning_activities_new RENAME TO learning_activities;
CREATE INDEX idx_learning_activities_lesson_id
    ON learning_activities(lesson_id, position);

-- ── learning_custom_questions: widen the kind CHECK ─────────────────────────
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
    kind                 TEXT NOT NULL CHECK (kind IN ('single_choice', 'true_false', 'fill_in_blank')),
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
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL
);
INSERT INTO learning_custom_questions_new (
    id, custom_question_id, user_id, kind, prompt, config_json, concept_id,
    due_at, stability_days, difficulty, review_count, lapse_count,
    last_reviewed_at, created_at, updated_at
)
SELECT id, custom_question_id, user_id, kind, prompt, config_json, concept_id,
       due_at, stability_days, difficulty, review_count, lapse_count,
       last_reviewed_at, created_at, updated_at
FROM learning_custom_questions;
DROP TABLE learning_custom_questions;
ALTER TABLE learning_custom_questions_new RENAME TO learning_custom_questions;
CREATE INDEX idx_learning_custom_questions_user_due
    ON learning_custom_questions (user_id, due_at);
CREATE INDEX idx_learning_custom_questions_user_id
    ON learning_custom_questions (user_id);
CREATE INDEX idx_learning_custom_questions_concept_id
    ON learning_custom_questions (concept_id);
