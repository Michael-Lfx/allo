-- Goal persistence for the Nomi goal-driven session loop (/goal).
--
-- One row per session: `session_id` carries the conversation_id for Nomi
-- runtimes (a non-reference identity column, registered in
-- NON_REFERENCE_ID_COLUMNS). Snapshot columns mirror
-- `nomi_agent::goal::state::GoalState`; the JSON columns (`subgoals_json`,
-- `contract_json`) carry the phase-2/3 payloads verbatim so the engine stays
-- the single authority over their internal schema.
--
-- v3 contract: local AUTOINCREMENT row identity, no physical foreign keys,
-- no triggers; the session link is a plain indexed column.
CREATE TABLE goals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    objective TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    turns_used INTEGER NOT NULL DEFAULT 0,
    max_turns INTEGER NOT NULL DEFAULT 20,
    created_at INTEGER NOT NULL,
    last_turn_at INTEGER,
    last_verdict TEXT,
    last_reason TEXT,
    paused_reason TEXT,
    consecutive_parse_failures INTEGER NOT NULL DEFAULT 0,
    consecutive_transport_failures INTEGER NOT NULL DEFAULT 0,
    subgoals_json TEXT NOT NULL DEFAULT '[]',
    contract_json TEXT,
    waiting_on_pid INTEGER,
    waiting_on_session TEXT,
    waiting_until INTEGER,
    waiting_reason TEXT,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX idx_goals_session_id ON goals (session_id);
CREATE INDEX idx_goals_status ON goals (status);
