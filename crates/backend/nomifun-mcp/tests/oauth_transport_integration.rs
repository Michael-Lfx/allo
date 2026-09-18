use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use nomifun_db::{IOAuthTokenRepository, SqliteOAuthTokenRepository, UpsertOAuthTokenParams};
use nomifun_mcp::{McpConnectionTestService, McpOAuthService, McpServerTransport};

fn test_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("test http client")
}

#[tokio::test]
async fn http_probe_injects_oauth_and_retries_once_after_401() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let seen_for_server = seen.clone();

    let server_handle = tokio::spawn(async move {
        let app = axum::Router::new()
            .route(
                "/mcp",
                post(move |headers: HeaderMap, Json(request): Json<serde_json::Value>| {
                    let seen = seen_for_server.clone();
                    async move {
                        seen.lock().unwrap().push(
                            headers
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok())
                                .map(str::to_owned),
                        );
                        if headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok())
                            != Some("Bearer new-access")
                        {
                            return (
                                StatusCode::UNAUTHORIZED,
                                [(axum::http::header::WWW_AUTHENTICATE, "Bearer realm=\"mock\"")],
                                "",
                            )
                                .into_response();
                        }
                        let result = if request["method"] == "tools/list" {
                            serde_json::json!({"tools": []})
                        } else {
                            serde_json::json!({
                                "protocolVersion": "2024-11-05",
                                "capabilities": {},
                                "serverInfo": {"name": "mock", "version": "1"}
                            })
                        };
                        Json(serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": request["id"].clone(),
                            "result": result
                        }))
                        .into_response()
                    }
                }),
            )
            .route(
                "/mcp/.well-known/oauth-authorization-server",
                get({
                    let token_endpoint = format!("http://{addr}/token");
                    move || {
                        let token_endpoint = token_endpoint.clone();
                        async move {
                            Json(serde_json::json!({
                                "authorization_endpoint": "http://127.0.0.1.invalid/authorize",
                                "token_endpoint": token_endpoint
                            }))
                        }
                    }
                }),
            )
            .route(
                "/token",
                post(|| async {
                    Json(serde_json::json!({
                        "access_token": "new-access",
                        "token_type": "bearer",
                        "expires_in": 3600
                    }))
                }),
            );
        axum::serve(listener, app).await.unwrap();
    });

    let db = nomifun_db::init_database_memory().await.unwrap();
    let repo: Arc<dyn IOAuthTokenRepository> =
        Arc::new(SqliteOAuthTokenRepository::new(db.pool().clone()));
    let server_url = format!("http://{addr}/mcp");
    repo.upsert(UpsertOAuthTokenParams {
        server_url: &server_url,
        access_token: "old-access",
        refresh_token: Some("refresh-token"),
        token_type: "bearer",
        expires_at: Some(nomifun_common::now_ms() + 3_600_000),
    registration_id: None,
    principal_id: None,
    })
    .await
    .unwrap();

    let oauth = McpOAuthService::new(repo.clone(), test_http_client());
    let service = McpConnectionTestService::new(test_http_client()).with_oauth_service(oauth);

    // The seeded token predates registration tracking (legacy row). Design
    // doc §4.3: a legacy token only refreshes through an explicit
    // pre-registered client identity — never a fabricated default. Verify
    // that legacy + env pre-registered client refresh path end to end.
    unsafe {
        std::env::set_var("MCP_OAUTH_CLIENT_ID", "pre-registered-legacy");
        std::env::remove_var("MCP_OAUTH_CLIENT_SECRET");
    }
    let result = async {
        service
            .test_connection(
                "oauth-server",
                &McpServerTransport::Http {
                    url: server_url.clone(),
                    headers: HashMap::new(),
                },
            )
            .await
    }
    .await;
    unsafe { std::env::remove_var("MCP_OAUTH_CLIENT_ID") };

    assert!(result.success, "OAuth retry should complete: {result:?}");
    let seen = seen.lock().unwrap().clone();
    let old_count = seen
        .iter()
        .filter(|value| value.as_deref() == Some("Bearer old-access"))
        .count();
    let new_count = seen
        .iter()
        .filter(|value| value.as_deref() == Some("Bearer new-access"))
        .count();
    assert_eq!(old_count, 1, "the old credential must be retried only once");
    assert!(new_count >= 2, "the refreshed credential must serve the handshake");
    assert_eq!(
        repo.get_by_url(&server_url)
            .await
            .unwrap()
            .unwrap()
            .access_token,
        "new-access"
    );

    server_handle.abort();
}
