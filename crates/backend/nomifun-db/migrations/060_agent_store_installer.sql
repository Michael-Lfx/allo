-- Agent Store Installer (roadmap Phase 2): per-component installation state.
--
-- An imported snapshot stays immutable; installation registers its components
-- into the runtime (skills on disk, connectors in mcp_servers, agents/teams as
-- Presets) and records that state on each component row:
--   installed       0 = not installed (import-only), 1 = installed (enabled)
--   disabled        1 = installed but disabled (runtime artifacts stay in place)
--   preset_id       agent/team components: the Preset created by the installer
--   runtime_ref     JSON: { "type": "skill|connector|preset",
--                           "location": "<on-disk path or mcp name>",
--                           "mcp_server_id": "..." }  (internal traceability)
--
-- Rules enforced by the installer layer (docs/agent-store/02 §install):
--   - installing never executes content; it copies/registers only;
--   - credential values NEVER enter these columns;
--   - uninstalling removes runtime artifacts but keeps the snapshot + rows.

ALTER TABLE plugin_snapshot_components ADD COLUMN installed INTEGER NOT NULL DEFAULT 0
    CHECK (installed IN (0, 1));
ALTER TABLE plugin_snapshot_components ADD COLUMN disabled INTEGER NOT NULL DEFAULT 0
    CHECK (disabled IN (0, 1));
ALTER TABLE plugin_snapshot_components ADD COLUMN installed_at INTEGER;
ALTER TABLE plugin_snapshot_components ADD COLUMN preset_id TEXT;
ALTER TABLE plugin_snapshot_components ADD COLUMN runtime_ref TEXT;

CREATE INDEX idx_plugin_snapshot_components_installed
    ON plugin_snapshot_components(installed, disabled);
