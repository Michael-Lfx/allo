//! RFC 7591 dynamic client registration + RFC 8414/9728 discovery acceptance
//! tests (design doc `docs/agent-store/06-connector-oauth-security.md` §14).
//!
//! Every test drives a local mock OAuth + MCP platform (axum) — no real
//! services are contacted. Coverage:
//!   1. metadata without registration_endpoint and no pre-registered client
//!      → `pre_registered_client_required`, no token request;
//!   2. RFC 7591 payload + persisted client id; authorize/token exchange use
//!      that id and the PKCE verifier;
//!   3. rebuilt service reuses the persisted registration: no re-register and
//!      refresh uses the original dynamic client id;
//!   4. pre-registered client skips dynamic registration (env channel);
//!   5. registration rejects redirect_uri → `redirect_uri_not_allowed`,
//!      the authorize flow never starts;
//!   6. GET 405 + unauthenticated initialize POST 401 with resource_metadata
//!      → RFC 9728 discovery succeeds;
//!   7. callback path/state validation;
//!   9. login error surfaces never carry codes/verifiers/authorization URLs.
//!
//! Acceptance #8 (401 → refresh once → retry once) is covered end-to-end by
//! `nomifun-ai-agent/tests/mcp_oauth_e2e.rs` over the runtime transport.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::Json;
use axum::http::Request;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use nomifun_api_types::OAuthLoginResponse;
use nomifun_db::{
    IOAuthClientRegistrationRepository, IOAuthTokenRepository, SqliteOAuthClientRegistrationRepository,
    SqliteOAuthTokenRepository,
};
use nomifun_mcp::McpOAuthService;

// ---------------------------------------------------------------------------
// Shared mock platform
// ---------------------------------------------------------------------------

#[derive(Default)]
struct MockLog {
    register_bodies: Vec<serde_json::Value>,
    token_bodies: Vec<String>,
    token_auth_headers: Vec<String>,
    authorize_hits: usize,
    mcp_post_hits: usize,
}

type SharedLog = Arc<Mutex<MockLog>>;

/// Tests in this file touch `MCP_OAUTH_*` process environment variables (the
/// pre-registered client channel), so all tests serialize on one lock; cargo
/// may otherwise interleave them and leak env state between cases.
fn env_lock() -> &'static tokio::sync::Mutex<()> {
    use std::sync::OnceLock;
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn base64_encode(input: &str) -> String {
    let bytes = input.as_bytes();
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[derive(Clone)]
struct MockConfig {
    /// GET on the MCP path returns 405 (forces the initialize POST probe).
    get_method_not_allowed: bool,
    /// GET on the MCP path carries the RFC 9728 challenge (default); when
    /// false the GET is a plain 404 so only the POST probe can resolve.
    get_challenge: bool,
    /// Include `registration_endpoint` in RFC 8414 metadata.
    registration_endpoint: bool,
    /// `/oauth/register` rejects with a redirect_uri error.
    reject_redirect_uri: bool,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            get_method_not_allowed: false,
            get_challenge: true,
            registration_endpoint: true,
            reject_redirect_uri: false,
        }
    }
}

