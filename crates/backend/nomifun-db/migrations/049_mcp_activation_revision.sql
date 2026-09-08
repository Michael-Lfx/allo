-- Guard asynchronous MCP connection tests against stale configuration writes.
-- Existing rows start at revision zero. Any repository update that can change
-- the saved MCP definition advances the revision.
ALTER TABLE mcp_servers ADD COLUMN config_revision INTEGER NOT NULL DEFAULT 0;
