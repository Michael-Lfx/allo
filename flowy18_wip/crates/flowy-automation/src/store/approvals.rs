use super::{
    AutomationStore, ReceiptState, StoreError, commands, repository as repo,
};
use crate::{
    AutomationCommand, CallerContext, CallerSource, CapabilityRequest,
    CommandEnvelope, CommandReceipt, GrantId, HostAccessDecision, PrincipalId,
    RequestKey, Revision, SessionRef,
};
use rusqlite::{Connection, params};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalProposal {
    pub envelope: CommandEnvelope,
    pub session_id: SessionRef,
    pub title: crate::PrivateText,
    pub scope: CapabilityRequest,
    pub expected_revision: Option<Revision>,
}

impl AutomationStore {
    pub async fn pending_approvals(
        &self,
        p: PrincipalId,
        now: i64,
    ) -> Result<Vec<ApprovalProposal>, StoreError> {
        self.call(move |c| {
            let now = commands::observe_time(c, &p, now)?;
            let mut query = c.prepare("SELECT request_key,approval_session_id FROM command_receipts WHERE principal_authority=?1 AND principal_user_id=?2 AND state='pending_approval' AND source_removed=0 AND expires_at_ms>?3 AND approval_session_id IS NOT NULL ORDER BY id LIMIT 64")?;
            let rows = query.query_map(params![p.authority(),p.external_user_id(),now], |row| Ok((row.get::<_, String>(0)?,row.get::<_,String>(1)?)))?;
            let mut result = Vec::new();
            for row in rows {
                let (key, session) = row?;
                let key = RequestKey::parse(key).map_err(|_| StoreError::Corrupt("approval key"))?;
                let receipt = repo::receipt(c, &p, &key)?.ok_or(StoreError::Corrupt("approval receipt"))?;
                let session = SessionRef::parse(session).map_err(|_| StoreError::Corrupt("approval session"))?;
                if let Some(proposal) = proposal(c, &p, receipt.envelope, session)? {
                    result.push(proposal);
                }
            }
            Ok(result)
        }).await
    }

    /// The host holds its account/Grant authority across the SQLite CAS.
    /// Its durable decision is allowed to exist when the SQL commit fails.
    pub async fn resolve_approval<F>(
        &self,
        caller: CallerContext,
        expected: ApprovalProposal,
        now: i64,
        authorize: F,
    ) -> Result<Option<CommandReceipt>, StoreError>
    where
        F: FnOnce(
                &mut dyn FnMut(
                    Option<GrantId>,
                )
                    -> Result<CommandReceipt, StoreError>,
            ) -> Result<Option<CommandReceipt>, StoreError>
            + Send
            + 'static,
    {
        self.call(move |c| {
            let p = &caller.principal;
            let key = &expected.envelope.request_key;
            let now = commands::observe_time(c, p, now)?;
            let receipt = repo::receipt(c, p, key)?.ok_or(StoreError::Reference("approval receipt missing"))?;
            if receipt.source_removed || receipt.envelope != expected.envelope { return Err(StoreError::Conflict); }
            if now >= receipt.envelope.expires_at_ms { return Err(StoreError::RequestExpired); }
            let session: Option<String> = c.query_row(
                "SELECT approval_session_id FROM command_receipts WHERE principal_authority=?1 AND principal_user_id=?2 AND request_key=?3",
                params![p.authority(),p.external_user_id(),key.as_str()], |row| row.get(0))?;
            if session.as_deref() != Some(expected.session_id.as_str())
                || proposal(c, p, receipt.envelope.clone(), expected.session_id.clone())?.as_ref()
                    != Some(&expected)
            { return Err(StoreError::Conflict); }
            if receipt.state == ReceiptState::PendingApproval {
                validate_current(c,p,&receipt.envelope.command)?;
            }
            let bound = CallerContext { source: CallerSource::ConfirmedApproval { session_id: expected.session_id }, ..caller };
            authorize(&mut |grant| {
                let decision = if grant.is_some() { HostAccessDecision::Allowed } else { HostAccessDecision::Denied };
                commands::apply(c,&bound,receipt.envelope.clone(),now,decision,grant,None)
            })
        }).await
    }
}

fn validate_current(
    c: &Connection,
    p: &PrincipalId,
    command: &AutomationCommand,
) -> Result<(), StoreError> {
    match command {
        AutomationCommand::UpdateManual {
            automation_id,
            expected_revision,
            ..
        }
        | AutomationCommand::RunNow {
            automation_id,
            expected_revision,
        } => {
            let current = repo::automation(c, p, automation_id)?
                .ok_or(StoreError::Reference("automation not found"))?;
            if current.current_revision != *expected_revision
                || !matches!(
                    current.state,
                    crate::DefinitionState::Enabled
                        | crate::DefinitionState::Paused
                )
            {
                return Err(StoreError::Conflict);
            }
        }
        _ => {}
    }
    Ok(())
}

fn proposal(
    c: &Connection,
    p: &PrincipalId,
    envelope: CommandEnvelope,
    session_id: SessionRef,
) -> Result<Option<ApprovalProposal>, StoreError> {
    let (definition, expected_revision) = match &envelope.command {
        AutomationCommand::CreateManual { definition } => {
            (definition.as_ref().clone(), None)
        }
        AutomationCommand::UpdateManual {
            expected_revision,
            definition,
            ..
        } => (definition.as_ref().clone(), Some(*expected_revision)),
        AutomationCommand::RunNow {
            automation_id,
            expected_revision,
        } => {
            let automation = repo::automation(c, p, automation_id)?
                .ok_or(StoreError::Reference("automation not found"))?;
            if !matches!(
                automation.state,
                crate::DefinitionState::Enabled
                    | crate::DefinitionState::Paused
            ) {
                return Ok(None);
            }
            let revision =
                repo::revision(c, p, automation_id, *expected_revision)?
                    .ok_or(StoreError::Reference(
                        "automation revision not found",
                    ))?;
            (revision.definition, Some(*expected_revision))
        }
        _ => return Ok(None),
    };
    let mut scope = definition.capabilities.clone();
    scope.grant_ref = None;
    Ok(Some(ApprovalProposal {
        title: definition.title,
        scope,
        envelope,
        session_id,
        expected_revision,
    }))
}
