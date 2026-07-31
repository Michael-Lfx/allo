use nomifun_common::TimestampMs;
use serde::{Deserialize, Serialize};

/// Immutable `SKILL.md` body captured when a conversation explicitly loads a
/// catalog Skill. This is a ledger row, not mutable session configuration.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConversationSkillLoadRow {
    pub id: i64,
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
