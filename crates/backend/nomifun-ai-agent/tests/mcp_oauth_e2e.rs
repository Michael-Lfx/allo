//! End-to-end MCP OAuth runtime loop against a local OAuth-protected MCP
//! server:
//!
//! login (RFC 9728 discovery → RFC 7591 dynamic registration → PKCE →
//! loopback callback) → encrypted token storage → request-time bearer
//! injection (Authorization header) → real MCP tool call → server-side token
//! revocation → 401 → refresh once → header update → single retry → success.
//!
//! The browser step is driven by a hook that captures the authorization URL;
//! no real browser is involved.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nomifun_ai_agent::factory::mcp_oauth::{NomiMcpOAuthRefresher, inject_oauth_bearer};
use nomifun_common::now_ms;
use nomifun_db::models::OAuthTokenRow;
use nomifun_db::{IOAuthTokenRepository, UpsertOAuthTokenParams};
use nomifun_mcp::McpOAuthService;
use nomi_mcp::config::{McpServerConfig, TransportType};
use nomi_mcp::manager::McpManager;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

// ---------------------------------------------------------------------------
// In-memory token repository
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MemoryTokenRepo {
    rows: Arc<Mutex<HashMap<String, OAuthTokenRow>>>,
}

#[async_trait]
impl IOAuthTokenRepository for MemoryTokenRepo {
    async fn get_by_url(&self, server_url: &str) -> Result<Option<OAuthTokenRow>, nomifun_db::DbError> {
        Ok(self.rows.lock().unwrap().get(server_url).cloned())
    }

    async fn upsert(&self, params: UpsertOAuthTokenParams<'_>) -> Result<OAuthTokenRow, nomifun_db::DbError> {
        let now = now_ms();
        let row = OAuthTokenRow {
            id: 1,
            server_url: params.server_url.to_owned(),
            access_token: params.access_token.to_owned(),
            refresh_token: params.refresh_token.map(str::to_owned),
            token_type: params.token_type.to_owned(),
            expires_at: params.expires_at,
            created_at: now,
            updated_at: now,
        };
        self.rows.lock().unwrap().insert(params.server_url.to_owned(), row.clone());
        Ok(row)
    }

    async fn delete(&self, server_url: &str) -> Result<(), nomifun_db::DbError> {
        self.rows
            .lock()
            .unwrap()
            .remove(server_url)
            .map(|_| ())
            .ok_or_else(|| nomifun_db::DbError::NotFound(format!("OAuth token for '{server_url}' not found")))
    }

    async fn list_authenticated_urls(&self) -> Result<Vec<String>, nomifun_db::DbError> {
        Ok(self.rows.lock().unwrap().keys().cloned().collect())
    }
}

// ---------------------------------------------------------------------------
// Local OAuth-protected MCP server (wiremock)
// ---------------------------------------------------------------------------

/// Dynamic `Respond` for endpoints whose reply depends on the request
/// (authorize redirect, token grant selection, MCP method dispatch and the
/// Authorization-header gate).
struct FnRespond<F>(F);

impl<F> Respond for FnRespond<F>
where
    F: Fn(&Request) -> ResponseTemplate + Send + Sync,
{
    fn respond(&self, request: &Request) -> ResponseTemplate {
        (self.0)(request)
    }
}

const ACCESS_TOKEN: &str = "e2e-access-token";
const REFRESHED_ACCESS_TOKEN: &str = "e2e-refreshed-access-token";
const REFRESH_TOKEN: &str = "e2e-refresh-token";

