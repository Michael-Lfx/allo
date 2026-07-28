//! Goal control DTOs for `/api/conversations/{conversation_id}/goal`.
//!
//! The frontend `/goal` slash command family maps onto one POST (actions)
//! plus one GET (status). Field names follow the engine's `GoalState`
//! snake_case contract so values pass through unmodified.

use serde::{Deserialize, Serialize};

/// POST body for a goal action on a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalActionRequest {
    /// One of `"set"` | `"pause"` | `"resume"` | `"clear"` | `"status"` |
    /// `"add_subgoal"` | `"remove_subgoal"` | `"clear_subgoals"` |
    /// `"list_subgoals"`. `"status"` is accepted for slash-command symmetry
    /// and behaves like the GET endpoint (no mutation); `"list_subgoals"` is
    /// an alias of `"status"` — the response's `subgoals` field already
    /// carries the full list.
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
    /// Criterion text. Required for `action = "add_subgoal"`.
    #[serde(default)]
    pub subgoal: Option<String>,
    /// 1-based criterion index. Required for `action = "remove_subgoal"`
    /// (matches the numbering shown by `/subgoal list`).
    #[serde(default)]
    pub index_1based: Option<u64>,
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
    /// Time-based wait barrier deadline (epoch ms). Present while `status`
    /// is `"waiting"` on a timed barrier — the frontend renders a countdown.
    #[serde(default)]
    pub waiting_until: Option<u64>,
    /// Human-readable reason for the wait barrier.
    #[serde(default)]
    pub waiting_reason: Option<String>,
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
        assert!(req.subgoal.is_none());
        assert!(req.index_1based.is_none());
    }

    #[test]
    fn goal_action_request_carries_subgoal_fields() {
        let req: GoalActionRequest =
            serde_json::from_str(r#"{"action":"add_subgoal","subgoal":"tests added"}"#).unwrap();
        assert_eq!(req.action, "add_subgoal");
        assert_eq!(req.subgoal.as_deref(), Some("tests added"));

        let req: GoalActionRequest =
            serde_json::from_str(r#"{"action":"remove_subgoal","index_1based":2}"#).unwrap();
        assert_eq!(req.index_1based, Some(2));
    }

    #[test]
    fn goal_status_response_round_trips_inactive_default() {
        let json = serde_json::to_string(&GoalStatusResponse::default()).unwrap();
        let parsed: GoalStatusResponse = serde_json::from_str(&json).unwrap();
        assert!(!parsed.active);
        assert!(parsed.status.is_none());
        assert!(parsed.subgoals.is_empty());
        assert!(parsed.waiting_until.is_none());
        assert!(parsed.waiting_reason.is_none());
    }
}
