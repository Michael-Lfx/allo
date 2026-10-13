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

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// In-memory client IP rate limiter (e.g. 5 requests per 60s window).
#[derive(Clone)]
pub struct IpRateLimiter {
    records: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
    max_requests: u32,
    window: Duration,
}

impl IpRateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub async fn check_and_record(&self, ip: &str) -> bool {
        let mut map = self.records.lock().await;
        let now = Instant::now();
        map.retain(|_, (_, start)| now.duration_since(*start) < self.window * 2);

        let entry = map.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) >= self.window {
            *entry = (1, now);
            true
        } else if entry.0 < self.max_requests {
            entry.0 += 1;
            true
        } else {
            false
        }
    }
}

impl Default for IpRateLimiter {
    fn default() -> Self {
        Self::new(5, 60)
    }
}

#[derive(Clone)]
pub struct ServerState {
    pub oobe: OobeManager,
    pub rate_limiter: IpRateLimiter,
    pub voice_event_tx: Option<tokio::sync::broadcast::Sender<crate::voice::VoiceCompanionEvent>>,
    pub voice_runtime: Option<Arc<crate::voice::VoiceCompanionRuntime>>,
}

fn get_client_ip(req: &Request) -> String {
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(s) = forwarded.to_str() {
            if let Some(first) = s.split(',').next() {
                return first.trim().to_string();
            }
        }
    }
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(s) = real_ip.to_str() {
            return s.trim().to_string();
        }
    }
    if let Some(connect_info) = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>() {
        return connect_info.0.ip().to_string();
    }
    "127.0.0.1".to_string()
}

/// Guard middleware requiring OOBE pairing for destructive or administrative endpoints
/// and enforcing client IP rate limits on verification requests.
async fn pairing_guard_middleware(
    State(state): State<ServerState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let method = req.method();

    // Rate limit /api/oobe/verify: max 5 requests per 60s per client IP
    if path == "/api/oobe/verify" && method == axum::http::Method::POST {
        let client_ip = get_client_ip(&req);
        if !state.rate_limiter.check_and_record(&client_ip).await {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "status": "rate_limited",
                    "message": "Too many verification requests from this IP. Please wait 60 seconds."
                })),
            )
                .into_response();
        }
    }

    // Non-API routes (e.g. /, /healthz, or 404 pages) pass through to standard router handlers
    if !path.starts_with("/api") {
        return next.run(req).await;
    }

    // Whitelisted routes available before pairing:
    // 1. OOBE verification & status APIs: /api/oobe/*
    // 2. Read-only device diagnostics: GET /api/voice/diagnostics/device
    // 3. Model read-only status and progress stream for the OOBE UI:
    //    GET /api/voice/models/status
    //    GET /api/voice/models/progress
    // 4. Voice companion live events stream & wake trigger for dashboard observability:
    //    GET /api/voice/events
    //    POST /api/voice/wake
    let is_whitelisted = path.starts_with("/api/oobe")
        || (method == axum::http::Method::GET && path == "/api/voice/diagnostics/device")
        || (method == axum::http::Method::GET
            && (path == "/api/voice/models/status"
                || path == "/api/voice/models/progress"
                || path == "/api/voice/events"))
        || (method == axum::http::Method::POST && path == "/api/voice/wake");

    if is_whitelisted {
        return next.run(req).await;
    }

    // All management and mutation endpoints (e.g. POST install, DELETE models) require pairing
    if !state.oobe.is_paired().await {
        return (StatusCode::FORBIDDEN, "Pairing required to access management APIs").into_response();
    }

    next.run(req).await
}

use axum::response::sse::{Event, KeepAlive, Sse};

/// `GET /api/voice/events`: Real-time SSE stream of companion state machine & acoustic events.
pub async fn voice_events_handler(
    State(state): State<ServerState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx_opt = state.voice_event_tx.as_ref().map(|tx| tx.subscribe());
    let stream = futures_util::stream::unfold(rx_opt, |mut rx_opt| async move {
        if let Some(ref mut rx) = rx_opt {
            while let Ok(event) = rx.recv().await {
                let data = serde_json::to_string(&event).unwrap_or_default();
                return Some((Ok(Event::default().data(data)), rx_opt));
            }
        }
        tokio::time::sleep(Duration::from_secs(15)).await;
        Some((Ok(Event::default().comment("heartbeat")), rx_opt))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(serde::Deserialize)]
pub struct VoiceWakeRequest {
    pub keyword: Option<String>,
}

/// `POST /api/voice/wake`: External wake trigger (e.g. from desktop cascade recognizer).
pub async fn voice_wake_handler(
    State(state): State<ServerState>,
    axum::Json(payload): axum::Json<VoiceWakeRequest>,
) -> impl IntoResponse {
    let kw = payload.keyword.unwrap_or_else(|| "Hey Flowy".to_string());
    if let Some(ref rt) = state.voice_runtime {
        rt.handle_wake_keyword(&kw).await;
        (StatusCode::OK, Json(json!({"success": true, "keyword": kw}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"success": false, "error": "Voice runtime not available"})),
        )
            .into_response()
    }
}

/// Construct the Axum HTTP router with default OOBE manager.
pub fn build_router(config: &Config, manager: Arc<ModelManager>) -> Router {
    build_router_with_oobe(config, manager, OobeManager::new())
}

/// Construct the Axum HTTP router with a specific OOBE manager instance.
pub fn build_router_with_oobe(
    config: &Config,
    manager: Arc<ModelManager>,
    oobe: OobeManager,
) -> Router {
    build_router_full(config, manager, oobe, None, None)
}

/// Construct the Axum HTTP router with all services and companion event streaming.
pub fn build_router_full(
    _config: &Config,
    manager: Arc<ModelManager>,
    oobe: OobeManager,
    voice_event_tx: Option<tokio::sync::broadcast::Sender<crate::voice::VoiceCompanionEvent>>,
    voice_runtime: Option<Arc<crate::voice::VoiceCompanionRuntime>>,
) -> Router {
    let state = ServerState {
        oobe: oobe.clone(),
        rate_limiter: IpRateLimiter::default(),
        voice_event_tx,
        voice_runtime,
    };
    let events_router = Router::new()
        .route("/api/voice/events", get(voice_events_handler))
        .route("/api/voice/wake", axum::routing::post(voice_wake_handler))
        .with_state(state.clone());

    Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/", get(index_handler))
        .merge(events_router)
        .merge(nomifun_voice_models::voice_model_routes(manager))
        .merge(nomifun_audio::voice_diagnostics_routes())
        .merge(oobe_routes(oobe.clone()))
        .layer(middleware::from_fn_with_state(
            state,
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
