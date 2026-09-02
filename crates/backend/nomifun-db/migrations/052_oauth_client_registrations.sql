-- OAuth dynamic client registrations (RFC 7591) tied to a resolved OAuth
-- server identity (RFC 8414 metadata + RFC 9728 protected-resource metadata).
--
-- The identity key is
--   (mcp_server_url, resource_identifier, authorization_server_issuer, redirect_uri)
-- so the same MCP endpoint behind different resources/issuers/clients never
-- shares a registration. Sensitive values (client_secret,
-- registration_access_token) are only referenced via `*_ref` columns; the
-- values themselves live in the secure credential store.
CREATE TABLE oauth_client_registrations (
    id                           INTEGER PRIMARY KEY AUTOINCREMENT,
    mcp_server_url               TEXT NOT NULL
                                 CHECK (length(mcp_server_url) BETWEEN 1 AND 2048),
    resource_identifier          TEXT NOT NULL DEFAULT ''
                                 CHECK (length(resource_identifier) <= 2048),
    authorization_server_issuer  TEXT NOT NULL DEFAULT ''
                                 CHECK (length(authorization_server_issuer) <= 2048),
    redirect_uri                 TEXT NOT NULL
                                 CHECK (length(redirect_uri) BETWEEN 1 AND 2048),
    registration_mode            TEXT NOT NULL
                                 CHECK (registration_mode IN ('dynamic', 'pre_registered')),
    client_id                    TEXT NOT NULL
                                 CHECK (length(client_id) BETWEEN 1 AND 2048),
    client_secret_ref            TEXT,
    registration_access_token_ref TEXT,
    client_id_issued_at          INTEGER,
    client_secret_expires_at     INTEGER,
    registration_client_uri      TEXT,
    created_at                   INTEGER NOT NULL,
    updated_at                   INTEGER NOT NULL,
    UNIQUE (mcp_server_url, resource_identifier, authorization_server_issuer, redirect_uri)
);

CREATE INDEX idx_oauth_client_registrations_server
    ON oauth_client_registrations (mcp_server_url);

-- Link stored tokens to the registration identity that minted them so
-- refresh always uses the original client identity (never a default or a
-- re-registered one). Legacy rows keep NULL and are treated as
-- `requires_reauthorization` when no registration can be resolved.
-- `registration_id` is a logical link (v3 schema forbids physical foreign keys).
ALTER TABLE oauth_tokens ADD COLUMN registration_id INTEGER;
ALTER TABLE oauth_tokens ADD COLUMN principal_id TEXT;

CREATE INDEX idx_oauth_tokens_registration
    ON oauth_tokens (registration_id);

CREATE INDEX idx_oauth_tokens_principal
    ON oauth_tokens (principal_id);