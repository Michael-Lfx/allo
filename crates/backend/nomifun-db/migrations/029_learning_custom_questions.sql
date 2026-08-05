-- Learner-authored questions that live outside any course. They carry
-- their own FSRS schedule row-for-row so the review queue can serve them
-- without an enrollment or concept. `concept_id` is an optional link back
-- to an existing concept (including orphaned ones from deleted courses).
CREATE TABLE learning_custom_questions (
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
    kind                 TEXT NOT NULL CHECK (kind IN ('single_choice', 'true_false')),
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

CREATE INDEX idx_learning_custom_questions_user_due
    ON learning_custom_questions (user_id, due_at);

CREATE INDEX idx_learning_custom_questions_user_id
    ON learning_custom_questions (user_id);

CREATE INDEX idx_learning_custom_questions_concept_id
    ON learning_custom_questions (concept_id);
