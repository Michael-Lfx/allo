use super::automation_access::Authority;
use flowy_agent::AgentHandle;
use flowy_automation::retention::{RetentionHost, RetentionUsage};
use flowy_automation::store::{AttemptRecord, DeletionPlanRecord};
use flowy_automation::{PortError, PortFuture, PrincipalId, RunSpec};

pub(super) struct AgentRetention {
    agent: AgentHandle,
    authority: Authority,
}

impl AgentRetention {
    pub(super) const fn new(agent: AgentHandle, authority: Authority) -> Self {
        Self { agent, authority }
    }
}

impl RetentionHost for AgentRetention {
    fn usage<'a>(
        &'a self,
        owner: &'a PrincipalId,
        attempts: Vec<AttemptRecord>,
    ) -> PortFuture<'a, Result<RetentionUsage, PortError>> {
        let agent = self.agent.clone();
        let owner = owner.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                agent.automation_storage_usage(&owner, &attempts)
            })
            .await
            .map_err(|_| PortError::Storage)?
        })
    }

    fn protected<'a>(
        &'a self,
        plan: &'a DeletionPlanRecord,
        attempts: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<bool, PortError>> {
        let agent = self.agent.clone();
        Box::pin(async move {
            let specs = scoped_attempts(plan, attempts)?;
            tokio::task::spawn_blocking(move || {
                for spec in specs {
                    if agent.automation_retention_protected(&spec)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            })
            .await
            .map_err(|_| PortError::Storage)?
        })
    }

    fn remove<'a>(
        &'a self,
        plan: &'a DeletionPlanRecord,
        attempts: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<(), PortError>> {
        let agent = self.agent.clone();
        let authority = self.authority.clone();
        Box::pin(async move {
            let specs = scoped_attempts(plan, attempts)?;
            tokio::task::spawn_blocking(move || {
                for spec in specs {
                    if authority
                        .account_binding()
                        .is_none_or(|(owner, _)| owner != spec.principal)
                    {
                        return Err(PortError::Forbidden);
                    }
                    agent.remove_automation_result(&spec)?;
                }
                Ok(())
            })
            .await
            .map_err(|_| PortError::Storage)?
        })
    }
}

fn scoped_attempts(
    plan: &DeletionPlanRecord,
    attempts: &[AttemptRecord],
) -> Result<Vec<RunSpec>, PortError> {
    if !plan.plan.artifact_receipts.is_empty()
        || attempts.len() != plan.plan.sessions.len()
        || attempts.iter().any(|attempt| {
            let spec = &attempt.spec;
            spec.principal != plan.principal
                || spec.run_id != plan.run_id
                || spec.session_id.as_ref().is_none_or(|session| {
                    !plan.plan.sessions.contains(session)
                        || plan.plan.private_workspaces.contains(session)
                            != spec.workspace.is_none()
                })
        })
    {
        return Err(PortError::Storage);
    }
    Ok(attempts
        .iter()
        .map(|attempt| attempt.spec.clone())
        .collect())
}
