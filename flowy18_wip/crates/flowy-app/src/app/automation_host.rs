//! Durable automation assembly shared by production and isolated validation.
//! Startup waits for Agent recovery and uses bounded storage admission.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use flowy_agent::automation_executor::{
    AgentRunExecutor, AutomationRunResolver, ResolvedAutomationRun,
    execution_metadata,
};
use flowy_agent::{AgentHandle, LlmConfig};
use flowy_automation::store::{AutomationStore, RevisionRecord, RunRecord};
use flowy_automation::{
    AccessOperation, AdmissionDecision, AdmissionPoint, AdmissionPolicy,
    AdmissionRequest, AttemptId, AutomationHandle, AutomationWorker,
    BlockedReason, CallerContext, CallerSource, Clock, ClockReading,
    CommandService, CredentialRef, HostAccess, HostAccessDecision,
    HostAccessRequest, PortError, PortFuture, RunSpec, SessionRef, StopReason,
    SubmissionRef,
};

use super::automation_access::{
    Authority, AutomationLeaseRequest, GrantAuthority, current_time_ms,
};
use super::automation_grants::GrantRequest;
use flowy_automation::delivery::DeliveryService;
use flowy_automation::recovery::RecoveryService;
use flowy_automation::retention::RetentionService;
use flowy_automation::store::{ApprovalProposal, StoreError};
use tokio::sync::Notify;

#[derive(Debug)]
pub(super) struct ManualAutomation {
    pub(super) handle: AutomationHandle,
    pub(super) store: AutomationStore,
    worker: Arc<AutomationWorker>,
    stopped: Arc<AtomicBool>,
    task: tokio::sync::Mutex<
        Option<tokio::task::JoinHandle<Result<(), PortError>>>,
    >,
}

impl ManualAutomation {
    // Host wiring is kept explicit at this single construction boundary.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn start(
        store: AutomationStore,
        agent: AgentHandle,
        authority: Authority,
        grants: GrantAuthority,
        admission: Arc<dyn AdmissionPolicy>,
        runtime: tokio::runtime::Handle,
        wake: Arc<Notify>,
        notifications: Option<
            Arc<dyn flowy_automation::delivery::NotificationDelivery>,
        >,
        retention: Option<RetentionService>,
    ) -> Result<Self, PortError> {
        if agent.initialization_state()
            != flowy_agent::AgentInitialization::Ready
        {
            return Err(PortError::Unavailable);
        }
        let resolver = Arc::new(HostRunResolver {
            authority: authority.clone(),
            grants: grants.clone(),
        });
        let executor = Arc::new(AgentRunExecutor::new(agent, resolver));
        let clock: Arc<dyn Clock> = Arc::new(HostClock(Instant::now()));
        if !grants.available() {
            return Err(PortError::Storage);
        }
        let recovery = RecoveryService::new(
            store.clone(),
            executor.clone(),
            clock.clone(),
        );
        for principal in store.recovery_owners().await.map_err(port_error)? {
            recovery.reconcile(principal).await?;
        }
        store.reset_delivery_claims().await.map_err(port_error)?;
        let access: Arc<dyn HostAccess> = Arc::new(HostAutomationAccess {
            authority: authority.clone(),
            grants: grants.clone(),
        });
        let service = CommandService::new(
            store.clone(),
            clock.clone(),
            access.clone(),
            admission.clone(),
        )
        .with_wakeup(wake.clone());
        let handle = AutomationHandle::new(Arc::new(service));
        let worker = Arc::new(AutomationWorker::new(
            store.clone(),
            executor,
            access.clone(),
            admission.clone(),
            clock.clone(),
        ));
        let delivery = DeliveryService::new(
            store.clone(),
            clock.clone(),
            access,
            notifications,
        );
        let stopped = Arc::new(AtomicBool::new(false));
        let task_worker = worker.clone();
        let task_stop = stopped.clone();
        let task_store = store.clone();
        let task = runtime.spawn(async move {
            let ready = AtomicBool::new(false);
            let work = async {
                let mut event = true;
                while !task_stop.load(Ordering::Acquire) {
                    if ready.load(Ordering::Acquire)
                        && let Some(caller) = current_caller(&authority)
                    {
                        if let Err(error) = recovery.retry_requested(&caller.principal).await {
                            tracing::warn!(%error, "automation commit retry remains pending");
                        }
                        match task_worker.run_once_with_wakeup(&caller, event).await {
                            Ok(_) | Err(PortError::Forbidden | PortError::Unavailable) => {}
                            Err(error) => tracing::warn!(%error, "automation worker will recheck persisted state"),
                        }
                    }
                    event = tokio::select! {
                        () = wake.notified() => true,
                        () = tokio::time::sleep(Duration::from_secs(1)) => false,
                    };
                }
            };
            let maintenance = async {
                // UTC is read afresh each tick, including after resume or a
                // wall-clock adjustment. This loop never waits for a model.
                while !task_stop.load(Ordering::Acquire) {
                    let caller = current_caller(&authority);
                    let result = maintain(&task_store, &grants, &delivery, admission.as_ref(), retention.as_ref(), caller.as_ref(), clock.now().utc_ms).await;
                    ready.store(result.is_ok(), Ordering::Release);
                    if let Err(error) = result {
                        tracing::warn!(%error, "automation maintenance will retry");
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            };
            tokio::join!(work, maintenance);
            Ok(())
        });
        Ok(Self {
            handle,
            store,
            worker,
            stopped,
            task: tokio::sync::Mutex::new(Some(task)),
        })
    }

    pub(super) fn request_stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.worker.request_stop(StopReason::Shutdown);
    }

    pub(super) async fn join(&self) -> Result<(), PortError> {
        self.request_stop();
        let mut task = self.task.lock().await;
        if let Some(running) = task.as_mut() {
            // Retain the JoinHandle if a bounded shutdown drops this wait.
            // A later join can still wait for the real execution to finish.
            running.await.map_err(|_| PortError::Unavailable)??;
        }
        *task = None;
        let result =
            self.store.shutdown().await.map_err(|_| PortError::Storage);
        drop(task);
        result
    }
}

