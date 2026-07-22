CREATE TABLE IF NOT EXISTS preset_mcp_servers (
  preset_id TEXT NOT NULL,
  mcp_server_id TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (preset_id, mcp_server_id),
  FOREIGN KEY (preset_id) REFERENCES presets(id) ON DELETE CASCADE
);
