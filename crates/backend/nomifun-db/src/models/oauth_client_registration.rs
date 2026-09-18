use nomifun_common::TimestampMs;
use serde::{Deserialize, Serialize};

/// Row mapping for the `oauth_client_registrations` table.
///
/// A persisted client identity obtained either through RFC 7591 dynamic
/// registration or a pre-registered connector credential. The identity key is
/// `(mcp_server_url, resource_identifier, authorization_server_issuer,
/// redirect_uri)`; refresh and token exchange always reuse the SAME
/// registration that minted the original tokens.
///
/// Sensitive values (`client_secret_ref`, `registration_access_token_ref`)
/// are references into the secure credential store, never plaintext columns.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
pub struct OAuthClientRegistrationRow {
    pub id: i64,
    /// Normalized MCP endpoint URL.
    pub mcp_server_url: String,
    /// RFC 9728 protected-resource identifier (empty when not advertised).
    pub resource_identifier: String,
    /// RFC 8414 `issuer` of the authorization server (empty when unknown).
    pub authorization_server_issuer: String,
    /// The exact redirect URI registered for this client.
    pub redirect_uri: String,
    /// `dynamic` (RFC 7591) or `pre_registered` (connector credential).
    pub registration_mode: String,
    pub client_id: String,
    /// Secure-store reference for the client secret (OAuth confidential clients).
    pub client_secret_ref: Option<String>,
    /// Secure-store reference for the RFC 7591 registration access token.
    pub registration_access_token_ref: Option<String>,
    pub client_id_issued_at: Option<TimestampMs>,
    pub client_secret_expires_at: Option<TimestampMs>,
    pub registration_client_uri: Option<String>,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

impl OAuthClientRegistrationRow {
    /// The stable identity key that decides registration reuse.
    pub fn identity_key(
        mcp_server_url: &str,
        resource_identifier: &str,
        authorization_server_issuer: &str,
        redirect_uri: &str,
    ) -> [String; 4] {
        [
            mcp_server_url.to_owned(),
            resource_identifier.to_owned(),
            authorization_server_issuer.to_owned(),
            redirect_uri.to_owned(),
        ]
    }
}