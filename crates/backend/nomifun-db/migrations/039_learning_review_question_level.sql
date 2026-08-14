-- Review scheduling moves from concept-level to question-level.
--
-- A review item is the atomic unit of the FSRS schedule. Before this
-- migration one item represented one concept and shared its memory state
-- (due_at / stability / difficulty / review counts) across every objective
-- question of that concept, with the queue rotating questions by position.
-- Now one item represents exactly one objective activity (question) of an
-- enrollment, so every question carries its own independent memory curve.
--
-- Concept links are dropped from the item: concepts are reached through
-- learning_activity_concepts when mastery evidence or display titles are
-- needed. Existing concept-level items cannot be mapped to questions, so
-- they are re-seeded instead: every objective question of every completed
-- lesson gets a fresh item due immediately (memory curve reset).

CREATE TABLE learning_review_items_new (
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
    activity_id      TEXT NOT NULL CHECK (
        length(activity_id) = 36
        AND lower(activity_id) = activity_id
        AND activity_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(activity_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    due_at           INTEGER NOT NULL,
    stability_days   REAL NOT NULL DEFAULT 0.0 CHECK (stability_days >= 0.0),
    difficulty       REAL NOT NULL DEFAULT 5.0 CHECK (difficulty >= 1.0 AND difficulty <= 10.0),
    review_count     INTEGER NOT NULL DEFAULT 0 CHECK (review_count >= 0),
    lapse_count      INTEGER NOT NULL DEFAULT 0 CHECK (lapse_count >= 0),
    last_reviewed_at INTEGER,
    updated_at       INTEGER NOT NULL,
    UNIQUE (enrollment_id, activity_id)
);

-- Re-seed: every objective question of every completed lesson enters the
-- queue immediately. SQLite has no UUID function, so review ids are built
-- from randomblob(16) with the fixed version (`7`) and variant (`8`)
-- nibbles required by the CHECK constraint above.
WITH seeded AS (
    SELECT p.enrollment_id,
           a.activity_id,
           lower(hex(randomblob(16))) AS h
    FROM learning_lesson_progress p
    JOIN learning_lessons l ON l.lesson_id = p.lesson_id
    JOIN learning_activities a ON a.lesson_id = l.lesson_id
    WHERE p.status = 'completed'
      AND a.kind IN ('single_choice', 'true_false', 'fill_in_blank')
)
INSERT INTO learning_review_items_new
    (review_item_id, enrollment_id, activity_id, due_at, stability_days, difficulty,
     review_count, lapse_count, last_reviewed_at, updated_at)
SELECT
    substr(h, 1, 8) || '-' || substr(h, 9, 4) || '-7' || substr(h, 14, 3)
        || '-8' || substr(h, 17, 3) || '-' || substr(h, 20, 12),
    enrollment_id,
    activity_id,
    CAST(strftime('%s', 'now') AS INTEGER) * 1000,
    0,
    5.0,
    0,
    0,
    NULL,
    CAST(strftime('%s', 'now') AS INTEGER) * 1000
FROM seeded;

DROP TABLE learning_review_items;
ALTER TABLE learning_review_items_new RENAME TO learning_review_items;

CREATE INDEX idx_learning_review_items_enrollment_id
    ON learning_review_items(enrollment_id, due_at);
CREATE INDEX idx_learning_review_items_activity_id
    ON learning_review_items(activity_id);
