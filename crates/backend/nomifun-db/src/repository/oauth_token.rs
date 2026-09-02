use crate::error::DbError;
use crate::models::OAuthTokenRow;

/// OAuth token data access abstraction for MCP server authentication.
///
/// Provides upsert/get/delete operations keyed by server URL, plus lookup by
/// the registration identity that minted the token. Token values are stored
/// encrypted; callers handle encryption/decryption.
///
/// Object-safe via `async_trait` to support `Arc<dyn IOAuthTokenRepository>`.
#[async_trait::async_trait]
pub trait IOAuthTokenRepository: Send + Sync {
    /// Gets a token by server URL, or `None` if not found.
    async fn get_by_url(&self, server_url: &str) -> Result<Option<OAuthTokenRow>, DbError>;

    /// Inserts or updates a token for the given server URL.
    async fn upsert(&self, params: UpsertOAuthTokenParams<'_>) -> Result<OAuthTokenRow, DbError>;

    /// Deletes a token by server URL. Returns `DbError::NotFound` if the URL
    /// doesn't exist.
    async fn delete(&self, server_url: &str) -> Result<(), DbError>;

    /// Returns the list of server URLs that have stored tokens.
    async fn list_authenticated_urls(&self) -> Result<Vec<String>, DbError>;

    /// Gets a token linked to a client registration, or `None` if not found.
    ///
    /// Default implementation returns `None` so lightweight mocks stay
    /// source-compatible; SQLite-backed repositories override it.
    async fn get_by_registration(
        &self,
        _registration_id: i64,
    ) -> Result<Option<OAuthTokenRow>, DbError> {
        Ok(None)
    }
}

/// Parameters for inserting or updating an OAuth token.
#[derive(Debug)]
pub struct UpsertOAuthTokenParams<'a> {
    pub server_url: &'a str,
    pub access_token: &'a str,
    pub refresh_token: Option<&'a str>,
    pub token_type: &'a str,
    pub expires_at: Option<nomifun_common::TimestampMs>,
    /// Owning `oauth_client_registrations.id`; `None` for legacy/unlinked rows.
    pub registration_id: Option<i64>,
    /// Owning principal id; `None` in the current single-device mode.
    pub principal_id: Option<&'a str>,
}

impl<'a> UpsertOAuthTokenParams<'a> {
    /// Minimal constructor keeping existing call sites compact.
    pub fn new(
        server_url: &'a str,
        access_token: &'a str,
        refresh_token: Option<&'a str>,
        token_type: &'a str,
        expires_at: Option<nomifun_common::TimestampMs>,
    ) -> Self {
        Self {
            server_url,
            access_token,
            refresh_token,
            token_type,
            expires_at,
            registration_id: None,
            principal_id: None,
        }
    }

    pub fn with_registration(mut self, registration_id: i64) -> Self {
        self.registration_id = Some(registration_id);
        self
    }
}