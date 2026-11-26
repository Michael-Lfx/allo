use super::AppHost;
use flowy_automation::delivery::NotificationDelivery;
use flowy_automation::store::OutboxRecord;
use flowy_automation::{CallerContext, PortError, PortFuture, RunId};
use flowy_i18n::{I18n, Locale};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tokio::sync::oneshot;

struct Submission {
    caller: CallerContext,
    run_id: RunId,
    receipt: oneshot::Sender<Result<(), PortError>>,
}

#[derive(Clone, Debug)]
pub(super) struct DesktopNotifications {
    queue: mpsc::SyncSender<Submission>,
    locale: Arc<Mutex<Locale>>,
}

impl NotificationDelivery for DesktopNotifications {
    fn submit<'a>(
        &'a self,
        caller: &'a CallerContext,
        item: &'a OutboxRecord,
    ) -> PortFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            if item.principal != caller.principal {
                return Err(PortError::Forbidden);
            }
            let (receipt, ack) = oneshot::channel();
            self.queue
                .try_send(Submission {
                    caller: caller.clone(),
                    run_id: item.run_id.clone(),
                    receipt,
                })
                .map_err(|_| PortError::Unavailable)?;
            ack.await.map_err(|_| PortError::Unavailable)?
        })
    }
}

impl AppHost {
    pub(super) fn notifications(
        &self,
    ) -> Option<Arc<dyn NotificationDelivery>> {
        self.notification_slot()
            .lock()
            .unwrap()
            .clone()
            .map(|port| -> Arc<dyn NotificationDelivery> { Arc::new(port) })
    }

    pub(super) fn set_notification_locale(&self, locale: Locale) {
        if let Some(port) = self.notification_slot().lock().unwrap().as_ref() {
            *port.locale.lock().unwrap() = locale;
        }
    }

    pub(super) fn install_automation_notifications(&self, cx: &gpui::App) {
        let (queue, pending) = mpsc::sync_channel::<Submission>(8);
        let locale = Arc::new(Mutex::new(
            crate::prefs::Preferences::load(self.root()).locale,
        ));
        *self.notification_slot().lock().unwrap() =
            Some(DesktopNotifications {
                queue,
                locale: locale.clone(),
            });
        let authority = self.automation_authority().clone();
        cx.spawn(async move |cx| {
            let mut i18n = I18n::embedded(crate::brand::product());
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                while let Ok(item) = pending.try_recv() {
                    if item.receipt.is_closed() {
                        continue;
                    }
                    i18n.set_locale(*locale.lock().unwrap());
                    let owner = (
                        &item.caller.principal,
                        &item.caller.account_generation,
                    );
                    let result = cx.update(|cx| {
                        if authority.account_binding().as_ref().map(
                            |(principal, generation)| (principal, generation),
                        ) != Some(owner)
                        {
                            return Err(PortError::Forbidden);
                        }
                        flowy_shell::show_automation_notification(
                            cx,
                            item.run_id.as_str(),
                            &i18n.t("auto.notification_title"),
                            &i18n.t("auto.notification_body"),
                            &i18n.t("auto.view_run"),
                        )
                        .then_some(())
                        .ok_or(PortError::InvalidRequest)
                    });
                    let _ = item.receipt.send(result);
                }
            }
        })
        .detach();
    }
}
