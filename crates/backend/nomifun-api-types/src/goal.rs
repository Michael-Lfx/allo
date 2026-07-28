//! Goal control DTOs for `/api/conversations/{conversation_id}/goal`.
//!
//! The frontend `/goal` slash command family maps onto one POST (actions)
//! plus one GET (status). Field names follow the engine's `GoalState`
//! snake_case contract so values pass through unmodified.

use serde::{Deserialize, Serialize};

/// POST body for a goal action on a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalActionRequest {
    /// One of `"set"` | `"pause"` | `"resume"` | `"clear"` | `"status"`.
    /// `"status"` is accepted for slash-command symmetry and behaves like
    /// the GET endpoint (no mutation).
    pub action: String,
    /// Objective text. Required for `action = "set"`, ignored otherwise.
    #[serde(default)]
    pub objective: Option<String>,
    /// Auto-continuation budget for `action = "set"`. Falls back to the
    /// server default when unset.
    #[serde(default)]
    pub max_turns: Option<u64>,
    /// Optional human-readable pause reason for `action = "pause"`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Goal snapshot returned by both the GET endpoint and every POST action.
///
/// `active` means "a goal snapshot exists for this conversation" (whatever
/// its status); `status` carries the engine state machine value:
/// `"active"` | `"complete"` | `"blocked"` | `"paused"` | `"waiting"` |
/// `"cleared"`. All snapshot fields are `None`/empty when `active == false`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalStatusResponse {
    pub active: bool,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub turns_used: Option<u64>,
    #[serde(default)]
    pub max_turns: Option<u64>,
    /// `"done"` | `"continue"` | `"wait"` | `"skipped"`, absent before the
    /// first judge verdict.
    #[serde(default)]
    pub last_verdict: Option<String>,
    #[serde(default)]
    pub last_reason: Option<String>,
    #[serde(default)]
    pub paused_reason: Option<String>,
    #[serde(default)]
    pub subgoals: Vec<String>,
    /// Epoch milliseconds.
    #[serde(default)]
    pub created_at: Option<u64>,
    /// Epoch milliseconds of the last goal-judged turn.
    #[serde(default)]
    pub last_turn_at: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_action_request_accepts_minimal_json() {
        let req: GoalActionRequest = serde_json::from_str(r#"{"action":"pause"}"#).unwrap();
        assert_eq!(req.action, "pause");
        assert!(req.objective.is_none());
        assert!(req.max_turns.is_none());
        assert!(req.reason.is_none());
    }

    #[test]
    fn goal_status_response_round_trips_inactive_default() {
        let json = serde_json::to_string(&GoalStatusResponse::default()).unwrap();
        let parsed: GoalStatusResponse = serde_json::from_str(&json).unwrap();
        assert!(!parsed.active);
        assert!(parsed.status.is_none());
        assert!(parsed.subgoals.is_empty());
    }
}
