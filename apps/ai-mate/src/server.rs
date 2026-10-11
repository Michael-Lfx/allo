//! AI Mate Voice Companion HTTP Server (Wave 0 T0-15)
//!
//! Provides the Axum HTTP service listening on port 8080:
//! - `GET /healthz`: Liveness probe returning `{"status": "ok", "app": "ai-mate"}`
//! - `GET /`: Serves the mobile-first OOBE pairing page static shell adhering to `ui-motion.zh.md` §4

use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};

use crate::config::Config;

/// Default listen address for the companion daemon service.
pub const DEFAULT_SERVER_ADDR: &str = "0.0.0.0:8080";

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

/// Construct the Axum HTTP router with all endpoints registered.
pub fn build_router(_config: &Config) -> Router {
    Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/", get(index_handler))
}

/// Start and run the HTTP server on the provided TCP listener.
pub async fn run_server(config: Config, listener: tokio::net::TcpListener) -> Result<(), std::io::Error> {
    let app = build_router(&config);
    tracing::info!("AI Mate daemon listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await
}
