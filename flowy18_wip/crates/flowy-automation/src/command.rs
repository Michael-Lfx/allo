use crate::{
    AccountGeneration, ActionSpec, AttemptId, AutomationId, CancelDisposition,
    CapabilityRequest, DefinitionState, GenerationEpoch, GrantId, ModelBinding,
    PortError, PortFuture, PrincipalId, PrivateText, RequestKey, Revision,
    RunBudget, RunId, RunOutcome, RunPhase, SessionRef, SubmissionRef,
    WorkspaceSpec,
};
use serde::{Deserialize, Serialize};
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
}

/// Initial manual slice. Scheduling and editing commands are added with their
/// service behavior, rather than accepted before they can be persisted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationCommand {
    CreateManual {
        definition: Box<ManualAutomationInput>,
    },
    RunNow {
        automation_id: AutomationId,
        expected_revision: Revision,
    },
    CancelRun {
        run_id: RunId,
    },
}

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
    Created {
        automation_id: AutomationId,
        revision: Revision,
    },
    RunQueued {
        run_id: RunId,
    },
    CancelRequested {
        run_id: RunId,
        disposition: CancelDisposition,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub request_key: RequestKey,
    pub outcome: CommandOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutomationQuery {
    Automation { automation_id: AutomationId },
    Run { run_id: RunId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationSnapshot {
    pub automation_id: AutomationId,
    pub revision: Revision,
    pub state: DefinitionState,
    pub title: PrivateText,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub automation_id: AutomationId,
    pub revision: Revision,
    pub current_attempt_id: Option<AttemptId>,
    pub phase: RunPhase,
    pub outcome: Option<RunOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuerySnapshot {
    Automation(AutomationSnapshot),
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
