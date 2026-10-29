//! AI Mate Voice Companion HTTP Server (Wave 0 T0-15, Wave 1 T1-08, Wave 4 T4-04)
//!
//! Provides the Axum HTTP service listening on port 8080:
//! - `GET /healthz`: Liveness probe returning `{"status": "ok", "app": "ai-mate"}`
//! - `GET /`: Serves the mobile-first OOBE pairing page static shell adhering to `ui-motion.zh.md` §4
//! - `/api/voice/models/*`: Voice model REST & SSE management endpoints
//! - `/api/oobe/*`: OOBE 6-digit dynamic code verification & rate limiting

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::get;
use nomifun_voice_models::ModelManager;
use serde_json::{Value, json};

use crate::config::Config;
use crate::oobe::{OobeManager, oobe_routes};

/// Default listen address for the companion daemon service (restricted to localhost by default).
pub const DEFAULT_SERVER_ADDR: &str = "127.0.0.1:8080";

/// Embedded OOBE pairing static page shell.
pub const INDEX_HTML: &str = include_str!("../static/index.html");

/// `/healthz` liveness probe endpoint handler.
pub async fn healthz_handler() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "app": "voice-companion"
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
    pub control_token: Option<String>,
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
    if let Some(connect_info) = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    {
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
    if path.starts_with("/api/voice")
        && let Some(token) = &state.control_token
    {
        let header_token = req
            .headers()
            .get("x-voice-companion-control-token")
            .or_else(|| req.headers().get("x-ai-mate-control-token"))
            .and_then(|value| value.to_str().ok());
        if header_token != Some(token.as_str())
        {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"success": false, "error": "control_authorization_required"})),
            )
                .into_response();
        }
    }

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
        return (
            StatusCode::FORBIDDEN,
            "Pairing required to access management APIs",
        )
            .into_response();
    }

    next.run(req).await
}

use axum::response::sse::{Event, KeepAlive, Sse};

/// `GET /api/voice/events`: Real-time SSE stream of companion state machine & acoustic events.
pub async fn voice_events_handler(
    State(state): State<ServerState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx_opt = state.voice_event_tx.as_ref().map(|tx| tx.subscribe());
    let snapshot = if let Some(runtime) = &state.voice_runtime {
        Some(crate::voice::VoiceCompanionEvent::Snapshot {
            snapshot: runtime.snapshot(),
        })
    } else {
        None
    };
    let runtime = state.voice_runtime.clone();
    let stream = futures_util::stream::unfold(
        (rx_opt, snapshot, runtime),
        |(mut rx_opt, snapshot, runtime)| async move {
            if let Some(event) = snapshot {
                let data = serde_json::to_string(&event).unwrap_or_default();
                return Some((Ok(Event::default().data(data)), (rx_opt, None, runtime)));
            }
            if let Some(ref mut rx) = rx_opt {
                loop {
                    let event = match rx.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            match &runtime {
                                Some(runtime) => crate::voice::VoiceCompanionEvent::Snapshot {
                                    snapshot: runtime.snapshot(),
                                },
                                None => return None,
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    };
                    let data = serde_json::to_string(&event).unwrap_or_default();
                    return Some((Ok(Event::default().data(data)), (rx_opt, None, runtime)));
                }
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
            Some((
                Ok(Event::default().comment("heartbeat")),
                (rx_opt, None, runtime),
            ))
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(serde::Deserialize)]
pub struct VoiceWakeRequest {
    pub keyword: Option<String>,
    pub request_id: Option<String>,
    pub locale: Option<String>,
}

/// `POST /api/voice/wake`: External wake trigger (e.g. from desktop cascade recognizer).
pub async fn voice_wake_handler(
    State(state): State<ServerState>,
    axum::Json(payload): axum::Json<VoiceWakeRequest>,
) -> impl IntoResponse {
    let kw = payload.keyword.unwrap_or_else(|| "Hey Flowy".to_string());
    if let Some(ref rt) = state.voice_runtime {
        match rt
            .handle_wake_request_with_locale(
                &kw,
                payload.request_id.as_deref(),
                payload.locale.as_deref(),
            )
            .await {
            Ok(()) => (
                StatusCode::OK,
                Json(json!({"success": true, "keyword": kw})),
            )
                .into_response(),
            Err(crate::voice::VoiceWakeError::Busy) if rt.snapshot().session_id.is_some()
                && rt.snapshot().ended_reason.is_none()
                && rt.snapshot().call_request_id.as_deref() == payload.request_id.as_deref() => (
                    StatusCode::OK,
                    Json(json!({"success":true,"already_active":true})),
                ).into_response(),
            Err(error) => {
                let status = match error {
                    crate::voice::VoiceWakeError::PairingRequired => StatusCode::FORBIDDEN,
                    crate::voice::VoiceWakeError::Busy => StatusCode::CONFLICT,
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                };
                (
                    status,
                    Json(json!({"success": false, "error": error.to_string()})),
                )
                    .into_response()
            }
        }
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"success": false, "error": "Voice runtime not available"})),
        )
            .into_response()
    }
}

