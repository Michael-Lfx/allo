//! Async host checks surround a single repository transaction per command.
use crate::store::{AutomationStore, ReceiptState, StoreError};
use crate::{
    AccessOperation, AdmissionDecision, AdmissionPoint, AdmissionPolicy,
    AdmissionRequest, AutomationCommand, AutomationQuery, AutomationService,
    BlockedReason, CallerContext, CapabilityRequest, ChangeSubscription,
    ChangeVersion, Clock, CommandEnvelope, CommandReceipt, HostAccess,
    HostAccessDecision, HostAccessRequest, PortError, PortFuture,
    QuerySnapshot,
};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct CommandService {
    store: AutomationStore,
    clock: Arc<dyn Clock>,
    access: Arc<dyn HostAccess>,
    admission: Arc<dyn AdmissionPolicy>,
    wakeup: Option<Arc<tokio::sync::Notify>>,
}

impl std::fmt::Debug for CommandService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandService").finish_non_exhaustive()
    }
}

impl CommandService {
    pub fn new(
        store: AutomationStore,
        clock: Arc<dyn Clock>,
        access: Arc<dyn HostAccess>,
        admission: Arc<dyn AdmissionPolicy>,
    ) -> Self {
        Self {
            store,
            clock,
            access,
            admission,
            wakeup: None,
        }
    }

    #[must_use]
    pub fn with_wakeup(mut self, wakeup: Arc<tokio::sync::Notify>) -> Self {
        self.wakeup = Some(wakeup);
        self
    }

    async fn identity(
        &self,
        caller: &CallerContext,
        operation: AccessOperation,
    ) -> Result<(), PortError> {
        require_identity(self.access.as_ref(), caller, operation).await
    }

    async fn capabilities(
        &self,
        caller: &CallerContext,
        command: &AutomationCommand,
    ) -> Result<(HostAccessDecision, Option<crate::GrantId>), PortError> {
        let (mut requested, run_id, operation) = match command {
            AutomationCommand::CreateManual { definition }
            | AutomationCommand::UpdateManual { definition, .. } => (
                definition.capabilities.clone(),
                None,
                AccessOperation::Command,
            ),
            AutomationCommand::RunNow {
                automation_id,
                expected_revision,
            } => {
                let revision = self
                    .store
                    .revisions(caller.principal.clone(), automation_id.clone())
                    .await
                    .map_err(store_error)?
                    .into_iter()
                    .find(|r| r.revision == *expected_revision)
                    .ok_or(PortError::RevisionConflict)?;
                (
                    revision.definition.capabilities,
                    None,
                    AccessOperation::Start,
                )
            }
            _ => return Ok((HostAccessDecision::Allowed, None)),
        };
        if operation == AccessOperation::Command
            && requested
                .grant_ref
                .as_ref()
                .is_some_and(|grant| !caller.grant_refs.contains(grant))
        {
            return Ok((HostAccessDecision::Denied, None));
        }
        let check = async |requested: CapabilityRequest| {
            self.access
                .check(&HostAccessRequest {
                    principal: caller.principal.clone(),
                    account_generation: caller.account_generation.clone(),
                    run_id: run_id.clone(),
                    grant_ref: requested.grant_ref.clone(),
                    operation,
                    requested,
                })
                .await
        };
        let decision = check(requested.clone()).await?;
        if decision
            == HostAccessDecision::Blocked(BlockedReason::ApprovalRequired)
            && operation == AccessOperation::Command
            && requested.grant_ref.is_none()
        {
            let mut candidates = caller.grant_refs.clone();
            if let AutomationCommand::UpdateManual {
                automation_id,
                expected_revision,
                ..
            } = command
                && let Some(grant) = self
                    .store
                    .revisions(caller.principal.clone(), automation_id.clone())
                    .await
                    .map_err(store_error)?
                    .into_iter()
                    .find(|r| r.revision == *expected_revision)
                    .and_then(|r| r.grant_ref)
            {
                candidates.push(grant);
            }
            for grant in candidates {
                requested.grant_ref = Some(grant.clone());
                if check(requested.clone()).await?
                    == HostAccessDecision::Allowed
                {
                    return Ok((HostAccessDecision::Allowed, Some(grant)));
                }
            }
        }
        Ok((decision, None))
    }

