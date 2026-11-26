//! Storage admission and resumable removal of settled, unprotected results.
use crate::store::{AttemptRecord, AutomationStore, DeletionPlanRecord};
use crate::{
    AdmissionDecision, AdmissionPoint, AdmissionPolicy, AdmissionRequest,
    CapacityReason, PortError, PortFuture, PrincipalId,
};
use std::num::NonZeroU64;
use std::sync::Arc;

const MIB: u64 = 1024 * 1024;
const DAY_MS: u64 = 86_400_000;
pub(crate) const RESULT_RESERVE_BYTES: u64 = 64 * 1024;
pub(crate) const OUTBOX_RESERVE_BYTES: u64 = 8 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct RetentionLimits {
    pub history_age_ms: NonZeroU64,
    pub history_sessions: NonZeroU64,
    pub owner_history_bytes: NonZeroU64,
    pub shared_history_bytes: NonZeroU64,
    pub private_workspace_bytes: NonZeroU64,
    pub database_bytes: NonZeroU64,
    pub pending_outbox_items: NonZeroU64,
    pub pending_outbox_bytes: NonZeroU64,
    pub summary_age_ms: NonZeroU64,
    pub summary_items: NonZeroU64,
    pub receipt_age_ms: NonZeroU64,
}

impl Default for RetentionLimits {
    fn default() -> Self {
        Self {
            history_age_ms: NonZeroU64::new(30 * DAY_MS).unwrap(),
            history_sessions: NonZeroU64::new(500).unwrap(),
            owner_history_bytes: NonZeroU64::new(64 * MIB).unwrap(),
            shared_history_bytes: NonZeroU64::new(256 * MIB).unwrap(),
            private_workspace_bytes: NonZeroU64::new(1024 * MIB).unwrap(),
            database_bytes: NonZeroU64::new(256 * MIB).unwrap(),
            pending_outbox_items: NonZeroU64::new(10_000).unwrap(),
            pending_outbox_bytes: NonZeroU64::new(16 * MIB).unwrap(),
            summary_age_ms: NonZeroU64::new(90 * DAY_MS).unwrap(),
            summary_items: NonZeroU64::new(5_000).unwrap(),
            receipt_age_ms: NonZeroU64::new(7 * DAY_MS).unwrap(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RetentionUsage {
    pub sessions: u64,
    pub owner_history_bytes: u64,
    pub shared_history_bytes: u64,
    pub private_workspace_bytes: u64,
    pub artifact_headroom_bytes: u64,
    pub artifact_headroom_items: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DatabaseUsage {
    pub(crate) bytes: u64,
    pub(crate) pending_items: u64,
    pub(crate) pending_bytes: u64,
    pub(crate) reserved_runs: u64,
    pub(crate) summaries: u64,
}

impl DatabaseUsage {
    pub(crate) const fn admits(
        self,
        limits: RetentionLimits,
        additional: u64,
    ) -> bool {
        let reserved = self.reserved_runs.saturating_add(additional);
        self.bytes
            .saturating_add(reserved.saturating_mul(RESULT_RESERVE_BYTES))
            <= limits.database_bytes.get()
            && self
                .pending_items
                .saturating_add(reserved.saturating_mul(2))
                <= limits.pending_outbox_items.get()
            && self
                .pending_bytes
                .saturating_add(reserved.saturating_mul(OUTBOX_RESERVE_BYTES))
                <= limits.pending_outbox_bytes.get()
            && self.summaries <= limits.summary_items.get()
    }
}

pub trait RetentionHost: Send + Sync {
    fn usage<'a>(
        &'a self,
        owner: &'a PrincipalId,
        attempts: Vec<AttemptRecord>,
    ) -> PortFuture<'a, Result<RetentionUsage, PortError>>;
    /// Recheck trusted repository bindings and every local protection.
    fn protected<'a>(
        &'a self,
        plan: &'a DeletionPlanRecord,
        attempts: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<bool, PortError>>;
    /// Idempotent. An occupied or untrusted path returns an error and stays charged.
    fn remove<'a>(
        &'a self,
        plan: &'a DeletionPlanRecord,
        attempts: &'a [AttemptRecord],
    ) -> PortFuture<'a, Result<(), PortError>>;
}

#[derive(Clone)]
pub struct RetentionService {
    store: AutomationStore,
    host: Arc<dyn RetentionHost>,
    limits: RetentionLimits,
}

impl std::fmt::Debug for RetentionService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetentionService")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl RetentionService {
    pub fn new(
        store: AutomationStore,
        host: Arc<dyn RetentionHost>,
        limits: RetentionLimits,
    ) -> Self {
        store.install_retention_limits(limits);
        Self {
            store,
            host,
            limits,
        }
    }

    async fn usage(
        &self,
        principal: &PrincipalId,
    ) -> Result<RetentionUsage, PortError> {
        let attempts = self
            .store
            .retention_attempts(principal.clone())
            .await
            .map_err(|_| PortError::Storage)?;
        self.host.usage(principal, attempts).await
    }

    pub async fn maintain(
        &self,
        principal: &PrincipalId,
        now: i64,
    ) -> Result<(), PortError> {
        let now = self
            .store
            .observe_command_time(principal.clone(), now)
            .await
            .map_err(|_| PortError::Storage)?;
        self.store
            .prune_retention_metadata(principal.clone(), now, self.limits)
            .await
            .map_err(|_| PortError::Storage)?;
        let usage = self.usage(principal).await?;
        let db = self
            .store
            .retention_db_usage(principal.clone())
            .await
            .map_err(|_| PortError::Storage)?;
        let pressure = self.over_history(usage, 1)
            || db.bytes.saturating_add(RESULT_RESERVE_BYTES)
                >= self.limits.database_bytes.get();
        let candidates = self
            .store
            .retention_candidates(
                principal.clone(),
                cutoff(now, self.limits.history_age_ms),
                pressure,
            )
            .await
            .map_err(|_| PortError::Storage)?;
        let mut deferred_error = None;
        for (plan, attempts) in candidates {
            if self.host.protected(&plan, &attempts).await? {
                continue;
            }
            if let Some(plan) = self
                .store
                .commit_retention_plan(plan, now)
                .await
                .map_err(|_| PortError::Storage)?
            {
                match self.host.remove(&plan, &attempts).await {
                    Ok(()) => {
                        if let Err(error) = self
                            .store
                            .finish_retention_plan(plan, now)
                            .await
                            .map_err(|_| PortError::Storage)
                        {
                            deferred_error.get_or_insert(error);
                        }
                        // Re-measure before selecting another candidate.
                        break;
                    }
                    Err(error) => {
                        // A locked/corrupt source must not starve later,
                        // independently removable runs.
                        deferred_error.get_or_insert(error);
                    }
                }
            }
        }
        deferred_error.map_or(Ok(()), Err)
    }

    const fn over_history(
        &self,
        usage: RetentionUsage,
        additional: u64,
    ) -> bool {
        usage.sessions.saturating_add(additional)
            > self.limits.history_sessions.get()
            || usage
                .owner_history_bytes
                .saturating_add(RESULT_RESERVE_BYTES)
                > self.limits.owner_history_bytes.get()
            || usage
                .shared_history_bytes
                .saturating_add(RESULT_RESERVE_BYTES)
                > self.limits.shared_history_bytes.get()
            || usage.private_workspace_bytes
                >= self.limits.private_workspace_bytes.get()
    }
}

impl AdmissionPolicy for RetentionService {
    fn check<'a>(
        &'a self,
        request: &'a AdmissionRequest,
    ) -> PortFuture<'a, Result<AdmissionDecision, PortError>> {
        Box::pin(async move {
            let (Ok(usage), Ok(db)) = (
                self.usage(&request.principal).await,
                self.store
                    .retention_db_usage(request.principal.clone())
                    .await,
            ) else {
                return Ok(AdmissionDecision::SuspendedStorage(
                    CapacityReason::MeasurementUnavailable,
                ));
            };
            let new = u64::from(request.point == AdmissionPoint::CreateRun);
            let reserved = db.reserved_runs.saturating_add(new);
            if self.over_history(usage, reserved.max(1))
                || !db.admits(self.limits, new)
                || usage.artifact_headroom_items == 0
                || usage.artifact_headroom_bytes < RESULT_RESERVE_BYTES
            {
                return Ok(AdmissionDecision::SuspendedStorage(
                    CapacityReason::QuotaExceeded,
                ));
            }
            Ok(AdmissionDecision::Allowed)
        })
    }
}

