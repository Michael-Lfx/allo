-- Add `files_shell` to Agent Execution step tool policy.
--
-- `full` remains the unrestricted inherited set (host pipelines included).
-- `files_shell` is the long-term default for implementation work that only
-- needs files + shell + managed search, so a news/media worker cannot lose
-- Write while a research worker cannot accidentally keep briefing_create.
-- SQLite cannot widen a CHECK in place, so the table is rebuilt.

CREATE TABLE agent_execution_steps_new (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    step_id                 TEXT NOT NULL UNIQUE
                            CHECK (
                                length(step_id) = 36
                                AND lower(step_id) = step_id
                                AND step_id GLOB '????????-????-7???-[89ab]???-????????????'
                                AND replace(step_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                            ),
    execution_id            TEXT NOT NULL,
    title                   TEXT NOT NULL CHECK (trim(title) <> ''),
    spec                    TEXT NOT NULL,
    role                    TEXT,
    tool_policy             TEXT NOT NULL DEFAULT 'full'
                            CHECK (tool_policy IN ('full', 'files_shell', 'read_only', 'read_shell')),
    kind                    TEXT NOT NULL CHECK (kind IN ('agent', 'verify', 'judge', 'loop')),
    agent_mode              TEXT,
    profile                 TEXT,
    fanout_group            TEXT,
    control_policy          TEXT,
    delegation_depth        INTEGER NOT NULL DEFAULT 0 CHECK (delegation_depth BETWEEN 0 AND 4),
    status                  TEXT NOT NULL CHECK (status IN (
                                'pending', 'running', 'waiting_input', 'completed',
                                'failed', 'skipped', 'cancelled'
                            )),
    assigned_participant_id TEXT
                            CHECK (
                                assigned_participant_id IS NULL
                                OR (
                                    length(assigned_participant_id) = 36
                                    AND lower(assigned_participant_id) = assigned_participant_id
                                    AND assigned_participant_id GLOB '????????-????-7???-[89ab]???-????????????'
                                    AND replace(assigned_participant_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                                )
                            ),
    assignment_score        REAL,
    assignment_rationale    TEXT,
    assignment_source       TEXT,
    assignment_locked       INTEGER NOT NULL DEFAULT 0 CHECK (assignment_locked IN (0, 1)),
    failure_policy          TEXT NOT NULL DEFAULT 'fail_execution'
                            CHECK (failure_policy IN ('fail_execution', 'skip_dependents')),
    preset_prompt           TEXT,
    graph_x                 REAL,
    graph_y                 REAL,
    dispatch_after          INTEGER,
    version                 INTEGER NOT NULL DEFAULT 0 CHECK (version >= 0),
    introduced_in_revision  INTEGER NOT NULL CHECK (introduced_in_revision >= 0),
    superseded_in_revision  INTEGER,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL,
    CHECK (length(execution_id) = 36 AND lower(execution_id) = execution_id AND execution_id GLOB '????????-????-7???-[89ab]???-????????????' AND replace(execution_id, '-', '') NOT GLOB '*[^0-9a-f]*')
);

INSERT INTO agent_execution_steps_new (
    id, step_id, execution_id, title, spec, role, tool_policy, kind, agent_mode,
    profile, fanout_group, control_policy, delegation_depth, status,
    assigned_participant_id, assignment_score, assignment_rationale,
    assignment_source, assignment_locked, failure_policy, preset_prompt,
    graph_x, graph_y, dispatch_after, version, introduced_in_revision,
    superseded_in_revision, created_at, updated_at
)
SELECT
    id, step_id, execution_id, title, spec, role, tool_policy, kind, agent_mode,
    profile, fanout_group, control_policy, delegation_depth, status,
    assigned_participant_id, assignment_score, assignment_rationale,
    assignment_source, assignment_locked, failure_policy, preset_prompt,
    graph_x, graph_y, dispatch_after, version, introduced_in_revision,
    superseded_in_revision, created_at, updated_at
FROM agent_execution_steps;

DROP TABLE agent_execution_steps;
ALTER TABLE agent_execution_steps_new RENAME TO agent_execution_steps;

CREATE INDEX idx_execution_steps_execution_id ON agent_execution_steps(execution_id);
CREATE INDEX idx_execution_steps_assigned_participant_id ON agent_execution_steps(assigned_participant_id);
