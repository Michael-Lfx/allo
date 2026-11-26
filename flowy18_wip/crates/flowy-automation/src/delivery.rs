//! Delivery advances one outbox target at a time. A stable delivery key is the
//! inbox card identity and is also provided to the platform adapter.
use crate::store::{AutomationStore, OutboxRecord};
use crate::{
    AccessOperation, CallerContext, CapabilityRequest, Clock, DeliveryTarget,
    HostAccess, HostAccessDecision, HostAccessRequest, PortError, PortFuture,
};
use std::sync::Arc;

pub trait NotificationDelivery: Send + Sync {
    /// Recheck the account at submission, and use the delivery key for OS deduplication.
    fn submit<'a>(
        &'a self,
        caller: &'a CallerContext,
        item: &'a OutboxRecord,
    ) -> PortFuture<'a, Result<(), PortError>>;
}

pub struct DeliveryService {
    store: AutomationStore,
    clock: Arc<dyn Clock>,
    access: Arc<dyn HostAccess>,
    notifications: Option<Arc<dyn NotificationDelivery>>,
}

impl std::fmt::Debug for DeliveryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeliveryService").finish_non_exhaustive()
    }
}

impl DeliveryService {
    pub fn new(
        store: AutomationStore,
        clock: Arc<dyn Clock>,
        access: Arc<dyn HostAccess>,
        notifications: Option<Arc<dyn NotificationDelivery>>,
    ) -> Self {
        Self {
            store,
            clock,
            access,
            notifications,
        }
    }

    pub async fn pump(
        &self,
        caller: Option<&CallerContext>,
    ) -> Result<(), PortError> {
        self.store
            .suspend_delivery(
                caller.map(|c| c.principal.clone()),
                self.clock.now().utc_ms,
            )
            .await
            .map_err(|_| PortError::Storage)?;
        let Some(caller) = caller else {
            return Ok(());
        };
        for target in
            [DeliveryTarget::Inbox, DeliveryTarget::SystemNotification]
        {
            if target == DeliveryTarget::SystemNotification
                && self.notifications.is_none()
            {
                continue;
            }
            for _ in 0..32 {
                let Some(item) = self
                    .store
                    .claim_delivery(
                        caller.principal.clone(),
                        target,
                        self.clock.now().utc_ms,
                    )
                    .await
                    .map_err(|_| PortError::Storage)?
                else {
                    break;
                };
                let request = HostAccessRequest {
                    principal: caller.principal.clone(),
                    account_generation: caller.account_generation.clone(),
                    run_id: Some(item.run_id.clone()),
                    grant_ref: None,
                    operation: AccessOperation::DeliverResult,
                    requested: CapabilityRequest::default(),
                };
                let status = match self.access.check(&request).await {
                    Ok(HostAccessDecision::Allowed)
                        if target == DeliveryTarget::Inbox =>
                    {
                        Ok(())
                    }
                    Ok(HostAccessDecision::Allowed) => {
                        let port = self
                            .notifications
                            .as_ref()
                            .ok_or(PortError::Unavailable)?;
                        tokio::time::timeout(
                            std::time::Duration::from_secs(15),
                            port.submit(caller, &item),
                        )
                        .await
                        .unwrap_or(Err(PortError::Unavailable))
                    }
                    Ok(_) => Err(PortError::Forbidden),
                    Err(error) => Err(error),
                };
                self.store
                    .finish_delivery(item, status, self.clock.now().utc_ms)
                    .await
                    .map_err(|_| PortError::Storage)?;
                if status == Err(PortError::Forbidden) {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}
