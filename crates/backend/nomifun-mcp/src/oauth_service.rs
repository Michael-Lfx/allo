use std::sync::Arc;
use std::time::Duration;

use nomifun_api_types::{OAuthLoginResponse, OAuthStatusResponse};
use nomifun_common::{TimestampMs, now_ms};
use nomifun_db::{
    IOAuthClientRegistrationRepository, IOAuthTokenRepository, OAuthClientRegistrationRow,
    UpsertOAuthClientRegistrationParams, UpsertOAuthTokenParams,
};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::error::McpError;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default timeout for the OAuth callback server waiting for the redirect.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);

/// The MCP protocol version this client advertises. Shared with the engine
/// transport (`nomi-mcp::MCP_PROTOCOL_VERSION`) so OAuth discovery probes and
/// runtime `initialize` never drift; the equality is asserted in
/// `nomifun-ai-agent`.
pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

/// Token expiry safety margin (refresh 5 minutes before expiration).
const EXPIRY_MARGIN_MS: i64 = 5 * 60 * 1000;

// ---------------------------------------------------------------------------
// Discovery response
// ---------------------------------------------------------------------------

/// OAuth Authorization Server Metadata (RFC 8414) — subset of fields we need.
#[derive(Debug, Deserialize)]
struct OAuthServerMetadata {
    #[serde(default)]
    issuer: Option<String>,
    authorization_endpoint: String,
    token_endpoint: String,
    /// RFC 7591 dynamic client registration endpoint. `None` when the server
    /// does not advertise one.
    #[serde(default)]
    registration_endpoint: Option<String>,
    #[serde(default)]
    code_challenge_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    scopes_supported: Option<Vec<String>>,
}

/// The resolved OAuth server for one MCP resource (RFC 9728 discovery output).
///
/// `resource_identifier` and `authorization_server_issuer` participate in the
/// client-identity key so genuinely different OAuth topologies never share a
/// registration or a stored token.
#[derive(Debug, Clone)]
struct ResolvedOAuthServer {
    resource_identifier: String,
    authorization_server_issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: Option<String>,
    supported_pkce_methods: Option<Vec<String>>,
    scopes_supported: Option<Vec<String>>,
}

/// RFC 7591 dynamic client registration result.
#[derive(Debug, Clone)]
struct RegisteredClient {
    client_id: String,
    client_secret: Option<String>,
}

/// The resolved client identity for one login flow: either a pre-registered
/// connector credential or a persisted RFC 7591 dynamic registration.
///
/// `registration_id` links the identity to its
/// `oauth_client_registrations` row (when the registration repository is
/// attached) so the token record and refresh keep using the SAME identity.
#[derive(Debug, Clone)]
struct ClientIdentity {
    client_id: String,
    client_secret: Option<String>,
    registration_id: Option<i64>,
}

impl ClientIdentity {
    fn from_row(row: &OAuthClientRegistrationRow) -> Self {
        // `client_secret_ref` currently carries the credential itself (same
        // storage horizon as token columns until a dedicated vault exists);
        // `registration_access_token_ref` is only used for RFC 7591 client
        // management, never for token exchange/refresh.
        Self {
            client_id: row.client_id.clone(),
            client_secret: row.client_secret_ref.clone(),
            registration_id: Some(row.id),
        }
    }
}

// ---------------------------------------------------------------------------
// Pending login state
// ---------------------------------------------------------------------------

/// State held while waiting for the OAuth callback redirect.
///
/// Stores the resolved endpoint URLs and the EXACT client identity chosen for
/// this flow (pre-registered or RFC 7591 dynamic) so the authorization-code
/// exchange reuses the same identity — never a default or a re-registered one.
struct PendingLogin {
    csrf_token: CsrfToken,
    pkce_verifier: PkceCodeVerifier,
    auth_url: String,
    token_url: String,
    redirect_url: String,
    client_id: String,
    client_secret: Option<String>,
    /// `oauth_client_registrations.id` when the identity is persisted; `None`
    /// only in the (legacy) in-process fallback path.
    registration_id: Option<i64>,
}

// ---------------------------------------------------------------------------
// McpOAuthService
// ---------------------------------------------------------------------------

/// Service for MCP server OAuth 2.0 PKCE authentication.
///
/// Manages the full lifecycle: discovery → client identity (pre-registered or
/// RFC 7591 dynamic) → authorize → callback → token exchange → storage →
/// refresh → logout. Client identities are persisted in
/// `oauth_client_registrations` so an application restart reuses the SAME
/// identity for exchange/refresh instead of re-registering or falling back to
/// a default client id.
#[derive(Clone)]
pub struct McpOAuthService {
    token_repo: Arc<dyn IOAuthTokenRepository>,
    registration_repo: Arc<dyn IOAuthClientRegistrationRepository>,
    http_client: HttpClientFactory,
    /// Mutex protecting the pending login state (only one login at a time).
    pending: Arc<Mutex<Option<PendingLogin>>>,
    /// Serializes `login` so only one PKCE flow owns the shared `pending` slot
    /// at a time. `auth_start` spawns logins in background tasks; without a
    /// gate two concurrent flows overwrite each other's pending state and the
    /// first callback fails with a CSRF state mismatch (live: two concurrent
    /// connector logins → one login broken). `Arc` so every clone shares it.
    login_gate: Arc<tokio::sync::Mutex<()>>,
    /// Optional browser-open hook. Replaces the system-browser launch during
    /// the authorize step; used by tests to capture the authorization URL and
    /// drive the redirect without a real browser. `None` → `open::that`.
    browser_hook: Option<BrowserHook>,
}

/// Browser-open hook signature (`Fn(&str) + Send + Sync`).
type BrowserHook = Arc<dyn Fn(&str) + Send + Sync>;

type HttpClientFactory = Arc<dyn Fn() -> reqwest::Client + Send + Sync>;

impl McpOAuthService {
    pub fn new(token_repo: Arc<dyn IOAuthTokenRepository>, http_client: reqwest::Client) -> Self {
        Self::new_with_browser_hook(token_repo, http_client, None)
    }

    pub fn new_dynamic(token_repo: Arc<dyn IOAuthTokenRepository>) -> Self {
        Self::new_with_browser_hook(token_repo, nomifun_net::http_client(), None)
    }