/// Serve the complete OAuth + MCP surface. When `revoked` flips, the MCP
/// endpoint rejects the original access token with 401 (forcing the
/// refresh-retry path) and only accepts the refreshed token.
async fn serve_oauth_mcp_server(revoked: Arc<AtomicBool>) -> MockServer {
    let server = MockServer::start().await;
    let base = server.uri();

    // RFC 9728: the resource itself challenges with the metadata pointer.
    Mock::given(method("GET"))
        .and(path("/mcp"))
        .respond_with(ResponseTemplate::new(401).insert_header(
            "WWW-Authenticate",
            format!("Bearer error=\"invalid_request\", resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp/\"")
        ))
        .mount(&server)
        .await;

    // Protected-resource metadata document.
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource/mcp/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": format!("{base}/mcp"),
            "authorization_servers": [format!("{base}/oauth")],
            "bearer_methods_supported": ["header"]
        })))
        .mount(&server)
        .await;

    // RFC 8414 authorization server metadata (with dynamic registration).
    Mock::given(method("GET"))
        .and(path("/oauth/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorization_endpoint": format!("{base}/oauth/authorize"),
            "token_endpoint": format!("{base}/oauth/token"),
            "registration_endpoint": format!("{base}/oauth/register")
        })))
        .mount(&server)
        .await;

    // RFC 7591 dynamic client registration.
    Mock::given(method("POST"))
        .and(path("/oauth/register"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "client_id": "e2e-client",
            "client_secret": null,
            "redirect_uris": [],
            "grant_types": ["authorization_code", "refresh_token"],
            "token_endpoint_auth_method": "none"
        })))
        .mount(&server)
        .await;

    // Authorize: echo state back through the registered redirect URI.
    let authorize_base = base.clone();
    Mock::given(method("GET"))
        .and(path("/oauth/authorize"))
        .respond_with(FnRespond(move |request: &Request| {
            let redirect_uri = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "redirect_uri")
                .map(|(_, value)| value.to_string())
                .unwrap_or_else(|| format!("{authorize_base}/callback"));
            let state = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "state")
                .map(|(_, value)| value.to_string())
                .unwrap_or_default();
            ResponseTemplate::new(302).insert_header(
                "Location",
                format!("{redirect_uri}?code=e2e-auth-code&state={state}"),
            )
        }))
        .mount(&server)
        .await;

    // Token endpoint: authorization code → access token; refresh_token grant →
    // refreshed access token.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(FnRespond(|request: &Request| {
            let body = String::from_utf8_lossy(&request.body);
            if body.contains("refresh_token") {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": REFRESHED_ACCESS_TOKEN,
                    "token_type": "bearer",
                    "expires_in": 3600,
                    "refresh_token": REFRESH_TOKEN
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": ACCESS_TOKEN,
                    "token_type": "bearer",
                    "expires_in": 3600,
                    "refresh_token": REFRESH_TOKEN
                }))
            }
        }))
        .mount(&server)
        .await;

    // MCP streamable-HTTP endpoint, gated on the bearer token.
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(FnRespond(move |request: &Request| {
            let auth = request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            let valid = auth == format!("Bearer {ACCESS_TOKEN}") || auth == format!("Bearer {REFRESHED_ACCESS_TOKEN}");
            if !valid || (revoked.load(Ordering::SeqCst) && auth == format!("Bearer {ACCESS_TOKEN}")) {
                return ResponseTemplate::new(401);
            }
            let body = String::from_utf8_lossy(&request.body);
            let response = if body.contains("\"initialize\"") {
                serde_json::json!({
                    "jsonrpc": "2.0", "id": 1,
                    "result": { "protocolVersion": "2025-03-26", "capabilities": { "tools": {} } }
                })
            } else if body.contains("notifications/initialized") {
                return ResponseTemplate::new(200).set_body_string("");
            } else if body.contains("tools/list") {
                serde_json::json!({
                    "jsonrpc": "2.0", "id": 2,
                    "result": { "tools": [{
                        "name": "echo",
                        "description": "echo back the input",
                        "inputSchema": { "type": "object", "properties": {} }
                    }] }
                })
            } else if body.contains("tools/call") {
                serde_json::json!({
                    "jsonrpc": "2.0", "id": 3,
                    "result": { "content": [{ "type": "text", "text": "pong" }], "isError": false }
                })
            } else {
                return ResponseTemplate::new(200).set_body_string("");
            };
            ResponseTemplate::new(200).set_body_json(response)
        }))
        .mount(&server)
        .await;

    server
}

