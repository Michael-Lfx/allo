use super::{AutomationStore, OutboxRecord, StoreError, repository as repo};
use crate::{DeliveryTarget, PortError, PrincipalId};
use rusqlite::params;

impl AutomationStore {
    pub async fn inbox(
        &self,
        p: PrincipalId,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        self.call(move |c| inbox(c, &p)).await
    }

    pub async fn suspend_delivery(
        &self,
        active: Option<PrincipalId>,
        now: i64,
    ) -> Result<(), StoreError> {
        self.call(move |c| {
            c.execute(
                "UPDATE delivery_outbox SET state='suspended_account',claim_until_ms=NULL,updated_at_ms=?1
                 WHERE state IN ('pending','retry_wait') AND (?2 IS NULL OR principal_authority!=?2 OR principal_user_id!=?3)",
                params![now,active.as_ref().map(PrincipalId::authority),active.as_ref().map(PrincipalId::external_user_id)]
            )?;
            Ok(())
        }).await
    }

    pub async fn claim_delivery(
        &self,
        p: PrincipalId,
        target: DeliveryTarget,
        now: i64,
    ) -> Result<Option<OutboxRecord>, StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let item = {
                let mut stmt = tx.prepare(
                    "SELECT * FROM delivery_outbox WHERE principal_authority=?1 AND principal_user_id=?2 AND target=?3
                     AND state IN ('pending','retry_wait','suspended_account') AND (next_attempt_at_ms<=?4 OR updated_at_ms>?4 OR state='suspended_account')
                     AND (claim_until_ms IS NULL OR claim_until_ms<=?4 OR updated_at_ms>?4)
                     ORDER BY next_attempt_at_ms, delivery_key LIMIT 1"
                )?;
                let mut rows = stmt.query(params![p.authority(),p.external_user_id(),target.as_str(),now])?;
                rows.next()?.map(repo::read_outbox).transpose()?
            };
            let Some(mut item) = item else { return Ok(None); };
            item.attempt_count = item.attempt_count.checked_add(1).ok_or(StoreError::Corrupt("delivery attempts exhausted"))?;
            tx.execute("UPDATE delivery_outbox SET claim_until_ms=?1,attempt_count=?2,updated_at_ms=?3 WHERE delivery_key=?4",
                params![now.saturating_add(30_000),item.attempt_count,now,item.delivery_key])?;
            tx.commit()?;
            Ok(Some(item))
        }).await
    }

    pub async fn finish_delivery(
        &self,
        item: OutboxRecord,
        status: Result<(), PortError>,
        now: i64,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let (state, error) = match status {
                Ok(()) => ("delivered", None),
                Err(PortError::Forbidden) => ("suspended_account", Some("account_unavailable")),
                Err(PortError::Storage) => ("retry_wait", Some("storage_error")),
                Err(_) => ("retry_wait", Some("delivery_unavailable")),
            };
            let delay = 1_000_i64 << item.attempt_count.min(10);
            let changed = tx.execute(
                "UPDATE delivery_outbox SET state=?1,last_error=?2,next_attempt_at_ms=?3,updated_at_ms=?4,claim_until_ms=NULL
                 WHERE delivery_key=?5 AND principal_authority=?6 AND principal_user_id=?7 AND attempt_count=?8 AND claim_until_ms IS NOT NULL",
                params![state,error,now.saturating_add(delay),now,item.delivery_key,item.principal.authority(),item.principal.external_user_id(),item.attempt_count],
            )? == 1;
            super::runs::finish_transition(tx, &item.principal, now, changed)
        }).await
    }

    pub async fn reset_delivery_claims(&self) -> Result<(), StoreError> {
        self.call(|c| {
            c.execute("UPDATE delivery_outbox SET claim_until_ms=NULL WHERE claim_until_ms IS NOT NULL", [])?;
            Ok(())
        }).await
    }
}

pub(super) fn inbox(
    c: &rusqlite::Connection,
    p: &PrincipalId,
) -> Result<Vec<OutboxRecord>, StoreError> {
    let mut query = c.prepare(
        "SELECT * FROM delivery_outbox WHERE principal_authority=?1 AND principal_user_id=?2 AND target='inbox' AND state='delivered' ORDER BY created_at_ms DESC, delivery_key LIMIT 500"
    )?;
    let mut rows = query.query(params![p.authority(), p.external_user_id()])?;
    let mut cards = Vec::new();
    while let Some(row) = rows.next()? {
        cards.push(repo::read_outbox(row)?);
    }
    Ok(cards)
}

pub(super) fn statuses(
    c: &rusqlite::Connection,
    p: &PrincipalId,
    run: &crate::RunId,
) -> Result<Vec<crate::DeliverySnapshot>, StoreError> {
    let mut statement = c.prepare(
        "SELECT target,state,attempt_count,next_attempt_at_ms,last_error FROM delivery_outbox
         WHERE principal_authority=?1 AND principal_user_id=?2 AND run_id=?3 ORDER BY target"
    )?;
    let mut rows = statement.query(params![
        p.authority(),
        p.external_user_id(),
        run.as_str()
    ])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let state =
            serde_json::from_value(serde_json::Value::String(row.get(1)?))?;
        result.push(crate::DeliverySnapshot {
            target: row.get(0)?,
            state,
            attempt_count: row.get(2)?,
            next_attempt_at_ms: row.get::<_, Option<i64>>(3)?.filter(|_| {
                matches!(
                    state,
                    crate::OutboxState::Pending | crate::OutboxState::RetryWait
                )
            }),
            last_error: row.get(4)?,
        });
    }
    Ok(result)
}
