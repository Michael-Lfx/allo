use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::{NotifyError, SystemNotification, SystemTaskNotifier};

/// Late-bound [`SystemTaskNotifier`]: empty until a desktop/web host attaches.
///
/// `AppServices` constructs the slot at boot; the Tauri shell attaches a
/// `DesktopTauriNotifier` once `AppHandle` exists. Web/headless hosts leave it
/// empty so requirement completion stays silent at the OS layer.
#[derive(Default)]
pub struct SystemNotifierSlot {
    inner: RwLock<Option<Arc<dyn SystemTaskNotifier>>>,
}

impl SystemNotifierSlot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach(&self, notifier: Arc<dyn SystemTaskNotifier>) {
        match self.inner.write() {
            Ok(mut guard) => *guard = Some(notifier),
            Err(poisoned) => *poisoned.into_inner() = Some(notifier),
        }
    }

    pub fn is_attached(&self) -> bool {
        self.inner
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or_else(|poisoned| poisoned.into_inner().is_some())
    }
}

#[async_trait]
impl SystemTaskNotifier for SystemNotifierSlot {
    async fn notify(&self, message: &SystemNotification) -> Result<(), NotifyError> {
        let notifier = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        match notifier {
            Some(n) => n.notify(message).await,
            None => Ok(()),
        }
    }
}