/// Serve the full OAuth + MCP mock platform. Returns the MCP URL.
async fn serve_platform(config: MockConfig, log: SharedLog) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let challenge = |base: &str| {
        format!(
            "Bearer error=\"invalid_request\", resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp/\""
        )
    };
    let challenge_get = challenge(&base);
    let challenge_post = challenge(&base);

    let router = axum::Router::new()
        .route(
            "/mcp",
            get({
                let config = config.clone();
                let challenge = challenge_get.clone();
                move || {
                    let config = config.clone();
                    let challenge = challenge.clone();
                    async move {
                        if config.get_method_not_allowed {
                            return (StatusCode::METHOD_NOT_ALLOWED, String::new()).into_response();
                        }
                        if config.get_challenge {
                            return (
                                StatusCode::UNAUTHORIZED,
                                [(
                                    axum::http::header::WWW_AUTHENTICATE,
                                    challenge,
                                )],
                                "",
                            )
                                .into_response();
                        }
                        (StatusCode::NOT_FOUND, String::new()).into_response()
                    }
                }
            }),
        )
        .route(
            "/mcp",
            post({
                let log = log.clone();
                let config = config.clone();
                let challenge = challenge_post.clone();
                move |headers: axum::http::HeaderMap| {
                    let log = log.clone();
                    let config = config.clone();
                    let challenge = challenge.clone();
                    async move {
                        log.lock().unwrap().mcp_post_hits += 1;
                        if headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .map(|value| value.starts_with("Bearer "))
                            .unwrap_or(false)
                        {
                            return (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "jsonrpc": "2.0", "id": 1,
                                    "result": { "protocolVersion": "2025-11-25", "capabilities": {} }
                                })),
                            )
                                .into_response();
                        }
                        if config.get_challenge {
                            (
                                StatusCode::UNAUTHORIZED,
                                [(
                                    axum::http::header::WWW_AUTHENTICATE,
                                    challenge,
                                )],
                                "",
                            )
                                .into_response()
                        } else {
                            (StatusCode::NOT_FOUND, String::new()).into_response()
                        }
                    }
                }
            }),
        )
        .route(
            "/.well-known/oauth-protected-resource/mcp/",
            get({
                let base = base.clone();
                move || {
                    let base = base.clone();
                    async move {
                        Json(serde_json::json!({
                            "resource": format!("{base}/mcp"),
                            "authorization_servers": [format!("{base}/oauth")],
                            "bearer_methods_supported": ["header"]
                        }))
                    }
                }
            }),
        )
        .route(
            "/oauth/.well-known/oauth-authorization-server",
            get({
                let base = base.clone();
                let registration_endpoint = config.registration_endpoint;
                move || {
                    let base = base.clone();
                    async move {
                        let mut metadata = serde_json::json!({
                            "issuer": format!("{base}/oauth"),
                            "authorization_endpoint": format!("{base}/oauth/authorize"),
                            "token_endpoint": format!("{base}/oauth/token"),
                            "code_challenge_methods_supported": ["S256"],
                            "scopes_supported": ["email", "offline_access"]
                        });
                        if registration_endpoint {
                            metadata["registration_endpoint"] =
                                serde_json::json!(format!("{base}/oauth/register"));
                        }
                        Json(metadata)
                    }
                }
            }),
        )
        .route(
            "/oauth/register",
            post({
                let log = log.clone();
                let reject_redirect_uri = config.reject_redirect_uri;
                move |Json(body): Json<serde_json::Value>| {
                    let log = log.clone();
                    async move {
                        log.lock().unwrap().register_bodies.push(body.clone());
                        if reject_redirect_uri {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(serde_json::json!({
                                    "error": "invalid_redirect_uri",
                                    "error_description": "redirect_uri is not in the allow-list"
                                })),
                            )
                                .into_response();
                        }
                        (
                            StatusCode::CREATED,
                            Json(serde_json::json!({
                                "client_id": "dyn-client-1",
                                "client_secret": "dyn-secret",
                                "client_id_issued_at": 1700000000,
                                "redirect_uris": body.get("redirect_uris").cloned().unwrap_or_default(),
                                "grant_types": ["authorization_code", "refresh_token"],
                                "token_endpoint_auth_method": "none"
                            })),
                        )
                            .into_response()
                    }
                }
            }),
        )
        .route(
            "/oauth/authorize",
            get({
                let log = log.clone();
                let base = base.clone();
                move |query: axum::extract::Query<HashMap<String, String>>| {
                    let log = log.clone();
                    async move {
                        log.lock().unwrap().authorize_hits += 1;
                        let redirect_uri = query
                            .get("redirect_uri")
                            .cloned()
                            .unwrap_or_else(|| format!("{base}/callback"));
                        let state = query.get("state").cloned().unwrap_or_default();
                        (
                            StatusCode::FOUND,
                            [(
                                axum::http::header::LOCATION,
                                format!("{redirect_uri}?code=auth-code-1&state={state}"),
                            )],
                            "",
                        )
                            .into_response()
                    }
                }
            }),
        )
        .route(
            "/oauth/token",
            post({
                let log = log.clone();
                move |req: Request<axum::body::Body>| {
                    let log = log.clone();
                    async move {
                        let auth = req
                            .headers()
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or_default()
                            .to_owned();
                        let bytes =
                            axum::body::to_bytes(req.into_body(), 64 * 1024).await.unwrap();
                        let body = String::from_utf8_lossy(&bytes).into_owned();
                        let mut guard = log.lock().unwrap();
                        guard.token_bodies.push(body.clone());
                        guard.token_auth_headers.push(auth);
                        drop(guard);
                        let refreshed = body.contains("refresh_token");
                        Json(serde_json::json!({
                            "access_token": if refreshed { "refreshed-token" } else { "access-token-1" },
                            "token_type": "bearer",
                            "expires_in": 3600,
                            "refresh_token": "refresh-token-1"
                        }))
                    }
                }
            }),
        );

    let serve = router;

    // Borrow config pieces before moving into the task.
    let _ = &config;
    tokio::spawn(async move {
        axum::serve(listener, serve).await.unwrap();
    });

    format!("{base}/mcp")
}

