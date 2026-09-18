//! Adapts [`SystemTaskNotifier`] onto [`CompletionNotifier`] so system toasts
//! ride the same terminal-status fire points as webhooks.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_db::models::RequirementRow;
use nomifun_db::ISettingsRepository;
use nomifun_requirement::CompletionNotifier;
use tracing::warn;

use crate::{NotificationStatus, SystemNotification, SystemTaskNotifier};

/// Bridges requirement terminal transitions to a [`SystemTaskNotifier`].
pub struct SystemCompletionNotifier {
    inner: Arc<dyn SystemTaskNotifier>,
    settings_repo: Arc<dyn ISettingsRepository>,
}

impl SystemCompletionNotifier {
    pub fn new(
        inner: Arc<dyn SystemTaskNotifier>,
        settings_repo: Arc<dyn ISettingsRepository>,
    ) -> Self {
        Self {
            inner,
            settings_repo,
        }
    }

    pub fn into_arc(self) -> Arc<dyn CompletionNotifier> {
        Arc::new(self)
    }
}

fn build_notification(requirement: &RequirementRow) -> Option<SystemNotification> {
    let status = NotificationStatus::from_db(&requirement.status)?;
    let route = format!(
        "/requirements?tag={}&id={}",
        urlencoding_lite(&requirement.tag),
        urlencoding_lite(&requirement.requirement_id),
    );
    let click_target = Some(format!(
        "flowy://navigate?route={}",
        urlencoding_lite(&route),
    ));
    Some(SystemNotification {
        title: requirement.title.clone(),
        status,
        body: requirement.completion_note.clone(),
        tag: requirement.tag.clone(),
        task_id: requirement.requirement_id.clone(),
        click_target,
        source: "requirement".to_owned(),
    })
}

/// Minimal query-value escaping (no extra crate).
fn urlencoding_lite(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[async_trait]
impl CompletionNotifier for SystemCompletionNotifier {
    async fn notify_completion(&self, requirement: &RequirementRow) {
        match self.settings_repo.get_settings().await {
            Ok(Some(settings)) if !settings.notification_enabled => return,
            Ok(_) => {}
            Err(error) => {
                warn!(
                    error = %error,
                    requirement_id = %requirement.requirement_id,
                    "failed to read notification setting; keeping notifications enabled"
                );
            }
        }

        let Some(message) = build_notification(requirement) else {
            return;
        };
        if let Err(error) = self.inner.notify(&message).await {
            warn!(
                requirement_id = %requirement.requirement_id,
                error = %error,
                "system notification delivery failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LogNotifier, NotifyError};
    use nomifun_db::models::SystemSettings;
    use nomifun_db::{DbError, ISettingsRepository};
    use std::sync::Mutex;

    struct TestSettings {
        notification_enabled: bool,
        fail: bool,
    }

    #[async_trait]
    impl ISettingsRepository for TestSettings {
        async fn get_settings(&self) -> Result<Option<SystemSettings>, DbError> {
            if self.fail {
                return Err(DbError::Init("settings unavailable".into()));
            }
            Ok(Some(SystemSettings {
                id: 1,
                singleton_key: "default".into(),
                language: "zh-CN".into(),
                notification_enabled: self.notification_enabled,
                cron_notification_enabled: false,
                command_queue_enabled: true,
                save_upload_to_workspace: false,
                updated_at: 0,
            }))
        }

        async fn upsert_settings(
            &self,
            _language: &str,
            _notification_enabled: bool,
            _cron_notification_enabled: bool,
            _command_queue_enabled: bool,
            _save_upload_to_workspace: bool,
        ) -> Result<SystemSettings, DbError> {
            unimplemented!("test settings are read-only")
        }
    }

    fn enabled_settings() -> Arc<dyn ISettingsRepository> {
        Arc::new(TestSettings {
            notification_enabled: true,
            fail: false,
        })
    }

    fn row(status: &str) -> RequirementRow {
        RequirementRow {
            id: 1,
            requirement_id: "0190f5fe-7c00-7a00-8000-000000000001".into(),
            display_no: 1,
            title: "优化查询".into(),
            content: "body".into(),
            tag: "dev/ops".into(),
            order_key: "1".into(),
            sort_seq: "1".into(),
            status: status.into(),
            priority: 0,
            completion_note: Some("note".into()),
            owner_conversation_id: None,
            owner_terminal_id: None,
            active_turn_started_at: None,
            lease_expires_at: None,
            started_at: None,
            completed_at: None,
            attempt_count: 0,
            claim_generation: 0,
            claim_token: None,
            created_by: "user".into(),
            extra: "{}".into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[derive(Default)]
    struct Recording {
        titles: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SystemTaskNotifier for Recording {
        async fn notify(&self, message: &SystemNotification) -> Result<(), NotifyError> {
            self.titles.lock().unwrap().push(message.title.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn maps_done_title_and_status() {
        let recorder = Arc::new(Recording::default());
        let notifier = SystemCompletionNotifier::new(recorder.clone(), enabled_settings());
        notifier.notify_completion(&row("done")).await;
        assert_eq!(recorder.titles.lock().unwrap().as_slice(), ["优化查询"]);
        let built = build_notification(&row("done")).unwrap();
        assert_eq!(built.status, NotificationStatus::Done);
        assert_eq!(built.status.label_zh(), "已完成");
        assert!(
            built.click_target.as_deref().unwrap().starts_with("flowy://navigate?route=")
        );
        let route = built
            .click_target
            .as_deref()
            .and_then(|t| t.strip_prefix("flowy://navigate?route="))
            .map(|encoded| {
                // one layer of percent-decoding for the route query value
                let mut out = String::new();
                let bytes = encoded.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] == b'%' && i + 2 < bytes.len() {
                        let h = u8::from_str_radix(
                            std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                            16,
                        );
                        if let Ok(b) = h {
                            out.push(b as char);
                            i += 3;
                            continue;
                        }
                    }
                    out.push(bytes[i] as char);
                    i += 1;
                }
                out
            })
            .unwrap_or_default();
        assert!(route.starts_with("/requirements?"));
        assert!(route.contains("tag=dev%2Fops"));
    }

    #[tokio::test]
    async fn skips_non_terminal_status() {
        let recorder = Arc::new(Recording::default());
        let notifier = SystemCompletionNotifier::new(recorder.clone(), enabled_settings());
        notifier.notify_completion(&row("pending")).await;
        assert!(recorder.titles.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn log_notifier_never_fails() {
        let notifier = SystemCompletionNotifier::new(Arc::new(LogNotifier), enabled_settings());
        notifier.notify_completion(&row("failed")).await;
    }

    #[tokio::test]
    async fn disabled_system_notifications_do_not_reach_the_sink() {
        let recorder = Arc::new(Recording::default());
        let notifier = SystemCompletionNotifier::new(
            recorder.clone(),
            Arc::new(TestSettings {
                notification_enabled: false,
                fail: false,
            }),
        );
        notifier.notify_completion(&row("done")).await;
        assert!(recorder.titles.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn settings_read_failure_preserves_enabled_default() {
        let recorder = Arc::new(Recording::default());
        let notifier = SystemCompletionNotifier::new(
            recorder.clone(),
            Arc::new(TestSettings {
                notification_enabled: true,
                fail: true,
            }),
        );
        notifier.notify_completion(&row("done")).await;
        assert_eq!(recorder.titles.lock().unwrap().as_slice(), ["优化查询"]);
    }
}
