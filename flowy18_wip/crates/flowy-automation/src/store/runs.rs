//! Compare-and-swap lifecycle writes, independent of execution and model I/O.
use super::{
    AutomationStore, StoreError,
    records::{
        AttemptRecord, CommandClockAdvance, OutboxRecord, RunRecord,
        StoreMutation,
    },
    repository as repo,
};
use crate::{
    ActionSpec, ArtifactState, AttemptId, CommitRef, DefinitionState,
    ExecutionOutcome, GenerationEpoch, HistoryCommitState, OutboxState,
    PrincipalId, PrivateText, ProgressPhase, ResultRef, RunCompletion, RunId,
    RunOutcome, RunPhase, RunProgress, RunSpec,
};
use rusqlite::{Connection, params};

fn records(
    c: &Connection,
    p: &PrincipalId,
    phases: &str,
) -> Result<Vec<RunRecord>, StoreError> {
    let mut statement=c.prepare(&format!("SELECT * FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND phase IN ({phases}) ORDER BY created_at_ms,run_id"))?;
    let mut rows =
        statement.query(params![p.authority(), p.external_user_id()])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(repo::read_run(row)?);
    }
    Ok(result)
}

impl AutomationStore {
    pub async fn recovery_owners(
        &self,
    ) -> Result<Vec<PrincipalId>, StoreError> {
        self.call(|c| {
            let mut stmt = c.prepare("SELECT DISTINCT principal_authority,principal_user_id FROM runs WHERE phase IN ('running','cancelling','finalizing')")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?;
            rows.map(|r| { let (authority, user) = r?; PrincipalId::cloud(authority,user).map_err(|_| StoreError::Corrupt("recovery principal")) }).collect()
        }).await
    }

