CREATE TABLE learning_courses (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    domain TEXT NOT NULL DEFAULT 'general' CHECK (trim(domain) <> ''),
    source_kb_id TEXT REFERENCES knowledge_bases(id) ON DELETE SET NULL,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE learning_modules (
    id TEXT PRIMARY KEY NOT NULL,
    course_id TEXT NOT NULL REFERENCES learning_courses(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (course_id, position)
);

CREATE TABLE learning_lessons (
    id TEXT PRIMARY KEY NOT NULL,
    module_id TEXT NOT NULL REFERENCES learning_modules(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    summary TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL CHECK (position >= 0),
    estimated_minutes INTEGER NOT NULL DEFAULT 10 CHECK (estimated_minutes > 0),
    source_path TEXT,
    source_start INTEGER CHECK (source_start IS NULL OR source_start >= 0),
    source_end INTEGER CHECK (source_end IS NULL OR source_end >= source_start),
    UNIQUE (module_id, position)
);

CREATE TABLE learning_concepts (
    id TEXT PRIMARY KEY NOT NULL,
    course_id TEXT NOT NULL REFERENCES learning_courses(id) ON DELETE CASCADE,
    concept_key TEXT NOT NULL CHECK (trim(concept_key) <> ''),
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    UNIQUE (course_id, concept_key)
);

CREATE TABLE learning_concept_prerequisites (
    concept_id TEXT NOT NULL REFERENCES learning_concepts(id) ON DELETE CASCADE,
    prerequisite_concept_id TEXT NOT NULL REFERENCES learning_concepts(id) ON DELETE CASCADE,
    PRIMARY KEY (concept_id, prerequisite_concept_id),
    CHECK (concept_id <> prerequisite_concept_id)
);

CREATE TABLE learning_lesson_concepts (
    lesson_id TEXT NOT NULL REFERENCES learning_lessons(id) ON DELETE CASCADE,
    concept_id TEXT NOT NULL REFERENCES learning_concepts(id) ON DELETE CASCADE,
    PRIMARY KEY (lesson_id, concept_id)
);

CREATE TABLE learning_activities (
    id TEXT PRIMARY KEY NOT NULL,
    lesson_id TEXT NOT NULL REFERENCES learning_lessons(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('single_choice', 'true_false', 'reflection')),
    prompt TEXT NOT NULL CHECK (trim(prompt) <> ''),
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(config_json) AND json_type(config_json) = 'object'
    ),
    position INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (lesson_id, position)
);

CREATE TABLE learning_activity_concepts (
    activity_id TEXT NOT NULL REFERENCES learning_activities(id) ON DELETE CASCADE,
    concept_id TEXT NOT NULL REFERENCES learning_concepts(id) ON DELETE CASCADE,
    PRIMARY KEY (activity_id, concept_id)
);

CREATE TABLE learning_enrollments (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT NOT NULL REFERENCES learning_courses(id) ON DELETE CASCADE,
    enrolled_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (user_id, course_id)
);

CREATE TABLE learning_lesson_progress (
    enrollment_id TEXT NOT NULL REFERENCES learning_enrollments(id) ON DELETE CASCADE,
    lesson_id TEXT NOT NULL REFERENCES learning_lessons(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('not_started', 'in_progress', 'completed')),
    started_at INTEGER,
    completed_at INTEGER,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (enrollment_id, lesson_id),
    CHECK (
        (status = 'not_started' AND started_at IS NULL AND completed_at IS NULL)
        OR (status = 'in_progress' AND started_at IS NOT NULL AND completed_at IS NULL)
        OR (status = 'completed' AND started_at IS NOT NULL AND completed_at IS NOT NULL)
    )
);

CREATE TABLE learning_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    enrollment_id TEXT NOT NULL REFERENCES learning_enrollments(id) ON DELETE CASCADE,
    activity_id TEXT NOT NULL REFERENCES learning_activities(id) ON DELETE CASCADE,
    response_json TEXT NOT NULL CHECK (json_valid(response_json)),
    score REAL NOT NULL CHECK (score >= 0.0 AND score <= 1.0),
    passed INTEGER NOT NULL CHECK (passed IN (0, 1)),
    feedback TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
);

CREATE TABLE learning_mastery_states (
    enrollment_id TEXT NOT NULL REFERENCES learning_enrollments(id) ON DELETE CASCADE,
    concept_id TEXT NOT NULL REFERENCES learning_concepts(id) ON DELETE CASCADE,
    mastery REAL NOT NULL DEFAULT 0.0 CHECK (mastery >= 0.0 AND mastery <= 1.0),
    evidence_count INTEGER NOT NULL DEFAULT 0 CHECK (evidence_count >= 0),
    last_practiced_at INTEGER,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (enrollment_id, concept_id)
);

CREATE TABLE learning_review_items (
    id TEXT PRIMARY KEY NOT NULL,
    enrollment_id TEXT NOT NULL REFERENCES learning_enrollments(id) ON DELETE CASCADE,
    concept_id TEXT NOT NULL REFERENCES learning_concepts(id) ON DELETE CASCADE,
    due_at INTEGER NOT NULL,
    stability_days REAL NOT NULL DEFAULT 0.0 CHECK (stability_days >= 0.0),
    difficulty REAL NOT NULL DEFAULT 5.0 CHECK (difficulty >= 1.0 AND difficulty <= 10.0),
    review_count INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
    lapse_count INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
    last_reviewed_at INTEGER,
    updated_at INTEGER NOT NULL,
    UNIQUE (enrollment_id, concept_id)
);

CREATE INDEX idx_learning_modules_course ON learning_modules(course_id, position);
CREATE INDEX idx_learning_lessons_module ON learning_lessons(module_id, position);
CREATE INDEX idx_learning_attempts_enrollment ON learning_attempts(enrollment_id, created_at DESC);
CREATE INDEX idx_learning_review_due ON learning_review_items(enrollment_id, due_at);
