-- Host-side skill discovery telemetry, session pins, and per-skill
-- model-auto overrides. The webpage is a view of these tables.

CREATE TABLE skill_selection_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL
        CHECK (
            length(conversation_id) = 36
            AND lower(conversation_id) = conversation_id
            AND conversation_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(conversation_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    turn INTEGER NOT NULL DEFAULT 0,
    query_hash TEXT NOT NULL
        CHECK (
            length(query_hash) = 64
            AND lower(query_hash) = query_hash
            AND query_hash NOT GLOB '*[^0-9a-f]*'
        ),
    candidate_ids TEXT NOT NULL CHECK (length(trim(candidate_ids)) > 0),
    model_loaded_ids TEXT NOT NULL DEFAULT '[]',
    method TEXT NOT NULL CHECK (length(trim(method)) > 0),
    listing_truncated INTEGER NOT NULL DEFAULT 0 CHECK (listing_truncated IN (0, 1)),
    applied_to_l1 INTEGER NOT NULL DEFAULT 0 CHECK (applied_to_l1 IN (0, 1)),
    latency_ms INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_skill_selection_event_conversation
    ON skill_selection_event(conversation_id, created_at, id);

CREATE TABLE conversation_skill_pins (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL
        CHECK (
            length(conversation_id) = 36
            AND lower(conversation_id) = conversation_id
            AND conversation_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(conversation_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    skill_id TEXT NOT NULL CHECK (length(trim(skill_id)) > 0),
    pinned_at INTEGER NOT NULL,
    UNIQUE (conversation_id, skill_id)
);

CREATE INDEX idx_conversation_skill_pins_conversation
    ON conversation_skill_pins(conversation_id);

CREATE TABLE skill_invocation_overrides (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    skill_id TEXT NOT NULL UNIQUE CHECK (length(trim(skill_id)) > 0),
    allow_model_auto INTEGER NOT NULL CHECK (allow_model_auto IN (0, 1)),
    updated_at INTEGER NOT NULL
);