    pub async fn worker_blocked(
        &self,
        p: PrincipalId,
        id: RunId,
        expected: RunPhase,
        reason: crate::BlockedReason,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            if !matches!(expected, RunPhase::Queued | RunPhase::Blocked | RunPhase::WaitingWorkspace) {
                return Ok(false);
            }
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let expected = repo::encode(&expected)?;
            let reason = repo::encode(&reason)?;
            let changed = tx.execute(
                "UPDATE runs SET phase='blocked', blocked_reason=?1, next_recheck_at_ms=?2, updated_at_ms=?3
                 WHERE principal_authority=?4 AND principal_user_id=?5 AND run_id=?6 AND phase=?7 AND current_attempt_id IS NULL",
                params![reason.trim_matches('"'),now.saturating_add(2_000),now,p.authority(),p.external_user_id(),id.as_str(),expected.trim_matches('"')],
            )? == 1;
            if changed && expected.trim_matches('"') == "blocked" {
                tx.commit()?;
                return Ok(true);
            }
            finish_transition(tx, &p, now, changed)
        }).await
    }

    pub async fn worker_blocked_due(
        &self,
        p: PrincipalId,
        id: RunId,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            Ok(c.query_row(
                "SELECT coalesce(next_recheck_at_ms<=?1 OR updated_at_ms>?1,1)
                 FROM runs WHERE principal_authority=?2 AND principal_user_id=?3 AND run_id=?4",
                params![now,p.authority(),p.external_user_id(),id.as_str()], |row| row.get(0),
            )?)
        }).await
    }

    pub async fn worker_requeue(
        &self,
        p: PrincipalId,
        id: RunId,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let changed = tx.execute(
                "UPDATE runs SET phase='queued',blocked_reason=NULL,next_recheck_at_ms=NULL,updated_at_ms=?1
                 WHERE principal_authority=?2 AND principal_user_id=?3 AND run_id=?4 AND phase='blocked' AND current_attempt_id IS NULL AND latest_start_at_ms>=?1",
                params![now,p.authority(),p.external_user_id(),id.as_str()],
            )? == 1;
            finish_transition(tx, &p, now, changed)
        }).await
    }

    pub async fn worker_candidates(
        &self,
        p: PrincipalId,
    ) -> Result<Vec<RunRecord>, StoreError> {
        self.call(move |c| {
            records(c, &p, "'queued','blocked','waiting_workspace'")
        })
        .await
    }
    pub async fn commit_retry_candidates(
        &self,
        p: PrincipalId,
        now: i64,
    ) -> Result<Vec<RunRecord>, StoreError> {
        self.call(move |c| {
            let mut stmt = c.prepare("SELECT * FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND phase='finalizing' AND commit_retry_at_ms<=?3 ORDER BY commit_retry_at_ms LIMIT 16")?;
            let mut rows = stmt.query(params![p.authority(),p.external_user_id(),now])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? { result.push(repo::read_run(row)?); }
            Ok(result)
        }).await
    }

    pub async fn defer_commit_retry(
        &self,
        p: PrincipalId,
        run: RunId,
        now: i64,
    ) -> Result<(), StoreError> {
        self.call(move |c| {
            c.execute("UPDATE runs SET commit_retry_at_ms=?1 WHERE principal_authority=?2 AND principal_user_id=?3 AND run_id=?4 AND phase='finalizing' AND commit_retry_at_ms<=?5",
                params![now.saturating_add(5_000),p.authority(),p.external_user_id(),run.as_str(),now])?;
            Ok(())
        }).await
    }

    pub(crate) async fn abandon_requested(
        &self,
        owner: PrincipalId,
        run_id: RunId,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            Ok(c.query_row(
                "SELECT abandon_requested FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND run_id=?3",
                params![owner.authority(),owner.external_user_id(),run_id.as_str()], |row| row.get(0),
            )?)
        }).await
    }

    pub async fn refresh_unstarted(
        &self,
        active: Option<PrincipalId>,
        now: i64,
    ) -> Result<(), StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let candidates = {
                let mut stmt = tx.prepare(
                    "SELECT * FROM runs WHERE current_attempt_id IS NULL AND phase IN ('queued','blocked','waiting_workspace')
                     AND (latest_start_at_ms<?1 OR ((?2 IS NULL OR principal_authority!=?2 OR principal_user_id!=?3)
                         AND (phase!='blocked' OR blocked_reason IS NOT 'account_unavailable')))
                     ORDER BY latest_start_at_ms LIMIT 128"
                )?;
                let mut rows = stmt.query(params![now,active.as_ref().map(PrincipalId::authority),active.as_ref().map(PrincipalId::external_user_id)])?;
                let mut result = Vec::new();
                while let Some(row) = rows.next()? { result.push(repo::read_run(row)?); }
                result
            };
            let mut changed = std::collections::BTreeSet::new();
            for run in candidates {
                if run.latest_start_at_ms < now {
                    tx.execute("UPDATE runs SET phase='settled',outcome='expired',blocked_reason=NULL,next_recheck_at_ms=NULL,updated_at_ms=?1 WHERE run_id=?2", params![now,run.run_id.as_str()])?;
                } else {
                    tx.execute("UPDATE runs SET phase='blocked',blocked_reason='account_unavailable',next_recheck_at_ms=?1,updated_at_ms=?2 WHERE run_id=?3",
                        params![now.saturating_add(2_000),now,run.run_id.as_str()])?;
                }
                changed.insert(run.principal);
            }
            for p in changed {
                repo::advance_command_clock(&tx,&p,&CommandClockAdvance { principal:p.clone(), observed_now_ms:now, epoch:GenerationEpoch::generate() })?;
            }
            tx.commit()?;
            Ok(())
        }).await
    }

    pub async fn worker_recoverable(
        &self,
        p: PrincipalId,
    ) -> Result<Vec<RunRecord>, StoreError> {
        self.call(move |c| {
            records(c, &p, "'running','cancelling','finalizing'")
        })
        .await
    }
    pub async fn worker_waiting(
        &self,
        p: PrincipalId,
        id: RunId,
        expected: RunPhase,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move|c| {
            if !matches!(expected,RunPhase::Queued|RunPhase::Blocked|RunPhase::WaitingWorkspace){return Err(StoreError::Invalid("not a waiting candidate"));}
            if expected==RunPhase::WaitingWorkspace {return Ok(repo::run(c,&p,&id)?.is_some_and(|r|r.phase==expected && r.current_attempt_id.is_none()));}
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let expected=repo::encode(&expected)?;let expected=expected.trim_matches('"');
            let changed=tx.execute("UPDATE runs SET phase='waiting_workspace',updated_at_ms=?1 WHERE principal_authority=?2 AND principal_user_id=?3 AND run_id=?4 AND phase=?5 AND current_attempt_id IS NULL",params![now,p.authority(),p.external_user_id(),id.as_str(),expected])?==1;
            finish_transition(tx,&p,now,changed)
        }).await
    }
    pub async fn worker_start(
        &self,
        p: PrincipalId,
        id: RunId,
        expected: RunPhase,
        spec: RunSpec,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move|c|{
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let r=repo::run(&tx,&p,&id)?.ok_or(StoreError::Reference("run missing"))?;
            if r.phase!=expected || r.current_attempt_id.is_some(){return Ok(false);}
            if !matches!(r.phase,RunPhase::Queued|RunPhase::Blocked|RunPhase::WaitingWorkspace) || now>r.latest_start_at_ms {return Ok(false);}
            if spec.principal!=p || spec.run_id!=id || spec.automation_id!=r.automation_id || spec.revision!=r.revision || spec.latest_start_at_ms!=r.latest_start_at_ms {return Err(StoreError::Invalid("attempt binding"));}
            if repo::automation(&tx,&p,&r.automation_id)?.is_none_or(|a|a.state==DefinitionState::Archived){return Ok(false);}
            repo::apply_mutations(&tx,&p,vec![StoreMutation::InsertAttempt(AttemptRecord {spec:spec.clone(),started_at_ms:now,completion:None}),StoreMutation::SetCurrentAttempt {run_id:id.clone(),expected:None,attempt_id:spec.attempt_id.clone()}])?;
            tx.execute("UPDATE runs SET phase='running',spec_json=?1,updated_at_ms=?2 WHERE run_id=?3 AND principal_authority=?4 AND principal_user_id=?5",params![repo::encode(&spec)?,now,id.as_str(),p.authority(),p.external_user_id()])?;
            repo::advance_command_clock(&tx,&p,&CommandClockAdvance {principal:p.clone(),observed_now_ms:now,epoch:GenerationEpoch::generate()})?;
            validate(&tx)?;tx.commit()?;Ok(true)
        }).await
    }
    pub async fn worker_progress(
        &self,
        p: PrincipalId,
        progress: RunProgress,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move|c|{
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let Some(r)=repo::run(&tx,&p,&progress.run_id)? else{return Ok(false)};
            if r.current_attempt_id.as_ref()!=Some(&progress.attempt_id){return Ok(false);}
            let phase=match progress.phase { ProgressPhase::Finalizing=>"finalizing",ProgressPhase::CancellationRequested=>"cancelling",ProgressPhase::Running=>"running",ProgressPhase::WaitingWorkspace=>return Ok(false) };
            if matches!(r.phase,RunPhase::Settled|RunPhase::Interrupted|RunPhase::Finalizing) || (r.phase==RunPhase::Cancelling && phase=="running"){return Ok(false);}
            let phase_before=repo::encode(&r.phase)?;
            if phase_before.trim_matches('"')==phase {return Ok(false);}
            let changed=tx.execute("UPDATE runs SET phase=?1,updated_at_ms=?2 WHERE run_id=?3 AND principal_authority=?4 AND principal_user_id=?5",params![phase,now,r.run_id.as_str(),p.authority(),p.external_user_id()])?==1;
            finish_transition(tx,&p,now,changed)
        }).await
    }
    pub async fn worker_complete(
        &self,
        p: PrincipalId,
        id: RunId,
        attempt: Option<AttemptId>,
        completion: RunCompletion,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            complete(c, &p, &id, attempt.as_ref(), &completion, now)
        })
        .await
    }
    pub async fn worker_expire(
        &self,
        p: PrincipalId,
        id: RunId,
        expected: RunPhase,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move|c|{let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;let Some(r)=repo::run(&tx,&p,&id)? else{return Ok(false)};if r.phase!=expected || r.current_attempt_id.is_some() || now<=r.latest_start_at_ms || !matches!(r.phase,RunPhase::Queued|RunPhase::Blocked|RunPhase::WaitingWorkspace) {return Ok(false);}
            let changed=tx.execute("UPDATE runs SET phase='settled',outcome='expired',updated_at_ms=?1 WHERE run_id=?2 AND principal_authority=?3 AND principal_user_id=?4",params![now,id.as_str(),p.authority(),p.external_user_id()])?==1;
            finish_transition(tx,&p,now,changed)
        }).await
    }
    pub async fn worker_interrupt(
        &self,
        p: PrincipalId,
        id: RunId,
        attempt: Option<AttemptId>,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move|c| {let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;let Some(r)=repo::run(&tx,&p,&id)? else{return Ok(false)};if r.current_attempt_id!=attempt || !matches!(r.phase,RunPhase::Running|RunPhase::Cancelling|RunPhase::Finalizing) {return Ok(false);}
            let changed=tx.execute("UPDATE runs SET phase='interrupted',outcome='interrupted',updated_at_ms=?1 WHERE run_id=?2 AND principal_authority=?3 AND principal_user_id=?4",params![now,id.as_str(),p.authority(),p.external_user_id()])?==1;
            finish_transition(tx,&p,now,changed)
        }).await
    }
}