fn test_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test http client")
}

/// Build a service with a browser hook that records the authorization URL.
fn service_with_hook(
    token_repo: Arc<dyn IOAuthTokenRepository>,
    registration_repo: Arc<dyn IOAuthClientRegistrationRepository>,
    capture: Arc<Mutex<Option<String>>>,
) -> McpOAuthService {
    let hook = {
        let capture = capture.clone();
        Arc::new(move |url: &str| {
            *capture.lock().unwrap() = Some(url.to_owned());
        }) as Arc<dyn Fn(&str) + Send + Sync>
    };
    McpOAuthService::new_with_browser_hook(token_repo, test_http_client(), Some(hook))
        .with_registration_repository(registration_repo)
}

async fn make_repos() -> (
    Arc<dyn IOAuthTokenRepository>,
    Arc<dyn IOAuthClientRegistrationRepository>,
) {
    let db = nomifun_db::init_database_memory().await.unwrap();
    (
        Arc::new(SqliteOAuthTokenRepository::new(db.pool().clone())),
        Arc::new(SqliteOAuthClientRegistrationRepository::new(db.pool().clone())),
    )
}

/// Run a login to completion: spawn `login`, follow the authorize redirect
/// (the "browser"), wait for the result.
async fn run_login(
    oauth: &McpOAuthService,
    capture: &Arc<Mutex<Option<String>>>,
    server_url: &str,
) -> OAuthLoginResponse {
    let oauth = oauth.clone();
    let server_url = server_url.to_owned();
    let task = tokio::spawn(async move { oauth.login(&server_url).await });
    let login_error: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    let watched_error = login_error.clone();
    let watched = task;
    let handle = tokio::spawn(async move {
        let result = watched.await.expect("login task panicked").expect("login returns");
        if !result.success {
            *watched_error.lock().unwrap() = Some(format!("{:?}", result));
        }
        result
    });

    // Wait for the authorize URL, then use the browser. The lock is released
// before any await (no guard held across await points).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let captured = capture.lock().unwrap().clone();
        if let Some(url) = captured {
            let _browser = reqwest::Client::new()
                .get(&url)
                .send()
                .await
                .expect("browser GET");
            break;
        }
        if let Some(error) = login_error.lock().unwrap().clone() {
            panic!("login failed before authorize: {error}");
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "authorize URL was not captured"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = &capture;
    let join_result = tokio::time::timeout(Duration::from_secs(60), handle).await;
    join_result
        .expect("login task join timed out — login is stuck")
        .expect("login returns")
}

use std::time::Duration;

/// Run a login that is EXPECTED to fail before the browser step (discovery or
/// client-identity resolution errors). Needed because `run_login` drives the
/// browser; these failures never reach the authorize redirect.
async fn run_failing_login(oauth: &McpOAuthService, server_url: &str) -> OAuthLoginResponse {
    let oauth = oauth.clone();
    let server_url = server_url.to_owned();
    tokio::time::timeout(Duration::from_secs(45), async move { oauth.login(&server_url).await })
        .await
        .expect("failing login must return within 45s")
        .expect("login returns")
}

/// Extract the loopback callback origin+path (the `redirect_uri` query
/// parameter of the captured authorize URL, percent-decoded).
fn callback_base_from_authorize(capture: &Arc<Mutex<Option<String>>>) -> String {
    let url = capture
        .lock()
        .unwrap()
        .clone()
        .expect("authorize URL must have been captured");
    let redirect = url
        .split("redirect_uri=")
        .nth(1)
        .expect("authorize URL must carry redirect_uri")
        .split('&')
        .next()
        .expect("redirect_uri query fragment");
    percent_decode(redirect)
}

fn percent_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or_default();
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Acceptance tests
// ---------------------------------------------------------------------------

/// §10 #6 — GET is rejected (405), discovery resolves through the
/// unauthenticated initialize POST challenge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_via_initialize_post_when_get_rejected() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(
        MockConfig {
            get_method_not_allowed: true,
            ..Default::default()
        },
        log.clone(),
    )
    .await;

    let (token_repo, registration_repo) = make_repos().await;
    let capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let oauth = service_with_hook(token_repo.clone(), registration_repo.clone(), capture.clone());

    let result = run_login(&oauth, &capture, &mcp_url).await;
    assert!(result.success, "login must succeed via POST discovery: {:?}", result.error);

    // The MCP endpoint must have been probed with the initialize POST.
    let hits = log.lock().unwrap().mcp_post_hits;
    assert!(hits >= 1, "initialize POST probe must hit the resource");
}

