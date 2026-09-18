-- Durable completed-response receipts for App Server idempotency keys.
--
-- These rows intentionally have no physical foreign keys. Principal and client
-- identities belong to the App Server protocol, and a completed receipt must
-- survive deletion or rebuilding of the business object named in its response.
CREATE TABLE app_server_idempotency_receipts (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    principal_id        TEXT NOT NULL
                        CHECK (length(principal_id) BETWEEN 1 AND 256),
    client_id           TEXT NOT NULL
                        CHECK (length(client_id) BETWEEN 1 AND 256),
    method              TEXT NOT NULL
                        CHECK (length(method) BETWEEN 1 AND 512),
    idempotency_key     TEXT NOT NULL
                        CHECK (length(idempotency_key) BETWEEN 1 AND 1024),
    request_fingerprint TEXT NOT NULL
                        CHECK (
                            length(request_fingerprint) = 71
                            AND request_fingerprint GLOB 'sha256:[0-9a-f]*'
                            AND substr(request_fingerprint, 8) NOT GLOB '*[^0-9a-f]*'
                        ),
    response_json       TEXT NOT NULL
                        CHECK (
                            json_valid(response_json)
                            AND json_type(response_json) = 'object'
                        ),
    created_at          INTEGER NOT NULL,
    UNIQUE (principal_id, client_id, method, idempotency_key)
);

CREATE TRIGGER app_server_idempotency_receipts_immutable
BEFORE UPDATE ON app_server_idempotency_receipts
BEGIN
    SELECT RAISE(ABORT, 'App Server idempotency receipts are immutable');
END;

CREATE TRIGGER app_server_idempotency_receipts_no_delete
BEFORE DELETE ON app_server_idempotency_receipts
BEGIN
    SELECT RAISE(ABORT, 'App Server idempotency receipts are retained indefinitely');
END;