    /// Attach a persistent client-registration repository (RFC 7591 identity
    /// store). Without it the service falls back to an in-process registry
    /// (fine for tests and single-process flows; identities do not survive a
    /// restart — see `docs/agent-store/06-connector-oauth-security.md` §14).
    pub fn with_registration_repository(
        mut self,
        registration_repo: Arc<dyn IOAuthClientRegistrationRepository>,
    ) -> Self {
        self.registration_repo = registration_repo;
        self
    }
    /// Test seam: like `new`, but the authorize step invokes `hook` with the
    /// authorization URL instead of launching the system browser.
    pub fn new_with_browser_hook(
        token_repo: Arc<dyn IOAuthTokenRepository>,
        http_client: reqwest::Client,
        hook: Option<BrowserHook>,
    ) -> Self {
        Self {
            token_repo,
            registration_repo: Arc::new(InMemoryOAuthClientRegistrationRepository::default()),
            http_client: Arc::new(move || http_client.clone()),
            pending: Arc::new(Mutex::new(None)),
            login_gate: Arc::new(tokio::sync::Mutex::new(())),
            browser_hook: hook,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        (self.http_client)()
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Check whether the given server URL has a usable OAuth credential.
    ///
    /// Returns a structured state (design doc §8): `not_authenticated` when
    /// no credential is stored, `reauthorization_required` when the stored
    /// credential is expired with no refresh path, otherwise `authenticated`
    /// (a valid credential is stored; `connected` — initialize/tools/list
    /// probe — is derived by the caller from the probe result).
    pub async fn check_oauth_status(&self, server_url: &str) -> Result<OAuthStatusResponse, McpError> {
        use nomifun_api_types::oauth_state::*;

        let Some(row) = self.token_repo.get_by_url(server_url).await? else {
            return Ok(OAuthStatusResponse {
                authenticated: false,
                state: NOT_AUTHENTICATED.to_owned(),
            });
        };
        let expired = row
            .expires_at
            .is_some_and(|expires_at| now_ms() >= expires_at);
        if expired && row.refresh_token.is_none() {
            return Ok(OAuthStatusResponse {
                authenticated: false,
                state: REAUTHORIZATION_REQUIRED.to_owned(),
            });
        }
        Ok(OAuthStatusResponse {
            authenticated: true,
            state: AUTHENTICATED.to_owned(),
        })
    }

    /// Start the OAuth PKCE login flow for the given MCP server URL.
    ///
    /// 1. Discover authorization/token endpoints
    /// 2. Generate PKCE challenge
    /// 3. Start local callback server on a random port
    /// 4. Build authorization URL and open it in the system browser
    /// 5. Wait for the redirect with the authorization code
    /// 6. Exchange code for tokens and persist them
    pub async fn login(&self, server_url: &str) -> Result<OAuthLoginResponse, McpError> {
        // Serialize PKCE flows: the shared `pending` slot holds one login at a
        // time (concurrent `auth_start` background tasks would otherwise
        // overwrite each other → CSRF mismatch on the first callback).
        let _gate = self.login_gate.lock().await;
        let (authorize_url, listener) = match self.prepare_login_flow(server_url).await {
            Ok(value) => value,
            Err(error) => {
                // Structured, UI-readable error without tokens, codes or the
                // full authorization URL (design doc §6.2/§7.1).
                return Ok(OAuthLoginResponse {
                    success: false,
                    error: Some(sanitize_oauth_error(&error)),
                    error_code: error.oauth_error_code().map(str::to_owned),
                });
            }
        };

        // Open browser (or hand the URL to the configured hook — tests drive
        // the redirect themselves).
        debug!(url = %authorize_url, "Opening browser for OAuth authorization");
        if let Some(hook) = self.browser_hook.as_ref() {
            hook(&authorize_url);
        } else if let Err(e) = open::that(&authorize_url) {
            warn!("Failed to open browser: {e}");
        }

        // Wait for callback.
        let code = match self.wait_for_callback(listener).await {
            Ok(code) => code,
            Err(e) => {
                let code = e.oauth_error_code().map(str::to_owned);
                self.clear_pending().await;
                return Ok(OAuthLoginResponse {
                    success: false,
                    error: Some(sanitize_oauth_error(&e)),
                    error_code: code,
                });
            }
        };

        // Exchange code for tokens.
        match self.exchange_code(server_url, code).await {
            Ok(()) => Ok(OAuthLoginResponse {
                success: true,
                error: None,
                error_code: None,
            }),
            Err(e) => {
                let code = e.oauth_error_code().map(str::to_owned);
                self.clear_pending().await;
                Ok(OAuthLoginResponse {
                    success: false,
                    error: Some(sanitize_oauth_error(&e)),
                    error_code: code,
                })
            }
        }
    }

    /// Logout from the given MCP server URL (delete stored token).
    ///
    /// Idempotent: returns Ok even if no token was stored.
    pub async fn logout(&self, server_url: &str) -> Result<(), McpError> {
        match self.token_repo.delete(server_url).await {
            Ok(()) => {
                debug!(server_url, "OAuth token deleted");
                Ok(())
            }
            Err(nomifun_db::DbError::NotFound(_)) => {
                debug!(server_url, "No OAuth token to delete (idempotent)");
                Ok(())
            }
            Err(e) => Err(McpError::Database(e)),
        }
    }

    /// Return the list of server URLs that have stored OAuth tokens.
    pub async fn get_authenticated_servers(&self) -> Result<Vec<String>, McpError> {
        let urls = self.token_repo.list_authenticated_urls().await?;
        Ok(urls)
    }

    /// Get a valid access token for the given server URL.
    ///
    /// If the stored token is expired and a refresh token is available,
    /// automatically refreshes before returning.
    /// Returns `None` if no token is stored for this URL.
    pub async fn get_token(&self, server_url: &str) -> Result<Option<String>, McpError> {
        let row = match self.token_repo.get_by_url(server_url).await? {
            Some(row) => row,
            None => return Ok(None),
        };

        // Check if token is expired (with safety margin).
        if let Some(expires_at) = row.expires_at {
            let now = now_ms();
            if now >= expires_at - EXPIRY_MARGIN_MS
                && let Some(ref refresh_token) = row.refresh_token
            {
                match self.refresh_token(server_url, refresh_token).await {
                    Ok(new_token) => return Ok(Some(new_token)),
                    Err(e) => {
                        warn!(
                            server_url,
                            error = %e,
                            "Token refresh failed; reauthorization is required"
                        );
                        return Err(McpError::ReauthorizationRequired);
                    }
                }
            } else if now >= expires_at - EXPIRY_MARGIN_MS {
                return Err(McpError::ReauthorizationRequired);
            }
        }

        Ok(Some(row.access_token))
    }

    /// Refresh a token after an authenticated MCP request receives 401.
    /// The caller is responsible for one bounded retry with the returned token.
    pub async fn refresh_access_token(&self, server_url: &str) -> Result<String, McpError> {
        let row = self
            .token_repo
            .get_by_url(server_url)
            .await?
            .ok_or(McpError::ReauthorizationRequired)?;
        let refresh_token = row
            .refresh_token
            .as_deref()
            .ok_or(McpError::ReauthorizationRequired)?;
        self.refresh_token(server_url, refresh_token)
            .await
            .map_err(|error| {
                warn!(server_url, error = %error, "MCP OAuth request refresh failed");
                McpError::ReauthorizationRequired
            })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Discover endpoints, build OAuth client, generate PKCE, bind callback
    /// server, store pending state, and return the authorization URL + listener.
    async fn prepare_login_flow(&self, server_url: &str) -> Result<(String, TcpListener), McpError> {
        let resolved = self.discover_endpoints(server_url).await?;
        if let Some(scopes) = resolved.scopes_supported.as_ref() {
            debug!(server_url, ?scopes, "OAuth server advertised scopes_supported");
        }
        if let Some(methods) = resolved.supported_pkce_methods.as_ref() {
            debug!(server_url, ?methods, "OAuth server advertised code_challenge_methods_supported");
        }

        let auth_url_str = resolved.authorization_endpoint.clone();
        let token_url_str = resolved.token_endpoint.clone();

        let auth_url = AuthUrl::new(resolved.authorization_endpoint.clone())
            .map_err(|e| McpError::OAuth(format!("Invalid auth URL: {e}")))?;
        let token_url =
            TokenUrl::new(resolved.token_endpoint.clone()).map_err(|e| McpError::OAuth(format!("Invalid token URL: {e}")))?;

        // Fixed redirect URIs must be loopback with an explicit port; the
        // listener binds that host:port and the callback handler enforces the
        // exact request path (design doc §7.1).
        let (listener, redirect_url_str) = Self::bind_callback_listener().await?;
        let redirect = RedirectUrl::new(redirect_url_str.clone())
            .map_err(|e| McpError::OAuth(format!("Invalid redirect URL: {e}")))?;

        // Client identity decision (design doc §6.1):
        //   预注册 client 存在           → 使用预注册 client（不触发动态注册）
        //   同一 identity key 的持久化注册 → 复用该 registration（不重新注册）
        //   服务方发布 registration_endpoint → RFC 7591 动态注册并持久化
        //   否则                          → pre_registered_client_required
        let identity = self
            .resolve_client_identity(server_url, &resolved, &redirect_url_str)
            .await?;
        let (client_id, client_secret) = (identity.client_id.clone(), identity.client_secret.clone());

        let mut client = BasicClient::new(ClientId::new(client_id))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect);
        if let Some(secret) = client_secret.as_ref() {
            client = client.set_client_secret(ClientSecret::new(secret.clone()));
        }

        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let (authorize_url, csrf_token) = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .url();

        {
            let mut pending = self.pending.lock().await;
            *pending = Some(PendingLogin {
                csrf_token,
                pkce_verifier,
                auth_url: auth_url_str,
                token_url: token_url_str,
                redirect_url: redirect_url_str,
                client_id: identity.client_id,
                client_secret,
                registration_id: identity.registration_id,
            });
        }

        Ok((authorize_url.to_string(), listener))
    }

    /// Build the loopback callback listener for the effective redirect URI.
    ///
    /// With `MCP_OAUTH_REDIRECT_URI` set, the URI must be
    /// `http://127.0.0.1:<port>/<path>` (or `http://localhost:…`); the
    /// listener binds that host and port (design doc §7.1).
    async fn bind_callback_listener() -> Result<(TcpListener, String), McpError> {
        let Some(redirect_uri) = Self::env_redirect_uri() else {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| McpError::OAuth(format!("Failed to bind callback server: {e}")))?;
            let callback_port = listener
                .local_addr()
                .map_err(|e| McpError::OAuth(format!("Failed to get callback port: {e}")))?
                .port();
            return Ok((listener, format!("http://127.0.0.1:{callback_port}/callback")));
        };

        let remainder = redirect_uri
            .strip_prefix("http://")
            .ok_or_else(|| McpError::UnsupportedAuth("MCP_OAUTH_REDIRECT_URI must start with http://".into()))?;
        let host_port = remainder
            .split_once('/')
            .map(|(host_port, _)| host_port)
            .unwrap_or(remainder);
        let (host, port) = host_port.rsplit_once(':').ok_or_else(|| {
            McpError::UnsupportedAuth(
                "MCP_OAUTH_REDIRECT_URI must include a port, e.g. http://127.0.0.1:8765/callback".into(),
            )
        })?;
        if !matches!(host, "127.0.0.1" | "localhost") {
            return Err(McpError::UnsupportedAuth(format!(
                "MCP_OAUTH_REDIRECT_URI host must be loopback (127.0.0.1 or localhost), got '{host}'"
            )));
        }
        let port: u16 = port.parse().map_err(|_| {
            McpError::UnsupportedAuth(format!("MCP_OAUTH_REDIRECT_URI has an invalid port: {port}"))
        })?;
        let path = remainder
            .split_once('/')
            .map(|(_, path)| path)
            .unwrap_or_default();
        if path.is_empty() {
            return Err(McpError::UnsupportedAuth(
                "MCP_OAUTH_REDIRECT_URI must include a path, e.g. /callback".into(),
            ));
        }
        let listener = TcpListener::bind((host, port))
            .await
            .map_err(|e| McpError::OAuth(format!("Failed to bind callback server: {e}")))?;
        Ok((listener, redirect_uri))
    }

    /// Discover the OAuth authorization server for an MCP resource (design doc
    /// §5.1).
    ///
    /// Order:
    /// 1. RFC 8414 `.well-known/oauth-authorization-server` (then
    ///    `.well-known/openid-configuration`) at the server URL **and at its
    ///    origin root** — many servers publish metadata only at the issuer
    ///    root (e.g. `https://api.mail.qq.com/.well-known/...` for
    ///    `https://api.mail.qq.com/mcp`);
    /// 2. RFC 9728 protected-resource metadata: the resource URL answers with
    ///    `WWW-Authenticate: ... resource_metadata="<url>"` — probed with a
    ///    GET first, then with an **unauthenticated Streamable HTTP
    ///    `initialize` POST** when the GET carries no challenge (servers that
    ///    reject GET or only challenge MCP `initialize` still work); the
    ///    document lists `resource` and `authorization_servers`, whose RFC
    ///    8414 metadata is then fetched;
    /// 3. a clear error when no standard metadata is published — dynamic
    ///    registration is then impossible and a pre-registered client id is
    ///    required.
    async fn discover_endpoints(&self, server_url: &str) -> Result<ResolvedOAuthServer, McpError> {
        let base = server_url.trim_end_matches('/');
        let origin = origin_of(base);

        for well_known in [
            format!("{base}/.well-known/oauth-authorization-server"),
            format!("{base}/.well-known/openid-configuration"),
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ] {
            if let Ok(metadata) = self.fetch_metadata(&well_known).await {
                debug!(server_url, "Discovered OAuth metadata via RFC 8414 well-known: {well_known}");
                return Ok(ResolvedOAuthServer {
                    resource_identifier: String::new(),
                    authorization_server_issuer: metadata
                        .issuer
                        .clone()
                        .unwrap_or_else(|| origin.to_owned()),
                    authorization_endpoint: metadata.authorization_endpoint,
                    token_endpoint: metadata.token_endpoint,
                    registration_endpoint: metadata.registration_endpoint,
                    supported_pkce_methods: metadata.code_challenge_methods_supported,
                    scopes_supported: metadata.scopes_supported,
                });
            }
        }

        // RFC 9728: protected resource metadata advertised through the
        // `WWW-Authenticate` challenge (GET probe, then unauthenticated
        // `initialize` POST probe).
        if let Some(prm_url) = self.discover_protected_resource_metadata(base).await? {
            let prm = self.fetch_json(&prm_url).await?;
            let resource_identifier = prm
                .get("resource")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if let Some(auth_server) = prm
                .get("authorization_servers")
                .and_then(serde_json::Value::as_array)
                .and_then(|servers| servers.first())
                .and_then(serde_json::Value::as_str)
                .map(|issuer| issuer.to_owned())
                .filter(|issuer| !issuer.trim().is_empty())
                .map(|issuer| issuer.trim_end_matches('/').to_owned())
            {
                debug!(
                    server_url,
                    "Discovered OAuth authorization server via RFC 9728: {auth_server} (resource={resource_identifier})"
                );
                for well_known in [
                    format!("{auth_server}/.well-known/oauth-authorization-server"),
                    format!("{auth_server}/.well-known/openid-configuration"),
                ] {
                    if let Ok(metadata) = self.fetch_metadata(&well_known).await {
                        return Ok(ResolvedOAuthServer {
                            resource_identifier,
                            authorization_server_issuer: metadata
                                .issuer
                                .clone()
                                .unwrap_or(auth_server.clone()),
                            authorization_endpoint: metadata.authorization_endpoint,
                            token_endpoint: metadata.token_endpoint,
                            registration_endpoint: metadata.registration_endpoint,
                            supported_pkce_methods: metadata.code_challenge_methods_supported,
                            scopes_supported: metadata.scopes_supported,
                        });
                    }
                }
                return Err(McpError::OAuth(format!(
                    "authorization server {auth_server} does not publish RFC 8414 metadata; \
                     dynamic client registration is not supported — a pre-registered \
                     client id (MCP_OAUTH_CLIENT_ID) is required"
                )));
            }
        }

        Err(McpError::OAuth(format!(
            "Failed to discover OAuth endpoints for '{server_url}': \
             no .well-known/oauth-authorization-server, \
             .well-known/openid-configuration or RFC 9728 protected-resource metadata found"
        )))
    }

    /// RFC 9728 discovery: request the protected resource and read the
    /// `resource_metadata` pointer from its `WWW-Authenticate` challenge.
    ///
    /// Probes with GET first; when the GET yields no challenge (405/404 or no
    /// header), probes with an unauthenticated Streamable HTTP `initialize`
    /// POST — the shape real MCP servers actually challenge (design doc §5.1
    /// steps 5-7, §10 acceptance #6).
    async fn discover_protected_resource_metadata(
        &self,
        resource_url: &str,
    ) -> Result<Option<String>, McpError> {
        let client = self.http_client();
        let response = client
            .get(resource_url)
            .header("Accept", "application/json, text/event-stream")
            .send()
            .await
            .map_err(|e| McpError::OAuth(format!("Protected resource request failed: {e}")))?;
        let challenge = response
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        // `WWW-Authenticate: Bearer ..., resource_metadata="<url>"` (RFC 9728 §2)
        if let Some(prm_url) = parse_resource_metadata_challenge(challenge) {
            return Ok(Some(prm_url));
        }

        // GET carried no challenge → fall back to the unauthenticated MCP
        // initialize POST against the resource itself.
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "oauth-discovery",
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "Nomifun MCP Client",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }
        });
        let post_response = client
            .post(resource_url)
            .header("Accept", "application/json, text/event-stream")
            .json(&payload)
            .send()
            .await
            .map_err(|e| McpError::OAuth(format!("OAuth discovery initialize POST failed: {e}")))?;
        if post_response.status() == reqwest::StatusCode::UNAUTHORIZED {
            let challenge = post_response
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            if let Some(prm_url) = parse_resource_metadata_challenge(challenge) {
                debug!(
                    resource_url,
                    "Discovered RFC 9728 resource_metadata via initialize POST challenge"
                );
                return Ok(Some(prm_url));
            }
        }
        Ok(None)
    }

    /// Fetch and parse arbitrary JSON (e.g. an RFC 9728 protected-resource
    /// metadata document, whose shape is not the RFC 8414 metadata struct).
    async fn fetch_json(&self, url: &str) -> Result<serde_json::Value, McpError> {
        let client = self.http_client();
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| McpError::OAuth(format!("HTTP request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(McpError::OAuth(format!(
                "Metadata endpoint returned {}",
                resp.status()
            )));
        }

        resp.json()
            .await
            .map_err(|e| McpError::OAuth(format!("Failed to parse metadata: {e}")))
    }

    /// Fetch and parse OAuth server metadata from a URL.
    async fn fetch_metadata(&self, url: &str) -> Result<OAuthServerMetadata, McpError> {
        let client = self.http_client();
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| McpError::OAuth(format!("HTTP request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(McpError::OAuth(format!("Metadata endpoint returned {}", resp.status())));
        }

        resp.json()
            .await
            .map_err(|e| McpError::OAuth(format!("Failed to parse metadata: {e}")))
    }

    /// Env-driven pre-registered client override, mirroring the reference
    /// client (`MCP_CLIENT_ID` / `MCP_CLIENT_SECRET`). Authorization servers
    /// without dynamic registration (e.g. GitHub OAuth) require a
    /// pre-registered public client; set `MCP_OAUTH_CLIENT_ID` (and, when the
    /// app is confidential, `MCP_OAUTH_CLIENT_SECRET`) plus the matching
    /// `MCP_OAUTH_REDIRECT_URI`.
    fn env_registered_client() -> Option<RegisteredClient> {
        let client_id = std::env::var("MCP_OAUTH_CLIENT_ID")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())?;
        Some(RegisteredClient {
            client_id,
            client_secret: std::env::var("MCP_OAUTH_CLIENT_SECRET")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
        })
    }

    /// Env-driven fixed loopback redirect URI (must match the pre-registered
    /// client's callback). The callback listener binds its host/port/path.
    fn env_redirect_uri() -> Option<String> {
        std::env::var("MCP_OAUTH_REDIRECT_URI")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    /// Resolve the client identity for a login flow (design doc §6.1).
    ///
    /// Priority:
    ///   有效的预注册 client 配置          → 使用预注册 client，不触发动态注册
    ///   同一 identity key 的持久化 registration → 复用该 registration
    ///   authorization server 发布 registration_endpoint → RFC 7591 动态注册
    ///   否则                              → `pre_registered_client_required`
    ///
    /// The chosen identity is persisted under
    /// `(mcp_server_url, resource, issuer, redirect_uri)` so the exchange,
    /// any refresh, and later application restarts reuse the SAME client
    /// identity. A generic default client id is never fabricated.
    async fn resolve_client_identity(
        &self,
        server_url: &str,
        resolved: &ResolvedOAuthServer,
        redirect_uri: &str,
    ) -> Result<ClientIdentity, McpError> {
        // 1. Pre-registered client (env) wins and never triggers dynamic
        //    registration. Persisted under mode=pre_registered so refresh
        //    binds to it explicitly.
        if let Some(registered) = Self::env_registered_client() {
            let existing = self
                .registration_repo
                .get_by_identity_key(
                    server_url,
                    &resolved.resource_identifier,
                    &resolved.authorization_server_issuer,
                    redirect_uri,
                )
                .await
                .map_err(McpError::Database)?;
            let registration_id = match existing {
                Some(row) if row.client_id == registered.client_id => row.id,
                _ => self
                    .registration_repo
                    .upsert(UpsertOAuthClientRegistrationParams {
                        mcp_server_url: server_url,
                        resource_identifier: &resolved.resource_identifier,
                        authorization_server_issuer: &resolved.authorization_server_issuer,
                        redirect_uri,
                        registration_mode: "pre_registered",
                        client_id: &registered.client_id,
                        client_secret_ref: registered.client_secret.as_deref(),
                        registration_access_token_ref: None,
                        client_id_issued_at: None,
                        client_secret_expires_at: None,
                        registration_client_uri: None,
                    })
                    .await
                    .map_err(McpError::Database)?
                    .id,
            };
            debug!(
                server_url,
                %registration_id,
                "OAuth client identity: pre-registered via environment"
            );
            return Ok(ClientIdentity {
                client_id: registered.client_id,
                client_secret: registered.client_secret,
                registration_id: Some(registration_id),
            });
        }

        // 2. Reuse a persisted dynamic registration for the same identity key.
        if let Some(row) = self
            .registration_repo
            .get_by_identity_key(
                server_url,
                &resolved.resource_identifier,
                &resolved.authorization_server_issuer,
                redirect_uri,
            )
            .await
            .map_err(McpError::Database)?
        {
            debug!(
                server_url,
                id = row.id,
                "OAuth client identity: reusing persisted registration (RFC 7591)"
            );
            return Ok(ClientIdentity::from_row(&row));
        }

        // 3. RFC 7591 dynamic registration and persist the identity.
        if let Some(endpoint) = resolved.registration_endpoint.as_deref() {
            let registered = self
                .register_client(server_url, endpoint, redirect_uri, resolved)
                .await?;
            debug!(server_url, "OAuth client identity: dynamically registered (RFC 7591)");
            return Ok(registered);
        }

        // 4. No pre-registered client and no registration endpoint — fail
        //    loudly instead of fabricating a default identity.
        Err(McpError::PreRegisteredClientRequired(format!(
            "{server_url} publishes no RFC 7591 registration endpoint and no \
             pre-registered client (MCP_OAUTH_CLIENT_ID) is configured"
        )))
    }

    /// Register this client with the authorization server (RFC 7591) and
    /// persist the registration under its identity key.
    ///
    /// Public client (no secret), PKCE-verified authorization code flow
    /// (design doc §6.2). Error classification keeps the UI readable without
    /// leaking tokens, codes or the full authorization URL.
    async fn register_client(
        &self,
        server_url: &str,
        endpoint: &str,
        redirect_url: &str,
        resolved: &ResolvedOAuthServer,
    ) -> Result<ClientIdentity, McpError> {
        let payload = serde_json::json!({
            "client_name": "Nomifun MCP Client",
            "redirect_uris": [redirect_url],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        });
        let client = self.http_client();
        let response = client
            .post(endpoint)
            .json(&payload)
            .send()
            .await
            .map_err(|e| McpError::DynamicRegistrationFailed(format!("request failed: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::BAD_REQUEST
                && body_text.to_ascii_lowercase().contains("redirect_uri")
            {
                return Err(McpError::RedirectUriNotAllowed(format!(
                    "authorization server rejected redirect_uri '{redirect_url}'"
                )));
            }
            return Err(McpError::DynamicRegistrationFailed(format!(
                "registration endpoint returned HTTP {status}"
            )));
        }
        let body: serde_json::Value = response.json().await.map_err(|e| {
            McpError::InvalidRegistrationResponse(format!("registration response is not JSON: {e}"))
        })?;
        let client_id = body
            .get("client_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                McpError::InvalidRegistrationResponse("registration response lacks client_id".into())
            })?
            .to_owned();
        let client_secret = body
            .get("client_secret")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let registration_access_token = body
            .get("registration_access_token")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let client_id_issued_at = body
            .get("client_id_issued_at")
            .and_then(serde_json::Value::as_i64)
            .map(|secs| secs.saturating_mul(1000));
        let client_secret_expires_at = body
            .get("client_secret_expires_at")
            .and_then(serde_json::Value::as_i64)
            .map(|secs| secs.saturating_mul(1000));
        let registration_client_uri = body
            .get("registration_client_uri")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);

        let row = self
            .registration_repo
            .upsert(UpsertOAuthClientRegistrationParams {
                mcp_server_url: server_url,
                resource_identifier: &resolved.resource_identifier,
                authorization_server_issuer: &resolved.authorization_server_issuer,
                redirect_uri: redirect_url,
                registration_mode: "dynamic",
                client_id: &client_id,
                client_secret_ref: client_secret.as_deref(),
                registration_access_token_ref: registration_access_token.as_deref(),
                client_id_issued_at,
                client_secret_expires_at,
                registration_client_uri: registration_client_uri.as_deref(),
            })
            .await
            .map_err(McpError::Database)?;

        Ok(ClientIdentity {
            client_id,
            client_secret,
            registration_id: Some(row.id),
        })
    }

    /// Wait for the OAuth callback redirect on the given listener.
    async fn wait_for_callback(&self, listener: TcpListener) -> Result<String, McpError> {
        let (code_tx, code_rx) = tokio::sync::oneshot::channel::<Result<String, McpError>>();
        let pending = self.pending.clone();

        tokio::spawn(async move {
            let result = Self::handle_callback_connection(listener, pending).await;
            let _ = code_tx.send(result);
        });

        match tokio::time::timeout(CALLBACK_TIMEOUT, code_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(McpError::OAuth("Callback channel closed unexpectedly".to_string())),
            Err(_) => Err(McpError::OAuth(
                "OAuth callback timed out — no redirect received within 120s".to_string(),
            )),
        }
    }

    /// Handle a single HTTP connection on the callback server.
    ///
    /// Enforces the exact redirect path registered with the authorization
    /// server (`MCP_OAUTH_REDIRECT_URI` or the ephemeral `/callback`), and
    /// validates the CSRF state before returning the code (design doc §7.1).
    /// Failed path/state checks do NOT consume the pending login state.
    async fn handle_callback_connection(
        listener: TcpListener,
        pending: Arc<Mutex<Option<PendingLogin>>>,
    ) -> Result<String, McpError> {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|e| McpError::OAuth(format!("Failed to accept connection: {e}")))?;

        let mut buf = vec![0u8; 4096];
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| McpError::OAuth(format!("Failed to read request: {e}")))?;

        let request = String::from_utf8_lossy(&buf[..n]);

        // Path check against the registered redirect URI before touching the
        // pending state (wrong path → reject, keep state for the real one).
        let expected_path = {
            let guard = pending.lock().await;
            let pending_login = guard
                .as_ref()
                .ok_or_else(|| McpError::OAuth("No pending login state".to_string()))?;
            redirect_path(&pending_login.redirect_url)
        };
        let request_path = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default()
            .split('?')
            .next()
            .unwrap_or_default()
            .to_owned();
        if request_path != expected_path {
            return Err(McpError::UnsupportedAuth(format!(
                "callback path mismatch: expected '{expected_path}', got '{request_path}'"
            )));
        }

        let (code, state) = parse_callback_query(&request)?;

        // Validate CSRF state.
        let guard = pending.lock().await;
        let pending_login = guard
            .as_ref()
            .ok_or_else(|| McpError::OAuth("No pending login state".to_string()))?;

        if state != *pending_login.csrf_token.secret() {
            return Err(McpError::OAuth("CSRF state mismatch".to_string()));
        }

        // Send a success response to the browser.
        let response = "HTTP/1.1 200 OK\r\n\
            Content-Type: text/html; charset=utf-8\r\n\
            Connection: close\r\n\r\n\
            <html><body><h1>Authorization successful!</h1>\
            <p>You can close this window and return to Nomi.</p>\
            </body></html>";

        let _ = stream.write_all(response.as_bytes()).await;

        Ok(code)
    }

    /// Build a no-redirect reqwest client for OAuth token exchange.
    fn build_no_redirect_client() -> Result<reqwest::Client, McpError> {
        nomifun_net::proxy::apply_detected_proxy(reqwest::ClientBuilder::new())
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| McpError::OAuth(format!("Failed to build HTTP client: {e}")))
    }

    /// Exchange the authorization code for tokens and persist them.
    ///
    /// Uses the EXACT client identity chosen during `prepare_login_flow`
    /// (pre-registered or persisted dynamic registration) — never a default
    /// or a re-registered one — and links the stored token to that
    /// registration.
    async fn exchange_code(&self, server_url: &str, code: String) -> Result<(), McpError> {
        let (auth_url_str, token_url_str, redirect_url_str, pkce_verifier, client_id, client_secret, registration_id) =
            {
                let mut guard = self.pending.lock().await;
                let pending = guard
                    .take()
                    .ok_or_else(|| McpError::OAuth("No pending login state".to_string()))?;
                (
                    pending.auth_url,
                    pending.token_url,
                    pending.redirect_url,
                    pending.pkce_verifier,
                    pending.client_id,
                    pending.client_secret,
                    pending.registration_id,
                )
            };

        let auth_url = AuthUrl::new(auth_url_str).map_err(|e| McpError::OAuth(format!("Invalid auth URL: {e}")))?;
        let token_url = TokenUrl::new(token_url_str).map_err(|e| McpError::OAuth(format!("Invalid token URL: {e}")))?;
        let redirect =
            RedirectUrl::new(redirect_url_str).map_err(|e| McpError::OAuth(format!("Invalid redirect URL: {e}")))?;

        let mut client = BasicClient::new(ClientId::new(client_id))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(redirect);
        if let Some(secret) = client_secret.as_ref() {
            client = client.set_client_secret(ClientSecret::new(secret.clone()));
        }

        let http_client = Self::build_no_redirect_client()?;

        let token_result = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&http_client)
            .await
            .map_err(|e| McpError::OAuth(format!("Token exchange failed: {e}")))?;

        self.persist_token(server_url, &token_result, registration_id)
            .await?;
        debug!(server_url, "OAuth tokens stored successfully");
        Ok(())
    }

    /// Refresh an expired access token using the refresh token.
    ///
    /// The refresh request uses the client identity that minted the original
    /// tokens:
    ///   - token linked to a registration → that registration's client id
    ///     (and secret ref when present);
    ///   - legacy unlinked token → the env pre-registered client when set,
    ///     otherwise `reauthorization_required` — never a fabricated default.
    async fn refresh_token(&self, server_url: &str, refresh_token_value: &str) -> Result<String, McpError> {
        // Re-read the token row so refresh binds to the registration recorded
        // at exchange time (design doc §7.2).
        let row = self
            .token_repo
            .get_by_url(server_url)
            .await?
            .ok_or(McpError::ReauthorizationRequired)?;
        let token_url = self.resolve_token_endpoint(server_url).await?;

        let identity = if let Some(registration_id) = row.registration_id {
            let registration = self
                .registration_repo
                .get_by_id(registration_id)
                .await
                .map_err(McpError::Database)?
                .ok_or(McpError::ReauthorizationRequired)?;
            ClientIdentity::from_row(&registration)
        } else if let Some(registered) = Self::env_registered_client() {
            // Legacy row + explicit env pre-registered identity: use it
            // (documented compatibility channel, §4.3).
            ClientIdentity {
                client_id: registered.client_id,
                client_secret: registered.client_secret,
                registration_id: None,
            }
        } else {
            warn!(
                server_url,
                "OAuth token has no registration link and no pre-registered client; \
                 refusing to guess a client identity"
            );
            return Err(McpError::ReauthorizationRequired);
        };

        let mut client =
            BasicClient::new(ClientId::new(identity.client_id)).set_token_uri(token_url);
        if let Some(secret) = identity.client_secret.as_ref() {
            client = client.set_client_secret(ClientSecret::new(secret.clone()));
        }

        let http_client = Self::build_no_redirect_client()?;

        let refresh_token = RefreshToken::new(refresh_token_value.to_string());
        let token_result = client
            .exchange_refresh_token(&refresh_token)
            .request_async(&http_client)
            .await
            .map_err(|e| McpError::OAuth(format!("Token refresh failed: {e}")))?;

        let new_access_token = token_result.access_token().secret().clone();

        let expires_at: Option<TimestampMs> = token_result.expires_in().map(|d| now_ms() + d.as_millis() as i64);

        // Prefer new refresh_token if provided, otherwise keep the old one.
        let new_refresh = token_result
            .refresh_token()
            .map(|t| t.secret().as_str())
            .unwrap_or(refresh_token_value);

        self.token_repo
            .upsert(UpsertOAuthTokenParams {
                server_url,
                access_token: &new_access_token,
                refresh_token: Some(new_refresh),
                token_type: "bearer",
                expires_at,
                registration_id: row.registration_id,
                principal_id: row.principal_id.as_deref(),
            })
            .await?;

        debug!(server_url, "OAuth token refreshed successfully");
        Ok(new_access_token)
    }

    /// Resolve the token endpoint for refresh without re-registering.
    async fn resolve_token_endpoint(&self, server_url: &str) -> Result<TokenUrl, McpError> {
        // Discover only the endpoints (identity is already bound to the
        // token's registration); discovery is cheap and keeps refresh working
        // when the auth server moved hosts.
        let resolved = self.discover_endpoints(server_url).await?;
        TokenUrl::new(resolved.token_endpoint)
            .map_err(|e| McpError::OAuth(format!("Invalid token URL: {e}")))
    }

    /// Persist token response to DB, linked to the registration that minted
    /// it (design doc §4.2).
    async fn persist_token<TR: TokenResponse>(
        &self,
        server_url: &str,
        token_result: &TR,
        registration_id: Option<i64>,
    ) -> Result<(), McpError> {
        let expires_at: Option<TimestampMs> = token_result.expires_in().map(|d| now_ms() + d.as_millis() as i64);

        self.token_repo
            .upsert(UpsertOAuthTokenParams {
                server_url,
                access_token: token_result.access_token().secret(),
                refresh_token: token_result.refresh_token().map(|t| t.secret().as_str()),
                token_type: "bearer",
                expires_at,
                registration_id,
                principal_id: None,
            })
            .await?;

        Ok(())
    }

    /// Clear the pending login state.
    async fn clear_pending(&self) {
        let mut guard = self.pending.lock().await;
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// In-process fallback registration repository
// ---------------------------------------------------------------------------

/// In-process registration store backing `McpOAuthService::new[_dynamic]`
/// when no persistent repository is attached.
///
/// Keeps the same trait contract (identity key, upsert, list) so callers are
/// source-compatible with the SQLite repository; identities do NOT survive a
/// restart (design doc §11 requires the SQLite repository in production).
#[derive(Debug, Default)]
struct InMemoryOAuthClientRegistrationRepository {
    rows: std::sync::Mutex<Vec<OAuthClientRegistrationRow>>,
    next_id: std::sync::Mutex<i64>,
}

#[async_trait::async_trait]
impl IOAuthClientRegistrationRepository for InMemoryOAuthClientRegistrationRepository {
    async fn get_by_identity_key(
        &self,
        mcp_server_url: &str,
        resource_identifier: &str,
        authorization_server_issuer: &str,
        redirect_uri: &str,
    ) -> Result<Option<OAuthClientRegistrationRow>, nomifun_db::DbError> {
        let rows = self.rows.lock().expect("in-memory registration lock");
        Ok(rows
            .iter()
            .find(|row| {
                row.mcp_server_url == mcp_server_url
                    && row.resource_identifier == resource_identifier
                    && row.authorization_server_issuer == authorization_server_issuer
                    && row.redirect_uri == redirect_uri
            })
            .cloned())
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<OAuthClientRegistrationRow>, nomifun_db::DbError> {
        let rows = self.rows.lock().expect("in-memory registration lock");
        Ok(rows.iter().find(|row| row.id == id).cloned())
    }

    async fn insert(
        &self,
        params: UpsertOAuthClientRegistrationParams<'_>,
    ) -> Result<OAuthClientRegistrationRow, nomifun_db::DbError> {
        self.upsert(params).await
    }

    async fn upsert(
        &self,
        params: UpsertOAuthClientRegistrationParams<'_>,
    ) -> Result<OAuthClientRegistrationRow, nomifun_db::DbError> {
        let mut rows = self.rows.lock().expect("in-memory registration lock");
        let existing = rows.iter_mut().find(|row| {
            row.mcp_server_url == params.mcp_server_url
                && row.resource_identifier == params.resource_identifier
                && row.authorization_server_issuer == params.authorization_server_issuer
                && row.redirect_uri == params.redirect_uri
        });
        let now = nomifun_common::now_ms();
        if let Some(row) = existing {
            row.registration_mode = params.registration_mode.to_owned();
            row.client_id = params.client_id.to_owned();
            row.client_secret_ref = params.client_secret_ref.map(str::to_owned);
            row.registration_access_token_ref = params.registration_access_token_ref.map(str::to_owned);
            row.client_id_issued_at = params.client_id_issued_at;
            row.client_secret_expires_at = params.client_secret_expires_at;
            row.registration_client_uri = params.registration_client_uri.map(str::to_owned);
            row.updated_at = now;
            return Ok(row.clone());
        }
        let mut next = self.next_id.lock().expect("in-memory registration id lock");
        *next += 1;
        let row = OAuthClientRegistrationRow {
            id: *next,
            mcp_server_url: params.mcp_server_url.to_owned(),
            resource_identifier: params.resource_identifier.to_owned(),
            authorization_server_issuer: params.authorization_server_issuer.to_owned(),
            redirect_uri: params.redirect_uri.to_owned(),
            registration_mode: params.registration_mode.to_owned(),
            client_id: params.client_id.to_owned(),
            client_secret_ref: params.client_secret_ref.map(str::to_owned),
            registration_access_token_ref: params.registration_access_token_ref.map(str::to_owned),
            client_id_issued_at: params.client_id_issued_at,
            client_secret_expires_at: params.client_secret_expires_at,
            registration_client_uri: params.registration_client_uri.map(str::to_owned),
            created_at: now,
            updated_at: now,
        };
        rows.push(row.clone());
        Ok(row)
    }

    async fn list_by_server_url(
        &self,
        mcp_server_url: &str,
    ) -> Result<Vec<OAuthClientRegistrationRow>, nomifun_db::DbError> {
        let rows = self.rows.lock().expect("in-memory registration lock");
        Ok(rows
            .iter()
            .filter(|row| row.mcp_server_url == mcp_server_url)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: i64) -> Result<(), nomifun_db::DbError> {
        let mut rows = self.rows.lock().expect("in-memory registration lock");
        let before = rows.len();
        rows.retain(|row| row.id != id);
        if rows.len() == before {
            return Err(nomifun_db::DbError::NotFound(format!(
                "OAuth client registration '{id}' not found"
            )));
        }
        Ok(())
    }
}

/// Extract the path component (`/path`) from a redirect URL.
fn redirect_path(redirect_url: &str) -> String {
    redirect_url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, path)| format!("/{path}"))
        .unwrap_or_else(|| "/callback".to_owned())
}

/// Make an OAuth error message safe for UIs and logs: no tokens, no codes,
/// no full authorization URLs (design doc §6.2).
fn sanitize_oauth_error(error: &McpError) -> String {
    let message = error.to_string();
    // Authorization URLs carry `code`/`state` query parameters; keep only a
    // stable descriptor when one appears.
    if (message.contains("http://") || message.contains("https://"))
        && let Some(code) = error.oauth_error_code()
    {
        return format!("{code}: OAuth authorization flow failed");
    }
    message
}

/// Parse the RFC 9728 `resource_metadata="<url>"` pointer from a
/// `WWW-Authenticate` challenge value. `None` when absent or not an http(s)
/// URL.
fn parse_resource_metadata_challenge(challenge: &str) -> Option<String> {
    let challenge = challenge.trim();
    let challenge = challenge
        .strip_prefix("Bearer ")
        .or_else(|| challenge.strip_prefix("bearer "))
        .unwrap_or(challenge);
    challenge
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            let (key, value) = part.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("resource_metadata")
                .then(|| value.trim_matches('"'))
        })
        .next()
        .map(str::to_owned)
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
}