/// §10 #1 — no registration endpoint and no pre-registered client:
/// `pre_registered_client_required`, no token request ever sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_registration_endpoint_without_pre_registered_client_fails_loudly() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(
        MockConfig {
            registration_endpoint: false,
            ..Default::default()
        },
        log.clone(),
    )
    .await;

    let (token_repo, registration_repo) = make_repos().await;
    let oauth = McpOAuthService::new_dynamic(token_repo).with_registration_repository(registration_repo);

    let result = run_failing_login(&oauth, &mcp_url).await;
    assert!(!result.success, "login must fail without any client identity");
    assert_eq!(
        result.error_code.as_deref(),
        Some("pre_registered_client_required")
    );
    assert!(
        log.lock().unwrap().token_bodies.is_empty(),
        "no token request may be sent without a client identity"
    );
    assert!(
        log.lock().unwrap().register_bodies.is_empty(),
        "no dynamic registration may be attempted without a registration endpoint"
    );
    assert_eq!(log.lock().unwrap().authorize_hits, 0);
}

/// §10 #2 — RFC 7591 payload; client id persisted; exchange uses the
/// registered id with a PKCE verifier.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dynamic_registration_persists_and_exchange_uses_registered_id() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(MockConfig::default(), log.clone()).await;

    let (token_repo, registration_repo) = make_repos().await;
    let capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let oauth = service_with_hook(token_repo.clone(), registration_repo.clone(), capture.clone());

    let result = run_login(&oauth, &capture, &mcp_url).await;
    assert!(result.success, "login failed: {:?}", result.error);

    // Snapshot all mock observations inside one guard scope — a `&` borrow
    // through the temporary guard would extend the guard's lifetime to the
    // end of this function and deadlock the next `log.lock()`.
    let (redirect, token_auth, token_body, register_payload) = {
        let guard = log.lock().unwrap();
        let register = &guard.register_bodies[0];
        // RFC 7591 payload shape (§6.2).
        assert_eq!(register["client_name"], "Nomifun MCP Client");
        assert_eq!(
            register["token_endpoint_auth_method"],
            serde_json::json!("none")
        );
        assert_eq!(
            register["grant_types"],
            serde_json::json!(["authorization_code", "refresh_token"])
        );
        let redirects = register["redirect_uris"]
            .as_array()
            .expect("redirect_uris must be present");
        assert_eq!(redirects.len(), 1);
        let redirect = redirects[0].as_str().unwrap().to_owned();
        assert!(
            redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"),
            "loopback callback expected, got {redirect}"
        );

        // Token exchange authenticates with the REGISTERED client identity
        // (confidential client → HTTP Basic) and carries the PKCE verifier.
        let token_auth = guard.token_auth_headers[0].clone();
        let expected = format!("Basic {}", base64_encode("dyn-client-1:dyn-secret"));
        assert_eq!(
            token_auth.as_str(),
            expected.as_str(),
            "exchange must authenticate as the dynamically registered client"
        );

        let token_body = guard.token_bodies[0].clone();
        assert!(
            token_body.contains("code_verifier="),
            "PKCE verifier must be present in the exchange: {token_body}"
        );
        (redirect, token_auth, token_body, register.clone())
    };
    let _ = (token_auth, token_body, register_payload);

    // The registration is persisted under the identity key.
    let registrations = registration_repo
        .list_by_server_url(&mcp_url)
        .await
        .expect("list registrations");
    assert_eq!(registrations.len(), 1, "exactly one registration");
    let row = &registrations[0];
    assert_eq!(row.registration_mode, "dynamic");
    assert_eq!(row.client_id, "dyn-client-1");
    assert_eq!(row.redirect_uri, redirect);
    assert_eq!(row.resource_identifier, format!("{mcp_url}"));

    // Token is linked to the registration.
    let token = token_repo
        .get_by_url(&mcp_url)
        .await
        .expect("token lookup")
        .expect("token stored");
    assert_eq!(token.registration_id, Some(row.id));

    // Authorize URL was built from the registered redirect URI and carries the
    // dynamically registered client id.
    let authorize_url = capture.lock().unwrap().clone().unwrap();
    assert!(authorize_url.contains("/oauth/authorize"), "{authorize_url}");
    assert!(
        authorize_url.contains("client_id=dyn-client-1"),
        "authorize must carry the registered client id: {authorize_url}"
    );
}