async fn voice_snapshot_handler(State(state): State<ServerState>) -> Response {
    if let Some(runtime) = &state.voice_runtime {
        Json(runtime.snapshot()).into_response()
    } else {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}

async fn voice_notification_handler(State(state):State<ServerState>,axum::extract::Path(attempt):axum::extract::Path<String>)->Response {
    match state.voice_runtime {
        Some(runtime)=>match runtime.notification_status(&attempt).await {
            Some(receipt)=>Json(receipt).into_response(),None=>StatusCode::NOT_FOUND.into_response(),
        },None=>StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}
async fn voice_task_result_handler(
    State(state): State<ServerState>,
    Json(request): Json<nomifun_api_types::voice::VoiceTaskResultAnnouncement>,
) -> Response {
    let Some(runtime)=state.voice_runtime else {return StatusCode::SERVICE_UNAVAILABLE.into_response();};
    let attempt=request.attempt_id.clone().unwrap_or_else(||request.command_id.clone());
    let owned=runtime.clone();
    // A disconnected HTTP caller must not abandon an already admitted playback.
    match tokio::spawn(async move {owned.announce_task_result(request).await}).await {
        Ok(Ok(()))=>match runtime.notification_status(&attempt).await {
            Some(receipt)=>Json(receipt).into_response(),None=>StatusCode::NOT_FOUND.into_response(),
        },
        Ok(Err(code))=>(StatusCode::CONFLICT,Json(json!({"code":code}))).into_response(),
        Err(_)=>StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn voice_confirmation_handler(
    State(state):State<ServerState>,
    Json(request):Json<nomifun_api_types::voice::VoiceToolConfirmationPrompt>,
)->Response {
    let Some(runtime)=state.voice_runtime else{return StatusCode::SERVICE_UNAVAILABLE.into_response();};
    match tokio::spawn(async move{runtime.request_tool_confirmation(request).await}).await {
        Ok(Ok(reply))=>Json(reply).into_response(),
        Ok(Err(code))=>(StatusCode::CONFLICT,Json(json!({"code":code}))).into_response(),
        Err(_)=>StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum VoiceTaskStatusPayload {
    Versioned {
        task: Option<nomifun_api_types::voice::VoiceTaskSnapshot>,
        instance_id: Option<String>,
        seq: Option<u64>,
    },
    Legacy(Option<nomifun_api_types::voice::VoiceTaskSnapshot>),
}

async fn voice_task_status_handler(
    State(state): State<ServerState>,
    Json(payload): Json<VoiceTaskStatusPayload>,
) -> Response {
    if let Some(runtime) = &state.voice_runtime {
        match payload {
            VoiceTaskStatusPayload::Versioned { task, instance_id, seq } => {
                runtime.update_task_status_versioned(task, instance_id.as_deref(), seq);
            }
            VoiceTaskStatusPayload::Legacy(task) => {
                runtime.update_task_status(task);
            }
        }
        StatusCode::OK.into_response()
    } else {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}

async fn voice_stop_handler(
    State(state): State<ServerState>,
    Json(request): Json<nomifun_api_types::voice::VoiceSessionControlRequest>,
) -> Response {
    if let Some(runtime) = &state.voice_runtime {
        match runtime.stop_session(&request).await {
            Ok(()) => Json(json!({"success": true})).into_response(),
            Err(_) => (
                StatusCode::CONFLICT,
                Json(json!({"success": false, "error": "session_scope_changed"})),
            )
                .into_response(),
        }
    } else {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    }
}

async fn voice_feedback_handler(State(state): State<ServerState>) -> Response {
    match &state.voice_runtime {
        Some(runtime) => Json(runtime.feedback()).into_response(),
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn voice_update_feedback_handler(
    State(state): State<ServerState>,
    Json(patch): Json<nomifun_api_types::voice::VoiceFeedbackPatch>,
) -> Response {
    match &state.voice_runtime {
        Some(runtime) => match runtime.update_feedback(patch) {
            Ok(()) => Json(runtime.feedback()).into_response(),
            Err(code) => (StatusCode::BAD_REQUEST, Json(json!({"code":code}))).into_response(),
        },
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn voice_preview_feedback_handler(
    State(state): State<ServerState>,
    Json(request): Json<nomifun_api_types::voice::VoiceFeedbackPreviewRequest>,
) -> Response {
    match &state.voice_runtime {
        Some(runtime) => match runtime.preview_feedback(request.kind).await {
            Ok(()) => Json(json!({"success":true})).into_response(),
            Err(code) => (StatusCode::CONFLICT, Json(json!({"code":code}))).into_response(),
        },
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[derive(Debug, serde::Deserialize, Default)]
struct TtsSelfCheckRequest {
    #[serde(default)]
    check_id: Option<String>,
    #[serde(default)]
    locale: Option<String>,
    #[serde(default)]
    stage: Option<String>,
}

async fn voice_tts_self_check_handler(
    State(state): State<ServerState>,
    Json(request): Json<TtsSelfCheckRequest>,
) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = runtime
        .self_check_tts_with_options(
            request.check_id.as_deref().filter(|s| !s.is_empty()),
            request.locale.as_deref().filter(|s| !s.is_empty()),
            request.stage.as_deref().filter(|s| !s.is_empty()),
        )
        .await;
    match result {
        Ok(response) => Json(response).into_response(),
        Err(code) => (StatusCode::CONFLICT, Json(json!({ "code": code }))).into_response(),
    }
}

async fn voice_tts_self_check_cancel_handler(
    State(state): State<ServerState>,
    Json(request): Json<TtsSelfCheckRequest>,
) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(check_id) = request.check_id.filter(|value| !value.is_empty()) else {
        return (StatusCode::BAD_REQUEST, Json(json!({ "code": "check_id_required" }))).into_response();
    };
    if runtime.cancel_self_check(&check_id).await {
        Json(json!({ "cancelled": true })).into_response()
    } else {
        (StatusCode::CONFLICT, Json(json!({ "code": "self_check_not_active" }))).into_response()
    }
}
#[derive(Debug, serde::Deserialize)]
struct VoiceAdmissionRequest {
    #[serde(default)]
    scope: Option<String>,
    allowed: bool,
    #[serde(default)]
    expires_at_ms: Option<i64>,
}

async fn voice_admission_handler(
    State(state): State<ServerState>,
    Json(request): Json<VoiceAdmissionRequest>,
) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let expires_at_ms = request
        .expires_at_ms
        .unwrap_or_else(|| nomifun_common::now_ms().saturating_add(60_000));
    let scope = request.scope.as_deref().unwrap_or("wake");
    runtime.set_admission(scope, request.allowed, expires_at_ms);
    Json(json!({ "scope": scope, "allowed": request.allowed, "expires_at_ms": expires_at_ms }))
        .into_response()
}
#[derive(Debug, serde::Deserialize, Default)]
struct EngineSelfCheckQuery {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct EngineSelfCheckCancelRequest {
    #[serde(default)]
    request_id: Option<String>,
}

async fn voice_engine_self_check_handler(
    State(state): State<ServerState>,
    Query(query): Query<EngineSelfCheckQuery>,
) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    #[cfg(feature = "native-fault-injection")]
    if let Ok(code) = std::env::var("NOMIFUN_VOICE_FAULT_EXIT_CODE")
        && let Ok(code) = code.parse::<i32>()
        && code != 75
    {
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            std::process::exit(code);
        });
    }
    let timeout = std::time::Duration::from_millis(
        query.timeout_ms.unwrap_or(45_000).clamp(2_000, 120_000),
    );
    Json(
        runtime
            .self_check_engines_with_request(query.request_id, timeout)
            .await,
    )
    .into_response()
}

async fn voice_engine_self_check_cancel_handler(
    State(state): State<ServerState>,
    Json(request): Json<EngineSelfCheckCancelRequest>,
) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Some(request_id) = request.request_id.filter(|value| !value.is_empty()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "code": "request_id_required" })),
        )
            .into_response();
    };
    if runtime.cancel_engine_check(&request_id).await {
        Json(json!({ "cancelled": true, "request_id": request_id })).into_response()
    } else {
        (
            StatusCode::CONFLICT,
            Json(json!({ "code": "engine_check_not_active" })),
        )
            .into_response()
    }
}
async fn voice_test_cancel_handler(State(state): State<ServerState>) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    #[cfg(feature = "native-fault-injection")]
    {
        Json(json!({ "cancelled": runtime.test_abort_ordinary_turn() })).into_response()
    }
    #[cfg(not(feature = "native-fault-injection"))]
    {
        let _ = runtime;
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn voice_test_hardware_calls_handler() -> Response {
    #[cfg(feature = "native-fault-injection")]
    {
        let (mic, speaker) = crate::voice::hardware_attempts();
        Json(json!({ "mic_attempts": mic, "speaker_attempts": speaker, "fault_asr_entered": crate::voice::FAULT_ASR_ENTERED.load(std::sync::atomic::Ordering::Acquire), "fault_observation": crate::voice::fault_job_observation() })).into_response()
    }
    #[cfg(not(feature = "native-fault-injection"))]
    {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn voice_test_ordinary_utterance_handler(
    State(state): State<ServerState>,
    Query(query): Query<EngineSelfCheckQuery>,
) -> Response {
    let Some(runtime) = state.voice_runtime else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    #[cfg(feature = "native-fault-injection")]
    {
        let request_id = query.request_id.unwrap_or_else(|| "ordinary-test".into());
        let started = runtime.test_start_ordinary_utterance(&request_id);
        Json(json!({ "started": started, "request_id": request_id })).into_response()
    }
    #[cfg(not(feature = "native-fault-injection"))]
    {
        let _ = (runtime, query);
        StatusCode::NOT_FOUND.into_response()
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
    config: &Config,
    manager: Arc<ModelManager>,
    oobe: OobeManager,
    voice_event_tx: Option<tokio::sync::broadcast::Sender<crate::voice::VoiceCompanionEvent>>,
    voice_runtime: Option<Arc<crate::voice::VoiceCompanionRuntime>>,
) -> Router {
    let state = ServerState {
        control_token: config.control_token.clone(),
        oobe: oobe.clone(),
        rate_limiter: IpRateLimiter::default(),
        voice_event_tx,
        voice_runtime,
    };
    let events_router = Router::new()
        .route(
            "/api/voice/feedback",
            get(voice_feedback_handler).post(voice_update_feedback_handler),
        )
        .route(
            "/api/voice/feedback/preview",
            axum::routing::post(voice_preview_feedback_handler),
        )
        .route("/api/voice/snapshot", get(voice_snapshot_handler))
        .route("/api/voice/notifications/{attempt_id}", axum::routing::get(voice_notification_handler))
        .route("/api/voice/task-progress", axum::routing::post(voice_task_result_handler))
        .route("/api/voice/task-result", axum::routing::post(voice_task_result_handler))
        .route("/api/voice/task-status", axum::routing::post(voice_task_status_handler))
        .route("/api/voice/confirmation",axum::routing::post(voice_confirmation_handler))
        .route("/api/voice/stop", axum::routing::post(voice_stop_handler))
        .route("/api/voice/events", get(voice_events_handler))
        .route("/api/voice/wake", axum::routing::post(voice_wake_handler))
        .route(
            "/api/voice/diagnostics/tts-self-check",
            axum::routing::post(voice_tts_self_check_handler),
        )
        .route(
            "/api/voice/diagnostics/tts-self-check/cancel",
            axum::routing::post(voice_tts_self_check_cancel_handler),
        )
        .route(
            "/api/voice/admission",
            axum::routing::post(voice_admission_handler),
        )
        .route(
            "/api/voice/diagnostics/engine-self-check",
            axum::routing::get(voice_engine_self_check_handler),
        )
        .route(
            "/api/voice/diagnostics/test-hardware-calls",
            axum::routing::get(voice_test_hardware_calls_handler),
        )
        .route(
            "/api/voice/diagnostics/test-utterance",
            axum::routing::post(voice_test_ordinary_utterance_handler),
        )
        .route(
            "/api/voice/diagnostics/test-cancel",
            axum::routing::post(voice_test_cancel_handler),
        )
        .route(
            "/api/voice/diagnostics/engine-self-check/cancel",
            axum::routing::post(voice_engine_self_check_cancel_handler),
        )
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
