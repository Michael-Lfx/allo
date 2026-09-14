-- Review log: the honest record of scheduling. One append-only row lands
-- every time a review card is really advanced (FSRS state moved by an
-- answer or a self-rating); seeding a card writes a `synthetic` marker row
-- so "this card exists since" is auditable without polluting statistics.
-- True Retention, prediction calibration and a future FSRS parameter
-- optimizer all read from this one table; answer bookkeeping stays in
-- learning_attempts, daily check-in counting stays in learning_review_events.
--
-- The push gate ("advance only when due, stale repeats record the attempt
-- only") reads the (source, item_id, review_day) index to detect whether a
-- card was already pushed on the current review day.
CREATE TABLE learning_review_log (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    log_id    TEXT NOT NULL UNIQUE CHECK (
        length(log_id) = 36
        AND lower(log_id) = log_id
        AND log_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(log_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    user_id   TEXT NOT NULL CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    -- 'course' = review_item_id, 'custom' = custom_question_id
    source    TEXT NOT NULL CHECK (source IN ('course', 'custom')),
    item_id   TEXT NOT NULL CHECK (trim(item_id) <> ''),
    -- FSRS rating 1-4 (again/hard/good/easy); 0 marks the synthetic seeding
    -- row, which carries no real rating.
    rating    INTEGER NOT NULL CHECK (rating >= 0 AND rating <= 4),
    -- 'auto' = answer-driven rating (wrong answer or admitted forgot),
    -- 'self' = learner self-rating after a correct answer,
    -- 'synthetic' = seeding, never a real answer
    rating_source TEXT NOT NULL CHECK (rating_source IN ('auto', 'self', 'synthetic')),
    -- Review-day granular days since the previous real advance; 0 when the
    -- card had never been pushed before (or for synthetic rows).
    elapsed_days INTEGER NOT NULL CHECK (elapsed_days >= 0),
    -- Memory state before the advance. NULL when the card had none (first
    -- push or synthetic row): True Retention only counts pushes of cards
    -- that actually carried a memory state (due reviews of old cards).
    stability_before REAL,
    difficulty_before REAL,
    -- FSRS-predicted retrievability at the push moment (0..1); NULL when no
    -- memory state existed yet. Feeds the prediction-vs-actual calibration.
    r_pred REAL,
    -- Local review day (YYYYMMDD, 02:00 rollover) the push belongs to.
    review_day INTEGER NOT NULL CHECK (review_day >= 19000101),
    created_at INTEGER NOT NULL,
    -- Table-level range checks (they may reference several columns, so they
    -- come after every column definition):
    -- the snapshot is present exactly when the card had a memory state, and
    -- the prediction is only meaningful alongside one.
    CHECK ((stability_before IS NULL) = (difficulty_before IS NULL)),
    CHECK (stability_before IS NULL OR stability_before >= 0.0),
    CHECK (difficulty_before IS NULL OR (difficulty_before >= 1.0 AND difficulty_before <= 10.0)),
    CHECK (r_pred IS NULL OR (r_pred > 0.0 AND r_pred <= 1.0))
);

-- Gate lookup: has this card already been pushed on the current review day?
CREATE INDEX idx_learning_review_log_item_day
    ON learning_review_log(source, item_id, review_day);
-- Stats scans: True Retention / calibration buckets per user and day.
CREATE INDEX idx_learning_review_log_user_day
    ON learning_review_log(user_id, review_day);

-- Answering latency, reported by the review session (wall clock from card
-- shown to answer submitted). NULL for attempts recorded before this column
-- existed; feeds future anti-guessing heuristics.
ALTER TABLE learning_attempts ADD COLUMN elapsed_ms INTEGER;