/// §10 #3 — a rebuilt service (fresh instances over the same database)
/// reuses the persisted registration: no re-registration and refresh uses the
/// original dynamic client id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rebuilt_service_reuses_registration_for_refresh() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(MockConfig::default(), log.clone()).await;

    let db = nomifun_db::init_database_memory().await.unwrap();
    {
        let token_repo: Arc<dyn IOAuthTokenRepository> =
            Arc::new(SqliteOAuthTokenRepository::new(db.pool().clone()));
        let registration_repo: Arc<dyn IOAuthClientRegistrationRepository> =
            Arc::new(SqliteOAuthClientRegistrationRepository::new(db.pool().clone()));
        let capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let oauth = service_with_hook(token_repo.clone(), registration_repo.clone(), capture.clone());
        let result = run_login(&oauth, &capture, &mcp_url).await;
        assert!(result.success, "first login failed: {:?}", result.error);
    }

    // "Application restart": brand-new service + repositories on the same DB.
    let register_count_before = log.lock().unwrap().register_bodies.len();
    let token_repo: Arc<dyn IOAuthTokenRepository> =
        Arc::new(SqliteOAuthTokenRepository::new(db.pool().clone()));
    let registration_repo: Arc<dyn IOAuthClientRegistrationRepository> =
        Arc::new(SqliteOAuthClientRegistrationRepository::new(db.pool().clone()));
    let oauth = McpOAuthService::new_dynamic(token_repo.clone())
        .with_registration_repository(registration_repo.clone());

    // Refresh must use the ORIGINAL dynamic client id and NOT re-register.
    let refreshed = oauth
        .refresh_access_token(&mcp_url)
        .await
        .expect("refresh with persisted registration");
    assert_eq!(refreshed, "refreshed-token");

    {
        let log_guard = log.lock().unwrap();
        assert_eq!(
            log_guard.register_bodies.len(),
            register_count_before,
            "a rebuilt service must reuse the persisted registration, never re-register"
        );
        // Find the refresh request and verify it authenticates with the ORIGINAL
        // dynamic client identity (HTTP Basic, confidential client).
        let refresh_index = log_guard
            .token_bodies
            .iter()
            .position(|body| body.contains("refresh_token"))
            .expect("refresh request recorded");
        assert_eq!(
            log_guard.token_auth_headers[refresh_index],
            format!("Basic {}", base64_encode("dyn-client-1:dyn-secret")),
            "refresh must authenticate as the original dynamic client"
        );
    } // log guard dropped before any further await

    // The refreshed token stays linked to the same registration.
    let token = token_repo
        .get_by_url(&mcp_url)
        .await
        .expect("token lookup")
        .expect("token stored");
    let registration = registration_repo
        .get_by_id(token.registration_id.expect("linked registration"))
        .await
        .expect("registration lookup")
        .expect("registration exists");
    assert_eq!(registration.client_id, "dyn-client-1");
}

