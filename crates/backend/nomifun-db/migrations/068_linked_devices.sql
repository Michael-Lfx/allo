-- Paired Flowy Link phones. The phone is an installation-owner peer.
-- Refresh tokens are stored as SHA-256 hex only. Access tokens are JWTs
-- with aud=nomifun-link and are not stored.
--
-- user_id is a logical reference to users.user_id (CanonicalUuidV7).

CREATE TABLE linked_devices (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    linked_device_id        TEXT NOT NULL UNIQUE
                            CHECK (
                                length(linked_device_id) = 36
                                AND lower(linked_device_id) = linked_device_id
                                AND linked_device_id GLOB '????????-????-7???-[89ab]???-????????????'
                                AND replace(linked_device_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                            ),
    user_id                 TEXT NOT NULL,
    display_name            TEXT NOT NULL,
    platform                TEXT NOT NULL
                            CHECK (platform IN ('ios', 'android', 'harmony')),
    public_key_fingerprint TEXT NOT NULL UNIQUE,
    refresh_token_hash     TEXT NOT NULL,
    wake_enabled            INTEGER NOT NULL DEFAULT 1
                            CHECK (wake_enabled IN (0, 1)),
    app_version             TEXT NOT NULL DEFAULT '',
    created_at_ms           INTEGER NOT NULL,
    last_seen_at_ms         INTEGER NOT NULL,
    revoked_at_ms           INTEGER,
    CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    )
);

CREATE INDEX idx_linked_devices_user_id ON linked_devices(user_id);
