//! Claims calendar occurrences and advances their cursors on the SQLite owner.
use super::{
    AutomationRecord, AutomationStore, RevisionRecord, RunRecord, StoreError,
    StoreMutation, repository as repo,
};
use crate::schedule::{
    MisfirePolicy, OccurrenceWindow, TriggerSpec, latest_start_at_ms,
};
use crate::{
    AutomationId, DefinitionState, PrincipalId, RunId, RunOutcome, RunPhase,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

const SCAN_BATCH: u32 = 32;
/// Normal timer jitter is tolerated even for schedules that skip misfires.
const ON_TIME_MS: i64 = 5_000;

pub(super) fn reset(
    tx: &Transaction<'_>,
    a: &AutomationRecord,
    r: &RevisionRecord,
    now: i64,
) -> Result<(), StoreError> {
    let cursor = repo::watermark(tx, &a.principal, &a.automation_id)?
        .map_or(now, |w| now.max(w.processed_through_ms));
    let Some(policy) = r.definition.schedule else {
        // Keep any previous cursor as a replay fence across policy changes.
        tx.execute("UPDATE schedule_watermarks SET next_run_at_ms=NULL WHERE automation_id=?1", [a.automation_id.as_str()])?;
        return Ok(());
    };
    let next = policy.trigger.next_after(cursor).map_err(schedule_error)?;
    tx.execute(
        "INSERT INTO schedule_watermarks (automation_id,principal_authority,principal_user_id,revision,processed_through_ms,next_run_at_ms) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(automation_id) DO UPDATE SET revision=excluded.revision,processed_through_ms=excluded.processed_through_ms,next_run_at_ms=excluded.next_run_at_ms",
        params![a.automation_id.as_str(), a.principal.authority(), a.principal.external_user_id(), repo::revision_value(r.revision)?, cursor, next.filter(|_| a.state == DefinitionState::Enabled)],
    )?;
    Ok(())
}

pub(super) fn next(
    c: &Connection,
    id: &AutomationId,
) -> Result<Option<i64>, StoreError> {
    Ok(c.query_row(
        "SELECT next_run_at_ms FROM schedule_watermarks WHERE automation_id=?1",
        [id.as_str()],
        |r| r.get::<_, Option<i64>>(0),
    )
    .optional()?
    .flatten())
}

impl AutomationStore {
    pub async fn scan_due(
        &self,
        principal: PrincipalId,
        now: i64,
    ) -> Result<usize, StoreError> {
        let limits = self.retention_limits.get().copied();
        self.call(move |c| scan(c, &principal, now, limits)).await
    }
}

fn scan(
    c: &mut Connection,
    p: &PrincipalId,
    now: i64,
    limits: Option<crate::retention::RetentionLimits>,
) -> Result<usize, StoreError> {
    let tx =
        c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let candidates = {
        let mut s = tx.prepare(
            "SELECT a.* FROM automations a JOIN schedule_watermarks w ON a.automation_id=w.automation_id
             AND a.principal_authority=w.principal_authority AND a.principal_user_id=w.principal_user_id AND a.current_revision=w.revision
             WHERE a.principal_authority=?1 AND a.principal_user_id=?2 AND a.state='enabled'
             AND a.source_removed=0 AND w.source_removed=0 AND w.next_run_at_ms<=?3
             ORDER BY w.next_run_at_ms, a.automation_id LIMIT ?4",
        )?;
        let mut rows = s.query(params![
            p.authority(),
            p.external_user_id(),
            now,
            SCAN_BATCH
        ])?;
        let mut result = Vec::new();
        while let Some(row) = rows.next()? {
            result.push(repo::read_automation(row)?);
        }
        result
    };
    let mut scanned = 0;
    for a in &candidates {
        if let Some(limits) = limits
            && !super::retention::database_usage(&tx, p)?.admits(limits, 1)
        {
            break;
        }
        let w = repo::watermark(&tx, p, &a.automation_id)?
            .ok_or(StoreError::Corrupt("missing schedule cursor"))?;
        let revision = repo::revisions(&tx, p, &a.automation_id)?
            .into_iter()
            .find(|r| r.revision == w.revision)
            .ok_or(StoreError::Corrupt("missing schedule revision"))?;
        let policy = revision
            .definition
            .schedule
            .ok_or(StoreError::Corrupt("missing schedule policy"))?;
        let window = policy
            .trigger
            .occurrence_window(w.processed_through_ms, now)
            .map_err(schedule_error)?
            .ok_or(StoreError::Corrupt(
                "schedule cursor has no due occurrence",
            ))?;
        let overlapping = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND automation_id=?3 AND phase!='settled')",
            params![p.authority(), p.external_user_id(), a.automation_id.as_str()], |r| r.get::<_, bool>(0),
        )?;
        if overlapping {
            insert_occurrence(
                &tx,
                a,
                window,
                now,
                now,
                Some(RunOutcome::Skipped),
            )?;
        } else {
            let mut live = Some(window);
            if matches!(policy.trigger, TriggerSpec::Calendar(_))
                && policy.misfire == MisfirePolicy::Skip
            {
                let split = now
                    .saturating_sub(ON_TIME_MS + 1)
                    .max(w.processed_through_ms);
                if let Some(missed) = policy
                    .trigger
                    .occurrence_window(w.processed_through_ms, split)
                    .map_err(schedule_error)?
                {
                    insert_occurrence(
                        &tx,
                        a,
                        missed,
                        now,
                        now,
                        Some(RunOutcome::Skipped),
                    )?;
                    live = policy
                        .trigger
                        .occurrence_window(split, now)
                        .map_err(schedule_error)?;
                }
            }
            if let Some(window) = live {
                let deadline = latest_start_at_ms(
                    window.last_at_ms,
                    policy.max_lateness_ms,
                )
                .map_err(schedule_error)?;
                insert_occurrence(
                    &tx,
                    a,
                    window,
                    now,
                    deadline,
                    (now > deadline).then_some(RunOutcome::Expired),
                )?;
            }
        }
        let next = policy.trigger.next_after(now).map_err(schedule_error)?;
        let changed = tx.execute(
            "UPDATE schedule_watermarks SET processed_through_ms=?1,next_run_at_ms=?2 WHERE automation_id=?3 AND revision=?4 AND processed_through_ms=?5",
            params![now, next, a.automation_id.as_str(), repo::revision_value(w.revision)?, w.processed_through_ms],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict);
        }
        scanned += 1;
    }
    super::runs::finish_transition(tx, p, now, scanned != 0)?;
    Ok(scanned)
}