    async fn execute(
        &self,
        caller: &CallerContext,
        request: CommandEnvelope,
    ) -> Result<CommandReceipt, PortError> {
        self.identity(caller, AccessOperation::Command).await?;
        let now = self
            .store
            .observe_command_time(
                caller.principal.clone(),
                self.clock.now().utc_ms,
            )
            .await
            .map_err(store_error)?;
        request.verify_digest()?;
        request.validate_time(now)?;
        if request.issued_at_ms > now {
            return Err(PortError::InvalidRequest);
        }
        let (decision, grant) =
            self.capabilities(caller, &request.command).await?;
        if matches!(decision, HostAccessDecision::Blocked(reason) if reason != BlockedReason::ApprovalRequired)
        {
            return Err(PortError::Unavailable);
        }
        let prior = self
            .store
            .receipt(caller.principal.clone(), request.request_key.clone())
            .await
            .map_err(store_error)?;
        let completed = prior.as_ref().is_some_and(|receipt| {
            receipt.state != ReceiptState::PendingApproval
        });
        if decision == HostAccessDecision::Denied
            && prior
                .as_ref()
                .is_some_and(|receipt| receipt.state == ReceiptState::Applied)
        {
            return Err(PortError::Forbidden);
        }
        if !completed
            && decision == HostAccessDecision::Allowed
            && matches!(
                request.command,
                AutomationCommand::RunNow { .. }
                    | AutomationCommand::RetryInterrupted { .. }
            )
            && self
                .admission
                .check(&AdmissionRequest {
                    principal: caller.principal.clone(),
                    run_id: None,
                    point: AdmissionPoint::CreateRun,
                })
                .await?
                != AdmissionDecision::Allowed
        {
            return Err(PortError::Unavailable);
        }
        // Recheck account generation after asynchronous grant and quota lookups.
        self.identity(caller, AccessOperation::Command).await?;
        let result = self
            .store
            .apply_with_grant(
                caller.clone(),
                request,
                self.clock.now().utc_ms,
                decision,
                grant,
            )
            .await
            .map_err(store_error);
        if result.is_ok()
            && let Some(wake) = &self.wakeup
        {
            wake.notify_one();
        }
        result
    }
}

async fn require_identity(
    access: &dyn HostAccess,
    caller: &CallerContext,
    operation: AccessOperation,
) -> Result<(), PortError> {
    match access
        .check(&HostAccessRequest {
            principal: caller.principal.clone(),
            account_generation: caller.account_generation.clone(),
            run_id: None,
            grant_ref: None,
            operation,
            requested: CapabilityRequest::default(),
        })
        .await?
    {
        HostAccessDecision::Allowed => Ok(()),
        HostAccessDecision::Denied => Err(PortError::Forbidden),
        HostAccessDecision::Blocked(_) => Err(PortError::Unavailable),
    }
}

fn store_error(error: StoreError) -> PortError {
    match error {
        StoreError::AccessDenied => PortError::Forbidden,
        StoreError::Conflict => PortError::RevisionConflict,
        StoreError::RequestExpired => PortError::RequestExpired,
        StoreError::Invalid(_) => PortError::InvalidRequest,
        StoreError::Reference(_) => PortError::NotFound,
        StoreError::Unavailable => PortError::Unavailable,
        _ => PortError::Storage,
    }
}

impl AutomationService for CommandService {
    fn command<'a>(
        &'a self,
        caller: &'a CallerContext,
        request: CommandEnvelope,
    ) -> PortFuture<'a, Result<CommandReceipt, PortError>> {
        Box::pin(self.execute(caller, request))
    }

    fn query<'a>(
        &'a self,
        caller: &'a CallerContext,
        request: AutomationQuery,
    ) -> PortFuture<'a, Result<QuerySnapshot, PortError>> {
        Box::pin(async move {
            self.identity(caller, AccessOperation::Query).await?;
            let result = self
                .store
                .query(caller.principal.clone(), request)
                .await
                .map_err(store_error)?;
            self.identity(caller, AccessOperation::Query).await?;
            Ok(result)
        })
    }

    fn subscribe<'a>(
        &'a self,
        caller: &'a CallerContext,
    ) -> PortFuture<'a, Result<Box<dyn ChangeSubscription>, PortError>> {
        Box::pin(async move {
            self.identity(caller, AccessOperation::Query).await?;
            self.store
                .observe_command_time(
                    caller.principal.clone(),
                    self.clock.now().utc_ms,
                )
                .await
                .map_err(store_error)?;
            let subscription: Box<dyn ChangeSubscription> =
                Box::new(DurableSubscription {
                    store: self.store.clone(),
                    access: self.access.clone(),
                    caller: caller.clone(),
                    last: None,
                });
            Ok(subscription)
        })
    }
}

struct DurableSubscription {
    store: AutomationStore,
    access: Arc<dyn HostAccess>,
    caller: CallerContext,
    last: Option<ChangeVersion>,
}

impl ChangeSubscription for DurableSubscription {
    fn changed(&mut self) -> PortFuture<'_, Result<ChangeVersion, PortError>> {
        Box::pin(async {
            loop {
                require_identity(
                    self.access.as_ref(),
                    &self.caller,
                    AccessOperation::Query,
                )
                .await?;
                let clock = self
                    .store
                    .command_clock(self.caller.principal.clone())
                    .await
                    .map_err(store_error)?
                    .ok_or(PortError::Storage)?;
                let version = ChangeVersion {
                    epoch: clock.change_epoch,
                    sequence: clock.change_sequence,
                };
                require_identity(
                    self.access.as_ref(),
                    &self.caller,
                    AccessOperation::Query,
                )
                .await?;
                if self.last.as_ref() != Some(&version) {
                    self.last = Some(version.clone());
                    return Ok(version);
                }
                // Persisted cursors also observe commands from other service instances.
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
    }
}

#[cfg(test)]
mod tests;
