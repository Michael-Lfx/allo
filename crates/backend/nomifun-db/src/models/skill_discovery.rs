use nomifun_common::TimestampMs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SkillSelectionEvent {
    pub conversation_id: String,
    pub turn: i64,
    pub query_hash: String,
    pub candidate_ids: String,
    pub model_loaded_ids: String,
    pub method: String,
    pub listing_truncated: i64,
    pub applied_to_l1: i64,
    pub latency_ms: i64,
    pub created_at: TimestampMs,
}

#[derive(Debug, Clone)]
pub struct NewSkillSelectionEvent {
    pub conversation_id: String,
    pub turn: i64,
    pub query_hash: String,
    pub candidate_ids: String,
    pub model_loaded_ids: String,
    pub method: String,
    pub listing_truncated: bool,
    pub applied_to_l1: bool,
    pub latency_ms: i64,
    pub created_at: TimestampMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConversationSkillPin {
    pub conversation_id: String,
    pub skill_id: String,
    pub pinned_at: TimestampMs,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SkillInvocationOverride {
    pub skill_id: String,
    pub allow_model_auto: i64,
    pub updated_at: TimestampMs,
}
