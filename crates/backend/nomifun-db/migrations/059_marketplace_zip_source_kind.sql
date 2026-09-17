-- Doc 30: the `zip` marketplace source kind.
--
-- The official markets (experts / skills / connectors) now ship as one archive
-- per market instead of a file-per-entry tree the client mirrored over HTTP, so
-- `market/add` has to accept `source_kind = 'zip'`. 055 created this column with
-- `CHECK (source_kind IN ('directory', 'github', 'git', 'url'))`, and SQLite
-- cannot widen a CHECK in place — hence the rebuild.
--
-- The copy must carry **every** column added after 055, or the rebuild silently
-- drops them: `resolved_revision` / `staging_root` (056) and `source_etag` /
-- `source_last_modified` (057). Column order below matches the post-057 layout.
--
-- `plugin_snapshots` is deliberately untouched: snapshot provenance points at
-- the TEXT `marketplace_id`, never at this table's numeric `id`.

CREATE TABLE plugin_marketplaces_new (
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
        CHECK (source_kind IN ('directory', 'github', 'git', 'url', 'zip')),
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
    resolved_revision TEXT,
    staging_root TEXT,
    source_etag TEXT,
    source_last_modified TEXT,
    UNIQUE(source_kind, source_uri)
);

INSERT INTO plugin_marketplaces_new (
    id, marketplace_id, name, description, source_kind, source_uri,
    owner_json, version, content_digest, entries_json, auto_update, enabled,
    last_checked_at, added_at, updated_at, removed_at, resolved_revision,
    staging_root, source_etag, source_last_modified
)
SELECT
    id, marketplace_id, name, description, source_kind, source_uri,
    owner_json, version, content_digest, entries_json, auto_update, enabled,
    last_checked_at, added_at, updated_at, removed_at, resolved_revision,
    staging_root, source_etag, source_last_modified
FROM plugin_marketplaces;

DROP TABLE plugin_marketplaces;
ALTER TABLE plugin_marketplaces_new RENAME TO plugin_marketplaces;

CREATE INDEX idx_plugin_marketplaces_removed_at
    ON plugin_marketplaces(removed_at);
