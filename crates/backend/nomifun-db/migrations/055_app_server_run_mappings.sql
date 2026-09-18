-- Durable public App Server run identities mapped to internal AgentExecution IDs.
--
-- Mappings are owner-scoped at repository boundaries and intentionally use
-- logical references instead of physical foreign keys, consistent with the v3
-- schema contract.
CREATE TABLE app_server_run_mappings (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    public_run_id TEXT NOT NULL
                  CHECK (
                      length(public_run_id) = 36
                      AND lower(public_run_id) = public_run_id
                      AND public_run_id GLOB '????????-????-7???-[89ab]???-????????????'
                      AND replace(public_run_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                  ),
    execution_id  TEXT NOT NULL
                  CHECK (
                      length(execution_id) = 36
                      AND lower(execution_id) = execution_id
                      AND execution_id GLOB '????????-????-7???-[89ab]???-????????????'
                      AND replace(execution_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                  ),
    user_id       TEXT NOT NULL
                  CHECK (
                      length(user_id) = 36
                      AND lower(user_id) = user_id
                      AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
                      AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                  ),
    created_at    INTEGER NOT NULL
);

CREATE UNIQUE INDEX uq_app_server_run_mappings_public_run_id
    ON app_server_run_mappings(public_run_id);
CREATE UNIQUE INDEX uq_app_server_run_mappings_execution_id
    ON app_server_run_mappings(execution_id);
CREATE INDEX idx_app_server_run_mappings_user_id
    ON app_server_run_mappings(user_id);
