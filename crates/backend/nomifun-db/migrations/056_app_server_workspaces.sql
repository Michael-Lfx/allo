-- Durable owner-scoped workspace registry for the local App Server.
CREATE TABLE app_server_workspaces (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL UNIQUE
                 CHECK (
                     length(workspace_id) = 36
                     AND lower(workspace_id) = workspace_id
                     AND workspace_id GLOB '????????-????-7???-[89ab]???-????????????'
                     AND replace(workspace_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                 ),
    user_id      TEXT NOT NULL
                 CHECK (
                     length(user_id) = 36
                     AND lower(user_id) = user_id
                     AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
                     AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                 ),
    root_path    TEXT NOT NULL CHECK (length(root_path) > 0),
    status       TEXT NOT NULL DEFAULT 'active'
                 CHECK (status IN ('active', 'revoked')),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (user_id, workspace_id),
    UNIQUE (user_id, root_path)
);

CREATE INDEX idx_app_server_workspaces_user_id
    ON app_server_workspaces(user_id);
CREATE INDEX idx_app_server_workspaces_user_status
    ON app_server_workspaces(user_id, status);
