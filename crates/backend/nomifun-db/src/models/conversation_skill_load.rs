use nomifun_common::TimestampMs;
use serde::{Deserialize, Serialize};

/// Immutable `SKILL.md` body captured when a conversation explicitly loads a
/// catalog Skill. This is the read DTO for a ledger entry, not mutable session
/// configuration.
///
/// `message_id` is the durable product identity. SQLite's autoincrement `id`
/// remains an implementation detail for deterministic ordering and is never
/// exposed through this serializable contract.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConversationSkillLoad {
    pub conversation_id: String,
    /// The `skill_load` message that projects this ledger entry into history.
    pub message_id: String,
    /// Source-qualified catalog identity, for example `user:pdf`.
    ///
    /// This is intentionally named `catalog_key` in SQLite: it is an opaque
    /// catalog identity rather than a relational business-ID reference.
    pub catalog_key: String,
    pub skill_name: String,
    pub source: String,
    pub version_hash: String,
    pub content: String,
    pub created_at: TimestampMs,
}

/// Values accepted when writing one immutable explicit Skill snapshot.
///
/// The caller supplies the durable message identity while SQLite allocates its
/// own local ordering key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewConversationSkillLoad {
    pub conversation_id: String,
    /// The `skill_load` message that projects this ledger entry into history.
    pub message_id: String,
    /// Source-qualified catalog identity, for example `user:pdf`.
    ///
    /// This is intentionally named `catalog_key` in SQLite: it is an opaque
    /// catalog identity rather than a relational business-ID reference.
    pub catalog_key: String,
    pub skill_name: String,
    pub source: String,
    pub version_hash: String,
    pub content: String,
    pub created_at: TimestampMs,
}
