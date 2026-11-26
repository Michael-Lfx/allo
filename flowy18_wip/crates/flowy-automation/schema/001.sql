-- Immutable initial schema for the standalone GPUI automation dataset.
CREATE TABLE automation_schema (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    lineage TEXT NOT NULL,
    version INTEGER NOT NULL,
    schema_sha256 TEXT NOT NULL
);

CREATE TABLE automations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id TEXT NOT NULL UNIQUE CHECK (length(automation_id) = 36 AND automation_id = lower(automation_id) AND automation_id NOT GLOB '*[^0-9a-f-]*' AND substr(automation_id,9,1) = '-' AND substr(automation_id,14,1) = '-' AND substr(automation_id,19,1) = '-' AND substr(automation_id,24,1) = '-' AND substr(automation_id,15,1) = '7' AND substr(automation_id,20,1) IN ('8','9','a','b')),
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    current_revision INTEGER NOT NULL CHECK (current_revision > 0),
    state TEXT NOT NULL CHECK (state IN ('draft','enabled','paused','archived','suspended_storage')),
    title_json TEXT NOT NULL CHECK (json_valid(title_json) AND json_type(title_json) = 'text'),
    source_removed INTEGER NOT NULL DEFAULT 0 CHECK (source_removed IN (0,1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX automations_owner_revision ON automations (principal_authority, principal_user_id, automation_id, current_revision);

CREATE TABLE automation_revisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id TEXT NOT NULL,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    definition_json TEXT NOT NULL CHECK (json_valid(definition_json) AND json_type(definition_json) = 'object'),
    context_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(context_json) AND json_type(context_json) = 'object'),
    grant_ref TEXT,
    created_at_ms INTEGER NOT NULL,
    UNIQUE (automation_id, revision)
);
CREATE UNIQUE INDEX automation_revisions_owner_revision ON automation_revisions (principal_authority, principal_user_id, automation_id, revision);
CREATE INDEX automation_revisions_owner_grant ON automation_revisions (principal_authority, principal_user_id, grant_ref);

CREATE TABLE runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL UNIQUE CHECK (length(run_id) = 36 AND run_id = lower(run_id) AND run_id NOT GLOB '*[^0-9a-f-]*' AND substr(run_id,9,1) = '-' AND substr(run_id,14,1) = '-' AND substr(run_id,19,1) = '-' AND substr(run_id,24,1) = '-' AND substr(run_id,15,1) = '7' AND substr(run_id,20,1) IN ('8','9','a','b')),
    automation_id TEXT NOT NULL,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    current_attempt_id TEXT,
    retry_of_run_id TEXT,
    occurrence_key TEXT UNIQUE CHECK (occurrence_key IS NULL OR length(trim(occurrence_key)) > 0),
    phase TEXT NOT NULL CHECK (phase IN ('queued','blocked','waiting_workspace','running','cancelling','finalizing','interrupted','settled')),
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('succeeded','failed','cancelled','timed_out','budget_exceeded','skipped','expired','interrupted')),
    latest_start_at_ms INTEGER NOT NULL,
    source_removed INTEGER NOT NULL DEFAULT 0 CHECK (source_removed IN (0,1)),
    spec_json TEXT CHECK (spec_json IS NULL OR (json_valid(spec_json) AND json_type(spec_json) = 'object')),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX runs_owner_revision ON runs (principal_authority, principal_user_id, automation_id, revision);
CREATE INDEX runs_owner_retry ON runs (principal_authority, principal_user_id, retry_of_run_id);
CREATE INDEX runs_current_attempt ON runs (principal_authority, principal_user_id, current_attempt_id);
CREATE INDEX runs_queue ON runs (phase, latest_start_at_ms, created_at_ms);