impl Drop for ManualAutomation {
    fn drop(&mut self) {
        self.request_stop();
    }
}

pub(super) fn current_caller(authority: &Authority) -> Option<CallerContext> {
    let (principal, account_generation) = authority.account_binding()?;
    Some(CallerContext {
        principal,
        account_generation,
        source: CallerSource::UserInterface,
        grant_refs: vec![],
    })
}

async fn maintain(
    store: &AutomationStore,
    grants: &GrantAuthority,
    delivery: &DeliveryService,
    admission: &dyn AdmissionPolicy,
    retention: Option<&RetentionService>,
    caller: Option<&CallerContext>,
    now: i64,
) -> Result<(), PortError> {
    store
        .refresh_unstarted(caller.map(|caller| caller.principal.clone()), now)
        .await
        .map_err(port_error)?;
    delivery.pump(caller).await?;
    let Some(caller) = caller else {
        return Ok(());
    };
    if !grants.available() {
        return Err(PortError::Storage);
    }
    if let Some(retention) = retention
        && let Err(error) = retention.maintain(&caller.principal, now).await
    {
        tracing::warn!(%error, "automation retention will retry; delivery and recovery remain active");
    }
    for approval in store
        .pending_approvals(caller.principal.clone(), now)
        .await
        .map_err(port_error)?
    {
        match resolve_approval(store, grants, caller.clone(), approval, None)
            .await
        {
            Ok(())
            | Err(
                PortError::RevisionConflict
                | PortError::RequestExpired
                | PortError::Forbidden,
            ) => {}
            Err(error) => return Err(error),
        }
    }
    let allowed = admission
        .check(&AdmissionRequest {
            principal: caller.principal.clone(),
            run_id: None,
            point: AdmissionPoint::CreateRun,
        })
        .await?
        == AdmissionDecision::Allowed;
    if retention.is_some() {
        store
            .set_storage_suspension(caller.principal.clone(), now, !allowed)
            .await
            .map_err(port_error)?;
        let refs = store
            .retained_grant_refs(caller.principal.clone())
            .await
            .map_err(port_error)?;
        grants
            .prune_retired(caller, &refs, now)
            .map_err(|_| PortError::Storage)?;
    }
    if allowed {
        store
            .scan_due(caller.principal.clone(), now)
            .await
            .map_err(port_error)?;
    }
    Ok(())
}