/// Scheme + authority of an http(s) URL (`https://host:port`), used to probe
/// the issuer-root well-known location. Falls back to the input when it
/// cannot be parsed.
fn origin_of(url: &str) -> &str {
    let Some(scheme_end) = url.find("://") else {
        return url;
    };
    let scheme = &url[..scheme_end];
    if scheme != "http" && scheme != "https" {
        return url;
    }
    let rest = &url[scheme_end + 3..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    &url[..scheme_end + 3 + authority.len()]
}

// ---------------------------------------------------------------------------
// Query parameter parsing
// ---------------------------------------------------------------------------

/// Parse `code` and `state` from the first line of an HTTP request.
///
/// Expects: `GET /callback?code=xxx&state=yyy HTTP/1.1`
fn parse_callback_query(request: &str) -> Result<(String, String), McpError> {
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| McpError::OAuth("Empty HTTP request".to_string()))?;

    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| McpError::OAuth("Malformed HTTP request line".to_string()))?;

    let query_str = path
        .split_once('?')
        .map(|(_, q)| q)
        .ok_or_else(|| McpError::OAuth("No query parameters in callback".to_string()))?;

    let mut code = None;
    let mut state = None;

    for pair in query_str.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "code" => code = Some(url_decode(value)),
                "state" => state = Some(url_decode(value)),
                _ => {}
            }
        }
    }

    let code = code.ok_or_else(|| McpError::OAuth("Missing 'code' in callback".to_string()))?;
    let state = state.ok_or_else(|| McpError::OAuth("Missing 'state' in callback".to_string()))?;

    Ok((code, state))
}

