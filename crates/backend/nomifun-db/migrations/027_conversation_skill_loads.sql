-- Immutable snapshots for explicit catalog Skill loads. The row remains after
-- a transcript projection is cleared, so historical instructions cannot be
-- reconstructed from a later on-disk SKILL.md revision.
CREATE TABLE conversation_skill_loads (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL,
    message_id   TEXT NOT NULL UNIQUE,
    catalog_key  TEXT NOT NULL CHECK (length(trim(catalog_key)) > 0),
    skill_name   TEXT NOT NULL CHECK (length(trim(skill_name)) > 0),
    source       TEXT NOT NULL CHECK (length(trim(source)) > 0),
    version_hash TEXT NOT NULL
                 CHECK (
                     length(version_hash) = 64
                     AND lower(version_hash) = version_hash
                     AND version_hash NOT GLOB '*[^0-9a-f]*'
                 ),
    content      TEXT NOT NULL CHECK (length(content) > 0),
    created_at   INTEGER NOT NULL,
    CHECK (
        length(conversation_id) = 36
        AND lower(conversation_id) = conversation_id
        AND conversation_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(conversation_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    CHECK (
        length(message_id) = 36
        AND lower(message_id) = message_id
        AND message_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(message_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    )
);

CREATE INDEX idx_conversation_skill_loads_conversation_id
    ON conversation_skill_loads(conversation_id);
CREATE INDEX idx_conversation_skill_loads_message_id
    ON conversation_skill_loads(message_id);
CREATE INDEX idx_conversation_skill_loads_conv_created
    ON conversation_skill_loads(conversation_id, created_at, id);
