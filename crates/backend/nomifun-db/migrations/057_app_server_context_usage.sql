-- Durable per-conversation context occupancy snapshot for App Server chats.
--
-- App Server projects `used_tokens` (last provider prompt occupancy) and the
-- effective `window_tokens` on the public ConversationView. The snapshot is a
-- server-measured observability record, never a client-supplied presentation
-- field: clients can only read it, and the public `conversation/update`
-- surface cannot write it. One row per conversation; the App Server delete
-- path removes it with the conversation.
CREATE TABLE app_server_context_usage (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL
                    CHECK (
                        length(conversation_id) = 36
                        AND lower(conversation_id) = conversation_id
                        AND conversation_id GLOB '????????-????-7???-[89ab]???-????????????'
                        AND replace(conversation_id, '-', '') NOT GLOB '*[^0-9a-f]*'
                    ),
    context_tokens  INTEGER NOT NULL CHECK (context_tokens >= 0),
    window_tokens   INTEGER NOT NULL CHECK (window_tokens >= 0),
    updated_at      INTEGER NOT NULL
);

CREATE UNIQUE INDEX uq_app_server_context_usage_conversation_id
    ON app_server_context_usage(conversation_id);
CREATE INDEX idx_app_server_context_usage_conversation_id
    ON app_server_context_usage(conversation_id);