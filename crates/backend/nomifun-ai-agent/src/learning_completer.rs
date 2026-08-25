//! Production [`LearningCompleter`]: resolves a default provider/model and
//! runs a one-shot completion. Same layering as `LiveKnowledgeCompleter` —
//! the learning crate holds only the trait, this crate provides the
//! provider-backed implementation, and the app layer wires it via
//! `LearningService::set_generation_dependencies`.
//!
//! Unlike the knowledge completer, every call carries an explicit
//! `max_tokens` budget: the learning pipeline's stages differ wildly in
//! output size (a tiny grading JSON vs. a whole concept graph in one reply),
//! so each stage passes the budget it needs instead of a fixed cap.

use std::path::PathBuf;
use std::sync::Arc;

use nomifun_common::AppError;
use nomifun_db::{IProviderModelRepository, IProviderRepository};
use nomifun_learning::LearningCompleter;

use crate::factory::provider_config::{one_shot_completion, resolve_provider_config, user_message};
use crate::knowledge_completer::resolve_default_model;

/// Provider-backed completer for the learning pipeline (course generation,
/// concept graphs, reflection grading, figure repair).
pub struct LiveLearningCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

#[async_trait::async_trait]
impl LearningCompleter for LiveLearningCompleter {
    async fn complete(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<String, AppError> {
        let (provider_id, model) = match model_override {
            Some((provider_id, model)) => (provider_id.to_owned(), model.to_owned()),
            None => resolve_default_model(&self.provider_repo, &self.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "learning generation unavailable: no enabled provider/model is configured"
                            .into(),
                    )
                })?,
        };
        let cfg = resolve_provider_config(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            &provider_id,
            &model,
            &self.workspace,
        )
        .await?;
        one_shot_completion(&cfg, system, vec![user_message(user)], max_tokens).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_completer::tests::{ListOnlyModelRepo, ListOnlyRepo, model_row, provider};

    fn completer(
        providers: Vec<nomifun_db::models::Provider>,
        rows: Vec<nomifun_db::ProviderModelRow>,
    ) -> LiveLearningCompleter {
        LiveLearningCompleter {
            provider_repo: Arc::new(ListOnlyRepo(providers)),
            provider_model_repo: Arc::new(ListOnlyModelRepo(rows)),
            encryption_key: [0u8; 32],
            workspace: std::env::temp_dir(),
        }
    }

    /// An explicit override must drive resolution — never the enabled
    /// default. Pinned at the network-free resolution boundary: an absent
    /// provider id surfaces as `BadRequest("Provider '…' not found")`.
    #[tokio::test]
    async fn complete_with_override_resolves_the_explicit_provider_not_the_default() {
        // `p1` is enabled, so the default path would pick it; `px` is absent.
        let c = completer(
            vec![provider("p1", true)],
            vec![model_row("p1", "m1", true, 0)],
        );
        let err = c
            .complete(Some(("px", "model-z")), "sys", "usr", 4096)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err:?}");
        assert!(err.to_string().contains("Provider 'px' not found"), "{err}");
    }

    /// Without an override the default pick must be used: with the enabled
    /// default `p1` present it gets PAST provider resolution, so the failure
    /// is NOT "Provider not found" (no network in tests — the failure comes
    /// later, from building/calling the provider).
    #[tokio::test]
    async fn complete_without_override_uses_the_default_provider() {
        let c = completer(
            vec![provider("p1", true)],
            vec![model_row("p1", "m1", true, 0)],
        );
        let err = c.complete(None, "sys", "usr", 4096).await.unwrap_err();
        assert!(
            !err.to_string().contains("not found"),
            "default path must resolve the existing provider p1, got: {err}"
        );
    }

    /// No enabled provider/model at all → a truthful conflict, not a
    /// pretend-default call.
    #[tokio::test]
    async fn complete_without_configuration_errors_clearly() {
        let c = completer(
            vec![provider("p1", false)],
            vec![model_row("p1", "m1", true, 0)],
        );
        let err = c.complete(None, "sys", "usr", 4096).await.unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
        assert!(err.to_string().contains("no enabled provider"), "{err}");
    }
}
