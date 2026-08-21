use async_trait::async_trait;
use nomifun_db::models::RequirementRow;

/// Notified after a requirement reaches a terminal state (done|failed|…).
/// Implementations MUST be cheap / non-blocking (the caller spawns this).
///
/// Multiple sinks (webhook, OS system toast, …) are composed with
/// [`crate::FanOutCompletionNotifier`] at the assembly root — this trait stays
/// a single fire point inside `RequirementService`.
#[async_trait]
pub trait CompletionNotifier: Send + Sync {
    async fn notify_completion(&self, requirement: &RequirementRow);
}