/// §10 #4 — a pre-registered env client wins: no registration request, its
/// client id and fixed redirect URI are used for authorize and exchange.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_registered_client_skips_dynamic_registration() {
    let _env_guard = env_lock().lock().await;
    // Pick a free port for the fixed loopback redirect.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let redirect = format!("http://127.0.0.1:{port}/oauth/callback");

    unsafe {
        std::env::set_var("MCP_OAUTH_CLIENT_ID", "pre-registered-1");
        std::env::set_var("MCP_OAUTH_CLIENT_SECRET", "pre-secret");
        std::env::set_var("MCP_OAUTH_REDIRECT_URI", &redirect);
    }

    let result = async {
        let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
        let mcp_url = serve_platform(MockConfig::default(), log.clone()).await;
        let (token_repo, registration_repo) = make_repos().await;
        let capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let oauth = service_with_hook(token_repo.clone(), registration_repo.clone(), capture.clone());
        let login = run_login(&oauth, &capture, &mcp_url).await;
        assert!(login.success, "pre-registered login failed: {:?}", login.error);
        assert!(
            log.lock().unwrap().register_bodies.is_empty(),
            "no dynamic registration may run when a pre-registered client exists"
        );
        let token_auth = {
            let guard = log.lock().unwrap();
            guard.token_auth_headers[0].clone()
        };
        assert_eq!(
            token_auth.as_str(),
            format!("Basic {}", base64_encode("pre-registered-1:pre-secret")).as_str(),
            "exchange must authenticate as the pre-registered client"
        );
        let authorize_url = capture.lock().unwrap().clone().unwrap();
        assert!(
            authorize_url.contains(&format!("redirect_uri={}", urlencode(&redirect))),
            "authorize must carry the fixed redirect URI: {authorize_url}"
        );
        // Registration row records the pre-registered identity.
        let rows = registration_repo
            .list_by_server_url(&mcp_url)
            .await
            .expect("list registrations");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].registration_mode, "pre_registered");
        assert_eq!(rows[0].client_id, "pre-registered-1");
        assert_eq!(rows[0].client_secret_ref.as_deref(), Some("pre-secret"));
    }
    .await;

    unsafe {
        std::env::remove_var("MCP_OAUTH_CLIENT_ID");
        std::env::remove_var("MCP_OAUTH_CLIENT_SECRET");
        std::env::remove_var("MCP_OAUTH_REDIRECT_URI");
    }
    let () = result;
}

fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// §10 #5 — dynamic registration rejects the redirect URI:
/// `redirect_uri_not_allowed`, authorize flow never starts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registration_rejects_redirect_uri() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(
        MockConfig {
            reject_redirect_uri: true,
            ..Default::default()
        },
        log.clone(),
    )
    .await;

    let (token_repo, registration_repo) = make_repos().await;
    let oauth = McpOAuthService::new_dynamic(token_repo).with_registration_repository(registration_repo);

    let result = run_failing_login(&oauth, &mcp_url).await;
    assert!(!result.success, "login must fail when the redirect URI is rejected");
    assert_eq!(result.error_code.as_deref(), Some("redirect_uri_not_allowed"));
    assert_eq!(
        log.lock().unwrap().authorize_hits,
        0,
        "no authorize request may be built after registration rejection"
    );
    assert!(log.lock().unwrap().token_bodies.is_empty());
}

/// §10 #7a — callback with a wrong CSRF state is rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_state_mismatch_is_rejected() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(MockConfig::default(), log.clone()).await;

    let (token_repo, registration_repo) = make_repos().await;
    let capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let oauth = service_with_hook(token_repo.clone(), registration_repo.clone(), capture.clone());

    let oauth_task = oauth.clone();
    let server_url = mcp_url.clone();
    let task = tokio::spawn(async move { oauth_task.login(&server_url).await });

    // Extract the loopback callback redirect URI from the authorize URL and hit
    // it with a forged state.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    let callback = loop {
        if capture.lock().unwrap().is_some() {
            break callback_base_from_authorize(&capture);
        }
        assert!(tokio::time::Instant::now() < deadline, "no authorize URL");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let _ = reqwest::Client::new()
        .get(format!("{callback}?code=forged&state=wrong-state"))
        .send()
        .await; // 服务端拒收后直接断连（无响应），客户端可能报 IncompleteMessage

    let result = task.await.expect("login task").expect("login returns");
    assert!(!result.success, "forged state must fail the login");
    assert!(
        result.error.as_deref().is_some_and(|e| e.contains("state")),
        "error should mention the state mismatch: {:?}",
        result.error
    );
}

