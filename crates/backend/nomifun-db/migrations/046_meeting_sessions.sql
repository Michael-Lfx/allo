-- Meeting dual-track sessions, transcript segments, speakers, and local voiceprints.
-- v3 contract: local AUTOINCREMENT row identity, no physical foreign keys, no triggers.
-- Business IDs use the standard UUIDv7 GLOB/length/lowercase CHECK.

CREATE TABLE meeting_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL UNIQUE
        CHECK (
            length(session_id) = 36
            AND lower(session_id) = session_id
            AND session_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(session_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    user_id TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'created',
    bound_conversation_id TEXT,
    data_dir TEXT NOT NULL,
    mic_available INTEGER NOT NULL DEFAULT 0,
    loopback_available INTEGER NOT NULL DEFAULT 0,
    stt_backend TEXT NOT NULL DEFAULT 'auto',
    started_at INTEGER,
    ended_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    ),
    CHECK (
        bound_conversation_id IS NULL
        OR (
            length(bound_conversation_id) = 36
            AND lower(bound_conversation_id) = bound_conversation_id
            AND bound_conversation_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(bound_conversation_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        )
    )
);
CREATE INDEX idx_meeting_sessions_owner ON meeting_sessions (user_id);
CREATE INDEX idx_meeting_sessions_status ON meeting_sessions (status);
CREATE INDEX idx_meeting_sessions_bound_conversation ON meeting_sessions (bound_conversation_id);

CREATE TABLE meeting_segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    segment_id TEXT NOT NULL UNIQUE
        CHECK (
            length(segment_id) = 36
            AND lower(segment_id) = segment_id
            AND segment_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(segment_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    channel TEXT,
    speaker_id TEXT,
    speaker_label TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL DEFAULT '',
    is_partial INTEGER NOT NULL DEFAULT 0,
    is_manual_edit INTEGER NOT NULL DEFAULT 0,
    start_ms INTEGER NOT NULL DEFAULT 0,
    end_ms INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_meeting_segments_segment_id ON meeting_segments (segment_id);
CREATE INDEX idx_meeting_segments_session ON meeting_segments (session_id, start_ms);

CREATE TABLE meeting_speakers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    speaker_id TEXT NOT NULL
        CHECK (
            length(speaker_id) = 36
            AND lower(speaker_id) = speaker_id
            AND speaker_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(speaker_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    display_name TEXT NOT NULL,
    voiceprint_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_meeting_speakers_session_speaker
    ON meeting_speakers (session_id, speaker_id);
CREATE UNIQUE INDEX idx_meeting_speakers_speaker_id ON meeting_speakers (speaker_id);

CREATE TABLE meeting_voiceprints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    voiceprint_id TEXT NOT NULL UNIQUE
        CHECK (
            length(voiceprint_id) = 36
            AND lower(voiceprint_id) = voiceprint_id
            AND voiceprint_id GLOB '????????-????-7???-[89ab]???-????????????'
            AND replace(voiceprint_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    user_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    embedding_blob BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        length(user_id) = 36
        AND lower(user_id) = user_id
        AND user_id GLOB '????????-????-7???-[89ab]???-????????????'
        AND replace(user_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    )
);

CREATE UNIQUE INDEX idx_meeting_voiceprints_id ON meeting_voiceprints (voiceprint_id);
CREATE INDEX idx_meeting_voiceprints_owner ON meeting_voiceprints (user_id);