fn make_mcp_config(url: &str, headers: HashMap<String, String>) -> McpServerConfig {
    McpServerConfig {
        transport: TransportType::StreamableHttp,
        command: None,
        args: None,
        env: None,
        url: Some(url.to_owned()),
        headers: Some(headers),
        deferred: Some(false),
        request_timeout_secs: Some(30),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mcp_oauth_full_loop_login_inject_call_refresh_retry() {
    let revoked = Arc::new(AtomicBool::new(false));
    let server = serve_oauth_mcp_server(revoked.clone()).await;
    let mcp_url = format!("{}/mcp", server.uri());

    // --- 1. login: discovery → dynamic registration → PKCE → loopback ---
    let captured_authorize_url: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let hook = {
        let captured = captured_authorize_url.clone();
        Arc::new(move |url: &str| {
            *captured.lock().unwrap() = Some(url.to_owned());
        }) as Arc<dyn Fn(&str) + Send + Sync>
    };
    let repo = Arc::new(MemoryTokenRepo::default());
    let oauth = McpOAuthService::new_with_browser_hook(repo.clone(), reqwest::Client::new(), Some(hook));

    let login_task = tokio::spawn({
        let oauth = oauth.clone();
        let url = mcp_url.clone();
        async move { oauth.login(&url).await }
    });

    // Wait for the hook to capture the authorization URL, then "use the
    // browser": follow the authorize redirect (302 → loopback callback).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let authorize_url = loop {
        if let Some(url) = captured_authorize_url.lock().unwrap().clone() {
            break url;
        }
        assert!(tokio::time::Instant::now() < deadline, "authorize URL was not captured");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        authorize_url.contains("/oauth/authorize"),
        "authorize URL must point at the local authorization server: {authorize_url}"
    );
    let _browser = reqwest::Client::new().get(&authorize_url).send().await.expect("browser GET");
    let login_result = login_task.await.expect("login task").expect("login must not error");
    assert!(login_result.success, "login failed: {:?}", login_result.error);

    // --- 2. token storage ---
    let status = oauth.check_oauth_status(&mcp_url).await.expect("auth status");
    assert!(status.authenticated, "token must be stored and valid");
    let stored = oauth.get_token(&mcp_url).await.expect("get_token");
    assert_eq!(stored.as_deref(), Some(ACCESS_TOKEN));

    // --- 3. request-time injection (the factory helper used at session build) ---
    let mut headers = HashMap::new();
    inject_oauth_bearer(Some(&oauth), &mcp_url, &mut headers)
        .await
        .expect("injection must not error");
    assert_eq!(
        headers.get("Authorization").map(String::as_str),
        Some(format!("Bearer {ACCESS_TOKEN}").as_str()),
        "stored token must be injected as the Authorization header"
    );

    // --- 4. real tool call over the runtime (McpManager + refresher) ---
    let refresher = NomiMcpOAuthRefresher::new(oauth.clone());
    let mut configs = HashMap::new();
    configs.insert("e2e".to_owned(), make_mcp_config(&mcp_url, headers));
    let manager = McpManager::connect_all_with_oauth(&configs, Some(Arc::new(refresher)))
        .await
        .expect("manager must connect with the injected token");

    let tool_names = manager
        .all_tools()
        .into_iter()
        .map(|(server, tool)| format!("{server}/{}", tool.name))
        .collect::<Vec<_>>();
    assert_eq!(tool_names, vec!["e2e/echo"], "tools must be discovered: {tool_names:?}");

    let first = manager.call_tool("e2e", "echo", serde_json::json!({})).await.expect("first call");
    assert_eq!(first.text, "pong");

    // --- 5. token revocation → 401 → refresh once → header update → retry ---
    revoked.store(true, Ordering::SeqCst);
    let second = manager.call_tool("e2e", "echo", serde_json::json!({})).await.expect("retry succeeds");
    assert_eq!(second.text, "pong", "the refreshed token must succeed on the retry");

    // The refreshed access token must have been persisted.
    let refreshed = oauth.get_token(&mcp_url).await.expect("get_token after refresh");
    assert_eq!(refreshed.as_deref(), Some(REFRESHED_ACCESS_TOKEN));
}
