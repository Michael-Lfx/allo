use super::{AutomationStore, StoreError, repository as repo};
use crate::retention::{DatabaseUsage, RetentionLimits, cutoff};
use crate::store::{
    AttemptRecord, DeletionPlanRecord, DeletionPlanState, DeletionTargets,
    StoreMutation,
};
use crate::{DeletionPlanId, PrincipalId, RunId};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

impl AutomationStore {
    pub async fn retained_grant_refs(
        &self,
        owner: PrincipalId,
    ) -> Result<std::collections::BTreeSet<crate::GrantId>, StoreError> {
        self.call(move |c| {
            let mut statement = c.prepare("SELECT DISTINCT grant_ref FROM automation_revisions
                WHERE principal_authority=?1 AND principal_user_id=?2 AND grant_ref IS NOT NULL")?;
            let entries = statement.query_map(params![owner.authority(), owner.external_user_id()], |row| row.get::<_, String>(0))?;
            entries.map(|id| crate::GrantId::parse(id?).map_err(|_| StoreError::Corrupt("invalid grant reference"))).collect()
        }).await
    }

    pub async fn set_storage_suspension(
        &self,
        owner: PrincipalId,
        now: i64,
        suspended: bool,
    ) -> Result<(), StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = tx.execute(
                "UPDATE command_clocks SET storage_suspended=?1 WHERE principal_authority=?2 AND principal_user_id=?3
                 AND storage_suspended!=?1", params![suspended,owner.authority(),owner.external_user_id()],
            )? != 0;
            super::runs::finish_transition(tx, &owner, now, changed)?;
            Ok(())
        }).await
    }

    pub(crate) async fn retention_attempts(
        &self,
        owner: PrincipalId,
    ) -> Result<Vec<AttemptRecord>, StoreError> {
        self.call(move |c| {
            let mut statement = c.prepare(
                "SELECT * FROM run_attempts WHERE principal_authority=?1 AND principal_user_id=?2"
            )?;
            let mut rows = statement.query(params![owner.authority(), owner.external_user_id()])?;
            let mut attempts = Vec::new();
            while let Some(row) = rows.next()? {
                attempts.push(repo::read_attempt(row)?);
            }
            Ok(attempts)
        }).await
    }

    pub(crate) async fn retention_db_usage(
        &self,
        owner: PrincipalId,
    ) -> Result<DatabaseUsage, StoreError> {
        self.call(move |c| database_usage(c, &owner)).await
    }

    pub(crate) async fn retention_candidates(
        &self,
        owner: PrincipalId,
        oldest: i64,
        pressure: bool,
    ) -> Result<Vec<(DeletionPlanRecord, Vec<AttemptRecord>)>, StoreError> {
        self.call(move |c| {
            let mut statement = c.prepare(
                "SELECT r.run_id FROM runs r WHERE r.principal_authority=?1 AND r.principal_user_id=?2
                 AND r.phase='settled' AND r.compacted_at_ms IS NULL AND r.keep_result=0
                 AND (r.source_removed=1 OR r.updated_at_ms<=?3 OR ?4)
                 AND NOT EXISTS (SELECT 1 FROM delivery_outbox d WHERE d.run_id=r.run_id
                    AND d.state IN ('pending','retry_wait','suspended_account'))
                 ORDER BY r.source_removed DESC,r.updated_at_ms,r.run_id"
            )?;
            let mut rows = statement.query(params![owner.authority(), owner.external_user_id(), oldest, pressure])?;
            let mut candidates = Vec::new();
            while let Some(row) = rows.next()? {
                let id = RunId::parse(row.get::<_, String>(0)?)
                    .map_err(|_| StoreError::Corrupt("retention run ID"))?;
                let attempts = repo::attempts(c, &owner, &id)?;
                let plan = if let Some(plan) = find_plan(c, &owner, &id)? {
                    plan
                } else {
                    let sessions = attempts.iter().filter_map(|a| a.spec.session_id.clone()).collect();
                    let private_workspaces = attempts.iter().filter(|a| a.spec.workspace.is_none())
                        .filter_map(|a| a.spec.session_id.clone()).collect();
                    DeletionPlanRecord {
                        deletion_plan_id: DeletionPlanId::generate(),
                        principal: owner.clone(),
                        run_id: id,
                        state: DeletionPlanState::Planned,
                        plan: DeletionTargets { sessions, private_workspaces, artifact_receipts: vec![] },
                        created_at_ms: 0,
                        updated_at_ms: 0,
                    }
                };
                candidates.push((plan, attempts));
            }
            Ok(candidates)
        }).await
    }
    pub(crate) async fn commit_retention_plan(
        &self,
        mut plan: DeletionPlanRecord,
        now: i64,
    ) -> Result<Option<DeletionPlanRecord>, StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(existing) = find_plan(&tx, &plan.principal, &plan.run_id)? {
                return Ok(Some(existing));
            }
            let updated = tx.execute(
                "UPDATE runs SET source_removed=1 WHERE run_id=?1 AND principal_authority=?2 AND principal_user_id=?3
                 AND phase='settled' AND keep_result=0 AND source_removed=0 AND compacted_at_ms IS NULL
                 AND NOT EXISTS (SELECT 1 FROM delivery_outbox WHERE run_id=?1 AND state IN ('pending','retry_wait','suspended_account'))",
                params![plan.run_id.as_str(), plan.principal.authority(), plan.principal.external_user_id()],
            )?;
            if updated == 0 {
                return Ok(None);
            }
            plan.created_at_ms = now;
            plan.updated_at_ms = now;
            plan.state = DeletionPlanState::SourceRemoved;
            for session in &plan.plan.sessions {
                tx.execute("UPDATE automation_revisions SET context_json=json_set(context_json,'$.source_removed',json('true'))
                    WHERE principal_authority=?1 AND principal_user_id=?2 AND json_extract(context_json,'$.source_session_id')=?3",
                    params![plan.principal.authority(), plan.principal.external_user_id(), session.as_str()])?;
            }
            repo::apply_mutations(&tx, &plan.principal, vec![StoreMutation::InsertDeletionPlan(plan.clone())])?;
            super::runs::finish_transition(tx, &plan.principal, now, true)?;
            Ok(Some(plan))
        }).await
    }

    pub(crate) async fn finish_retention_plan(
        &self,
        plan: DeletionPlanRecord,
        now: i64,
    ) -> Result<(), StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let owner = &plan.principal;
            if find_plan(&tx, owner, &plan.run_id)?.as_ref() != Some(&plan) {
                return Err(StoreError::Conflict);
            }
            let id = plan.run_id.as_str();
            tx.execute(
                "UPDATE runs SET inbox_summary=EXISTS(SELECT 1 FROM delivery_outbox WHERE run_id=?1 AND target='inbox' AND state='delivered'),
                    compacted_at_ms=?2,current_attempt_id=NULL,spec_json=NULL,commit_retry_at_ms=NULL
                 WHERE run_id=?1 AND source_removed=1",
                params![id, now],
            )?;
            tx.execute("DELETE FROM run_attempts WHERE run_id=?1", [id])?;
            tx.execute("DELETE FROM delivery_outbox WHERE run_id=?1", [id])?;
            tx.execute("DELETE FROM deletion_plans WHERE deletion_plan_id=?1", [plan.deletion_plan_id.as_str()])?;
            super::runs::finish_transition(tx, owner, now, true)?;
            Ok(())
        }).await
    }

    pub(crate) async fn prune_retention_metadata(
        &self,
        owner: PrincipalId,
        now: i64,
        limits: RetentionLimits,
    ) -> Result<(), StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let key = params![owner.authority(), owner.external_user_id(), now, cutoff(now, limits.receipt_age_ms)];
            let receipts = tx.execute(
                "DELETE FROM command_receipts WHERE principal_authority=?1 AND principal_user_id=?2
                 AND expires_at_ms<=?3 AND (state='pending_approval' OR updated_at_ms<=?4)", key,
            )?;
            let summaries = tx.execute(
                "DELETE FROM runs WHERE run_id IN (
                  SELECT run_id FROM (
                    SELECT run_id,compacted_at_ms,row_number() OVER (ORDER BY compacted_at_ms DESC,run_id DESC) AS rank
                    FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND compacted_at_ms IS NOT NULL
                  ) AS summary WHERE (compacted_at_ms<=?3 OR rank>?4)
                    AND NOT EXISTS (SELECT 1 FROM runs AS retry WHERE retry.retry_of_run_id=summary.run_id)
                    AND NOT EXISTS (SELECT 1 FROM command_receipts WHERE result_run_id=summary.run_id)
                 )",
                params![owner.authority(), owner.external_user_id(), cutoff(now, limits.summary_age_ms),
                    i64::try_from(limits.summary_items.get()).unwrap_or(i64::MAX)],
            )?;
            let revisions = tx.execute(
                "DELETE FROM automation_revisions AS v WHERE v.principal_authority=?1 AND v.principal_user_id=?2
                 AND NOT EXISTS (SELECT 1 FROM automations a WHERE a.automation_id=v.automation_id AND a.current_revision=v.revision)
                 AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.automation_id=v.automation_id AND r.revision=v.revision)
                 AND NOT EXISTS (SELECT 1 FROM schedule_watermarks w WHERE w.automation_id=v.automation_id AND w.revision=v.revision)
                 AND NOT EXISTS (SELECT 1 FROM command_receipts c WHERE c.result_automation_id=v.automation_id
                    OR json_extract(c.command_json,'$.automation_id')=v.automation_id)",
                params![owner.authority(), owner.external_user_id()],
            )?;
            super::runs::finish_transition(tx, &owner, now, receipts + summaries + revisions > 0)?;
            Ok(())
        }).await
    }
}

