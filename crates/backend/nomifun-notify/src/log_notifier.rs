use async_trait::async_trait;
use tracing::info;

use crate::{NotifyError, SystemNotification, SystemTaskNotifier};

/// Dev / fallback notifier: logs and never fails.
pub struct LogNotifier;

#[async_trait]
impl SystemTaskNotifier for LogNotifier {
    async fn notify(&self, message: &SystemNotification) -> Result<(), NotifyError> {
        info!(
            title = %message.title,
            status = %message.status.label_zh(),
            tag = %message.tag,
            task_id = %message.task_id,
            "system notification (log fallback)"
        );
        Ok(())
    }
}