pub(super) fn finish_transition(
    tx: rusqlite::Transaction<'_>,
    p: &PrincipalId,
    now: i64,
    changed: bool,
) -> Result<bool, StoreError> {
    if !changed {
        return Ok(false);
    }
    repo::advance_command_clock(
        &tx,
        p,
        &CommandClockAdvance {
            principal: p.clone(),
            observed_now_ms: now,
            epoch: GenerationEpoch::generate(),
        },
    )?;
    validate(&tx)?;
    tx.commit()?;
    Ok(true)
}

fn validate(c: &Connection) -> Result<(), StoreError> {
    if super::audit::database_findings(c)?.is_empty() {
        Ok(())
    } else {
        Err(StoreError::Reference(
            "run transition leaves invalid records",
        ))
    }
}

fn complete(
    c: &mut Connection,
    p: &PrincipalId,
    id: &RunId,
    attempt: Option<&AttemptId>,
    completion: &RunCompletion,
    now: i64,
) -> Result<bool, StoreError> {
    let tx =
        c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let Some(r) = repo::run(&tx, p, id)? else {
        return Ok(false);
    };
    if r.current_attempt_id.as_ref() != attempt
        || matches!(r.phase, RunPhase::Settled | RunPhase::Interrupted)
    {
        return Ok(false);
    }
    let revision = repo::revisions(&tx, p, &r.automation_id)?
        .into_iter()
        .find(|v| v.revision == r.revision)
        .ok_or(StoreError::Corrupt("run revision missing"))?;
    if let Some(attempt) = attempt {
        let stored = repo::attempts(&tx, p, id)?
            .into_iter()
            .find(|a| a.spec.attempt_id == *attempt)
            .ok_or(StoreError::Reference("attempt missing"))?;
        let matches = |reference: &CommitRef| {
            stored.spec.session_id.as_ref() == Some(&reference.session_id)
                && stored.spec.submission_id.as_ref()
                    == Some(&reference.submission_id)
        };
        if completion.commit_ref.as_ref().is_some_and(|r| !matches(r))
            || completion.result_ref.as_ref().is_some_and(|r| match r {
                ResultRef::SessionTurn { commit } => !matches(commit),
                ResultRef::Reminder { .. } => true,
            })
        {
            return Err(StoreError::Invalid("completion binding"));
        }
        if completion.execution_outcome == ExecutionOutcome::Completed
            && completion.history_commit == HistoryCommitState::Committed
            && completion.commit_ref.is_none()
        {
            return Err(StoreError::Invalid("completion proof missing"));
        }
        tx.execute("UPDATE run_attempts SET completion_json=?1 WHERE attempt_id=?2 AND run_id=?3 AND principal_authority=?4 AND principal_user_id=?5",params![repo::encode(completion)?,attempt.as_str(),id.as_str(),p.authority(),p.external_user_id()])?;
    } else if !matches!(revision.definition.action, ActionSpec::Reminder { .. })
        || !matches!(
            r.phase,
            RunPhase::Queued | RunPhase::Blocked | RunPhase::WaitingWorkspace
        )
    {
        return Err(StoreError::Invalid("agent run needs attempt"));
    } else if now > r.latest_start_at_ms {
        return Ok(false);
    }
    if attempt.is_none()
        && (completion.history_commit != HistoryCommitState::NotStarted
            || completion.artifact_state != ArtifactState::NotRequired
            || completion.commit_ref.is_some()
            || completion.result_ref.as_ref()
                != Some(&ResultRef::Reminder { run_id: id.clone() }))
    {
        return Err(StoreError::Invalid("invalid reminder completion"));
    }
    let pending = matches!(
        completion.artifact_state,
        ArtifactState::Pending | ArtifactState::RetryableFailed
    ) || completion.execution_outcome
        == ExecutionOutcome::Pending;
    if pending {
        tx.execute("UPDATE runs SET phase='finalizing',updated_at_ms=?1 WHERE run_id=?2 AND principal_authority=?3 AND principal_user_id=?4",params![now,id.as_str(),p.authority(),p.external_user_id()])?;
    } else {
        let outcome = match completion.execution_outcome {
            ExecutionOutcome::Completed
                if completion
                    .is_successful_for(&revision.definition.action) =>
            {
                RunOutcome::Succeeded
            }
            ExecutionOutcome::Cancelled => RunOutcome::Cancelled,
            ExecutionOutcome::TimedOut => RunOutcome::TimedOut,
            ExecutionOutcome::BudgetExceeded => RunOutcome::BudgetExceeded,
            ExecutionOutcome::Interrupted => RunOutcome::Interrupted,
            _ => RunOutcome::Failed,
        };
        let outcome = repo::encode(&outcome)?;
        tx.execute("UPDATE runs SET phase='settled',outcome=?1,updated_at_ms=?2 WHERE run_id=?3 AND principal_authority=?4 AND principal_user_id=?5",params![outcome.trim_matches('"'),now,id.as_str(),p.authority(),p.external_user_id()])?;
        repo::apply_mutations(
            &tx,
            p,
            [
                crate::DeliveryTarget::Inbox,
                crate::DeliveryTarget::SystemNotification,
            ]
            .into_iter()
            .map(|target| {
                Ok(StoreMutation::InsertOutbox(OutboxRecord {
                    principal: p.clone(),
                    delivery_key: format!(
                        "run-{}-{}",
                        id.as_str(),
                        target.as_str()
                    ),
                    run_id: id.clone(),
                    target: target.as_str().into(),
                    message_kind: "run_result".into(),
                    state: OutboxState::Pending,
                    payload: PrivateText::new(repo::encode(completion)?),
                    next_attempt_at_ms: now,
                    attempt_count: 0,
                    created_at_ms: now,
                    updated_at_ms: now,
                }))
            })
            .collect::<Result<Vec<_>, StoreError>>()?,
        )?;
    }
    repo::advance_command_clock(
        &tx,
        p,
        &CommandClockAdvance {
            principal: p.clone(),
            observed_now_ms: now,
            epoch: GenerationEpoch::generate(),
        },
    )?;
    validate(&tx)?;
    tx.commit()?;
    Ok(true)
}
