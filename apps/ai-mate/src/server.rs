//! AI Mate Voice Companion HTTP Server (Wave 0 T0-15, Wave 1 T1-08, Wave 4 T4-04)
//!
//! Provides the Axum HTTP service listening on port 8080:
//! - `GET /healthz`: Liveness probe returning `{"status": "ok", "app": "ai-mate"}`
//! - `GET /`: Serves the mobile-first OOBE pairing page static shell adhering to `ui-motion.zh.md` §4
//! - `/api/voice/models/*`: Voice model REST & SSE management endpoints
//! - `/api/oobe/*`: OOBE 6-digit dynamic code verification & rate limiting

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use nomifun_voice_models::ModelManager;
use serde_json::{json, Value};

use crate::config::Config;
use crate::oobe::{oobe_routes, OobeManager};

/// Default listen address for the companion daemon service (restricted to localhost by default).
pub const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:8080";

/// Embedded OOBE pairing static page shell.
pub const INDEX_HTML: &str = include_str!("../static/index.html");

/// `/healthz` liveness probe endpoint handler.
pub async fn healthz_handler() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "app": "ai-mate"
    }))
}

/// `GET /` OOBE pairing page shell handler.
pub async fn index_handler() -> impl IntoResponse {
    Html(INDEX_HTML)
}

/// Guard middleware requiring OOBE pairing for destructive or administrative endpoints.
async fn pairing_guard_middleware(
    State(oobe): State<OobeManager>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let method = req.method();

    // Non-API routes (e.g. /, /healthz, or 404 pages) pass through to standard router handlers
    if !path.starts_with("/api") {
        return next.run(req).await;
    }

    // Whitelisted routes available before pairing:
    // 1. OOBE verification & status APIs: /api/oobe/*
    // 2. Model read-only status and progress stream for the OOBE UI:
    //    GET /api/voice/models/status
    //    GET /api/voice/models/progress
    let is_whitelisted = path.starts_with("/api/oobe")
        || (method == axum::http::Method::GET
            && (path == "/api/voice/models/status" || path == "/api/voice/models/progress"));

    if is_whitelisted {
        return next.run(req).await;
    }

    // All management and mutation endpoints (e.g. POST install, DELETE models) require pairing
    if !oobe.is_paired().await {
        return (StatusCode::FORBIDDEN, "Pairing required to access management APIs").into_response();
    }

    next.run(req).await
}

/// Construct the Axum HTTP router with default OOBE manager.
pub fn build_router(config: &Config, manager: Arc<ModelManager>) -> Router {
    build_router_with_oobe(config, manager, OobeManager::new())
}

/// Construct the Axum HTTP router with a specific OOBE manager instance.
pub fn build_router_with_oobe(
    _config: &Config,
    manager: Arc<ModelManager>,
    oobe: OobeManager,
) -> Router {
    Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/", get(index_handler))
        .merge(nomifun_voice_models::voice_model_routes(manager))
        .merge(oobe_routes(oobe.clone()))
        .layer(middleware::from_fn_with_state(
            oobe,
            pairing_guard_middleware,
        ))
}

/// Start and run the HTTP server on the provided TCP listener.
pub async fn run_server(
    config: Config,
    manager: Arc<ModelManager>,
    oobe: OobeManager,
    listener: tokio::net::TcpListener,
) -> Result<(), std::io::Error> {
    let app = build_router_with_oobe(&config, manager, oobe);
    tracing::info!("AI Mate daemon listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await
}