pub(crate) fn cutoff(now: i64, age: NonZeroU64) -> i64 {
    now.saturating_sub(i64::try_from(age.get()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_limits_are_positive_and_database_reserves_hold_at_the_boundary()
     {
        let limits = RetentionLimits::default();
        assert!(limits.history_age_ms.get() > 0);
        assert!(limits.history_sessions.get() > 0);
        assert!(limits.owner_history_bytes.get() > 0);
        assert!(limits.shared_history_bytes.get() > 0);
        assert!(limits.private_workspace_bytes.get() > 0);
        assert!(limits.database_bytes.get() > RESULT_RESERVE_BYTES);
        assert!(limits.pending_outbox_items.get() >= 2);
        assert!(limits.pending_outbox_bytes.get() > OUTBOX_RESERVE_BYTES);
        assert!(limits.summary_age_ms.get() > 0);
        assert!(limits.summary_items.get() > 0);
        assert!(limits.receipt_age_ms.get() > 0);

        let empty = DatabaseUsage {
            bytes: 0,
            pending_items: 0,
            pending_bytes: 0,
            reserved_runs: 0,
            summaries: 0,
        };
        assert!(empty.admits(limits, 0));
        assert!(
            DatabaseUsage {
                bytes: limits.database_bytes.get() - RESULT_RESERVE_BYTES,
                ..empty
            }
            .admits(limits, 1)
        );
        assert!(
            !DatabaseUsage {
                bytes: limits.database_bytes.get() - RESULT_RESERVE_BYTES,
                ..empty
            }
            .admits(limits, 2)
        );
        assert!(
            DatabaseUsage {
                pending_items: limits.pending_outbox_items.get() - 2,
                ..empty
            }
            .admits(limits, 1)
        );
        assert!(
            !DatabaseUsage {
                pending_bytes: limits.pending_outbox_bytes.get()
                    - OUTBOX_RESERVE_BYTES,
                ..empty
            }
            .admits(limits, 2)
        );
        assert!(
            !DatabaseUsage {
                summaries: limits.summary_items.get() + 1,
                ..empty
            }
            .admits(limits, 0)
        );
    }

    #[test]
    fn retention_cutoff_saturates_instead_of_wrapping() {
        assert_eq!(cutoff(i64::MIN, NonZeroU64::new(1).unwrap()), i64::MIN);
        assert_eq!(
            cutoff(10, NonZeroU64::new(u64::MAX).unwrap()),
            10_i64.saturating_sub(i64::MAX)
        );
    }
}
