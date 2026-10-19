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
//!
//! Completion disables thinking and drains with the reasoning fallback,
//! matching the project's side-task JSON convention: `llm_chat` / `image_analyze` /
//! title / auxiliary completions all send `thinking=Disabled` — "side-task JSON
//! generation does not need chain-of-thought" — because thinking-mode models
//! (deepseek-v4-flash, Minimax-M3) spend the whole budget drafting in
//! `reasoning_content` and leave `content` empty (observed: 65 KB of "We need…"
//! prose, not one `{`). The reasoning fallback only kicks in when `content`
//! stays empty after disabling (providers that ignore `thinking=Disabled`, e.g.
//! step-*), so normal models are unaffected. `text_or_reasoning` WITHOUT
//! disabling is reserved for the Agent Execution planner, where reasoning-as-
//! answer is acceptable.

use std::path::PathBuf;
use std::sync::Arc;

use nomifun_common::AppError;
use nomifun_db::{IProviderModelRepository, IProviderRepository};
use nomifun_learning::LearningCompleter;

use crate::factory::provider_config::{
    one_shot_completion_json_text_or_reasoning, one_shot_completion_no_thinking_text_or_reasoning,
    one_shot_completion_text_or_reasoning, resolve_provider_config_with_output_limit, user_message,
};
use crate::knowledge_completer::resolve_default_model;

/// Provider-backed completer for the learning pipeline (course generation,
/// concept graphs, reflection grading, figure repair).
pub struct LiveLearningCompleter {
    pub provider_repo: Arc<dyn IProviderRepository>,
    pub provider_model_repo: Arc<dyn IProviderModelRepository>,
    pub encryption_key: [u8; 32],
    pub workspace: PathBuf,
}

impl LiveLearningCompleter {
    /// Resolve the explicit `(provider_id, model)` pick, or fall back to the
    /// enabled default. Shared by every channel.
    async fn resolve_model(
        &self,
        model_override: Option<(&str, &str)>,
    ) -> Result<(String, String), AppError> {
        match model_override {
            Some((provider_id, model)) => Ok((provider_id.to_owned(), model.to_owned())),
            None => resolve_default_model(&self.provider_repo, &self.provider_model_repo)
                .await
                .ok_or_else(|| {
                    AppError::Conflict(
                        "learning generation unavailable: no enabled provider/model is configured"
                            .into(),
                    )
                }),
        }
    }

    async fn config_for(
        &self,
        provider_id: &str,
        model: &str,
    ) -> Result<(nomi_config::config::Config, Option<u32>), AppError> {
        resolve_provider_config_with_output_limit(
            &self.provider_repo,
            &self.provider_model_repo,
            &self.encryption_key,
            provider_id,
            model,
            &self.workspace,
        )
        .await
    }

    /// The effective output ceiling for a generation call. The caller budgets
    /// (nomifun-learning's COACH/STRUCTURE/… constants) are fallbacks for
    /// models with no declared ceiling; when the catalog or the model row
    /// declares one, that number IS the model's maximum output and must win —
    /// reasoning tokens share the budget with the artifact, and a capped
    /// constant below thinking + artifact deterministically truncates
    /// (observed 2026-10-16: AIPC-deepseek-v4.1-flash hit MaxTokens on all 3
    /// structure rounds under 16384 while the catalog declares 384000;
    /// learnhub's pipeline, which sends no cap, completes the same artifact).
    fn effective_max_tokens(max_tokens: u32, output_limit: Option<u32>) -> u32 {
        output_limit.unwrap_or(max_tokens)
    }
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
        let (cfg, output_limit) = self.config_for(&provider_id, &model).await?;
        let max_tokens = Self::effective_max_tokens(max_tokens, output_limit);
        one_shot_completion_no_thinking_text_or_reasoning(
            &cfg,
            system,
            vec![user_message(user)],
            max_tokens,
        )
        .await
    }

    /// The learning-graph coach is the one judgment-heavy stage in the
    /// pipeline: picking entry domains and descending the prerequisite ladder
    /// is deliberation work, and a no-thinking flash model answers it with a
    /// shallow "chapter 1 of a textbook" lookup (observed 2026-09-29: four
    /// straight batches of ε-δ / 排列组合 entries against a zero baseline).
    /// Thinking stays ON here — mirroring learnhub's coach loop, whose call
    /// records show the domain survey happening in the reasoning channel —
    /// and the text-or-reasoning drain still recovers the answer when the
    /// provider leaves `content` empty. Callers raise the budget accordingly:
    /// reasoning tokens come out of the same `max_tokens` ceiling.
    async fn complete_with_thinking(
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
        let (cfg, output_limit) = self.config_for(&provider_id, &model).await?;
        let max_tokens = Self::effective_max_tokens(max_tokens, output_limit);
        // Transient provider blips (nginx 502 between us and the model) arrive
        // in ~2s and would otherwise kill a whole growth batch instantly — the
        // 2026-09-29 fresh-course run died that way on its first coach call.
        // One quick retry matches learnhub's "整路重跑恰一次" failure
        // semantics; real configuration errors (4xx) are not retried.
        let mut attempt = 1;
        loop {
            match one_shot_completion_text_or_reasoning(
                &cfg,
                system,
                vec![user_message(user)],
                max_tokens,
            )
            .await
            {
                Err(error @ AppError::BadGateway(_)) if attempt == 1 => {
                    attempt += 1;
                    tracing::warn!(
                        provider = %provider_id,
                        model = %model,
                        attempt,
                        %error,
                        "transient provider error on learning coach call — retrying once"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
                other => return other,
            }
        }
    }

    async fn complete_json(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let (provider_id, model) = self.resolve_model(model_override).await?;
        let (cfg, output_limit) = self.config_for(&provider_id, &model).await?;
        let max_tokens = Self::effective_max_tokens(max_tokens, output_limit);
        one_shot_completion_json_text_or_reasoning(
            &cfg,
            system,
            vec![user_message(user)],
            max_tokens,
            true,
            temperature,
        )
        .await
    }

    /// Artifact-contract calls (structure + terrain generation) deliberate
    /// before emitting their JSON, so thinking stays ON; the transient-retry
    /// semantics match [`complete_with_thinking`]. A provider without
    /// json_object support fails the call loudly — course creation surfaces
    /// the reason instead of silently degrading to text mode.
    async fn complete_json_with_thinking(
        &self,
        model_override: Option<(&str, &str)>,
        system: &str,
        user: &str,
        max_tokens: u32,
        temperature: Option<f32>,
    ) -> Result<String, AppError> {
        let (provider_id, model) = self.resolve_model(model_override).await?;
        let (cfg, output_limit) = self.config_for(&provider_id, &model).await?;
        let max_tokens = Self::effective_max_tokens(max_tokens, output_limit);
        let mut attempt = 1;
        loop {
            match one_shot_completion_json_text_or_reasoning(
                &cfg,
                system,
                vec![user_message(user)],
                max_tokens,
                false,
                temperature,
            )
            .await
            {
                Err(error @ AppError::BadGateway(_)) if attempt == 1 => {
                    attempt += 1;
                    tracing::warn!(
                        provider = %provider_id,
                        model = %model,
                        attempt,
                        %error,
                        "transient provider error on learning artifact call — retrying once"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                }
                other => return other,
            }
        }
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
