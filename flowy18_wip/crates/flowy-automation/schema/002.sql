-- Append-only migration 2 for the standalone GPUI automation dataset.
-- request_high_water_ms never decreases: deleting receipts or moving the wall
-- clock backwards cannot reopen an expired request. change_epoch plus
-- change_sequence is the per-principal dataset generation for subscribers.
CREATE TABLE command_clocks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    principal_authority TEXT NOT NULL CHECK (length(trim(principal_authority)) > 0),
    principal_user_id INTEGER NOT NULL,
    request_high_water_ms INTEGER NOT NULL,
    change_epoch TEXT NOT NULL CHECK (length(change_epoch) = 36 AND change_epoch = lower(change_epoch) AND change_epoch NOT GLOB '*[^0-9a-f-]*' AND substr(change_epoch,15,1) = '7'),
    change_sequence INTEGER NOT NULL CHECK (change_sequence >= 0),
    updated_at_ms INTEGER NOT NULL,
    UNIQUE (principal_authority, principal_user_id)
);
