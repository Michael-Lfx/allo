//! LLM seam for the learning generation pipeline — the learning module's own
//! completer trait, mirroring `KnowledgeCompleter` (nomifun-knowledge) and
//! `CompanionCompleter` (nomifun-companion). The learning crate holds only the
//! trait; provider/model resolution and the actual completion call are the
//! implementor's concern (production: `LiveLearningCompleter` in
//! nomifun-ai-agent).
//!
//! Unlike `KnowledgeCompleter`, the signature carries an explicit output
//! budget (`max_tokens`): the learning pipeline's calls differ wildly in
//! output size — a tiny grading JSON vs. a whole concept graph in one reply —
//! so every stage passes the budget it actually needs instead of a
//! one-size-fits-all cap.

use nomifun_common::AppError;

#[async_trait::async_trait]
pub trait LearningCompleter: Send + Sync {
    /// One completion call. `model_override` is the caller's explicit
    /// `(provider_id, model)` pick when the user chose one (the two are
    /// always provided together, enforced at the HTTP layer); `None` leaves
    /// the model choice to the implementor's default.
    async fn complete(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError>;

    /// Judgment-heavy planning call (learning-graph coach): thinking stays ON
    /// so the model can actually deliberate (survey entry domains, descend the
    /// prerequisite ladder) before emitting the JSON contract; the drain still
    /// prefers `content` and only falls back to the reasoning channel when the
    /// provider leaves it empty. The default delegates to [`complete`] so test
    /// doubles and conservative implementors keep working.
    async fn complete_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        self.complete(model_override, system, user, max_tokens).await
    }
}
