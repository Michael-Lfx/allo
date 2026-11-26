//! Host-issued conversation delegation; model arguments carry no authority.
use super::automation_access::{Authority, GrantAuthority, current_time_ms};
use flowy_agent::{
    AgentHandle, ExecutionOwner, ExecutionSource,
    automation_tools::AutomationCallerFactory,
};
use flowy_automation::{
    CallerContext, CallerSource, GrantId, PortError, SessionRef, SubmissionRef,
};
use std::sync::Arc;

pub(super) fn factory(
    authority: Authority,
    grants: GrantAuthority,
    agent: AgentHandle,
    grant_refs: Vec<GrantId>,
) -> Result<AutomationCallerFactory, PortError> {
    let (principal, account_generation) =
        authority.account_binding().ok_or(PortError::Forbidden)?;
    let expected_owner = ExecutionOwner {
        authority: principal.authority().to_owned(),
        subject: principal.external_user_id().to_string(),
    };
    let validate = agent.weak_tool_context_validator();
    Ok(Arc::new(move |context, args| {
        if authority.account_binding()
            != Some((principal.clone(), account_generation.clone()))
            || !validate(context, args)
            || matches!(
                context.execution.source,
                ExecutionSource::Automation { .. }
            )
            || context
                .execution
                .owner
                .as_ref()
                .is_some_and(|owner| owner != &expected_owner)
        {
            return Err(PortError::Forbidden);
        }
        let call = context.trusted_call(args).ok_or(PortError::Forbidden)?;
        for grant in &grant_refs {
            grants
                .binding(
                    &principal,
                    &account_generation,
                    grant,
                    current_time_ms(),
                )
                .map_err(|_| PortError::Forbidden)?;
        }
        Ok(CallerContext {
            principal: principal.clone(),
            account_generation: account_generation.clone(),
            source: CallerSource::TrustedSubmission {
                session_id: SessionRef::parse(call.session_id)
                    .map_err(|_| PortError::Forbidden)?,
                submission_id: SubmissionRef::parse(call.submission_id)
                    .map_err(|_| PortError::Forbidden)?,
                call_id: call.call_id,
            },
            grant_refs: grant_refs.clone(),
        })
    }))
}