fn insert_occurrence(
    tx: &Transaction<'_>,
    a: &AutomationRecord,
    window: OccurrenceWindow,
    now: i64,
    deadline: i64,
    outcome: Option<RunOutcome>,
) -> Result<(), StoreError> {
    let run_id = RunId::generate();
    repo::apply_mutations(
        tx,
        &a.principal,
        vec![StoreMutation::InsertRun(RunRecord {
            run_id: run_id.clone(),
            automation_id: a.automation_id.clone(),
            revision: a.current_revision,
            principal: a.principal.clone(),
            current_attempt_id: None,
            retry_of_run_id: None,
            occurrence_key: Some(format!(
                "{}:{}:{}",
                a.automation_id.as_str(),
                a.current_revision.get(),
                window.last_at_ms
            )),
            phase: if outcome.is_some() {
                RunPhase::Settled
            } else {
                RunPhase::Queued
            },
            outcome,
            latest_start_at_ms: deadline,
            source_removed: false,
            spec: None,
            created_at_ms: now,
            updated_at_ms: now,
        })],
    )?;
    tx.execute(
        "UPDATE runs SET scheduled_at_ms=?1,covers_from_ms=?2 WHERE run_id=?3",
        params![window.last_at_ms, window.first_at_ms, run_id.as_str()],
    )?;
    Ok(())
}

const fn schedule_error(_: crate::schedule::ScheduleError) -> StoreError {
    StoreError::Invalid("invalid schedule time")
}
