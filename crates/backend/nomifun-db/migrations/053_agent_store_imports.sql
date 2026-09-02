-- Agent Store Importer (roadmap Phase 1): immutable PluginSnapshot cache and
-- standardized definitions registered into the Catalog.
--
-- Rules enforced by the importer layer (docs/agent-store/02-...import-spec.md):
--   - a snapshot is immutable once inserted; the same (plugin_id,
--     declared_version, content_digest) re-import is idempotent;
--   - a digest conflict for the same identity+version is blocked, never
--     overwrites an existing snapshot;
--   - credential values NEVER enter these columns (only CredentialSchema
--     field declarations inside payload_json);
--   - source_uri is internal traceability only and must not cross into the
--     public App Server protocol.

CREATE TABLE plugin_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id TEXT NOT NULL UNIQUE
                 CHECK (
                     length(snapshot_id) = 36
                     AND lower(snapshot_id) = snapshot_id
                     AND snapshot_id GLOB '????????-????-7???-[89ab]???-????????????'
                     AND replace(snapshot_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                 ),
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    source_uri TEXT,
    plugin_id TEXT NOT NULL,
    declared_version TEXT NOT NULL,
    resolved_revision TEXT,
    content_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('completed', 'completed-with-warnings', 'blocked', 'failed')),
    imported_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(plugin_id, declared_version, content_digest)
);

CREATE INDEX idx_plugin_snapshots_imported_at ON plugin_snapshots(imported_at);

CREATE TABLE plugin_snapshot_components (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id TEXT NOT NULL
                 CHECK (
                     length(snapshot_id) = 36
                     AND lower(snapshot_id) = snapshot_id
                     AND snapshot_id GLOB '????????-????-7???-[89ab]???-????????????'
                     AND replace(snapshot_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                 ),
    component_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    relative_path TEXT,
    compatibility_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE(snapshot_id, component_id)
);

CREATE INDEX idx_plugin_snapshot_components_snapshot_id
    ON plugin_snapshot_components(snapshot_id);