pub(super) async fn resolve_approval(
    store: &AutomationStore,
    grants: &GrantAuthority,
    caller: CallerContext,
    proposal: ApprovalProposal,
    user_decision: Option<bool>,
) -> Result<(), PortError> {
    let request = approval_request(&caller, &proposal);
    let grants = grants.clone();
    store
        .resolve_approval(caller, proposal, current_time_ms(), move |apply| {
            grants.complete_approval(request, user_decision, apply)
        })
        .await
        .map_err(port_error)?;
    Ok(())
}

pub(super) fn approval_request(
    caller: &CallerContext,
    proposal: &ApprovalProposal,
) -> GrantRequest {
    let envelope = &proposal.envelope;
    GrantRequest {
        principal: caller.principal.clone(),
        account_generation: caller.account_generation.clone(),
        request_key: envelope.request_key.clone(),
        content_digest: envelope.content_digest,
        expected_revision: proposal.expected_revision,
        requested_at_ms: envelope.issued_at_ms,
        request_expires_at_ms: envelope.expires_at_ms,
        grant_expires_at_ms: envelope
            .issued_at_ms
            .saturating_add(30 * 24 * 60 * 60 * 1_000),
        scope: proposal.scope.clone(),
    }
}

pub(super) fn port_error(error: StoreError) -> PortError {
    match error {
        StoreError::Conflict => PortError::RevisionConflict,
        StoreError::AccessDenied => PortError::Forbidden,
        StoreError::RequestExpired => PortError::RequestExpired,
        StoreError::Reference(_) => PortError::NotFound,
        StoreError::Invalid(_) => PortError::InvalidRequest,
        _ => PortError::Storage,
    }
}

struct HostClock(Instant);
impl Clock for HostClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            utc_ms: current_time_ms(),
            monotonic: self.0.elapsed(),
        }
    }
}

struct HostAutomationAccess {
    authority: Authority,
    grants: GrantAuthority,
}
impl HostAccess for HostAutomationAccess {
    fn check<'a>(
        &'a self,
        request: &'a HostAccessRequest,
    ) -> PortFuture<'a, Result<HostAccessDecision, PortError>> {
        Box::pin(async move {
            if self.authority.account_binding().as_ref()
                != Some(&(
                    request.principal.clone(),
                    request.account_generation.clone(),
                ))
            {
                return Ok(HostAccessDecision::Denied);
            }
            if request.requested.tools.is_empty()
                && request.requested.resources.is_empty()
                && request.requested.grant_ref.is_none()
            {
                return Ok(HostAccessDecision::Allowed);
            }
            use super::automation_grants::GrantError;
            match self.grants.check_request(
                &request.principal,
                &request.account_generation,
                &request.requested,
            ) {
                Ok(()) => Ok(HostAccessDecision::Allowed),
                Err(
                    GrantError::Storage(_)
                    | GrantError::Unavailable
                    | GrantError::Corrupt
                    | GrantError::UnsupportedVersion,
                ) => Err(PortError::Storage),
                Err(GrantError::Revoked | GrantError::Expired)
                    if request.operation == AccessOperation::Start =>
                {
                    Ok(HostAccessDecision::Blocked(
                        BlockedReason::ApprovalRequired,
                    ))
                }
                Err(_) => Ok(HostAccessDecision::Blocked(
                    BlockedReason::ApprovalRequired,
                )),
            }
        })
    }
}

struct HostRunResolver {
    authority: Authority,
    grants: GrantAuthority,
}

impl AutomationRunResolver for HostRunResolver {
    fn blocked_reason(
        &self,
        run: &RunRecord,
        revision: &RevisionRecord,
    ) -> Result<Option<BlockedReason>, PortError> {
        let Some((principal, generation)) = self.authority.account_binding()
        else {
            return Ok(Some(BlockedReason::AccountUnavailable));
        };
        if principal != run.principal {
            return Ok(Some(BlockedReason::AccountUnavailable));
        }
        if !self
            .authority
            .has_automation_credential(&principal, &generation)
        {
            return Ok(Some(BlockedReason::CredentialUnavailable));
        }
        let model = revision
            .definition
            .model
            .as_ref()
            .ok_or(PortError::Storage)?;
        if self
            .authority
            .resolve_route(&principal, &generation, model)
            .is_err()
        {
            return Ok(Some(BlockedReason::ModelUnavailable));
        }
        if revision
            .definition
            .workspace
            .as_ref()
            .is_some_and(|workspace| {
                workspace.roots.iter().any(|root| !root.is_dir())
            })
        {
            return Ok(Some(BlockedReason::WorkspaceUnavailable));
        }
        Ok(None)
    }

