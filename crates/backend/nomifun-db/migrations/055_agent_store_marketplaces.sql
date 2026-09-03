-- Agent Store Marketplace (roadmap Phase 2): marketplace registry + import provenance.
--
-- A marketplace is a catalog of discoverable plugins (marketplace.json /
-- connectors.json / cli connectors). Adding one registers it here; entries are
-- stored as a JSON projection for the discovery layer, while each entry's
-- actual import still flows through the normal PluginSnapshot pipeline and
-- records its provenance on the snapshot row (marketplace_id + entry_name).
--
-- Rules enforced by the marketplace layer (docs/agent-store/02 §8):
--   - a marketplace is added once per source; (source_kind, source_uri) is unique;
--   - removing a marketplace is a soft delete (removed_at set); restore keeps
--     the registry record and the snapshots' provenance;
--   - removing with confirmation cascades an uninstall of snapshots that were
--     installed from this marketplace (snapshot rows themselves are kept);
--   - auto_update is opt-in for third-party sources (CodeBuddy semantics);
--   - source_uri is internal traceability only, never exposed publicly.

CREATE TABLE plugin_marketplaces (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    marketplace_id TEXT NOT NULL UNIQUE
        CHECK (
            length(marketplace_id) BETWEEN 1 AND 64
            AND marketplace_id = lower(marketplace_id)
            AND marketplace_id GLOB '*[0-9a-z-]*'
        ),
    name TEXT NOT NULL DEFAULT '',
    description TEXT,
    source_kind TEXT NOT NULL
        CHECK (source_kind IN ('directory', 'github', 'git', 'url')),
    source_uri TEXT NOT NULL,
    owner_json TEXT,
    version TEXT,
    content_digest TEXT,
    entries_json TEXT NOT NULL DEFAULT '[]',
    auto_update INTEGER NOT NULL DEFAULT 0
        CHECK (auto_update IN (0, 1)),
    enabled INTEGER NOT NULL DEFAULT 1
        CHECK (enabled IN (0, 1)),
    last_checked_at INTEGER,
    added_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    removed_at INTEGER,
    UNIQUE(source_kind, source_uri)
);

CREATE INDEX idx_plugin_marketplaces_removed_at
    ON plugin_marketplaces(removed_at);

-- Import provenance: which marketplace entry produced a snapshot. Nullable so
-- plain manual imports keep working unchanged.
ALTER TABLE plugin_snapshots ADD COLUMN marketplace_id TEXT;
ALTER TABLE plugin_snapshots ADD COLUMN entry_name TEXT;

CREATE INDEX idx_plugin_snapshots_marketplace_id
    ON plugin_snapshots(marketplace_id);
