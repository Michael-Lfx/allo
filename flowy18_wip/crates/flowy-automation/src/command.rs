use crate::{
    AccountGeneration, ActionSpec, AttemptId, AutomationId, CancelDisposition,
    CapabilityRequest, DefinitionState, GenerationEpoch, GrantId, ModelBinding,
    PortError, PortFuture, PrincipalId, PrivateText, RequestKey, Revision,
    RunBudget, RunId, RunOutcome, RunPhase, SessionRef, SubmissionRef,
    WorkspaceSpec,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallerSource {
    TrustedSubmission {
        session_id: SessionRef,
        submission_id: SubmissionRef,
        call_id: String,
    },
    UserInterface,
    ConfirmedApproval {
        session_id: SessionRef,
    },
    AuthorizedAutomation {
        source_run_id: RunId,
    },
}

/// Constructed by the host, never deserialized from model tool arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallerContext {
    pub principal: PrincipalId,
    pub account_generation: AccountGeneration,
    pub source: CallerSource,
    pub grant_refs: Vec<GrantId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualAutomationInput {
    pub title: PrivateText,
    pub action: ActionSpec,
    pub workspace: Option<WorkspaceSpec>,
    pub capabilities: CapabilityRequest,
    pub model: Option<ModelBinding>,
    pub budget: Option<RunBudget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<crate::schedule::SchedulePolicy>,
}

/// Persistent manual commands shared by UI and trusted conversation callers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationCommand {
    CreateManual {
        definition: Box<ManualAutomationInput>,
    },
    UpdateManual {
        automation_id: AutomationId,
        expected_revision: Revision,
        definition: Box<ManualAutomationInput>,
    },
    Pause {
        automation_id: AutomationId,
        expected_revision: Revision,
    },
    Resume {
        automation_id: AutomationId,
        expected_revision: Revision,
    },
    Archive {
        automation_id: AutomationId,
        expected_revision: Revision,
    },
    RunNow {
        automation_id: AutomationId,
        expected_revision: Revision,
    },
    CancelRun {
        run_id: RunId,
    },
    RetryInterrupted {
        run_id: RunId,
    },
    AbandonPendingArtifacts {
        run_id: RunId,
    },
    SetResultKept {
        run_id: RunId,
        kept: bool,
    },
    RetryDelivery {
        run_id: RunId,
    },
    RetryDeliveryTarget {
        run_id: RunId,
        target: DeliveryTarget,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryTarget {
    Inbox,
    SystemNotification,
}

impl DeliveryTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::SystemNotification => "system_notification",
        }
    }
}

/// Synthetic source key for approval requests started from the management UI.
pub const UI_APPROVAL_SESSION: &str = "automation-ui";

/// A host-stamped, immutable envelope reused during transport recovery. The
/// service verifies its digest against the command and persists its receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub request_key: RequestKey,
    pub issued_at_ms: i64,
    pub expires_at_ms: i64,
    pub content_digest: [u8; 32],
    pub command: AutomationCommand,
}

impl CommandEnvelope {
    pub fn new(
        request_key: RequestKey,
        issued_at_ms: i64,
        expires_at_ms: i64,
        command: AutomationCommand,
    ) -> Result<Self, PortError> {
        let content_digest =
            Self::digest(issued_at_ms, expires_at_ms, &command)?;
        Ok(Self {
            request_key,
            issued_at_ms,
            expires_at_ms,
            content_digest,
            command,
        })
    }

    fn digest(
        issued_at_ms: i64,
        expires_at_ms: i64,
        command: &AutomationCommand,
    ) -> Result<[u8; 32], PortError> {
        let bytes = serde_json::to_vec(&(issued_at_ms, expires_at_ms, command))
            .map_err(|_| PortError::InvalidRequest)?;
        Ok(Sha256::digest(bytes).into())
    }

    pub fn verify_digest(&self) -> Result<(), PortError> {
        if self.content_digest
            != Self::digest(
                self.issued_at_ms,
                self.expires_at_ms,
                &self.command,
            )?
        {
            return Err(PortError::InvalidRequest);
        }
        Ok(())
    }

