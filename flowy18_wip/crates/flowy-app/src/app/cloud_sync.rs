//! Cloud model catalog, entitlement tier routing, and credit balance polling.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flowy_auth::{AuthClient, AuthError};
use gpui::Context;

use super::FlowyApp;

fn cloud_model_config(
    model: &flowy_agent::CatalogModel,
    token: &str,
) -> flowy_agent::LlmConfig {
    flowy_agent::LlmConfig {
        api_key: (!token.trim().is_empty()).then(|| token.to_owned()),
        base_url: model.endpoint.clone(),
        model: model.id.clone(),
        provider: "flowy".into(),
        context_window: model.context_window,
        max_tokens: model.max_tokens,
    }
}

/// Refresh is single-flight: one request at a time, for a signed-in session
/// that is not running a turn.
pub(super) const fn catalog_refresh_allowed(
    loading: bool,
    has_session: bool,
    busy: bool,
) -> bool {
    !loading && has_session && !busy
}

impl FlowyApp {
    pub(super) fn disconnected_config() -> flowy_agent::LlmConfig {
        flowy_agent::LlmConfig {
            api_key: None,
            base_url: String::new(),
            model: String::new(),
            provider: "flowy".into(),
            ..flowy_agent::LlmConfig::default()
        }
    }

    const fn plan_for_effective_tier(tier: u8) -> flowy_agent::Plan {
        flowy_agent::Plan::from_effective_tier(tier)
    }

    /// Builds the three tier rows from the current catalog and entitlement.
    /// Raw model ids stay in this layer; the menu only receives display data.
    fn tier_views(
        &self,
        cx: &Context<'_, Self>,
    ) -> Vec<flowy_studio::TierView> {
        let models = &self.studio.read(cx).catalog.models;
        flowy_agent::CLOUD_TIERS
            .iter()
            .map(|spec| {
                let resolved =
                    match flowy_agent::resolve_tier(spec.tier, models) {
                        flowy_agent::TierResolution::Available(model) => {
                            Some(model)
                        }
                        flowy_agent::TierResolution::Unavailable => None,
                    };
                let unavailable = resolved.is_none();
                let locked = flowy_agent::is_locked(
                    spec.tier,
                    self.effective_tier,
                    resolved,
                );
                flowy_studio::TierView {
                    tier: spec.tier,
                    pro_badge: spec.pro_badge,
                    locked,
                    unavailable,
                    selected: self.prefs.model == spec.tier,
                }
            })
            .collect()
    }

    pub(super) fn publish_tier_state(&self, cx: &mut Context<'_, Self>) {
        let views = self.tier_views(cx);
        let plan = Self::plan_for_effective_tier(self.effective_tier);
        self.studio.update(cx, |panel, cx| {
            panel.set_tiers(views, cx);
            panel.plan = plan;
            panel.model = self.prefs.model;
            cx.notify();
        });
    }

    /// Applies one cloud tier. This is the single place that turns a tier into
    /// an agent configuration; every entry (menu, boot, catalog refresh) goes
    /// through it.
    pub(super) fn select_tier(
        &mut self,
        tier: flowy_agent::Model,
        cx: &mut Context<'_, Self>,
    ) {
        if !tier.is_cloud() {
            // Flex BYOK is deferred; keep the option documented but inert.
            self.note("notice.model_unavailable", cx);
            return;
        }
        if self.catalog_loading {
            return;
        }
        let Some(session) = self.auth.session.clone() else {
            return;
        };
        if session.access_token.trim().is_empty()
            || self.studio.read(cx).agent.turn_status().is_busy()
        {
            return;
        }
        let models = self.studio.read(cx).catalog.models.clone();
        let flowy_agent::TierResolution::Available(model) =
            flowy_agent::resolve_tier(tier, &models)
        else {
            // Never move the selected tier onto a model that cannot be
            // routed: the chip would claim a tier the agent is not using.
            self.publish_tier_state(cx);
            self.note("notice.model_unavailable", cx);
            return;
        };
        let model = model.clone();
        if flowy_agent::is_locked(tier, self.effective_tier, Some(&model)) {
            // Keep the current route; the locked row is a discovery surface.
            self.publish_tier_state(cx);
            self.note("notice.upgrade_later", cx);
            return;
        }
        let config = cloud_model_config(&model, &session.access_token);
        match self.studio.read(cx).agent.apply_config(config) {
            Ok(()) => {
                self.applied_tier = Some(tier);
                self.prefs.model = tier;
                self.prefs.model_id.clone_from(&model.id);
                let save_failed =
                    self.prefs.save_checked(self.store.root()).is_err();
                let selected_id = model.id.clone();
                self.studio.update(cx, |panel, cx| {
                    panel.catalog.select(selected_id);
                    panel.model = tier;
                    panel.set_notice(
                        save_failed.then_some("notice.session_storage_failed"),
                        cx,
                    );
                    panel.notify_cloud_ready(cx);
                });
                self.publish_tier_state(cx);
            }
            Err(key) => {
                self.applied_tier = None;
                self.studio.update(cx, |panel, cx| {
                    panel.cloud_ready = false;
                    panel.refresh_env_status(cx);
                    panel.set_notice(Some(key), cx);
                });
                self.publish_tier_state(cx);
            }
        }
    }

