use crate::error::DbError;
use crate::models::OAuthClientRegistrationRow;

/// Parameters for inserting or updating an OAuth client registration.
#[derive(Debug, Clone)]
pub struct UpsertOAuthClientRegistrationParams<'a> {
    pub mcp_server_url: &'a str,
    pub resource_identifier: &'a str,
    pub authorization_server_issuer: &'a str,
    pub redirect_uri: &'a str,
    pub registration_mode: &'a str,
    pub client_id: &'a str,
    pub client_secret_ref: Option<&'a str>,
    pub registration_access_token_ref: Option<&'a str>,
    pub client_id_issued_at: Option<nomifun_common::TimestampMs>,
    pub client_secret_expires_at: Option<nomifun_common::TimestampMs>,
    pub registration_client_uri: Option<&'a str>,
}

/// OAuth client registration data access abstraction (RFC 7591 + identity).
///
/// Persists the client identity so an application restart reuses the SAME
/// registration for token exchange/refresh instead of re-registering or
/// falling back to a default client id.
///
/// Object-safe via `async_trait` to support `Arc<dyn ...>`.
#[async_trait::async_trait]
pub trait IOAuthClientRegistrationRepository: Send + Sync {
    /// Get the registration row for an exact identity key, if any.
    async fn get_by_identity_key(
        &self,
        mcp_server_url: &str,
        resource_identifier: &str,
        authorization_server_issuer: &str,
        redirect_uri: &str,
    ) -> Result<Option<OAuthClientRegistrationRow>, DbError>;

    /// Get a registration row by primary key.
    async fn get_by_id(&self, id: i64) -> Result<Option<OAuthClientRegistrationRow>, DbError>;

    /// Insert a new registration row. Fails with `DbError::Conflict` when the
    /// identity key already exists.
    async fn insert(
        &self,
        params: UpsertOAuthClientRegistrationParams<'_>,
    ) -> Result<OAuthClientRegistrationRow, DbError>;

    /// Insert or update the registration for an identity key.
    async fn upsert(
        &self,
        params: UpsertOAuthClientRegistrationParams<'_>,
    ) -> Result<OAuthClientRegistrationRow, DbError>;

    /// List all registrations for an MCP server URL.
    async fn list_by_server_url(
        &self,
        mcp_server_url: &str,
    ) -> Result<Vec<OAuthClientRegistrationRow>, DbError>;

    /// Delete a registration row by primary key.
    async fn delete(&self, id: i64) -> Result<(), DbError>;
}