//! Command semantics execute on the connection owner, in one immediate transaction.
use super::{
    StoreError,
    records::{
        AutomationRecord, CommandClockAdvance, ReceiptRecord, ReceiptState,
        RevisionContext, RevisionRecord, RunRecord, StoreMutation,
        validate_definition,
    },
    repository as repo,
};
use crate::{
    AutomationCommand, AutomationId, AutomationQuery, AutomationSnapshot,
    BlockedReason, CallerContext, CallerSource, CancelDisposition,
    CommandEnvelope, CommandOutcome, CommandReceipt, DefinitionState,
    GenerationEpoch, HostAccessDecision, OutboxState, PrincipalId,
    QuerySnapshot, Revision, RunId, RunPhase, RunSnapshot,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// Manual starts expire after ten minutes without acquiring resources.
pub const MANUAL_LATEST_START_DELAY_MS: i64 = 600_000;

pub(super) fn observe_time(
    c: &Connection,
    p: &PrincipalId,
    now: i64,
) -> Result<i64, StoreError> {
    c.execute("INSERT INTO command_clocks (principal_authority,principal_user_id,request_high_water_ms,change_epoch,change_sequence,updated_at_ms) VALUES (?1,?2,?3,?4,0,?3) ON CONFLICT(principal_authority,principal_user_id) DO UPDATE SET request_high_water_ms=max(request_high_water_ms,excluded.request_high_water_ms),updated_at_ms=max(updated_at_ms,excluded.updated_at_ms)", params![p.authority(),p.external_user_id(),now,GenerationEpoch::generate().as_str()])?;
    Ok(repo::command_clock(c, p)?
        .ok_or(StoreError::Unavailable)?
        .request_high_water_ms)
}

pub(super) fn apply(
    c: &mut Connection,
    caller: &CallerContext,
    envelope: CommandEnvelope,
    now: i64,
    decision: HostAccessDecision,
    approved_grant: Option<crate::GrantId>,
    limits: Option<crate::retention::RetentionLimits>,
) -> Result<CommandReceipt, StoreError> {
    envelope
        .verify_digest()
        .map_err(|_| StoreError::Invalid("command digest mismatch"))?;
    let owner = &caller.principal;
    // Persist the observation even if expiry prevents the command transaction.
    let now = observe_time(c, owner, now)?;
    let tx =
        c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let now = repo::command_clock(&tx, owner)?
        .map_or(now, |clock| now.max(clock.request_high_water_ms));
    if envelope.issued_at_ms > now
        || envelope.expires_at_ms <= envelope.issued_at_ms
    {
        return Err(StoreError::Invalid("command interval"));
    }
    if now >= envelope.expires_at_ms {
        return Err(StoreError::RequestExpired);
    }
    let previous = repo::receipt(&tx, owner, &envelope.request_key)?;
    if let Some(receipt) = &previous {
        if receipt.envelope != envelope {
            return Err(StoreError::Conflict);
        }
        if receipt.state != ReceiptState::PendingApproval {
            // Access is rechecked by the service before even an old receipt is read.
            return Ok(CommandReceipt {
                request_key: envelope.request_key,
                outcome: receipt
                    .outcome
                    .clone()
                    .unwrap_or(CommandOutcome::Rejected),
            });
        }
    }
    if let AutomationCommand::CreateManual { definition }
    | AutomationCommand::UpdateManual { definition, .. } = &envelope.command
    {
        validate_definition(definition)?;
        if limits.is_some() && serde_json::to_vec(definition)?.len() > 32 * 1024
        {
            return Err(StoreError::Invalid(
                "automation definition exceeds its storage allowance",
            ));
        }
    }
    if decision == HostAccessDecision::Allowed
        && matches!(
            envelope.command,
            AutomationCommand::RunNow { .. }
                | AutomationCommand::RetryInterrupted { .. }
        )
        && let Some(limits) = limits
        && !super::retention::database_usage(&tx, owner)?.admits(limits, 1)
    {
        return Err(StoreError::Unavailable);
    }
    let (state, outcome) = match decision {
        HostAccessDecision::Denied => {
            (ReceiptState::Rejected, CommandOutcome::Rejected)
        }
        HostAccessDecision::Blocked(BlockedReason::ApprovalRequired) => (
            ReceiptState::PendingApproval,
            CommandOutcome::PendingApproval,
        ),
        HostAccessDecision::Blocked(_) => return Err(StoreError::Unavailable),
        HostAccessDecision::Allowed => (
            ReceiptState::Applied,
            execute(
                &tx,
                caller,
                &envelope.command,
                approved_grant,
                now,
                envelope.issued_at_ms,
            )?,
        ),
    };
    if previous.as_ref().is_some_and(|old| old.state == state) {
        return Ok(CommandReceipt {
            request_key: envelope.request_key,
            outcome,
        });
    }
    if previous.is_some() {
        let (automation, run) = repo::outcome_parents(Some(&outcome));
        let changed = tx.execute(
            "UPDATE command_receipts SET state=?1,outcome_json=?2,result_automation_id=?3,result_run_id=?4,updated_at_ms=?5
             WHERE principal_authority=?6 AND principal_user_id=?7 AND request_key=?8 AND state='pending_approval' AND content_digest=?9",
            params![repo::encode(&state)?.trim_matches('"'), (state==ReceiptState::Applied).then(|| repo::encode(&outcome)).transpose()?,
                automation, run, now, owner.authority(), owner.external_user_id(), envelope.request_key.as_str(), &envelope.content_digest[..]],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict);
        }
    } else {
        repo::apply_mutations(
            &tx,
            owner,
            vec![StoreMutation::InsertReceipt(ReceiptRecord {
                principal: owner.clone(),
                envelope: envelope.clone(),
                state,
                outcome: (state == ReceiptState::Applied)
                    .then(|| outcome.clone()),
                source_removed: false,
                created_at_ms: now,
                updated_at_ms: now,
            })],
        )?;
        if state == ReceiptState::PendingApproval {
            let approval_session = match &caller.source {
                CallerSource::TrustedSubmission { session_id, .. } => {
                    Some(session_id.clone())
                }
                CallerSource::UserInterface => Some(
                    crate::SessionRef::parse(crate::UI_APPROVAL_SESSION)
                        .map_err(|_| {
                            StoreError::Invalid("invalid approval session")
                        })?,
                ),
                _ => None,
            };
            if let Some(session_id) = approval_session {
                tx.execute("UPDATE command_receipts SET approval_session_id=?1 WHERE principal_authority=?2 AND principal_user_id=?3 AND request_key=?4",
                    params![session_id.as_str(),owner.authority(),owner.external_user_id(),envelope.request_key.as_str()])?;
            }
        }
    }
    repo::advance_command_clock(
        &tx,
        owner,
        &CommandClockAdvance {
            principal: owner.clone(),
            observed_now_ms: now,
            epoch: GenerationEpoch::generate(),
        },
    )?;
    tx.commit()?;
    Ok(CommandReceipt {
        request_key: envelope.request_key,
        outcome,
    })
}

