//! MCP OAuth runtime seam: token injection at session-build time and the
//! engine-side 401 refresher.
//!
//! The engine (`nomi-mcp`) never sees the token store. The application layer:
//! - injects the stored bearer token into remote transport headers when the
//!   session is built (`inject_oauth_bearer`), and
//! - provides a [`NomiMcpOAuthRefresher`] that the engine calls on 401 to
//!   refresh once, update the Authorization header and retry once.

use std::collections::HashMap;

use nomifun_mcp::McpOAuthService;

/// Host-provided implementation of the engine's OAuth refresh hook.
///
/// `Ok(None)` means "refresh not applicable / reauthorization required" —
/// the engine then surfaces the original 401 without retrying. A refresh
/// failure is mapped to a transport error so the caller sees a stable
/// failure instead of a silent retry loop.
pub struct NomiMcpOAuthRefresher {
    oauth: McpOAuthService,
}

impl NomiMcpOAuthRefresher {
    pub fn new(oauth: McpOAuthService) -> Self {
        Self { oauth }
    }
}

#[async_trait::async_trait]
impl nomi_mcp::manager::McpOAuthRefresher for NomiMcpOAuthRefresher {
    async fn refresh(&self, url: &str) -> Result<Option<String>, nomi_mcp::transport::McpError> {
        match self.oauth.refresh_access_token(url).await {
            Ok(token) => Ok(Some(token)),
            Err(nomifun_mcp::McpError::ReauthorizationRequired) => Ok(None),
            Err(error) => Err(nomi_mcp::transport::McpError::Transport(format!(
                "MCP OAuth refresh failed: {error}"
            ))),
        }
    }
}

/// Inject a stored OAuth bearer token into a remote transport's headers.
///
/// - An explicitly configured `Authorization` header wins (user override).
/// - `get_token` auto-refreshes an expired token before returning it.
/// - Missing token / lookup failure leaves the config untouched: the engine's
///   401-refresh path covers token expiry at call time, and a connector that
///   never authenticated simply stays unauthorized.
pub async fn inject_oauth_bearer(
    oauth: Option<&McpOAuthService>,
    url: &str,
    headers: &mut HashMap<String, String>,
) -> Result<(), String> {
    let Some(oauth) = oauth else {
        return Ok(());
    };
    if url.trim().is_empty() {
        return Ok(());
    }
    if headers
        .keys()
        .any(|key| key.eq_ignore_ascii_case("authorization"))
    {
        return Ok(());
    }
    match oauth.get_token(url).await {
        Ok(Some(token)) => {
            headers.insert("Authorization".to_owned(), format!("Bearer {token}"));
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(error) => Err(format!("MCP OAuth token lookup failed for {url}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn inject_oauth_bearer_without_service_is_a_no_op() {
        let mut headers = HashMap::new();
        inject_oauth_bearer(None, "https://example.test/mcp", &mut headers)
            .await
            .unwrap();
        assert!(headers.is_empty());
    }

    #[tokio::test]
    async fn inject_oauth_bearer_skips_explicit_authorization_override() {
        // No oauth service either — the override check must run first.
        let mut headers = HashMap::new();
        headers.insert("authorization".into(), "Bearer explicit".into());
        inject_oauth_bearer(None, "https://example.test/mcp", &mut headers)
            .await
            .unwrap();
        assert_eq!(headers.get("authorization").map(String::as_str), Some("Bearer explicit"));
    }
}
