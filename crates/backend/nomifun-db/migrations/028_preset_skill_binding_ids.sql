-- Preset include bindings used to persist a display-name-only `skill_name`.
-- Keep the old raw values readable through the service's `legacy:` adapter;
-- the next preset save rewrites them as source-qualified catalog identities.
-- Auto-injected exclusion rows share this table and intentionally keep their
-- system-owned Skill name in the renamed column.
ALTER TABLE preset_skill_bindings RENAME COLUMN skill_name TO skill_id;
