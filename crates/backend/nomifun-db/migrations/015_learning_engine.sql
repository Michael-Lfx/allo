CREATE TABLE learning_courses (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    course_id    TEXT NOT NULL UNIQUE CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    title        TEXT NOT NULL CHECK (trim(title) <> ''),
    description  TEXT NOT NULL DEFAULT '',
    domain       TEXT NOT NULL DEFAULT 'general' CHECK (trim(domain) <> ''),
    source_kb_id TEXT CHECK (
        source_kb_id IS NULL OR (
            length(source_kb_id) = 36
            AND lower(source_kb_id) = source_kb_id
            AND source_kb_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(source_kb_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    ),
    version      INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE TABLE learning_modules (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id   TEXT NOT NULL UNIQUE CHECK (
        length(module_id) = 36
        AND lower(module_id) = module_id
        AND module_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(module_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id   TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    position    INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (course_id, position)
);

CREATE TABLE learning_lessons (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    lesson_id         TEXT NOT NULL UNIQUE CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    module_id         TEXT NOT NULL CHECK (
        length(module_id) = 36
        AND lower(module_id) = module_id
        AND module_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(module_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    title             TEXT NOT NULL CHECK (trim(title) <> ''),
    summary           TEXT NOT NULL DEFAULT '',
    position          INTEGER NOT NULL CHECK (position >= 0),
    estimated_minutes INTEGER NOT NULL DEFAULT 10 CHECK (estimated_minutes > 0),
    source_path       TEXT,
    source_start      INTEGER CHECK (source_start IS NULL OR source_start >= 0),
    source_end        INTEGER CHECK (source_end IS NULL OR source_end >= source_start),
    UNIQUE (module_id, position)
);

CREATE TABLE learning_concepts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    concept_id  TEXT NOT NULL UNIQUE CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id   TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    concept_key TEXT NOT NULL CHECK (trim(concept_key) <> ''),
    title       TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    UNIQUE (course_id, concept_key)
);

CREATE TABLE learning_concept_prerequisites (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    concept_id              TEXT NOT NULL CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    prerequisite_concept_id TEXT NOT NULL CHECK (
        length(prerequisite_concept_id) = 36
        AND lower(prerequisite_concept_id) = prerequisite_concept_id
        AND prerequisite_concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(prerequisite_concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    UNIQUE (concept_id, prerequisite_concept_id),
    CHECK (concept_id <> prerequisite_concept_id)
);

CREATE TABLE learning_lesson_concepts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    lesson_id  TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    concept_id TEXT NOT NULL CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    UNIQUE (lesson_id, concept_id)
);

CREATE TABLE learning_activities (
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
    kind        TEXT NOT NULL CHECK (kind IN ('single_choice', 'true_false', 'reflection')),
    prompt      TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    position    INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (lesson_id, position)
);

CREATE TABLE learning_activity_concepts (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id TEXT NOT NULL CHECK (
        length(activity_id) = 36
        AND lower(activity_id) = activity_id
        AND activity_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(activity_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    concept_id  TEXT NOT NULL CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    UNIQUE (activity_id, concept_id)
);

CREATE TABLE learning_enrollments (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    enrollment_id TEXT NOT NULL UNIQUE CHECK (
        length(enrollment_id) = 36
        AND lower(enrollment_id) = enrollment_id
        AND enrollment_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(enrollment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id       TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    course_id     TEXT NOT NULL CHECK (
        length(course_id) = 36
        AND lower(course_id) = course_id
        AND course_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(course_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    enrolled_at   INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    UNIQUE (user_id, course_id)
);

CREATE TABLE learning_lesson_progress (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    enrollment_id TEXT NOT NULL CHECK (
        length(enrollment_id) = 36
        AND lower(enrollment_id) = enrollment_id
        AND enrollment_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(enrollment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    lesson_id     TEXT NOT NULL CHECK (
        length(lesson_id) = 36
        AND lower(lesson_id) = lesson_id
        AND lesson_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(lesson_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    status        TEXT NOT NULL CHECK (status IN ('not_started', 'in_progress', 'completed')),
    started_at    INTEGER,
    completed_at  INTEGER,
    updated_at    INTEGER NOT NULL,
    UNIQUE (enrollment_id, lesson_id),
    CHECK (
        (status = 'not_started' AND started_at IS NULL AND completed_at IS NULL)
        OR (status = 'in_progress' AND started_at IS NOT NULL AND completed_at IS NULL)
        OR (status = 'completed' AND started_at IS NOT NULL AND completed_at IS NOT NULL)
    )
);

CREATE TABLE learning_attempts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id    TEXT NOT NULL UNIQUE CHECK (
        length(attempt_id) = 36
        AND lower(attempt_id) = attempt_id
        AND attempt_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(attempt_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    enrollment_id TEXT NOT NULL CHECK (
        length(enrollment_id) = 36
        AND lower(enrollment_id) = enrollment_id
        AND enrollment_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(enrollment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    activity_id   TEXT NOT NULL CHECK (
        length(activity_id) = 36
        AND lower(activity_id) = activity_id
        AND activity_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(activity_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    response_json TEXT NOT NULL CHECK (json_valid(response_json)),
    score         REAL NOT NULL CHECK (score >= 0.0 AND score <= 1.0),
    passed        INTEGER NOT NULL CHECK (passed IN (0, 1)),
    feedback      TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL
);

CREATE TABLE learning_mastery_states (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    enrollment_id     TEXT NOT NULL CHECK (
        length(enrollment_id) = 36
        AND lower(enrollment_id) = enrollment_id
        AND enrollment_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(enrollment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    concept_id        TEXT NOT NULL CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    mastery           REAL NOT NULL DEFAULT 0.0 CHECK (mastery >= 0.0 AND mastery <= 1.0),
    evidence_count    INTEGER NOT NULL DEFAULT 0 CHECK (evidence_count >= 0),
    last_practiced_at INTEGER,
    updated_at        INTEGER NOT NULL,
    UNIQUE (enrollment_id, concept_id)
);

CREATE TABLE learning_review_items (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    review_item_id   TEXT NOT NULL UNIQUE CHECK (
        length(review_item_id) = 36
        AND lower(review_item_id) = review_item_id
        AND review_item_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(review_item_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    enrollment_id    TEXT NOT NULL CHECK (
        length(enrollment_id) = 36
        AND lower(enrollment_id) = enrollment_id
        AND enrollment_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(enrollment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    concept_id       TEXT NOT NULL CHECK (
        length(concept_id) = 36
        AND lower(concept_id) = concept_id
        AND concept_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(concept_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    due_at           INTEGER NOT NULL,
    stability_days   REAL NOT NULL DEFAULT 0.0 CHECK (stability_days >= 0.0),
    difficulty       REAL NOT NULL DEFAULT 5.0 CHECK (difficulty >= 1.0 AND difficulty <= 10.0),
    review_count     INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
    lapse_count      INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
    last_reviewed_at INTEGER,
    updated_at       INTEGER NOT NULL,
    UNIQUE (enrollment_id, concept_id)
);

CREATE INDEX idx_learning_courses_source_kb_id
    ON learning_courses(source_kb_id);
CREATE INDEX idx_learning_modules_course_id
    ON learning_modules(course_id, position);
CREATE INDEX idx_learning_lessons_module_id
    ON learning_lessons(module_id, position);
CREATE INDEX idx_learning_concepts_course_id
    ON learning_concepts(course_id);
CREATE INDEX idx_learning_concept_prerequisites_concept_id
    ON learning_concept_prerequisites(concept_id);
CREATE INDEX idx_learning_concept_prerequisites_prerequisite_concept_id
    ON learning_concept_prerequisites(prerequisite_concept_id);
CREATE INDEX idx_learning_lesson_concepts_lesson_id
    ON learning_lesson_concepts(lesson_id);
CREATE INDEX idx_learning_lesson_concepts_concept_id
    ON learning_lesson_concepts(concept_id);
CREATE INDEX idx_learning_activities_lesson_id
    ON learning_activities(lesson_id, position);
CREATE INDEX idx_learning_activity_concepts_activity_id
    ON learning_activity_concepts(activity_id);
CREATE INDEX idx_learning_activity_concepts_concept_id
    ON learning_activity_concepts(concept_id);
CREATE INDEX idx_learning_enrollments_user_id
    ON learning_enrollments(user_id);
CREATE INDEX idx_learning_enrollments_course_id
    ON learning_enrollments(course_id);
CREATE INDEX idx_learning_lesson_progress_enrollment_id
    ON learning_lesson_progress(enrollment_id);
CREATE INDEX idx_learning_lesson_progress_lesson_id
    ON learning_lesson_progress(lesson_id);
CREATE INDEX idx_learning_attempts_enrollment_id
    ON learning_attempts(enrollment_id, created_at DESC);
CREATE INDEX idx_learning_attempts_activity_id
    ON learning_attempts(activity_id);
CREATE INDEX idx_learning_mastery_states_enrollment_id
    ON learning_mastery_states(enrollment_id);
CREATE INDEX idx_learning_mastery_states_concept_id
    ON learning_mastery_states(concept_id);
CREATE INDEX idx_learning_review_items_enrollment_id
    ON learning_review_items(enrollment_id, due_at);
CREATE INDEX idx_learning_review_items_concept_id
    ON learning_review_items(concept_id);
