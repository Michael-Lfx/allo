-- D4 ①: persist the HTTP source's **raw** conditional-request validators.
--
-- Until now only their digest was stored (in `resolved_revision`), and that
-- digest was sent as `If-None-Match`. A server can never match it against its
-- own ETag, so the 304 branch was unreachable in practice and every refresh
-- re-downloaded the manifest to detect "unchanged" by digest comparison.
--
-- Both columns are internal traceability, exactly like `resolved_revision`:
-- they never cross the public protocol (doc 18 §7). Nullable because non-HTTP
-- sources (git / directory) have no validators at all.
ALTER TABLE plugin_marketplaces ADD COLUMN source_etag TEXT;
ALTER TABLE plugin_marketplaces ADD COLUMN source_last_modified TEXT;
