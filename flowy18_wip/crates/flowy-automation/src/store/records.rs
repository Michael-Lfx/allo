use crate::{
    ActionSpec, AttemptId, AutomationId, CommandEnvelope, CommandOutcome,
    DefinitionState, DeletionPlanId, GenerationEpoch, GrantId,
    ManualAutomationInput, OutboxState, PrincipalId, PrivateText, Revision,
    RunCompletion, RunId, RunOutcome, RunPhase, RunSpec, SessionRef,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRecord {
    pub automation_id: AutomationId,
    pub principal: PrincipalId,
    pub current_revision: Revision,
    pub state: DefinitionState,
    pub title: PrivateText,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub source_removed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionContext {
    pub source_session_id: Option<SessionRef>,
    pub summary: Option<PrivateText>,
    #[serde(default)]
    pub source_removed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevisionRecord {
    pub automation_id: AutomationId,
    pub principal: PrincipalId,
    pub revision: Revision,
    pub definition: ManualAutomationInput,
    pub context: RevisionContext,
    pub grant_ref: Option<GrantId>,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: RunId,
    pub automation_id: AutomationId,
    pub revision: Revision,
    pub principal: PrincipalId,
    pub current_attempt_id: Option<AttemptId>,
    pub retry_of_run_id: Option<RunId>,
    pub occurrence_key: Option<String>,
    pub phase: RunPhase,
    pub outcome: Option<RunOutcome>,
    pub latest_start_at_ms: i64,
    pub source_removed: bool,
    pub spec: Option<RunSpec>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub spec: RunSpec,
    pub started_at_ms: i64,
    pub completion: Option<RunCompletion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    PendingApproval,
    Applied,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptRecord {
    pub principal: PrincipalId,
    pub envelope: CommandEnvelope,
    pub state: ReceiptState,
    pub outcome: Option<CommandOutcome>,
    pub source_removed: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRecord {
    pub principal: PrincipalId,
    pub delivery_key: String,
    pub run_id: RunId,
    pub target: String,
    pub message_kind: String,
    pub state: OutboxState,
    pub payload: PrivateText,
    pub next_attempt_at_ms: i64,
    pub attempt_count: u32,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionPlanState {
    Planned,
    SourceRemoved,
    Cleaning,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletionPlanRecord {
    pub deletion_plan_id: DeletionPlanId,
    pub principal: PrincipalId,
    pub run_id: RunId,
    pub state: DeletionPlanState,
    pub plan: DeletionTargets,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Managed identities only: a storage record cannot nominate an arbitrary
/// user-owned project path for deletion. The retention service resolves them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeletionTargets {
    pub sessions: Vec<SessionRef>,
    pub private_workspaces: Vec<SessionRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatermarkRecord {
    pub principal: PrincipalId,
    pub automation_id: AutomationId,
    pub revision: Revision,
    pub processed_through_ms: i64,
    pub source_removed: bool,
}

/// Per-principal command clock (schema 002).
///
/// The request high-water mark only
/// moves forward, so purging receipts or a wall-clock rollback cannot reopen an
/// expired request. The epoch is fixed at creation; the sequence advances once
/// per committed command and is the dataset generation seen by subscribers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandClockRecord {
    pub principal: PrincipalId,
    pub request_high_water_ms: i64,
    pub change_epoch: GenerationEpoch,
    pub change_sequence: u64,
    pub updated_at_ms: i64,
}

/// `epoch` is used only when the clock row is first created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandClockAdvance {
    pub principal: PrincipalId,
    pub observed_now_ms: i64,
    pub epoch: GenerationEpoch,
}

#[derive(Clone, Debug)]
pub enum StoreMutation {
    CreateAutomation {
        automation: AutomationRecord,
        revision: RevisionRecord,
    },
    AdvanceRevision {
        expected: Revision,
        automation: AutomationRecord,
        revision: RevisionRecord,
    },
    InsertRun(RunRecord),
    InsertAttempt(AttemptRecord),
    InsertReceipt(ReceiptRecord),
    InsertOutbox(OutboxRecord),
    InsertDeletionPlan(DeletionPlanRecord),
    SetWatermark(WatermarkRecord),
    AdvanceCommandClock(CommandClockAdvance),
    SetCurrentAttempt {
        run_id: RunId,
        expected: Option<AttemptId>,
        attempt_id: AttemptId,
    },
}

pub(super) fn validate_definition(
    input: &ManualAutomationInput,
) -> Result<(), super::StoreError> {
    use super::StoreError;
    if input.title.expose().trim().is_empty() {
        return Err(StoreError::Invalid("empty automation title"));
    }
    if let Some(policy) = input.schedule
        && (matches!(policy.trigger, crate::schedule::TriggerSpec::Manual)
            || policy.trigger.next_after(0).is_err()
            || crate::schedule::latest_start_at_ms(0, policy.max_lateness_ms)
                .is_err())
    {
        return Err(StoreError::Invalid("invalid schedule policy"));
    }
    match &input.action {
        ActionSpec::Reminder { text } => {
            if text.expose().trim().is_empty()
                || input.workspace.is_some()
                || input.model.is_some()
                || input.budget.is_some()
                || !input.capabilities.tools.is_empty()
                || !input.capabilities.resources.is_empty()
            {
                return Err(StoreError::Invalid("invalid reminder resources"));
            }
        }
        ActionSpec::AgentTask { prompt } => {
            if prompt.expose().trim().is_empty()
                || input.model.is_none()
                || input.budget.is_none()
                || input
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.roots.is_empty())
            {
                return Err(StoreError::Invalid("incomplete agent definition"));
            }
        }
    }
    Ok(())
}
