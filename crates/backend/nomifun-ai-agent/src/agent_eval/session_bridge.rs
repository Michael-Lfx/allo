//! Bridge from live eval cases onto Session Observation + conversation shells.
//!
//! Each eval case is one conversation_id under a run-scoped workspace:
//! - Observation JSONL uses the same `ObservationRecorder` path as ordinary sessions.
//! - An optional [`EvalSessionBridge`] creates a conversation shell and projects the
//!   captured trajectory into thinking / tool_call / text messages so ChatLayout
//!   looks like a real user turn (process rail + usage / credits).

use std::path::PathBuf;

use async_trait::async_trait;
use nomi_agent_eval::EvalTrajectoryEvent;
use nomifun_common::AppError;

/// Parameters for opening one eval case as a session shell.
#[derive(Debug, Clone)]
pub struct OpenEvalCaseSession {
    pub user_id: String,
    pub run_id: String,
    pub suite: String,
    pub case_id: String,
    pub case_category: String,
    pub prompt: String,
    pub workspace: PathBuf,
    /// Parent run workspace (business-named directory).
    pub run_workspace: PathBuf,
    pub run_workspace_label: String,
    pub provider_id: String,
    pub model: String,
    pub task_profile: Option<String>,
}

/// Token / wall-clock usage to persist on the conversation like a real turn.
#[derive(Debug, Clone, Default)]
pub struct EvalCaseTurnUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub elapsed_ms: u64,
    pub turns: u32,
}

/// Full turn payload projected into the conversation transcript.
#[derive(Debug, Clone)]
pub struct RecordEvalCaseTurn {
    pub user_id: String,
    pub conversation_id: String,
    /// Billing / root turn id (must match `with_flowy_billing_turn_id`).
    pub root_turn_id: String,
    pub user_prompt: String,
    pub assistant_text: String,
    pub trajectory: Vec<EvalTrajectoryEvent>,
    pub usage: EvalCaseTurnUsage,
    pub success: Option<bool>,
}

/// Host-owned side effects that bind an eval case to the session module.
#[async_trait]
pub trait EvalSessionBridge: Send + Sync {
    /// Create (or idempotently reclaim) a conversation shell for this case.
    /// Returns the conversation_id used for Session Observation.
    async fn open_case_session(&self, req: OpenEvalCaseSession) -> Result<String, AppError>;

    /// Persist a full turn transcript (user + process rail + assistant + usage).
    async fn record_case_turn(&self, req: RecordEvalCaseTurn) -> Result<(), AppError>;
}

/// Human-readable label for a suite used in workspace / session naming.
pub fn suite_business_label(suite: &str) -> &'static str {
    match suite.trim() {
        "office_tasks" => "办公任务",
        "agent_workflows" => "Agent工作流",
        "aider_polyglot" => "Aider编程",
        "classeval" => "ClassEval",
        "harness_control" => "Harness冒烟",
        _ => "Agent评测",
    }
}

/// Build a filesystem-safe, business-meaningful run workspace folder name.
pub fn eval_run_workspace_label(suite: &str, run_id: &str) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let short_run: String = run_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let label = suite_business_label(suite);
    format!("评测-{label}-{stamp}-{short_run}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_labels_cover_live_catalog() {
        assert_eq!(suite_business_label("office_tasks"), "办公任务");
        assert_eq!(suite_business_label("agent_workflows"), "Agent工作流");
        assert_eq!(suite_business_label("unknown"), "Agent评测");
    }

    #[test]
    fn workspace_label_embeds_suite_and_run_fragment() {
        let label = eval_run_workspace_label("office_tasks", "019abcdef0123456789");
        assert!(label.starts_with("评测-办公任务-"));
        assert!(label.contains("019abcde"));
        assert!(!label.contains('/') && !label.contains('\\'));
    }
}
