//! Flowy cloud billing turn attribution (`X-Flowy-Turn-Id`).
//!
//! The conversation layer mints one UUID per user send / Agent Run and scopes
//! it with [`with_flowy_billing_turn_id`] around the engine turn. Flowy-proxied
//! provider HTTP (OpenAI-compatible, Anthropic, Responses) and Flowy
//! `HttpTransport` side calls read the value and attach it as a header so the
//! server can aggregate multi-call credit usage.

use std::future::Future;

use nomi_config::compat::ProviderCompat;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::ProviderError;

/// Header name expected by Flowy model proxy / credits aggregation.
pub const FLOWY_TURN_ID_HEADER: &str = "x-flowy-turn-id";

tokio::task_local! {
    static FLOWY_BILLING_TURN_ID: Option<String>;
}

/// Current turn id for Flowy billing, if this task is inside a scoped agent run.
pub fn current_flowy_billing_turn_id() -> Option<String> {
    FLOWY_BILLING_TURN_ID
        .try_with(|value| value.clone())
        .ok()
        .flatten()
        .filter(|id| {
            let trimmed = id.trim();
            !trimmed.is_empty() && trimmed.len() <= 64
        })
}

/// Run `fut` with `turn_id` visible to [`current_flowy_billing_turn_id`].
pub async fn with_flowy_billing_turn_id<F, T>(turn_id: impl Into<String>, fut: F) -> T
where
    F: Future<Output = T>,
{
    let turn_id = turn_id.into();
    let value = {
        let trimmed = turn_id.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            None
        } else {
            Some(trimmed.to_string())
        }
    };
    FLOWY_BILLING_TURN_ID.scope(value, fut).await
}

/// Re-scope `fut` after `tokio::spawn`. Task-locals do not cross spawn
/// boundaries — capture with [`current_flowy_billing_turn_id`] first.
pub async fn with_optional_flowy_billing_turn_id<F, T>(turn_id: Option<String>, fut: F) -> T
where
    F: Future<Output = T>,
{
    match turn_id {
        Some(id) => with_flowy_billing_turn_id(id, fut).await,
        None => fut.await,
    }
}

/// Attach Flowy proxy headers: the configured bearer mirror (legacy `token`)
/// plus `X-Flowy-Turn-Id` when a billing turn is scoped.
///
/// No-op when `compat.mirror_bearer_header` is unset. That field is the signal
/// that this request goes to a Flowy-proxied endpoint; local / third-party
/// providers must not receive the turn header.
pub fn apply_flowy_proxy_headers(
    headers: &mut HeaderMap,
    compat: &ProviderCompat,
    api_key: &str,
) -> Result<(), ProviderError> {
    let Some(name) = compat
        .mirror_bearer_header
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };
    let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
        ProviderError::Connection(format!("Invalid mirror bearer header name: {e}"))
    })?;
    let value = HeaderValue::from_str(api_key).map_err(|e| {
        ProviderError::Connection(format!("Invalid mirror bearer header value: {e}"))
    })?;
    headers.insert(header_name, value);

    if let Some(turn_id) = current_flowy_billing_turn_id() {
        let value = HeaderValue::from_str(&turn_id).map_err(|e| {
            ProviderError::Connection(format!("Invalid X-Flowy-Turn-Id header: {e}"))
        })?;
        headers.insert(HeaderName::from_static(FLOWY_TURN_ID_HEADER), value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scope_exposes_turn_id() {
        assert!(current_flowy_billing_turn_id().is_none());
        let observed = with_flowy_billing_turn_id("550e8400-e29b-41d4-a716-446655440000", async {
            current_flowy_billing_turn_id()
        })
        .await;
        assert_eq!(
            observed.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert!(current_flowy_billing_turn_id().is_none());
    }

    #[tokio::test]
    async fn rejects_oversized_turn_id() {
        let long = "a".repeat(65);
        let observed = with_flowy_billing_turn_id(long, async { current_flowy_billing_turn_id() }).await;
        assert!(observed.is_none());
    }

    #[tokio::test]
    async fn spawned_task_does_not_inherit_turn_id() {
        let observed = with_flowy_billing_turn_id("turn-parent", async {
            tokio::spawn(async { current_flowy_billing_turn_id() })
                .await
                .expect("join spawned billing probe")
        })
        .await;
        assert!(
            observed.is_none(),
            "detached tasks must wrap with_flowy_billing_turn_id themselves"
        );
    }

    #[tokio::test]
    async fn optional_scope_restores_turn_id_after_spawn() {
        let observed = with_flowy_billing_turn_id("turn-parent", async {
            let id = current_flowy_billing_turn_id();
            tokio::spawn(async move {
                with_optional_flowy_billing_turn_id(id, async { current_flowy_billing_turn_id() })
                    .await
            })
            .await
            .expect("join spawned billing restore")
        })
        .await;
        assert_eq!(observed.as_deref(), Some("turn-parent"));
    }

    #[tokio::test]
    async fn apply_headers_attaches_turn_id_only_on_flowy_proxy() {
        let mut local = HeaderMap::new();
        apply_flowy_proxy_headers(&mut local, &ProviderCompat::default(), "sk-test")
            .expect("local compat");
        assert!(local.get(FLOWY_TURN_ID_HEADER).is_none());

        let mut compat = ProviderCompat::default();
        compat.mirror_bearer_header = Some("token".into());
        let headers = with_flowy_billing_turn_id("turn-parent", async {
            let mut headers = HeaderMap::new();
            apply_flowy_proxy_headers(&mut headers, &compat, "sk-test").expect("flowy compat");
            headers
        })
        .await;
        assert_eq!(
            headers.get("token").and_then(|v| v.to_str().ok()),
            Some("sk-test")
        );
        assert_eq!(
            headers
                .get(FLOWY_TURN_ID_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some("turn-parent")
        );
    }
}