    fn bind(
        &self,
        run: &RunRecord,
        revision: &RevisionRecord,
    ) -> Result<RunSpec, PortError> {
        let (principal, generation) = self
            .authority
            .account_binding()
            .ok_or(PortError::Unavailable)?;
        if principal != run.principal || revision.principal != principal {
            return Err(PortError::Forbidden);
        }
        let attempt_id = AttemptId::generate();
        let session_id =
            SessionRef::parse(format!("automation-{}", attempt_id.as_str()))
                .map_err(|_| PortError::InvalidRequest)?;
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| PortError::Unavailable)?;
        let submission_id = SubmissionRef::parse(format!(
            "submission-{:032x}",
            u128::from_be_bytes(bytes)
        ))
        .map_err(|_| PortError::InvalidRequest)?;
        let model = revision
            .definition
            .model
            .clone()
            .ok_or(PortError::Unavailable)?;
        let route = self
            .authority
            .resolve_route(&principal, &generation, &model)
            .map_err(|_| PortError::Unavailable)?;
        let grant = revision
            .definition
            .capabilities
            .grant_ref
            .as_ref()
            .ok_or(PortError::Forbidden)?;
        Ok(RunSpec {
            automation_id: run.automation_id.clone(),
            run_id: run.run_id.clone(),
            attempt_id,
            revision: run.revision,
            principal: principal.clone(),
            account_generation: generation,
            session_id: Some(session_id),
            submission_id: Some(submission_id),
            source_session_id: revision.context.source_session_id.clone(),
            action: revision.definition.action.clone(),
            context_summary: revision.context.summary.clone(),
            skill_refs: vec![],
            workspace: revision.definition.workspace.clone(),
            capabilities: revision.definition.capabilities.clone(),
            model: Some(model),
            credential_ref: Some(CredentialRef {
                principal,
                provider: route.provider,
                scope_ref: grant.as_str().to_owned(),
                credential_key: "current-cloud-account".into(),
            }),
            budget: revision.definition.budget.clone(),
            latest_start_at_ms: run.latest_start_at_ms,
        })
    }

    fn resolve(
        &self,
        spec: &RunSpec,
    ) -> Result<ResolvedAutomationRun, PortError> {
        if !spec.skill_refs.is_empty() {
            return Err(PortError::Unavailable);
        }
        let binding = spec.model.as_ref().ok_or(PortError::InvalidRequest)?;
        let route = self
            .authority
            .resolve_route(&spec.principal, &spec.account_generation, binding)
            .map_err(|_| PortError::Unavailable)?;
        let grant_id = spec
            .capabilities
            .grant_ref
            .as_ref()
            .ok_or(PortError::Forbidden)?;
        let credential =
            spec.credential_ref.as_ref().ok_or(PortError::Unavailable)?;
        if credential.principal != spec.principal
            || credential.provider != route.provider
            || credential.scope_ref != grant_id.as_str()
            || credential.credential_key != "current-cloud-account"
        {
            return Err(PortError::Forbidden);
        }
        let grant = self
            .grants
            .binding(
                &spec.principal,
                &spec.account_generation,
                grant_id,
                current_time_ms(),
            )
            .map_err(|_| PortError::Forbidden)?;
        let gate = self
            .grants
            .lease(AutomationLeaseRequest {
                principal: spec.principal.clone(),
                account_generation: spec.account_generation.clone(),
                execution: execution_metadata(spec),
                route: route.clone(),
                capabilities: spec.capabilities.clone(),
                grant,
            })
            .map_err(|_| PortError::Forbidden)?;
        Ok(ResolvedAutomationRun {
            config: LlmConfig {
                api_key: None,
                base_url: route.base_url,
                model: route.model,
                provider: route.provider,
                ..LlmConfig::default()
            },
            skills: vec![],
            gate,
        })
    }
}