CREATE TABLE run_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id TEXT NOT NULL UNIQUE CHECK (length(attempt_id) = 36 AND attempt_id = lower(attempt_id) AND attempt_id NOT GLOB '*[^0-9a-f-]*' AND substr(attempt_id,9,1) = '-' AND substr(attempt_id,14,1) = '-' AND substr(attempt_id,19,1) = '-' AND substr(attempt_id,24,1) = '-' AND substr(attempt_id,15,1) = '7' AND substr(attempt_id,20,1) IN ('8','9','a','b')),
    run_id TEXT NOT NULL,
    automation_id TEXT NOT NULL,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    session_id TEXT NOT NULL UNIQUE CHECK (length(trim(session_id)) > 0),
    submission_id TEXT NOT NULL CHECK (length(trim(submission_id)) > 0),
    spec_json TEXT NOT NULL CHECK (json_valid(spec_json) AND json_type(spec_json) = 'object'),
    started_at_ms INTEGER NOT NULL,
    completion_json TEXT CHECK (completion_json IS NULL OR (json_valid(completion_json) AND json_type(completion_json) = 'object'))
);
CREATE INDEX run_attempts_owner_run ON run_attempts (principal_authority, principal_user_id, run_id);
CREATE UNIQUE INDEX run_attempts_owner_session_submission ON run_attempts (principal_authority, principal_user_id, session_id, submission_id);

CREATE TABLE command_receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    request_key TEXT NOT NULL CHECK (length(trim(request_key)) > 0),
    issued_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > issued_at_ms),
    content_digest BLOB NOT NULL CHECK (length(content_digest) = 32),
    command_json TEXT NOT NULL CHECK (json_valid(command_json) AND json_type(command_json) = 'object'),
    state TEXT NOT NULL CHECK (state IN ('pending_approval','applied','rejected')),
    outcome_json TEXT CHECK (outcome_json IS NULL OR (json_valid(outcome_json) AND json_type(outcome_json) = 'object')),
    result_automation_id TEXT,
    result_run_id TEXT,
    source_removed INTEGER NOT NULL DEFAULT 0 CHECK (source_removed IN (0,1)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE (principal_authority, principal_user_id, request_key)
);
CREATE INDEX command_receipts_owner_automation ON command_receipts (principal_authority, principal_user_id, result_automation_id);
CREATE INDEX command_receipts_owner_run ON command_receipts (principal_authority, principal_user_id, result_run_id);

CREATE TABLE delivery_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    delivery_key TEXT NOT NULL UNIQUE CHECK (length(trim(delivery_key)) > 0),
    run_id TEXT NOT NULL,
    target TEXT NOT NULL CHECK (length(trim(target)) > 0),
    message_kind TEXT NOT NULL CHECK (length(trim(message_kind)) > 0),
    state TEXT NOT NULL CHECK (state IN ('pending','retry_wait','suspended_account','delivered','suppressed','abandoned')),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    next_attempt_at_ms INTEGER,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    UNIQUE (principal_authority, principal_user_id, run_id, target, message_kind)
);
CREATE INDEX delivery_outbox_owner_run ON delivery_outbox (principal_authority, principal_user_id, run_id);
CREATE INDEX delivery_outbox_pending ON delivery_outbox (state, next_attempt_at_ms);

CREATE TABLE deletion_plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    deletion_plan_id TEXT NOT NULL UNIQUE CHECK (length(deletion_plan_id) = 36 AND deletion_plan_id = lower(deletion_plan_id) AND deletion_plan_id NOT GLOB '*[^0-9a-f-]*' AND substr(deletion_plan_id,9,1) = '-' AND substr(deletion_plan_id,14,1) = '-' AND substr(deletion_plan_id,19,1) = '-' AND substr(deletion_plan_id,24,1) = '-' AND substr(deletion_plan_id,15,1) = '7' AND substr(deletion_plan_id,20,1) IN ('8','9','a','b')),
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    run_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planned','source_removed','cleaning','completed')),
    plan_json TEXT NOT NULL CHECK (json_valid(plan_json) AND json_type(plan_json) = 'object'),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX deletion_plans_owner_run ON deletion_plans (principal_authority, principal_user_id, run_id);

CREATE TABLE schedule_watermarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id TEXT NOT NULL UNIQUE,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    processed_through_ms INTEGER NOT NULL,
    source_removed INTEGER NOT NULL DEFAULT 0 CHECK (source_removed IN (0,1))
);
CREATE INDEX schedule_watermarks_owner_automation ON schedule_watermarks (principal_authority, principal_user_id, automation_id);
