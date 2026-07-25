CREATE TABLE preset_mcp_servers (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    preset_id     TEXT NOT NULL
                  CHECK (
                      length(preset_id) = 36
                      AND lower(preset_id) = preset_id
                      AND preset_id GLOB '????????-????-7???-[89ab]???-????????????'
                      AND replace(preset_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                  ),
    mcp_server_id TEXT NOT NULL
                  CHECK (
                      length(mcp_server_id) = 36
                      AND lower(mcp_server_id) = mcp_server_id
                      AND mcp_server_id GLOB '????????-????-7???-[89ab]???-????????????'
                      AND replace(mcp_server_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                  ),
    sort_order    INTEGER NOT NULL DEFAULT 0,
    UNIQUE (preset_id, mcp_server_id)
);

CREATE INDEX idx_preset_mcp_servers_preset_id
    ON preset_mcp_servers(preset_id);
CREATE INDEX idx_preset_mcp_servers_mcp_server_id
    ON preset_mcp_servers(mcp_server_id);
