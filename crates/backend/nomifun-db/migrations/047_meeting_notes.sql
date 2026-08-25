-- Meeting notes persistence (N3): structured notes JSON + generation status.
-- v3 contract: ALTER only; no physical FKs, no triggers.

ALTER TABLE meeting_sessions ADD COLUMN notes_json TEXT;
ALTER TABLE meeting_sessions ADD COLUMN notes_status TEXT NOT NULL DEFAULT 'none'
    CHECK (notes_status IN ('none', 'generating', 'ready', 'failed'));
