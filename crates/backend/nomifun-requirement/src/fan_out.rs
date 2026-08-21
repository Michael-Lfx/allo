use std::sync::Arc;

use async_trait::async_trait;
use nomifun_db::models::RequirementRow;

use crate::CompletionNotifier;

/// Fans a single completion event out to multiple notifiers (webhook + system, …).
///
/// Each child is invoked sequentially; implementations are expected to be
/// best-effort and non-blocking (the service already detaches this call).
pub struct FanOutCompletionNotifier {
    notifiers: Vec<Arc<dyn CompletionNotifier>>,
}

impl FanOutCompletionNotifier {
    pub fn new(notifiers: Vec<Arc<dyn CompletionNotifier>>) -> Self {
        Self { notifiers }
    }

    pub fn into_arc(self) -> Arc<dyn CompletionNotifier> {
        Arc::new(self)
    }
}

#[async_trait]
impl CompletionNotifier for FanOutCompletionNotifier {
    async fn notify_completion(&self, requirement: &RequirementRow) {
        for notifier in &self.notifiers {
            notifier.notify_completion(requirement).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recording {
        count: Mutex<usize>,
    }

    #[async_trait]
    impl CompletionNotifier for Recording {
        async fn notify_completion(&self, _requirement: &RequirementRow) {
            *self.count.lock().unwrap() += 1;
        }
    }

    #[tokio::test]
    async fn fans_out_to_all_children() {
        let a = Arc::new(Recording::default());
        let b = Arc::new(Recording::default());
        let fan = FanOutCompletionNotifier::new(vec![a.clone(), b.clone()]);
        let row = RequirementRow {
            id: 1,
            requirement_id: "r1".into(),
            display_no: 1,
            title: "t".into(),
            content: "c".into(),
            tag: "tag".into(),
            order_key: "1".into(),
            sort_seq: "1".into(),
            status: "done".into(),
            priority: 0,
            completion_note: None,
            owner_conversation_id: None,
            owner_terminal_id: None,
            active_turn_started_at: None,
            lease_expires_at: None,
            started_at: None,
            completed_at: None,
            attempt_count: 0,
            claim_generation: 0,
            claim_token: None,
            created_by: "u".into(),
            extra: "{}".into(),
            created_at: 0,
            updated_at: 0,
        };
        fan.notify_completion(&row).await;
        assert_eq!(*a.count.lock().unwrap(), 1);
        assert_eq!(*b.count.lock().unwrap(), 1);
    }
}
