-- Agent Store Marketplace phase B: remote-source fetch state.
--
-- GitHub/Git/URL marketplaces are materialized into a local staging area and
-- atomically promoted to the live root (backup + rename), so a failed refresh
-- never destroys the last-good snapshot. This migration records the resolved
-- source revision (git commit hash / HTTP freshness marker) and the staging
-- directory that holds the live checkout; both are internal traceability only
-- and never cross into the public protocol.
--
-- Rules enforced by the fetch layer (docs/agent-store/02 §8):
--   - a remote marketplace is fetched into staging first, fully validated
--     (manifest parse + complete entry tree) before promotion;
--   - promotion is atomic: old live root is renamed to a backup, new staging
--     renamed into place; on any failure the previous live root stays;
--   - a refresh with an unchanged revision is a no-op (freshness short-circuit);
--   - failures never rewrite the registry entries/last-good.

-- Directory-source marketplaces never materialize; these stay NULL for them.
ALTER TABLE plugin_marketplaces ADD COLUMN resolved_revision TEXT;
ALTER TABLE plugin_marketplaces ADD COLUMN staging_root TEXT;

-- Source revision on the snapshot itself: which remote revision produced this
-- immutable snapshot (internal traceability, mirrors resolved_revision).
ALTER TABLE plugin_snapshots ADD COLUMN source_revision TEXT;