    /// The service supplies its effective persisted wall-clock high-water mark.
    pub const fn validate_time(
        &self,
        effective_now_ms: i64,
    ) -> Result<(), PortError> {
        if self.expires_at_ms <= self.issued_at_ms {
            return Err(PortError::InvalidRequest);
        }
        if effective_now_ms >= self.expires_at_ms {
            return Err(PortError::RequestExpired);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOutcome {
    PendingApproval,
    Rejected,
    Created {
        automation_id: AutomationId,
        revision: Revision,
    },
    Updated {
        automation_id: AutomationId,
        revision: Revision,
    },
    StateChanged {
        automation_id: AutomationId,
        revision: Revision,
        state: DefinitionState,
    },
    DeliveryRetryQueued {
        run_id: RunId,
    },
    RunQueued {
        run_id: RunId,
    },
    CancelRequested {
        run_id: RunId,
        disposition: CancelDisposition,
    },
    ResultKept {
        run_id: RunId,
        kept: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub request_key: RequestKey,
    pub outcome: CommandOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutomationQuery {
    List,
    Inbox,
    InboxRuns,
    History { automation_id: AutomationId },
    RecentHistory { automation_id: AutomationId },
    Automation { automation_id: AutomationId },
    Run { run_id: RunId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AutomationSnapshot {
    pub automation_id: AutomationId,
    pub revision: Revision,
    pub state: DefinitionState,
    pub title: PrivateText,
    pub definition: ManualAutomationInput,
    pub next_run_at_ms: Option<i64>,
    pub storage_suspended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub automation_id: AutomationId,
    pub revision: Revision,
    pub current_attempt_id: Option<AttemptId>,
    pub phase: RunPhase,
    pub outcome: Option<RunOutcome>,
    pub blocked_reason: Option<crate::BlockedReason>,
    pub scheduled_at_ms: Option<i64>,
    pub covers_from_ms: Option<i64>,
    pub latest_start_at_ms: i64,
    pub delivery: Vec<DeliverySnapshot>,
    pub title: PrivateText,
    pub reminder: Option<PrivateText>,
    pub result_session_id: Option<SessionRef>,
    pub source_removed: bool,
    pub kept: bool,
    pub abandon_requested: bool,
    pub artifacts_abandoned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeliverySnapshot {
    pub target: String,
    pub state: crate::OutboxState,
    pub attempt_count: u32,
    pub next_attempt_at_ms: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum QuerySnapshot {
    Inbox(Vec<crate::store::OutboxRecord>),
    Automations(Vec<AutomationSnapshot>),
    History(Vec<RunSnapshot>),
    Automation(Box<AutomationSnapshot>),
    Run(RunSnapshot),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeVersion {
    pub epoch: GenerationEpoch,
    pub sequence: u64,
}

/// Each subscriber owns a cursor; events only invalidate cached projections.
pub trait ChangeSubscription: Send {
    fn changed(&mut self) -> PortFuture<'_, Result<ChangeVersion, PortError>>;
}

pub trait AutomationService: Send + Sync {
    fn command<'a>(
        &'a self,
        caller: &'a CallerContext,
        request: CommandEnvelope,
    ) -> PortFuture<'a, Result<CommandReceipt, PortError>>;
    fn query<'a>(
        &'a self,
        caller: &'a CallerContext,
        request: AutomationQuery,
    ) -> PortFuture<'a, Result<QuerySnapshot, PortError>>;
    fn subscribe<'a>(
        &'a self,
        caller: &'a CallerContext,
    ) -> PortFuture<'a, Result<Box<dyn ChangeSubscription>, PortError>>;
}

#[derive(Clone)]
pub struct AutomationHandle {
    service: Arc<dyn AutomationService>,
}

impl fmt::Debug for AutomationHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AutomationHandle(<service>)")
    }
}

impl AutomationHandle {
    pub const fn new(service: Arc<dyn AutomationService>) -> Self {
        Self { service }
    }

    pub fn command<'a>(
        &'a self,
        caller: &'a CallerContext,
        request: CommandEnvelope,
    ) -> PortFuture<'a, Result<CommandReceipt, PortError>> {
        self.service.command(caller, request)
    }

    pub fn query<'a>(
        &'a self,
        caller: &'a CallerContext,
        request: AutomationQuery,
    ) -> PortFuture<'a, Result<QuerySnapshot, PortError>> {
        self.service.query(caller, request)
    }

    pub fn subscribe<'a>(
        &'a self,
        caller: &'a CallerContext,
    ) -> PortFuture<'a, Result<Box<dyn ChangeSubscription>, PortError>> {
        self.service.subscribe(caller)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{Context, Poll, Waker};

    struct UnavailableService;
    impl AutomationService for UnavailableService {
        fn command<'a>(
            &'a self,
            _: &'a CallerContext,
            _: CommandEnvelope,
        ) -> PortFuture<'a, Result<CommandReceipt, PortError>> {
            Box::pin(async { Err(PortError::Unavailable) })
        }
        fn query<'a>(
            &'a self,
            _: &'a CallerContext,
            _: AutomationQuery,
        ) -> PortFuture<'a, Result<QuerySnapshot, PortError>> {
            Box::pin(async { Err(PortError::Unavailable) })
        }
        fn subscribe<'a>(
            &'a self,
            _: &'a CallerContext,
        ) -> PortFuture<'a, Result<Box<dyn ChangeSubscription>, PortError>>
        {
            Box::pin(async { Err(PortError::Unavailable) })
        }
    }

    fn envelope() -> CommandEnvelope {
        CommandEnvelope {
            request_key: RequestKey::parse("host-stable-request").unwrap(),
            issued_at_ms: 100,
            expires_at_ms: 200,
            content_digest: [1; 32],
            command: AutomationCommand::CancelRun {
                run_id: RunId::generate(),
            },
        }
    }

    #[test]
    fn expired_envelope_is_rejected_without_reissuing_its_time() {
        let request = envelope();
        assert_eq!(request.validate_time(199), Ok(()));
        assert_eq!(request.validate_time(200), Err(PortError::RequestExpired));
        let mut invalid = request;
        invalid.expires_at_ms = invalid.issued_at_ms;
        assert_eq!(invalid.validate_time(99), Err(PortError::InvalidRequest));
    }

    #[test]
    fn explicit_service_errors_are_preserved_without_runtime_or_fake_success() {
        let handle = AutomationHandle::new(Arc::new(UnavailableService));
        let caller = CallerContext {
            principal: PrincipalId::cloud("account.example", 7).unwrap(),
            account_generation: AccountGeneration {
                epoch: GenerationEpoch::generate(),
                sequence: 1,
            },
            source: CallerSource::UserInterface,
            grant_refs: vec![],
        };
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            handle
                .command(&caller, envelope())
                .as_mut()
                .poll(&mut context),
            Poll::Ready(Err(PortError::Unavailable))
        ));
        assert!(matches!(
            handle
                .query(
                    &caller,
                    AutomationQuery::Run {
                        run_id: RunId::generate()
                    }
                )
                .as_mut()
                .poll(&mut context),
            Poll::Ready(Err(PortError::Unavailable))
        ));
        assert!(matches!(
            handle.subscribe(&caller).as_mut().poll(&mut context),
            Poll::Ready(Err(PortError::Unavailable))
        ));
    }
}
