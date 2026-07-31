-- Preset include bindings used to persist a display-name-only `skill_name`.
-- Keep the old raw values readable through the service's `legacy:` adapter;
-- replacing one in the catalog and saving rewrites that binding as a
-- source-qualified identity.
-- This narrowly approved current-v3 transition is documented in
-- docs/adr/0001-source-qualified-skill-bindings.md.
-- Auto-injected exclusion rows share this table and intentionally keep their
-- system-owned Skill name in the renamed column.
ALTER TABLE preset_skill_bindings RENAME COLUMN skill_name TO skill_id;