fn current(
    tx: &Transaction<'_>,
    owner: &PrincipalId,
    id: &AutomationId,
    expected: Revision,
) -> Result<(AutomationRecord, RevisionRecord), StoreError> {
    let automation = repo::automation(tx, owner, id)?
        .ok_or(StoreError::Reference("automation not found"))?;
    if automation.current_revision != expected {
        return Err(StoreError::Conflict);
    }
    let revision = repo::revisions(tx, owner, id)?
        .into_iter()
        .find(|r| r.revision == expected)
        .ok_or(StoreError::Corrupt("revision missing"))?;
    Ok((automation, revision))
}

fn execute(
    tx: &Transaction<'_>,
    caller: &CallerContext,
    command: &AutomationCommand,
    approved_grant: Option<crate::GrantId>,
    now: i64,
    issued_at_ms: i64,
) -> Result<CommandOutcome, StoreError> {
    let owner = &caller.principal;
    match command {
        AutomationCommand::CreateManual { definition } => {
            validate_definition(definition)?;
            let mut definition = *definition.clone();
            if approved_grant.is_some() {
                definition.capabilities.grant_ref = approved_grant;
            }
            let id = AutomationId::generate();
            let revision = Revision::new(1).expect("positive revision");
            let context = RevisionContext {
                source_session_id: match &caller.source {
                    CallerSource::TrustedSubmission { session_id, .. }
                    | CallerSource::ConfirmedApproval { session_id } => {
                        Some(session_id.clone())
                    }
                    _ => None,
                },
                ..RevisionContext::default()
            };
            repo::apply_mutations(
                tx,
                owner,
                vec![StoreMutation::CreateAutomation {
                    automation: AutomationRecord {
                        automation_id: id.clone(),
                        principal: owner.clone(),
                        current_revision: revision,
                        state: DefinitionState::Enabled,
                        title: definition.title.clone(),
                        created_at_ms: now,
                        updated_at_ms: now,
                        source_removed: false,
                    },
                    revision: RevisionRecord {
                        automation_id: id.clone(),
                        principal: owner.clone(),
                        revision,
                        definition: definition.clone(),
                        context,
                        grant_ref: definition.capabilities.grant_ref,
                        created_at_ms: now,
                    },
                }],
            )?;
            let (created, initial) = current(tx, owner, &id, revision)?;
            // A Once proposal can become due while the original command
            // waits for approval. Its lateness window still starts at the
            // scheduled instant, and the occurrence cannot run before approval.
            let cursor = if initial.definition.schedule.is_some_and(|schedule| {
                matches!(schedule.trigger, crate::schedule::TriggerSpec::Once { scheduled_at_ms }
                    if scheduled_at_ms >= issued_at_ms && scheduled_at_ms <= now)
            }) {
                issued_at_ms.saturating_sub(1)
            } else { now };
            super::scheduler::reset(tx, &created, &initial, cursor)?;
            Ok(CommandOutcome::Created {
                automation_id: id,
                revision,
            })
        }
        AutomationCommand::UpdateManual {
            automation_id,
            expected_revision,
            definition,
        } => {
            let (mut a, mut r) =
                current(tx, owner, automation_id, *expected_revision)?;
            if a.state == DefinitionState::Archived {
                return Err(StoreError::Invalid("archived definition"));
            }
            validate_definition(definition)?;
            r.definition = *definition.clone();
            if approved_grant.is_some() {
                r.definition.capabilities.grant_ref = approved_grant;
            }
            r.grant_ref.clone_from(&r.definition.capabilities.grant_ref);
            a.title = definition.title.clone();
            advance(tx, owner, *expected_revision, &mut a, &mut r, now)?;
            Ok(CommandOutcome::Updated {
                automation_id: automation_id.clone(),
                revision: a.current_revision,
            })
        }
        AutomationCommand::Pause {
            automation_id,
            expected_revision,
        }
        | AutomationCommand::Resume {
            automation_id,
            expected_revision,
        }
        | AutomationCommand::Archive {
            automation_id,
            expected_revision,
        } => {
            let (mut a, mut r) =
                current(tx, owner, automation_id, *expected_revision)?;
            if a.state == DefinitionState::Archived {
                return Err(StoreError::Invalid("archived definition"));
            }
            a.state = match command {
                AutomationCommand::Pause { .. } => DefinitionState::Paused,
                AutomationCommand::Resume { .. } => DefinitionState::Enabled,
                _ => DefinitionState::Archived,
            };
            advance(tx, owner, *expected_revision, &mut a, &mut r, now)?;
            if a.state == DefinitionState::Archived {
                tx.execute("UPDATE runs SET phase='settled',outcome='cancelled',updated_at_ms=?1 WHERE principal_authority=?2 AND principal_user_id=?3 AND automation_id=?4 AND current_attempt_id IS NULL AND phase IN ('queued','blocked','waiting_workspace')",params![now,owner.authority(),owner.external_user_id(),automation_id.as_str()])?;
            }
            Ok(CommandOutcome::StateChanged {
                automation_id: automation_id.clone(),
                revision: a.current_revision,
                state: a.state,
            })
        }
        AutomationCommand::RunNow {
            automation_id,
            expected_revision,
        } => {
            let (mut automation, mut revision) =
                current(tx, owner, automation_id, *expected_revision)?;
            if !matches!(
                automation.state,
                DefinitionState::Enabled | DefinitionState::Paused
            ) {
                return Err(StoreError::Invalid("definition cannot run"));
            }
            // A RunNow approval grants a new authorization for the exact
            // current definition. Persist it as the next revision so the
            // detached run can verify the same grant later instead of reusing
            // the revoked reference that caused the approval request.
            let run_revision = if let Some(grant) = approved_grant {
                revision.definition.capabilities.grant_ref =
                    Some(grant.clone());
                revision.grant_ref = Some(grant);
                advance(
                    tx,
                    owner,
                    *expected_revision,
                    &mut automation,
                    &mut revision,
                    now,
                )?;
                automation.current_revision
            } else {
                *expected_revision
            };
            let id = RunId::generate();
            repo::apply_mutations(
                tx,
                owner,
                vec![StoreMutation::InsertRun(RunRecord {
                    run_id: id.clone(),
                    automation_id: automation_id.clone(),
                    revision: run_revision,
                    principal: owner.clone(),
                    current_attempt_id: None,
                    retry_of_run_id: None,
                    occurrence_key: None,
                    phase: RunPhase::Queued,
                    outcome: None,
                    latest_start_at_ms: now
                        .checked_add(MANUAL_LATEST_START_DELAY_MS)
                        .ok_or(StoreError::Invalid(
                            "manual deadline overflow",
                        ))?,
                    source_removed: false,
                    spec: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                })],
            )?;
            Ok(CommandOutcome::RunQueued { run_id: id })
        }
        AutomationCommand::CancelRun { run_id } => {
            let r = repo::run(tx, owner, run_id)?
                .ok_or(StoreError::Reference("run not found"))?;
            let disposition = match r.phase {
                RunPhase::Settled | RunPhase::Interrupted => {
                    CancelDisposition::AlreadyTerminal
                }
                RunPhase::Finalizing => CancelDisposition::TooLate,
                _ => {
                    let (phase, outcome) = if r.current_attempt_id.is_none() {
                        ("settled", Some("cancelled"))
                    } else {
                        ("cancelling", None)
                    };
                    tx.execute("UPDATE runs SET phase=?1,outcome=?2,updated_at_ms=?3 WHERE run_id=?4 AND principal_authority=?5 AND principal_user_id=?6",params![phase,outcome,now,run_id.as_str(),owner.authority(),owner.external_user_id()])?;
                    CancelDisposition::Accepted
                }
            };
            Ok(CommandOutcome::CancelRequested {
                run_id: run_id.clone(),
                disposition,
            })
        }
        AutomationCommand::SetResultKept { run_id, kept } => {
            let run = repo::run(tx, owner, run_id)?
                .ok_or(StoreError::Reference("run not found"))?;
            if run.source_removed || run.phase != RunPhase::Settled {
                return Err(StoreError::Conflict);
            }
            tx.execute(
                "UPDATE runs SET keep_result=?1 WHERE run_id=?2",
                params![kept, run_id.as_str()],
            )?;
            Ok(CommandOutcome::ResultKept {
                run_id: run_id.clone(),
                kept: *kept,
            })
        }
        AutomationCommand::AbandonPendingArtifacts { run_id } => {
            let run = repo::run(tx, owner, run_id)?
                .ok_or(StoreError::Reference("run not found"))?;
            if run.phase != RunPhase::Finalizing
                || run.current_attempt_id.is_none()
            {
                return Err(StoreError::Conflict);
            }
            tx.execute("UPDATE runs SET abandon_requested=1,commit_retry_at_ms=?1 WHERE run_id=?2",
                params![now, run_id.as_str()])?;
            Ok(CommandOutcome::DeliveryRetryQueued {
                run_id: run_id.clone(),
            })
        }
        AutomationCommand::RetryInterrupted { run_id } => {
            let old = repo::run(tx, owner, run_id)?
                .ok_or(StoreError::Reference("run not found"))?;
            let automation =
                repo::automation(tx, owner, &old.automation_id)?
                    .ok_or(StoreError::Reference("automation not found"))?;
            if old.phase != RunPhase::Interrupted
                || old.source_removed
                || !matches!(
                    automation.state,
                    DefinitionState::Enabled | DefinitionState::Paused
                )
            {
                return Err(StoreError::Conflict);
            }
            // The explicit decision starts a fresh attempt under the saved
            // revision. The old attempt and all its evidence remain intact.
            tx.execute("UPDATE runs SET phase='queued',outcome=NULL,current_attempt_id=NULL,
                spec_json=NULL,abandon_requested=0,commit_retry_at_ms=NULL,latest_start_at_ms=?1,
                updated_at_ms=?2 WHERE run_id=?3",
                params![now.saturating_add(MANUAL_LATEST_START_DELAY_MS), now, run_id.as_str()])?;
            Ok(CommandOutcome::RunQueued {
                run_id: run_id.clone(),
            })
        }
        AutomationCommand::RetryDelivery { run_id }
        | AutomationCommand::RetryDeliveryTarget { run_id, .. } => {
            let target = match command {
                AutomationCommand::RetryDeliveryTarget { target, .. } => {
                    Some(target.as_str())
                }
                _ => None,
            };
            let run = repo::run(tx, owner, run_id)?
                .ok_or(StoreError::Reference("run not found"))?;
            if target.is_none()
                && run.phase == RunPhase::Finalizing
                && run.current_attempt_id.is_some()
            {
                tx.execute(
                    "UPDATE runs SET commit_retry_at_ms=?1 WHERE run_id=?2",
                    params![now, run_id.as_str()],
                )?;
                return Ok(CommandOutcome::DeliveryRetryQueued {
                    run_id: run_id.clone(),
                });
            }
            if !repo::outbox(tx, owner, run_id)?.iter().any(|delivery| {
                target.is_none_or(|target| target == delivery.target)
                    && matches!(
                        delivery.state,
                        OutboxState::Pending
                            | OutboxState::RetryWait
                            | OutboxState::SuspendedAccount
                    )
            }) {
                return Err(StoreError::Invalid("no retryable delivery"));
            }
            tx.execute("UPDATE delivery_outbox SET state='pending',next_attempt_at_ms=?1,updated_at_ms=?1 WHERE run_id=?2 AND principal_authority=?3 AND principal_user_id=?4 AND state IN ('retry_wait','suspended_account') AND claim_until_ms IS NULL AND (?5 IS NULL OR target=?5)", params![now,run_id.as_str(),owner.authority(),owner.external_user_id(),target])?;
            Ok(CommandOutcome::DeliveryRetryQueued {
                run_id: run_id.clone(),
            })
        }
    }
}

