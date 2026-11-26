ALTER TABLE runs ADD COLUMN keep_result INTEGER NOT NULL DEFAULT 0 CHECK (keep_result IN (0,1));
ALTER TABLE runs ADD COLUMN compacted_at_ms INTEGER;
ALTER TABLE runs ADD COLUMN inbox_summary INTEGER NOT NULL DEFAULT 0 CHECK (inbox_summary IN (0,1));
ALTER TABLE runs ADD COLUMN abandon_requested INTEGER NOT NULL DEFAULT 0 CHECK (abandon_requested IN (0,1));
CREATE INDEX runs_retention ON runs (principal_authority, principal_user_id, phase, updated_at_ms);
ALTER TABLE command_clocks ADD COLUMN storage_suspended INTEGER NOT NULL DEFAULT 0 CHECK (storage_suspended IN (0,1));