    pub(super) fn apply_selected_tier(&mut self, cx: &mut Context<'_, Self>) {
        if !self.prefs.model.is_cloud() {
            self.prefs.model = flowy_agent::Model::Lite;
        }
        self.select_tier(self.prefs.model, cx);
    }

    pub(super) fn refresh_models(&mut self, cx: &mut Context<'_, Self>) {
        let busy = self.studio.read(cx).agent.turn_status().is_busy();
        let signed_in = self.auth.session.is_some();
        if !catalog_refresh_allowed(self.catalog_loading, signed_in, busy) {
            tracing::debug!(
                target: "flowy.boot",
                stage = "FlowyApp::refresh_models",
                loading = self.catalog_loading,
                signed_in,
                busy,
                "skip"
            );
            return;
        }
        let Some(session) = self.auth.session.clone() else {
            return;
        };
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::refresh_models",
            "fetch available_models"
        );
        let client = AuthClient::from_env();
        let account = session
            .user_id
            .map(|id| format!("{}:{id}", client.config().effective_base_url()));
        self.catalog_loading = true;
        let was_ready = self.studio.read(cx).cloud_ready;
        self.studio.update(cx, |panel, cx| {
            panel.cloud_ready = false;
            panel.catalog_loading = true;
            panel.refresh_env_status(cx);
            cx.notify();
        });
        if account.is_some()
            && account.as_deref() != Some(self.prefs.model_account.as_str())
        {
            self.effective_tier = 0;
            self.applied_tier = None;
            self.studio.update(cx, |panel, _| {
                panel.cloud_ready = false;
                panel.catalog = flowy_agent::ProviderCatalog::new();
                // The previous account's tier rows are meaningless now.
                panel.tiers.clear();
            });
        }
        self.catalog_generation = self.catalog_generation.wrapping_add(1);
        let generation = self.catalog_generation;
        let token = session.access_token;
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.available_models(&token);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.catalog_task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(result) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if generation != app.catalog_generation
                        || app.auth.session.is_none()
                    {
                        return;
                    }
                    app.catalog_loading = false;
                    app.studio.update(cx, |panel, _| {
                        panel.catalog_loading = false;
                    });
                    match result {
                        Ok(result) => {
                            // `effectiveModelTier` is the server's authoritative
                            // entitlement: it covers unexpired personal plans and
                            // team seats. `currentPlan` is display-only.
                            app.effective_tier = u8::try_from(
                                result.effective_model_tier.max(0),
                            )
                            .unwrap_or(0)
                            .min(2);
                            app.prefs.plan = Self::plan_for_effective_tier(
                                app.effective_tier,
                            );
                            let models = result
                                .cloud
                                .into_iter()
                                .map(|m| flowy_agent::CatalogModel {
                                    id: m.id,
                                    name: m.name,
                                    endpoint: m.endpoint,
                                    context_window: m.context_window,
                                    max_tokens: m.max_tokens,
                                    credit_rate: m.credit_rate,
                                    min_tier_level: u8::try_from(
                                        m.min_tier_level.max(0),
                                    )
                                    .unwrap_or(0)
                                    .min(2),
                                })
                                .collect::<Vec<_>>();
                            app.prefs.model_account =
                                account.clone().unwrap_or_default();
                            let catalog = flowy_agent::ProviderCatalog::new();
                            app.studio.update(cx, |panel, cx| {
                                panel.cloud_ready = false;
                                panel.catalog = catalog;
                                panel.catalog.replace(models);
                                panel.refresh_env_status(cx);
                                cx.notify();
                            });
                            app.publish_tier_state(cx);
                            app.apply_selected_tier(cx);
                            let _ = app.prefs.save_checked(app.store.root());
                        }
                        Err(AuthError::AuthRequired) => app.sign_out(cx),
                        Err(error) => {
                            let keep_ready =
                                was_ready && app.applied_tier.is_some();
                            app.studio.update(cx, |panel, cx| {
                                panel.cloud_ready = keep_ready;
                                panel.refresh_env_status(cx);
                                cx.notify();
                            });
                            app.note(error.key(), cx);
                        }
                    }
                });
                break;
            }
        }));
    }

    /// Refresh the account-scoped credit balance. Results are keyed by a
    /// generation and by the session token that produced them, so a slow
    /// response can never overwrite a newer refresh or another account's
    /// balance. Background failures only affect the display state; they never
    /// sign the user out.
    pub(super) fn refresh_credits(&mut self, cx: &mut Context<'_, Self>) {
        tracing::debug!(
            target: "flowy.boot",
            stage = "FlowyApp::refresh_credits",
            signed_in = self.auth.session.is_some(),
            "enter"
        );
        let Some(session) = self.auth.session.clone() else {
            self.studio.update(cx, |panel, cx| {
                if panel.credits != flowy_studio::CreditsViewState::Unavailable
                {
                    panel.credits = flowy_studio::CreditsViewState::Unavailable;
                    cx.notify();
                }
            });
            return;
        };
        let client = AuthClient::from_env();
        self.credits_generation = self.credits_generation.wrapping_add(1);
        let generation = self.credits_generation;
        let token = session.access_token;
        let token_marker = token.clone();
        if matches!(
            self.studio.read(cx).credits,
            flowy_studio::CreditsViewState::Unavailable
                | flowy_studio::CreditsViewState::Error
        ) {
            self.studio.update(cx, |panel, cx| {
                panel.credits = flowy_studio::CreditsViewState::Loading;
                cx.notify();
            });
        }
        let outcome = Arc::new(Mutex::new(None));
        let writer = Arc::clone(&outcome);
        std::thread::spawn(move || {
            let result = client.credits_balance(&token);
            if let Ok(mut guard) = writer.lock() {
                *guard = Some(result);
            }
        });
        self.credits_task = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                let Some(result) =
                    outcome.lock().ok().and_then(|mut guard| guard.take())
                else {
                    continue;
                };
                let _ = weak.update(cx, |app, cx| {
                    if generation != app.credits_generation {
                        return;
                    }
                    let same_session = app
                        .auth
                        .session
                        .as_ref()
                        .is_some_and(|s| s.access_token == token_marker);
                    if !same_session {
                        return;
                    }
                    let next = if let Ok(balance) = result {
                        credits_state_from_balance(balance)
                    } else {
                        if matches!(
                            app.studio.read(cx).credits,
                            flowy_studio::CreditsViewState::Ready { .. }
                        ) {
                            // Keep the last confirmed balance; a single failed
                            // poll must not blank it.
                            return;
                        }
                        flowy_studio::CreditsViewState::Error
                    };
                    app.studio.update(cx, |panel, cx| {
                        panel.credits = next;
                        cx.notify();
                    });
                });
                break;
            }
        }));
    }

    /// Poll the balance every ten minutes while a session exists.
    pub(super) fn start_credits_poll(&mut self, cx: &Context<'_, Self>) {
        self.credits_poll = Some(cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(600))
                    .await;
                if weak
                    .update(cx, |app, cx| {
                        if app.auth.session.is_some() {
                            app.refresh_credits(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}

/// Maps the cloud's signed balance onto the display state. Negative values
/// are clamped to zero and values above `u64::MAX` saturate.
pub(super) fn credits_state_from_balance(
    balance: i64,
) -> flowy_studio::CreditsViewState {
    let total = u64::try_from(balance.max(0)).unwrap_or(u64::MAX);
    flowy_studio::CreditsViewState::Ready {
        total,
        free: None,
        daily: None,
    }
}

#[cfg(test)]
mod cloud_config_tests {
    #[test]
    fn selected_cloud_model_sets_fixed_provider_and_account_credential() {
        let model = flowy_agent::CatalogModel {
            id: "selected-model".into(),
            endpoint: "https://example.com/claw/v1".into(),
            ..Default::default()
        };
        let config = super::cloud_model_config(&model, "account-token");
        assert_eq!(config.provider, "flowy");
        assert_eq!(config.model, "selected-model");
        assert_eq!(config.base_url, "https://example.com/claw/v1");
        assert_eq!(config.api_key.as_deref(), Some("account-token"));
        assert!(!super::cloud_model_config(&model, " ").uses_live());
    }
}