fn find_plan(
    c: &Connection,
    owner: &PrincipalId,
    run_id: &RunId,
) -> Result<Option<DeletionPlanRecord>, StoreError> {
    let plan_id: Option<String> = c.query_row(
        "SELECT deletion_plan_id FROM deletion_plans WHERE principal_authority=?1 AND principal_user_id=?2 AND run_id=?3
         AND state IN ('source_removed','cleaning') ORDER BY id LIMIT 1",
        params![owner.authority(), owner.external_user_id(), run_id.as_str()], |row| row.get(0),
    ).optional()?;
    plan_id
        .map(|id| {
            let id = DeletionPlanId::parse(id)
                .map_err(|_| StoreError::Corrupt("retention plan ID"))?;
            repo::deletion_plan(c, owner, &id)?
                .ok_or(StoreError::Corrupt("deletion plan missing"))
        })
        .transpose()
}

pub(super) fn database_usage(
    c: &Connection,
    owner: &PrincipalId,
) -> Result<DatabaseUsage, StoreError> {
    let pages: u64 = c.query_row("SELECT page_count - freelist_count FROM pragma_page_count(), pragma_freelist_count()", [], |r| r.get(0))?;
    let page_size: u64 =
        c.pragma_query_value(None, "page_size", |r| r.get(0))?;
    let (pending_items, pending_bytes) = c.query_row(
        "SELECT count(*),coalesce(sum(length(CAST(payload_json AS BLOB)) + 256),0)
        FROM delivery_outbox WHERE principal_authority=?1 AND principal_user_id=?2
        AND state IN ('pending','retry_wait','suspended_account')",
        params![owner.authority(), owner.external_user_id()], |r| Ok((r.get(0)?, r.get(1)?))
    )?;
    let (reserved_runs, summaries) = c.query_row(
        "SELECT count(*) FILTER (WHERE phase NOT IN ('settled','interrupted')),
         count(*) FILTER (WHERE compacted_at_ms IS NOT NULL)
         FROM runs WHERE principal_authority=?1 AND principal_user_id=?2",
        params![owner.authority(), owner.external_user_id()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(DatabaseUsage {
        bytes: pages.saturating_mul(page_size),
        pending_items,
        pending_bytes,
        reserved_runs,
        summaries,
    })
}
