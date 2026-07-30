use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Physical row of the `goals` table — one persisted goal snapshot per
/// session (`session_id` carries the conversation_id for Nomi runtimes).
///
/// Column semantics mirror `nomi_agent::goal::state::GoalState`:
/// `max_turns` stores the engine's `max_auto_continuations` budget and
/// `subgoals_json` / `contract_json` hold the engine-owned JSON payloads
/// verbatim. `version` is a monotonically-incrementing audit counter (NOT an
/// optimistic-lock version): the current product is a single-writer model
/// (last-writer-wins). If multi-writer concurrency is introduced in the
/// future, extend the upsert with an `expected_version` conditional update.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GoalRow {
    pub id: i64,
    pub session_id: String,
    pub objective: String,
    pub status: String,
    pub turns_used: i64,
    pub max_turns: i64,
    pub created_at: i64,
    pub last_turn_at: Option<i64>,
    pub last_verdict: Option<String>,
    pub last_reason: Option<String>,
    pub paused_reason: Option<String>,
    pub consecutive_parse_failures: i64,
    pub consecutive_transport_failures: i64,
    pub subgoals_json: String,
    pub contract_json: Option<String>,
    pub waiting_on_pid: Option<i64>,
    pub waiting_on_session: Option<String>,
    pub waiting_until: Option<i64>,
    pub waiting_reason: Option<String>,
    pub updated_at: i64,
    pub version: i64,
}
