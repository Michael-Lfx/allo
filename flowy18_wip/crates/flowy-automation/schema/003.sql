-- Durable schedule, eligibility, approval and delivery metadata.
ALTER TABLE schedule_watermarks ADD COLUMN next_run_at_ms INTEGER;
CREATE INDEX schedule_watermarks_due ON schedule_watermarks (principal_authority, principal_user_id, next_run_at_ms);
ALTER TABLE runs ADD COLUMN scheduled_at_ms INTEGER;
ALTER TABLE runs ADD COLUMN covers_from_ms INTEGER;
ALTER TABLE runs ADD COLUMN blocked_reason TEXT CHECK (blocked_reason IN ('account_unavailable','credential_unavailable','model_unavailable','approval_required','workspace_unavailable','storage_unavailable'));
ALTER TABLE runs ADD COLUMN next_recheck_at_ms INTEGER;
ALTER TABLE command_receipts ADD COLUMN approval_session_id TEXT CHECK (approval_session_id IS NULL OR length(trim(approval_session_id)) > 0);
CREATE INDEX command_receipts_approval_session ON command_receipts (principal_authority, principal_user_id, approval_session_id);
ALTER TABLE delivery_outbox ADD COLUMN last_error TEXT;
ALTER TABLE delivery_outbox ADD COLUMN claim_until_ms INTEGER;
ALTER TABLE runs ADD COLUMN commit_retry_at_ms INTEGER;
