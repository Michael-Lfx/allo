//! Minimal OpenAI-compatible chat proxy for Canvas Agent.
//!
//! Agent loop / tools / canvas ops stay in the frontend. This route only
//! authenticates against Flowy and forwards `/chat/completions`.

use axum::body::Body;
use axum::extract::{Extension, Json, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use futures::StreamExt;
use nomi_config::{GatewayConfig, config_yaml_path, load_user_config_file};
use nomifun_auth::CurrentUser;
use nomifun_cloud::{FlowyApiClient, ServerClientError, ServerSession};
use nomifun_common::AppError;
use serde_json::Value;
use tracing::warn;

use crate::state::CanvasRouterState;

pub async fn proxy_chat_completions(
    State(state): State<CanvasRouterState>,
    Extension(_user): Extension<CurrentUser>,
    Json(body): Json<Value>,
) -> Result<Response, AppError> {
    let data_dir = state.service.data_dir();
    let gateway: GatewayConfig = load_user_config_file(&config_yaml_path(Some(data_dir)))
        .map_err(|e| AppError::Internal(e))?;
    if !gateway.server.enabled {
        return Err(AppError::BadRequest(
            "Flowy cloud is disabled in local config".into(),
        ));
    }
    if !gateway.server.api_ready() {
        return Err(AppError::BadRequest(
            "Flowy cloud base URL is not configured".into(),
        ));
    }

    let api = FlowyApiClient::new(&gateway.server).map_err(map_cloud_err)?;
    let session = ServerSession::from_config(&gateway.server, data_dir);

    let upstream = api
        .llm_transport()
        .post_json("/chat/completions", Some(&session), body)
        .await
        .map_err(map_cloud_err)?;

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let is_sse = content_type
        .as_ref()
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("text/event-stream"));

    let mut builder = Response::builder().status(status);
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    if is_sse {
        builder = builder
            .header(header::CACHE_CONTROL, "no-cache")
            .header("x-accel-buffering", "no");
        let stream = upstream
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other));
        return builder
            .body(Body::from_stream(stream))
            .map_err(|e| AppError::Internal(e.to_string()));
    }

    let bytes = upstream
        .bytes()
        .await
        .map_err(|e| AppError::BadGateway(e.to_string()))?;
    if !status.is_success() {
        let preview = String::from_utf8_lossy(&bytes);
        warn!(%status, body = %truncate(&preview, 300), "canvas llm proxy upstream error");
    }
    builder
        .body(Body::from(bytes))
        .map_err(|e| AppError::Internal(e.to_string()))
}

fn map_cloud_err(err: ServerClientError) -> AppError {
    match err {
        ServerClientError::AuthRequired(msg) => AppError::Unauthorized(msg),
        ServerClientError::MissingBaseUrl => {
            AppError::BadRequest("server base_url not configured".into())
        }
        ServerClientError::Disabled => AppError::BadRequest("server client disabled".into()),
        ServerClientError::NotConfigured(msg) => AppError::BadRequest(msg),
        ServerClientError::Api { code, msg } if code == 401 || code == 403 => {
            AppError::Unauthorized(msg)
        }
        ServerClientError::Api { code, msg } if code == 400 => AppError::BadRequest(msg),
        other => AppError::BadGateway(truncate(&other.to_string(), 500)),
    }
}

fn truncate(message: &str, max: usize) -> String {
    let trimmed = message.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    trimmed.chars().take(max).collect::<String>() + "…"
}