fn advance(
    tx: &Transaction<'_>,
    owner: &PrincipalId,
    expected: Revision,
    a: &mut AutomationRecord,
    r: &mut RevisionRecord,
    now: i64,
) -> Result<(), StoreError> {
    let next = Revision::new(
        expected
            .get()
            .checked_add(1)
            .ok_or(StoreError::Invalid("revision exhausted"))?,
    )
    .ok_or(StoreError::Invalid("invalid revision"))?;
    a.current_revision = next;
    a.updated_at_ms = now;
    r.revision = next;
    r.created_at_ms = now;
    repo::apply_mutations(
        tx,
        owner,
        vec![StoreMutation::AdvanceRevision {
            expected,
            automation: a.clone(),
            revision: r.clone(),
        }],
    )?;
    super::scheduler::reset(tx, a, r, now)
}

fn automation_snapshot(
    c: &Connection,
    a: AutomationRecord,
) -> Result<AutomationSnapshot, StoreError> {
    let definition =
        repo::revision(c, &a.principal, &a.automation_id, a.current_revision)?
            .ok_or(StoreError::Corrupt("current revision is missing"))?
            .definition;
    let next_run_at_ms = super::scheduler::next(c, &a.automation_id)?;
    let storage_suspended = c
        .query_row(
            "SELECT storage_suspended FROM command_clocks
        WHERE principal_authority=?1 AND principal_user_id=?2",
            params![a.principal.authority(), a.principal.external_user_id()],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(false);
    Ok(AutomationSnapshot {
        automation_id: a.automation_id,
        revision: a.current_revision,
        state: a.state,
        title: a.title,
        definition,
        next_run_at_ms,
        storage_suspended,
    })
}
fn run_snapshot(
    c: &Connection,
    r: RunRecord,
) -> Result<RunSnapshot, StoreError> {
    let (blocked_reason, scheduled_at_ms, covers_from_ms, kept, abandon_requested, artifacts_abandoned) = c.query_row(
        "SELECT blocked_reason,scheduled_at_ms,covers_from_ms,keep_result,abandon_requested,
                coalesce((SELECT json_extract(completion_json,'$.artifact_state')='abandoned'
                  FROM run_attempts WHERE attempt_id=runs.current_attempt_id),0) FROM runs WHERE run_id=?1",
        [r.run_id.as_str()],
        |row| Ok((row.get::<_, Option<String>>(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
    )?;
    let blocked_reason = blocked_reason
        .filter(|_| r.phase == RunPhase::Blocked)
        .map(|value| serde_json::from_value(serde_json::Value::String(value)))
        .transpose()?;
    let delivery = super::delivery::statuses(c, &r.principal, &r.run_id)?;
    let definition =
        repo::revision(c, &r.principal, &r.automation_id, r.revision)?
            .ok_or(StoreError::Corrupt("run revision missing"))?
            .definition;
    let reminder = match definition.action {
        crate::ActionSpec::Reminder { text } if !r.source_removed => Some(text),
        _ => None,
    };
    let result_session_id = if r.source_removed {
        None
    } else {
        r.spec.as_ref().and_then(|spec| spec.session_id.clone())
    };
    Ok(RunSnapshot {
        run_id: r.run_id,
        automation_id: r.automation_id,
        revision: r.revision,
        current_attempt_id: r.current_attempt_id,
        phase: r.phase,
        outcome: r.outcome,
        blocked_reason,
        scheduled_at_ms,
        covers_from_ms,
        latest_start_at_ms: r.latest_start_at_ms,
        delivery,
        title: definition.title,
        reminder,
        result_session_id,
        source_removed: r.source_removed,
        kept,
        abandon_requested,
        artifacts_abandoned,
    })
}

pub(super) fn query(
    c: &Connection,
    p: &PrincipalId,
    q: AutomationQuery,
) -> Result<QuerySnapshot, StoreError> {
    match q {
        AutomationQuery::Inbox => {
            Ok(QuerySnapshot::Inbox(super::delivery::inbox(c, p)?))
        }
        AutomationQuery::InboxRuns => {
            let mut statement = c.prepare(
                "SELECT r.* FROM runs r LEFT JOIN delivery_outbox d ON r.run_id=d.run_id
                 AND r.principal_authority=d.principal_authority AND r.principal_user_id=d.principal_user_id
                 AND d.target='inbox' AND d.message_kind='run_result' AND d.state='delivered'
                 WHERE r.principal_authority=?1 AND r.principal_user_id=?2
                 AND (d.delivery_key IS NOT NULL OR r.inbox_summary=1)
                 ORDER BY coalesce(d.created_at_ms,r.updated_at_ms) DESC, r.run_id DESC LIMIT 100"
            )?;
            let mut rows = statement
                .query(params![p.authority(), p.external_user_id()])?;
            let mut runs = Vec::new();
            while let Some(row) = rows.next()? {
                runs.push(run_snapshot(c, repo::read_run(row)?)?);
            }
            Ok(QuerySnapshot::History(runs))
        }
        AutomationQuery::Automation { automation_id } => {
            Ok(QuerySnapshot::Automation(Box::new(automation_snapshot(
                c,
                repo::automation(c, p, &automation_id)?
                    .ok_or(StoreError::Reference("automation not found"))?,
            )?)))
        }
        AutomationQuery::Run { run_id } => {
            Ok(QuerySnapshot::Run(run_snapshot(
                c,
                repo::run(c, p, &run_id)?
                    .ok_or(StoreError::Reference("run not found"))?,
            )?))
        }
        AutomationQuery::List => {
            let mut statement=c.prepare("SELECT * FROM automations WHERE principal_authority=?1 AND principal_user_id=?2 ORDER BY created_at_ms,automation_id")?;
            let mut rows = statement
                .query(params![p.authority(), p.external_user_id()])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                result
                    .push(automation_snapshot(c, repo::read_automation(row)?)?);
            }
            Ok(QuerySnapshot::Automations(result))
        }
        query @ (AutomationQuery::History { .. }
        | AutomationQuery::RecentHistory { .. }) => {
            let recent = matches!(query, AutomationQuery::RecentHistory { .. });
            let (AutomationQuery::History { automation_id }
            | AutomationQuery::RecentHistory { automation_id }) = query
            else {
                unreachable!("both history variants handled above");
            };
            repo::automation(c, p, &automation_id)?
                .ok_or(StoreError::Reference("automation not found"))?;
            let mut statement=c.prepare(if recent {
                "SELECT * FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND automation_id=?3 ORDER BY created_at_ms DESC,run_id DESC LIMIT 100"
            } else {
                "SELECT * FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND automation_id=?3 ORDER BY created_at_ms,run_id"
            })?;
            let mut rows = statement.query(params![
                p.authority(),
                p.external_user_id(),
                automation_id.as_str()
            ])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                result.push(run_snapshot(c, repo::read_run(row)?)?);
            }
            Ok(QuerySnapshot::History(result))
        }
    }
}
