use crate::{
    AccountGeneration, AttemptId, AutomationId, GrantId, PrincipalId, RunId,
    SessionRef, SubmissionRef,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(NonZeroU64);

impl Revision {
    pub const fn new(value: u64) -> Option<Self> {
        match NonZeroU64::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Contents may be private user data. Debug never emits the contents.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrivateText(String);

impl PrivateText {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub const fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for PrivateText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrivateText(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionSpec {
    AgentTask { prompt: PrivateText },
    Reminder { text: PrivateText },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudget {
    pub max_execution_ms: NonZeroU64,
    pub max_model_calls: NonZeroU32,
    pub max_tool_calls: NonZeroU32,
}

impl RunBudget {
    pub fn new(
        execution_ms: u64,
        model_calls: u32,
        tool_calls: u32,
    ) -> Option<Self> {
        Some(Self {
            max_execution_ms: NonZeroU64::new(execution_ms)?,
            max_model_calls: NonZeroU32::new(model_calls)?,
            max_tool_calls: NonZeroU32::new(tool_calls)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    Read,
    Write,
    DeviceWrite,
}

/// Requested resources, not proof of canonicalization or an acquired permit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    pub roots: Vec<PathBuf>,
    pub access: WorkspaceAccess,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    pub tools: Vec<String>,
    pub resources: Vec<String>,
    pub grant_ref: Option<GrantId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBinding {
    pub selected: ModelRef,
    pub allowed_fallbacks: Vec<ModelRef>,
}

/// A host lookup reference. This type must never contain an access token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    pub principal: PrincipalId,
    pub provider: String,
    pub scope_ref: String,
    pub credential_key: String,
}

impl fmt::Debug for CredentialRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialRef(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSpec {
    pub automation_id: AutomationId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub revision: Revision,
    pub principal: PrincipalId,
    pub account_generation: AccountGeneration,
    pub session_id: Option<SessionRef>,
    pub submission_id: Option<SubmissionRef>,
    pub source_session_id: Option<SessionRef>,
    pub action: ActionSpec,
    pub context_summary: Option<PrivateText>,
    /// Immutable skill IDs plus version/content digest, resolved by the host.
    pub skill_refs: Vec<String>,
    pub workspace: Option<WorkspaceSpec>,
    pub capabilities: CapabilityRequest,
    pub model: Option<ModelBinding>,
    pub credential_ref: Option<CredentialRef>,
    pub budget: Option<RunBudget>,
    pub latest_start_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RunSpecError {
    #[error(
        "agent task requires session, submission, model, credential and finite budget"
    )]
    MissingAgentInput,
    #[error("reminder cannot use agent execution resources")]
    ReminderResources,
    #[error("credential reference belongs to another principal")]
    PrincipalMismatch,
    #[error("requested workspace is empty")]
    EmptyWorkspace,
    #[error("task text cannot be empty")]
    EmptyAction,
}

impl RunSpec {
    /// Structural checks only. Current grant, account generation, credentials,
    /// paths and capacity are always rechecked through host ports at runtime.
    pub fn validate(&self) -> Result<(), RunSpecError> {
        if self
            .credential_ref
            .as_ref()
            .is_some_and(|reference| reference.principal != self.principal)
        {
            return Err(RunSpecError::PrincipalMismatch);
        }
        if self
            .workspace
            .as_ref()
            .is_some_and(|workspace| workspace.roots.is_empty())
        {
            return Err(RunSpecError::EmptyWorkspace);
        }
        match &self.action {
            ActionSpec::AgentTask { prompt } => {
                if prompt.expose().trim().is_empty() {
                    return Err(RunSpecError::EmptyAction);
                }
                if self.session_id.is_none()
                    || self.submission_id.is_none()
                    || self.model.is_none()
                    || self.credential_ref.is_none()
                    || self.budget.is_none()
                {
                    return Err(RunSpecError::MissingAgentInput);
                }
            }
            ActionSpec::Reminder { text } => {
                if text.expose().trim().is_empty() {
                    return Err(RunSpecError::EmptyAction);
                }
                if self.session_id.is_some()
                    || self.submission_id.is_some()
                    || self.model.is_some()
                    || self.credential_ref.is_some()
                    || self.budget.is_some()
                    || self.workspace.is_some()
                    || !self.capabilities.tools.is_empty()
                    || !self.capabilities.resources.is_empty()
                    || !self.skill_refs.is_empty()
                {
                    return Err(RunSpecError::ReminderResources);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionState {
    Draft,
    Enabled,
    Paused,
    Archived,
    SuspendedStorage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    Queued,
    Blocked,
    WaitingWorkspace,
    Running,
    Cancelling,
    Finalizing,
    Interrupted,
    Settled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    BudgetExceeded,
    Skipped,
    Expired,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Pending,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    BudgetExceeded,
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryCommitState {
    NotStarted,
    Committed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    NotRequired,
    Pending,
    Committed,
    RetryableFailed,
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxState {
    Pending,
    RetryWait,
    SuspendedAccount,
    Delivered,
    Suppressed,
    Abandoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    UserCancelled,
    Deadline,
    ModelCallBudget,
    ToolCallBudget,
    AccountInvalidated,
    GrantRevoked,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRef {
    pub session_id: SessionRef,
    pub submission_id: SubmissionRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultRef {
    SessionTurn { commit: CommitRef },
    Reminder { run_id: RunId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCompletion {
    pub execution_outcome: ExecutionOutcome,
    pub history_commit: HistoryCommitState,
    pub artifact_state: ArtifactState,
    pub commit_ref: Option<CommitRef>,
    pub result_ref: Option<ResultRef>,
    pub error_code: Option<String>,
    pub stop_reason: Option<StopReason>,
}

impl RunCompletion {
    pub fn is_successful_for(&self, action: &ActionSpec) -> bool {
        self.execution_outcome == ExecutionOutcome::Completed
            && matches!(
                self.artifact_state,
                ArtifactState::NotRequired | ArtifactState::Committed
            )
            && (matches!(action, ActionSpec::Reminder { .. })
                || self.history_commit == HistoryCommitState::Committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GenerationEpoch;

    fn reminder_spec() -> RunSpec {
        RunSpec {
            automation_id: AutomationId::generate(),
            run_id: RunId::generate(),
            attempt_id: AttemptId::generate(),
            revision: Revision::new(1).unwrap(),
            principal: PrincipalId::cloud("account.example", 7).unwrap(),
            account_generation: AccountGeneration {
                epoch: GenerationEpoch::generate(),
                sequence: 1,
            },
            session_id: None,
            submission_id: None,
            source_session_id: None,
            action: ActionSpec::Reminder {
                text: PrivateText::new("local reminder body"),
            },
            context_summary: None,
            skill_refs: vec![],
            workspace: None,
            capabilities: CapabilityRequest::default(),
            model: None,
            credential_ref: None,
            budget: None,
            latest_start_at_ms: 1_000,
        }
    }

    #[test]
    fn reminder_has_no_agent_resources_and_credentials_cannot_cross_principals()
    {
        let mut spec = reminder_spec();
        assert_eq!(spec.validate(), Ok(()));
        spec.session_id = Some(SessionRef::parse("existing-session").unwrap());
        assert_eq!(spec.validate(), Err(RunSpecError::ReminderResources));
        spec.session_id = None;
        spec.credential_ref = Some(CredentialRef {
            principal: PrincipalId::cloud("other-authority.example", 7)
                .unwrap(),
            provider: "test-provider".into(),
            scope_ref: "test-scope-ref".into(),
            credential_key: "test-reference-not-a-secret".into(),
        });
        assert_eq!(spec.validate(), Err(RunSpecError::PrincipalMismatch));
        spec.credential_ref = None;
        spec.action = ActionSpec::AgentTask {
            prompt: PrivateText::new("test task"),
        };
        assert_eq!(spec.validate(), Err(RunSpecError::MissingAgentInput));
    }

    #[test]
    fn finite_positive_budget_is_required_in_rust_and_serde() {
        assert!(RunBudget::new(0, 1, 1).is_none());
        assert!(RunBudget::new(1, 0, 1).is_none());
        assert!(RunBudget::new(1, 1, 0).is_none());
        assert!(serde_json::from_str::<RunBudget>(r#"{"max_execution_ms":0,"max_model_calls":1,"max_tool_calls":1}"#).is_err());
        assert!(RunBudget::new(600_000, 20, 50).is_some());
        assert!(serde_json::from_str::<Revision>("0").is_err());
    }

    #[test]
    fn private_task_text_is_not_in_debug_and_pending_artifacts_are_not_success()
    {
        let action = ActionSpec::AgentTask {
            prompt: PrivateText::new("private content"),
        };
        assert!(!format!("{action:?}").contains("private content"));
        let mut completion = RunCompletion {
            execution_outcome: ExecutionOutcome::Completed,
            history_commit: HistoryCommitState::Committed,
            artifact_state: ArtifactState::RetryableFailed,
            commit_ref: None,
            result_ref: None,
            error_code: Some("ARTIFACT_COMMIT_FAILED".into()),
            stop_reason: Some(StopReason::Deadline),
        };
        assert!(!completion.is_successful_for(&action));
        completion.artifact_state = ArtifactState::Committed;
        assert!(completion.is_successful_for(&action));
        completion.history_commit = HistoryCommitState::Unknown;
        assert!(!completion.is_successful_for(&action));
    }
}