/// §10 #7b — callback on an unregistered path is rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_path_mismatch_is_rejected() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(MockConfig::default(), log.clone()).await;

    let (token_repo, registration_repo) = make_repos().await;
    let capture: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let oauth = service_with_hook(token_repo.clone(), registration_repo.clone(), capture.clone());

    let oauth_task = oauth.clone();
    let server_url = mcp_url.clone();
    let task = tokio::spawn(async move { oauth_task.login(&server_url).await });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    let callback = loop {
        if capture.lock().unwrap().is_some() {
            break callback_base_from_authorize(&capture);
        }
        assert!(tokio::time::Instant::now() < deadline, "no authorize URL");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let _ = reqwest::Client::new()
        .get(format!("{callback}/wrong/path?code=x&state=y"))
        .send()
        .await; // 同上：拒收即断连

    let result = task.await.expect("login task").expect("login returns");
    assert!(!result.success, "wrong callback path must fail the login");
    assert!(
        result.error.as_deref().is_some_and(|e| e.contains("mismatch")),
        "error should mention the path mismatch: {:?}",
        result.error
    );
}

/// §10 #9 — login errors never leak code/state/verifier or the full
/// authorization URL.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_errors_carry_no_sensitive_material() {
    let _env_guard = env_lock().lock().await;
    let log: SharedLog = Arc::new(Mutex::new(MockLog::default()));
    let mcp_url = serve_platform(
        MockConfig {
            registration_endpoint: false,
            ..Default::default()
        },
        log.clone(),
    )
    .await;

    let (token_repo, registration_repo) = make_repos().await;
    let oauth = McpOAuthService::new_dynamic(token_repo).with_registration_repository(registration_repo);

    let result = run_failing_login(&oauth, &mcp_url).await;
    assert!(!result.success);
    let error = result.error.expect("error message");
    for sensitive in ["?code=", "code=", "state=", "code_verifier", "authorization_endpoint"] {
        assert!(
            !error.contains(sensitive),
            "error must not leak {sensitive:?}: {error}"
        );
    }
}

/// §10 #10 — two concurrent logins on ONE service (the `auth_start` background
/// task shape) must both succeed: `login` serializes on the shared pending
/// slot, so the first callback is never rejected with a CSRF mismatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_logins_on_shared_service_both_succeed() {
    let _env_guard = env_lock().lock().await;
    let url1 = serve_platform(MockConfig::default(), Arc::new(Mutex::new(MockLog::default()))).await;
    let url2 = serve_platform(MockConfig::default(), Arc::new(Mutex::new(MockLog::default()))).await;

    let (token_repo, registration_repo) = make_repos().await;
    let captures: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let hook = {
        let captures = captures.clone();
        Arc::new(move |url: &str| captures.lock().unwrap().push(url.to_owned()))
            as Arc<dyn Fn(&str) + Send + Sync>
    };
    let oauth = McpOAuthService::new_with_browser_hook(token_repo, test_http_client(), Some(hook))
        .with_registration_repository(registration_repo);

    let o1 = oauth.clone();
    let u1 = url1.clone();
    let t1 = tokio::spawn(async move { o1.login(&u1).await });
    let o2 = oauth.clone();
    let u2 = url2.clone();
    let t2 = tokio::spawn(async move { o2.login(&u2).await });

    // Both authorize URLs must be driven as soon as each appears: the login
    // gate serializes flows, so login 2 only reaches its authorize step after
    // login 1 completes — waiting for both URLs before driving would deadlock.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut driven = std::collections::HashSet::new();
    loop {
        let urls = captures.lock().unwrap().clone();
        for url in urls {
            if driven.insert(url.clone()) {
                // 服务端拒收时直接断连，客户端可能报 IncompleteMessage —— 忽略。
                let _ = reqwest::Client::new().get(&url).send().await;
            }
        }
        if driven.len() >= 2 {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "both authorize URLs must be captured");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let r1 = tokio::time::timeout(Duration::from_secs(60), t1)
        .await
        .expect("login 1 join")
        .expect("login 1 returns")
        .expect("login 1 result");
    let r2 = tokio::time::timeout(Duration::from_secs(60), t2)
        .await
        .expect("login 2 join")
        .expect("login 2 returns")
        .expect("login 2 result");
    assert!(r1.success, "login 1 failed: {:?}", r1.error);
    assert!(r2.success, "login 2 failed: {:?}", r2.error);
    assert!(oauth.check_oauth_status(&url1).await.unwrap().authenticated);
    assert!(oauth.check_oauth_status(&url2).await.unwrap().authenticated);
}