/// Minimal percent-decoding for query parameter values.
fn url_decode(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.bytes();

    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next();
            let lo = chars.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                let hex = [h, l];
                if let Ok(s) = std::str::from_utf8(&hex)
                    && let Ok(byte) = u8::from_str_radix(s, 16)
                {
                    result.push(byte as char);
                    continue;
                }
                // Malformed percent-encoding: keep as-is.
                result.push('%');
                result.push(h as char);
                result.push(l as char);
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_callback_query ------------------------------------------------

    #[test]
    fn parse_valid_callback_query() {
        let request = "GET /callback?code=abc123&state=xyz789 HTTP/1.1\r\nHost: localhost\r\n";
        let (code, state) = parse_callback_query(request).unwrap();
        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz789");
    }

    #[test]
    fn parse_callback_query_reversed_params() {
        let request = "GET /callback?state=s1&code=c1 HTTP/1.1\r\n";
        let (code, state) = parse_callback_query(request).unwrap();
        assert_eq!(code, "c1");
        assert_eq!(state, "s1");
    }

    #[test]
    fn parse_callback_query_with_extra_params() {
        let request = "GET /callback?code=c&foo=bar&state=s HTTP/1.1\r\n";
        let (code, state) = parse_callback_query(request).unwrap();
        assert_eq!(code, "c");
        assert_eq!(state, "s");
    }

    #[test]
    fn parse_callback_query_missing_code() {
        let request = "GET /callback?state=s HTTP/1.1\r\n";
        let err = parse_callback_query(request).unwrap_err();
        assert!(err.to_string().contains("Missing 'code'"));
    }

    #[test]
    fn parse_callback_query_missing_state() {
        let request = "GET /callback?code=c HTTP/1.1\r\n";
        let err = parse_callback_query(request).unwrap_err();
        assert!(err.to_string().contains("Missing 'state'"));
    }

    #[test]
    fn parse_callback_query_no_query_string() {
        let request = "GET /callback HTTP/1.1\r\n";
        let err = parse_callback_query(request).unwrap_err();
        assert!(err.to_string().contains("No query parameters"));
    }

    #[test]
    fn parse_callback_query_empty_request() {
        let err = parse_callback_query("").unwrap_err();
        assert!(err.to_string().contains("Empty HTTP request"));
    }

    // -- url_decode ----------------------------------------------------------

    #[test]
    fn url_decode_no_encoding() {
        assert_eq!(url_decode("hello"), "hello");
    }

    #[test]
    fn url_decode_percent_encoded() {
        assert_eq!(url_decode("hello%20world"), "hello world");
    }

    #[test]
    fn url_decode_plus_sign() {
        assert_eq!(url_decode("hello+world"), "hello world");
    }

    #[test]
    fn url_decode_special_characters() {
        assert_eq!(url_decode("%3D%26%3F"), "=&?");
    }

    #[test]
    fn url_decode_mixed() {
        assert_eq!(url_decode("a%20b+c%3Dd"), "a b c=d");
    }

    // -- McpOAuthService construction ----------------------------------------

    #[test]
    fn service_clone_is_independent() {
        let repo: Arc<dyn IOAuthTokenRepository> = Arc::new(MockTokenRepo);
        let http = reqwest::Client::new();
        let svc = McpOAuthService::new(repo, http);
        let _clone = svc.clone();
    }

    // -- Mock repositories ---------------------------------------------------

    struct MockTokenRepo;

    #[async_trait::async_trait]
    impl IOAuthTokenRepository for MockTokenRepo {
        async fn get_by_url(&self, _: &str) -> Result<Option<nomifun_db::models::OAuthTokenRow>, nomifun_db::DbError> {
            Ok(None)
        }

        async fn upsert(
            &self,
            _: UpsertOAuthTokenParams<'_>,
        ) -> Result<nomifun_db::models::OAuthTokenRow, nomifun_db::DbError> {
            unimplemented!()
        }

        async fn delete(&self, _: &str) -> Result<(), nomifun_db::DbError> {
            Ok(())
        }

        async fn list_authenticated_urls(&self) -> Result<Vec<String>, nomifun_db::DbError> {
            Ok(vec![])
        }
    }

    struct IdempotentDeleteRepo;

    #[async_trait::async_trait]
    impl IOAuthTokenRepository for IdempotentDeleteRepo {
        async fn get_by_url(&self, _: &str) -> Result<Option<nomifun_db::models::OAuthTokenRow>, nomifun_db::DbError> {
            Ok(None)
        }

        async fn upsert(
            &self,
            _: UpsertOAuthTokenParams<'_>,
        ) -> Result<nomifun_db::models::OAuthTokenRow, nomifun_db::DbError> {
            unimplemented!()
        }

        async fn delete(&self, url: &str) -> Result<(), nomifun_db::DbError> {
            Err(nomifun_db::DbError::NotFound(format!(
                "OAuth token for '{url}' not found"
            )))
        }

        async fn list_authenticated_urls(&self) -> Result<Vec<String>, nomifun_db::DbError> {
            Ok(vec![])
        }
    }

    struct ValidTokenRepo;

    #[async_trait::async_trait]
    impl IOAuthTokenRepository for ValidTokenRepo {
        async fn get_by_url(&self, _: &str) -> Result<Option<nomifun_db::models::OAuthTokenRow>, nomifun_db::DbError> {
            Ok(Some(nomifun_db::models::OAuthTokenRow {
                id: 0,
                server_url: "https://example.com".to_string(),
                access_token: "valid_access_token".to_string(),
                refresh_token: None,
                token_type: "bearer".to_string(),
                expires_at: Some(now_ms() + 3_600_000),
                registration_id: None,
                principal_id: None,
                created_at: now_ms(),
                updated_at: now_ms(),
            }))
        }

        async fn upsert(
            &self,
            _: UpsertOAuthTokenParams<'_>,
        ) -> Result<nomifun_db::models::OAuthTokenRow, nomifun_db::DbError> {
            unimplemented!()
        }

        async fn delete(&self, _: &str) -> Result<(), nomifun_db::DbError> {
            Ok(())
        }

        async fn list_authenticated_urls(&self) -> Result<Vec<String>, nomifun_db::DbError> {
            Ok(vec!["https://example.com".to_string()])
        }
    }

    struct ExpiredTokenRepo;

    #[async_trait::async_trait]
    impl IOAuthTokenRepository for ExpiredTokenRepo {
        async fn get_by_url(&self, _: &str) -> Result<Option<nomifun_db::models::OAuthTokenRow>, nomifun_db::DbError> {
            Ok(Some(nomifun_db::models::OAuthTokenRow {
                id: 0,
                server_url: "https://example.com".to_string(),
                access_token: "expired_token".to_string(),
                refresh_token: None,
                token_type: "bearer".to_string(),
                expires_at: Some(1000),
                registration_id: None,
                principal_id: None,
                created_at: 500,
                updated_at: 500,
            }))
        }

        async fn upsert(
            &self,
            _: UpsertOAuthTokenParams<'_>,
        ) -> Result<nomifun_db::models::OAuthTokenRow, nomifun_db::DbError> {
            unimplemented!()
        }

        async fn delete(&self, _: &str) -> Result<(), nomifun_db::DbError> {
            Ok(())
        }

        async fn list_authenticated_urls(&self) -> Result<Vec<String>, nomifun_db::DbError> {
            Ok(vec![])
        }
    }

    struct NoExpiryTokenRepo;

    #[async_trait::async_trait]
    impl IOAuthTokenRepository for NoExpiryTokenRepo {
        async fn get_by_url(&self, _: &str) -> Result<Option<nomifun_db::models::OAuthTokenRow>, nomifun_db::DbError> {
            Ok(Some(nomifun_db::models::OAuthTokenRow {
                id: 0,
                server_url: "https://example.com".to_string(),
                access_token: "no_expiry_token".to_string(),
                refresh_token: None,
                token_type: "bearer".to_string(),
                expires_at: None,
                registration_id: None,
                principal_id: None,
                created_at: now_ms(),
                updated_at: now_ms(),
            }))
        }

        async fn upsert(
            &self,
            _: UpsertOAuthTokenParams<'_>,
        ) -> Result<nomifun_db::models::OAuthTokenRow, nomifun_db::DbError> {
            unimplemented!()
        }

        async fn delete(&self, _: &str) -> Result<(), nomifun_db::DbError> {
            Ok(())
        }

        async fn list_authenticated_urls(&self) -> Result<Vec<String>, nomifun_db::DbError> {
            Ok(vec!["https://example.com".to_string()])
        }
    }

    // -- Service behavior tests ----------------------------------------------

    #[tokio::test]
    async fn check_status_no_token_returns_false() {
        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let status = svc.check_oauth_status("https://example.com").await.unwrap();
        assert!(!status.authenticated);
    }

    #[tokio::test]
    async fn check_status_with_valid_token() {
        let svc = McpOAuthService::new(Arc::new(ValidTokenRepo), reqwest::Client::new());
        let status = svc.check_oauth_status("https://example.com").await.unwrap();
        assert!(status.authenticated);
    }

    #[tokio::test]
    async fn check_status_with_expired_token() {
        let svc = McpOAuthService::new(Arc::new(ExpiredTokenRepo), reqwest::Client::new());
        let status = svc.check_oauth_status("https://example.com").await.unwrap();
        assert!(!status.authenticated);
    }

    #[tokio::test]
    async fn check_status_no_expiry_treated_as_valid() {
        let svc = McpOAuthService::new(Arc::new(NoExpiryTokenRepo), reqwest::Client::new());
        let status = svc.check_oauth_status("https://example.com").await.unwrap();
        assert!(status.authenticated);
    }

    #[tokio::test]
    async fn logout_idempotent_for_nonexistent() {
        let svc = McpOAuthService::new(Arc::new(IdempotentDeleteRepo), reqwest::Client::new());
        svc.logout("https://nonexistent.example.com").await.unwrap();
    }

    #[tokio::test]
    async fn get_authenticated_servers_empty() {
        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let urls = svc.get_authenticated_servers().await.unwrap();
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn get_authenticated_servers_returns_urls() {
        let svc = McpOAuthService::new(Arc::new(ValidTokenRepo), reqwest::Client::new());
        let urls = svc.get_authenticated_servers().await.unwrap();
        assert_eq!(urls, vec!["https://example.com"]);
    }

    #[tokio::test]
    async fn get_token_returns_none_when_no_token() {
        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let token = svc.get_token("https://example.com").await.unwrap();
        assert!(token.is_none());
    }

    #[tokio::test]
    async fn get_token_returns_access_token() {
        let svc = McpOAuthService::new(Arc::new(ValidTokenRepo), reqwest::Client::new());
        let token = svc.get_token("https://example.com").await.unwrap();
        assert_eq!(token.as_deref(), Some("valid_access_token"));
    }

    #[tokio::test]
    async fn get_token_requires_reauthorization_when_expired_without_refresh() {
        let svc = McpOAuthService::new(Arc::new(ExpiredTokenRepo), reqwest::Client::new());
        let error = svc
            .get_token("https://example.com")
            .await
            .expect_err("expired token without refresh must fail closed");
        assert!(matches!(error, McpError::ReauthorizationRequired));
    }

    // -- RFC 9728 discovery & pre-registered client --------------------------

    #[test]
    fn parses_resource_metadata_from_www_authenticate_challenge() {
        assert_eq!(
            parse_resource_metadata_challenge(
                r#"Bearer error="invalid_request", error_description="no token", resource_metadata="https://api.example.com/.well-known/oauth-protected-resource/mcp/""#
            )
            .as_deref(),
            Some("https://api.example.com/.well-known/oauth-protected-resource/mcp/")
        );
        assert_eq!(
            parse_resource_metadata_challenge(
                r#"Bearer error="invalid_request", error_description="no token", resource_metadata="https://api.example.com/.well-known/oauth-protected-resource/mcp/""#
            )
            .as_deref(),
            Some("https://api.example.com/.well-known/oauth-protected-resource/mcp/")
        );
        assert_eq!(
            parse_resource_metadata_challenge("Bearer realm=\"mcp\""),
            None,
            "no resource_metadata pointer"
        );
        assert_eq!(
            parse_resource_metadata_challenge(
                r#"Bearer resource_metadata="file:///etc/passwd""#
            ),
            None,
            "non-http(s) pointers are rejected"
        );
        assert_eq!(
            parse_resource_metadata_challenge(
                r#"Bearer resource_metadata="https://api.example.com/.well-known/oauth-protected-resource/mcp/""#
            )
            .as_deref(),
            Some("https://api.example.com/.well-known/oauth-protected-resource/mcp/"),
            "single-parameter challenge (no comma) must match"
        );
    }

    #[test]
    fn origin_of_extracts_scheme_and_authority() {
        assert_eq!(origin_of("https://api.mail.qq.com/mcp"), "https://api.mail.qq.com");
        assert_eq!(origin_of("http://127.0.0.1:8080/mcp"), "http://127.0.0.1:8080");
        assert_eq!(origin_of("https://host/path?a=1"), "https://host");
        assert_eq!(origin_of("https://host"), "https://host");
        assert_eq!(origin_of("not-a-url"), "not-a-url");
    }

    #[tokio::test]
    async fn env_pre_registered_client_wins_without_dynamic_registration() {
        unsafe {
            std::env::remove_var("MCP_OAUTH_CLIENT_ID");
            std::env::remove_var("MCP_OAUTH_CLIENT_SECRET");
            std::env::remove_var("MCP_OAUTH_REDIRECT_URI");
        }
        assert!(McpOAuthService::env_registered_client().is_none());

        unsafe { std::env::set_var("MCP_OAUTH_CLIENT_ID", "Iv1.test-client") };
        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let resolved = ResolvedOAuthServer {
            resource_identifier: String::new(),
            authorization_server_issuer: "https://auth.example.test".to_owned(),
            authorization_endpoint: "https://auth.example.test/authorize".to_owned(),
            token_endpoint: "https://auth.example.test/token".to_owned(),
            // 即使服务方发布 registration_endpoint，预注册也应优先（不触发动态注册）
            registration_endpoint: Some("https://auth.example.test/register".to_owned()),
            supported_pkce_methods: None,
            scopes_supported: None,
        };
        let identity = svc
            .resolve_client_identity(
                "https://mcp.example.test/mcp",
                &resolved,
                "http://127.0.0.1:8765/callback",
            )
            .await
            .expect("env pre-registered client must win");
        assert_eq!(identity.client_id, "Iv1.test-client");
        assert!(identity.client_secret.is_none());
        assert!(identity.registration_id.is_some());

        // Secret follows the env override too.
        unsafe { std::env::set_var("MCP_OAUTH_CLIENT_SECRET", "shh") };
        let identity = svc
            .resolve_client_identity(
                "https://mcp.example.test/mcp",
                &resolved,
                "http://127.0.0.1:8765/callback",
            )
            .await
            .expect("env pre-registered client must win");
        assert_eq!(identity.client_secret.as_deref(), Some("shh"));

        unsafe {
            std::env::remove_var("MCP_OAUTH_CLIENT_ID");
            std::env::remove_var("MCP_OAUTH_CLIENT_SECRET");
            std::env::remove_var("MCP_OAUTH_REDIRECT_URI");
        }
    }

    #[tokio::test]
    async fn no_registration_endpoint_and_no_pre_registered_client_fails_loudly() {
        unsafe {
            std::env::remove_var("MCP_OAUTH_CLIENT_ID");
            std::env::remove_var("MCP_OAUTH_CLIENT_SECRET");
        }
        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let resolved = ResolvedOAuthServer {
            resource_identifier: String::new(),
            authorization_server_issuer: "https://auth.example.test".to_owned(),
            authorization_endpoint: "https://auth.example.test/authorize".to_owned(),
            token_endpoint: "https://auth.example.test/token".to_owned(),
            registration_endpoint: None,
            supported_pkce_methods: None,
            scopes_supported: None,
        };
        let error = svc
            .resolve_client_identity(
                "https://mcp.example.test/mcp",
                &resolved,
                "http://127.0.0.1:8765/callback",
            )
            .await
            .expect_err("no endpoint and no pre-registered client must fail");
        assert!(matches!(error, McpError::PreRegisteredClientRequired(_)));
        assert_eq!(error.oauth_error_code(), Some("pre_registered_client_required"));
    }

    #[tokio::test]
    async fn discovery_falls_back_to_rfc_9728_protected_resource_metadata() {
        // Minimal local OAuth provider: RFC 9728 challenge on the resource,
        // protected-resource metadata document, and RFC 8414 metadata for the
        // authorization server.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let port = addr.port();
        let server = tokio::spawn(async move {
            for _ in 0..12 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 2048];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer).await;
                let request = String::from_utf8_lossy(&buffer);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, body, extra_header) = if path == "/mcp" {
                    (
                        "HTTP/1.1 401 Unauthorized\r\n",
                        String::new(),
                        format!(
                            "WWW-Authenticate: Bearer error=\"invalid_request\", resource_metadata=\"http://127.0.0.1:{port}/.well-known/oauth-protected-resource/mcp/\"\r\n"
                        ),
                    )
                } else if path.starts_with("/.well-known/oauth-protected-resource") {
                    (
                        "HTTP/1.1 200 OK\r\n",
                        format!(
                            r#"{{"resource":"http://127.0.0.1:{port}/mcp","authorization_servers":["http://127.0.0.1:{port}/oauth"]}}"#
                        ),
                        "Content-Type: application/json\r\n".to_string(),
                    )
                } else if path.starts_with("/oauth/.well-known/oauth-authorization-server") {
                    (
                        "HTTP/1.1 200 OK\r\n",
                        format!(
                            r#"{{"authorization_endpoint":"http://127.0.0.1:{port}/oauth/authorize","token_endpoint":"http://127.0.0.1:{port}/oauth/token","registration_endpoint":"http://127.0.0.1:{port}/oauth/register"}}"#
                        ),
                        "Content-Type: application/json\r\n".to_string(),
                    )
                } else {
                    eprintln!("mock: unmatched path {path}");
                    ("HTTP/1.1 404 Not Found\r\n", String::new(), String::new())
                };
                eprintln!("mock: serving {path} -> {}", status.trim());
                let response = format!(
                    "{status}{extra_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
            }
        });

        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let metadata = svc
            .discover_endpoints(&format!("http://127.0.0.1:{port}/mcp"))
            .await
            .expect("RFC 9728 discovery must resolve");
        assert_eq!(metadata.authorization_endpoint, format!("http://127.0.0.1:{port}/oauth/authorize"));
        assert_eq!(metadata.token_endpoint, format!("http://127.0.0.1:{port}/oauth/token"));
        assert_eq!(
            metadata.registration_endpoint.as_deref(),
            Some(format!("http://127.0.0.1:{port}/oauth/register").as_str())
        );
        server.abort();
    }

    #[tokio::test]
    async fn discovery_reports_clear_error_when_authorization_server_publishes_no_metadata() {
        // Mirrors the GitHub shape: RFC 9728 metadata exists but the
        // authorization server (github.com/login/oauth) publishes no RFC 8414
        // metadata — dynamic registration is impossible.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let port = addr.port();
        let server = tokio::spawn(async move {
            for _ in 0..12 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 2048];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer).await;
                let request = String::from_utf8_lossy(&buffer);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, body, extra_header) = if path == "/mcp" {
                    (
                        "HTTP/1.1 401 Unauthorized\r\n",
                        String::new(),
                        format!(
                            "WWW-Authenticate: Bearer resource_metadata=\"http://127.0.0.1:{port}/.well-known/oauth-protected-resource/mcp/\"\r\n"
                        ),
                    )
                } else if path.starts_with("/.well-known/oauth-protected-resource") {
                    (
                        "HTTP/1.1 200 OK\r\n",
                        format!(
                            r#"{{"resource":"http://127.0.0.1:{port}/mcp","authorization_servers":["http://127.0.0.1:{port}/login/oauth"]}}"#
                        ),
                        "Content-Type: application/json\r\n".to_string(),
                    )
                } else {
                    ("HTTP/1.1 404 Not Found\r\n", String::new(), String::new())
                };
                let response = format!(
                    "{status}{extra_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await;
            }
        });

        let svc = McpOAuthService::new(Arc::new(MockTokenRepo), reqwest::Client::new());
        let error = svc
            .discover_endpoints(&format!("http://127.0.0.1:{port}/mcp"))
            .await
            .expect_err("discovery without server metadata must fail with a clear error");
        let message = error.to_string();
        assert!(message.contains("pre-registered"), "message must point at a pre-registered client: {message}");
        server.abort();
    }
}
