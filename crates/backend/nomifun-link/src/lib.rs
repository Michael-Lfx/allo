//! Pairing and device sessions for Flowy Link.
//!
//! The desktop local-trust secret never leaves the PC. A phone receives a
//! separate `aud=nomifun-link` access token after someone at the computer
//! allows the request.

mod store;
mod tls;

pub use store::{list_revoked_device_ids, list_wake_device_ids};
pub use tls::{
    LinkHost, LinkIdentity, LinkTlsError, advertise_addresses, advertise_link, current_host,
    load_or_create_identity, publish_host, serve_tls,
};

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use dashmap::DashMap;
use flowy_link_kernel::{LINK_TLS_PORT, LinkClass, build_pairing_url, classify_route};
use nomifun_auth::{JwtService, LocalTrusted, generate_random_hex_secret, require_local_trust_middleware};
use nomifun_common::AppError;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::store::DeviceRow;

const PAIR_TTL_MS: i64 = 2 * 60 * 1000;

#[derive(Clone)]
pub struct LinkHub {
    inner: Arc<Inner>,
}

struct Inner {
    pool: SqlitePool,
    user_id: Arc<str>,
    jwt: Arc<JwtService>,
    pending: DashMap<String, Pending>,
    paired_hook: std::sync::Mutex<Option<PairedHook>>,
    step_up: DashMap<String, StepUp>,
}

struct StepUp {
    method: String,
    path: String,
    expires_at_ms: i64,
    granted: bool,
}

pub type PairedHook = Arc<dyn Fn(PairedNotice) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct PairedNotice {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub app_version: String,
    pub wake_enabled: bool,
}

struct Pending {
    expires_at_ms: i64,
    request: Option<PairBody>,
    decision: Decision,
    issued: Option<Issued>,
    delivered: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Decision {
    Waiting,
    Allowed,
    Denied,
}

struct Issued {
    device_id: String,
    access_token: String,
    refresh_token: String,
}

impl LinkHub {
    pub fn new(pool: SqlitePool, user_id: Arc<str>, jwt: Arc<JwtService>) -> Self {
        Self {
            inner: Arc::new(Inner {
                pool,
                user_id,
                jwt,
                pending: DashMap::new(),
                paired_hook: std::sync::Mutex::new(None),
                step_up: DashMap::new(),
            }),
        }
    }

    pub fn set_paired_hook(&self, hook: PairedHook) {
        *self.inner.paired_hook.lock().expect("paired hook") = Some(hook);
    }

    pub async fn apply_revocations(&self, device_ids: &[String]) {
        for device_id in device_ids {
            self.inner.jwt.revoke_link_device(device_id);
        }
        let now = now_ms();
        if let Err(error) = store::revoke_ids(&self.inner.pool, device_ids, now).await {
            tracing::warn!(%error, "link revocation update failed");
        }
    }
}

pub fn routes(hub: LinkHub) -> Router {
    let open = Router::new()
        .route("/api/link/pair", post(submit_pair))
        .route("/api/link/pair/status", get(pair_status))
        .route("/api/link/session/refresh", post(refresh_session))
        .route("/api/link/whoami", get(whoami))
        .route("/api/link/wake", post(set_wake))
        .route("/api/link/step-up", post(request_step_up))
        .route("/api/link/step-up/status", get(step_up_status))
        .with_state(hub.clone());
    let desktop = Router::new()
        .route("/api/link/pairing-sessions", post(open_pairing))
        .route("/api/link/pair/pending", get(list_pending))
        .route("/api/link/pair/decide", post(decide))
        .route("/api/link/step-up/pending", get(list_step_up))
        .route("/api/link/step-up/decide", post(decide_step_up))
        .route_layer(middleware::from_fn(require_local_trust_middleware))
        .with_state(hub);
    open.merge(desktop)
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairBody {
    nonce: String,
    display_name: String,
    platform: String,
    public_key_fingerprint: String,
    #[serde(default)]
    app_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecideBody {
    nonce: String,
    allow: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefreshBody {
    refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WakeBody {
    enabled: bool,
}

async fn open_pairing(State(hub): State<LinkHub>) -> Result<Json<Value>, AppError> {
    let nonce = generate_random_hex_secret();
    hub.inner.pending.insert(
        nonce.clone(),
        Pending {
            expires_at_ms: now_ms() + PAIR_TTL_MS,
            request: None,
            decision: Decision::Waiting,
            issued: None,
            delivered: false,
        },
    );
    let host = crate::current_host();
    let addresses = crate::advertise_addresses();
    let fingerprint = host.as_ref().map(|item| item.fingerprint.clone()).unwrap_or_default();
    let name = host
        .as_ref()
        .map(|item| item.name.clone())
        .filter(|item| !item.is_empty())
        .unwrap_or_else(|| "Flowy".to_string());
    let url = if fingerprint.len() == 64 {
        Some(build_pairing_url(&flowy_link_kernel::PairingUrl {
            version: 1,
            port: LINK_TLS_PORT,
            fingerprint: fingerprint.clone(),
            nonce: nonce.clone(),
            addresses,
            name,
        }))
    } else {
        None
    };
    Ok(Json(json!({
        "nonce": nonce,
        "expiresAtMs": now_ms() + PAIR_TTL_MS,
        "port": LINK_TLS_PORT,
        "scheme": "flowy://pair",
        "fingerprint": fingerprint,
        "url": url,
    })))
}

async fn submit_pair(
    State(hub): State<LinkHub>,
    Json(body): Json<PairBody>,
) -> Result<Json<Value>, AppError> {
    validate_pair(&body)?;
    let mut entry = hub
        .inner
        .pending
        .get_mut(&body.nonce)
        .ok_or_else(|| AppError::NotFound("pairing session not found".into()))?;
    if now_ms() > entry.expires_at_ms {
        return Err(AppError::BadRequest("pairing session expired".into()));
    }
    if entry.request.is_some() {
        return Err(AppError::Conflict("pairing session already presented".into()));
    }
    entry.request = Some(body);
    Ok(Json(json!({ "status": "pending" })))
}

async fn list_pending(State(hub): State<LinkHub>) -> Json<Value> {
    let now = now_ms();
    let items: Vec<Value> = hub
        .inner
        .pending
        .iter()
        .filter(|entry| entry.decision == Decision::Waiting && entry.expires_at_ms >= now)
        .filter_map(|entry| {
            entry.request.as_ref().map(|request| {
                json!({
                    "nonce": entry.key(),
                    "displayName": request.display_name,
                    "platform": request.platform,
                    "publicKeyFingerprint": request.public_key_fingerprint,
                    "appVersion": request.app_version,
                })
            })
        })
        .collect();
    Json(json!({ "items": items }))
}

async fn decide(
    State(hub): State<LinkHub>,
    Json(body): Json<DecideBody>,
) -> Result<Json<Value>, AppError> {
    let mut entry = hub
        .inner
        .pending
        .get_mut(&body.nonce)
        .ok_or_else(|| AppError::NotFound("pairing session not found".into()))?;
    let request = entry
        .request
        .clone()
        .ok_or_else(|| AppError::BadRequest("phone has not presented this session".into()))?;
    if entry.decision != Decision::Waiting {
        return Err(AppError::Conflict("pairing session already decided".into()));
    }
    if !body.allow {
        entry.decision = Decision::Denied;
        return Ok(Json(json!({ "status": "denied" })));
    }
    let refresh = generate_random_hex_secret();
    let now = now_ms();
    let device_id = store::insert_device(
        &hub.inner.pool,
        &hub.inner.user_id,
        &request.display_name,
        &request.platform,
        &request.public_key_fingerprint,
        &store::hash_refresh(&refresh),
        &request.app_version,
        now,
    )
    .await
    .map_err(|error| AppError::Internal(format!("link device insert failed: {error}")))?;
    let access = hub
        .inner
        .jwt
        .sign_link(&hub.inner.user_id, &request.display_name, &device_id)
        .map_err(|error| AppError::Internal(format!("link token sign failed: {error}")))?;
    entry.decision = Decision::Allowed;
    entry.issued = Some(Issued {
        device_id: device_id.clone(),
        access_token: access,
        refresh_token: refresh,
    });
    drop(entry);
    if let Some(hook) = hub.inner.paired_hook.lock().expect("paired hook").clone() {
        hook(PairedNotice {
            device_id,
            display_name: request.display_name,
            platform: request.platform,
            app_version: request.app_version,
            wake_enabled: true,
        });
    }
    Ok(Json(json!({ "status": "allowed" })))
}

async fn pair_status(
    State(hub): State<LinkHub>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let nonce = headers
        .get("x-flowy-pair-nonce")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("missing pairing nonce".into()))?;
    let mut entry = hub
        .inner
        .pending
        .get_mut(nonce)
        .ok_or_else(|| AppError::NotFound("pairing session not found".into()))?;
    if now_ms() > entry.expires_at_ms && entry.decision == Decision::Waiting {
        return Ok(Json(json!({ "status": "expired" })));
    }
    match entry.decision {
        Decision::Waiting => Ok(Json(json!({ "status": "pending" }))),
        Decision::Denied => Ok(Json(json!({ "status": "denied" }))),
        Decision::Allowed => {
            if entry.delivered {
                return Ok(Json(json!({ "status": "consumed" })));
            }
            let issued = entry.issued.take().ok_or_else(|| AppError::Internal("missing issued tokens".into()))?;
            entry.delivered = true;
            Ok(Json(json!({
                "status": "approved",
                "deviceId": issued.device_id,
                "accessToken": issued.access_token,
                "refreshToken": issued.refresh_token,
                "expiresIn": 15 * 60,
            })))
        }
    }
}

async fn refresh_session(
    State(hub): State<LinkHub>,
    Json(body): Json<RefreshBody>,
) -> Result<Json<Value>, AppError> {
    let row = store::find_by_refresh(&hub.inner.pool, &store::hash_refresh(&body.refresh_token))
        .await
        .map_err(|error| AppError::Internal(format!("link refresh lookup failed: {error}")))?
        .ok_or_else(|| AppError::Unauthorized("refresh token rejected".into()))?;
    if row.revoked {
        return Err(AppError::Unauthorized("device revoked".into()));
    }
    let access = hub
        .inner
        .jwt
        .sign_link(&hub.inner.user_id, &row.display_name, &row.linked_device_id)
        .map_err(|error| AppError::Internal(format!("link token sign failed: {error}")))?;
    let _ = store::touch(&hub.inner.pool, &row.linked_device_id, now_ms()).await;
    Ok(Json(json!({
        "accessToken": access,
        "expiresIn": 15 * 60,
        "deviceId": row.linked_device_id,
    })))
}

async fn whoami(State(hub): State<LinkHub>, headers: HeaderMap) -> Result<Json<Value>, AppError> {
    let row = authorized_device(&hub, &headers).await?;
    Ok(Json(json!({
        "deviceId": row.linked_device_id,
        "displayName": row.display_name,
        "platform": row.platform,
        "wakeEnabled": row.wake_enabled,
        "port": LINK_TLS_PORT,
    })))
}

async fn set_wake(
    State(hub): State<LinkHub>,
    headers: HeaderMap,
    Json(body): Json<WakeBody>,
) -> Result<StatusCode, AppError> {
    let row = authorized_device(&hub, &headers).await?;
    store::set_wake(&hub.inner.pool, &row.linked_device_id, body.enabled)
        .await
        .map_err(|error| AppError::Internal(format!("link wake update failed: {error}")))?;
    if let Some(hook) = hub.inner.paired_hook.lock().expect("paired hook").clone() {
        hook(PairedNotice {
            device_id: row.linked_device_id,
            display_name: row.display_name,
            platform: row.platform,
            app_version: row.app_version,
            wake_enabled: body.enabled,
        });
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepUpBody {
    method: String,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepUpDecideBody {
    id: String,
    allow: bool,
}

async fn request_step_up(
    State(hub): State<LinkHub>,
    headers: HeaderMap,
    Json(body): Json<StepUpBody>,
) -> Result<Json<Value>, AppError> {
    authorized_device(&hub, &headers).await?;
    let method = body.method.trim().to_ascii_uppercase();
    let path = body.path.trim().to_string();
    if !matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE")
        || classify_route(&path) != Some(LinkClass::StepUp)
    {
        return Err(AppError::BadRequest("this route does not require desktop confirmation".into()));
    }
    let id = generate_random_hex_secret();
    hub.inner.step_up.insert(
        id.clone(),
        StepUp {
            method,
            path,
            expires_at_ms: now_ms() + PAIR_TTL_MS,
            granted: false,
        },
    );
    Ok(Json(json!({ "id": id, "status": "pending" })))
}

async fn step_up_status(State(hub): State<LinkHub>, headers: HeaderMap) -> Result<Json<Value>, AppError> {
    authorized_device(&hub, &headers).await?;
    let id = headers
        .get("x-flowy-step-up")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("missing step-up id".into()))?;
    let Some(entry) = hub.inner.step_up.get(id) else {
        return Ok(Json(json!({ "status": "missing" })));
    };
    if now_ms() > entry.expires_at_ms {
        return Ok(Json(json!({ "status": "expired" })));
    }
    Ok(Json(json!({ "status": if entry.granted { "approved" } else { "pending" } })))
}

async fn list_step_up(State(hub): State<LinkHub>) -> Json<Value> {
    let now = now_ms();
    let items: Vec<Value> = hub
        .inner
        .step_up
        .iter()
        .filter(|entry| !entry.granted && entry.expires_at_ms >= now)
        .map(|entry| {
            json!({
                "id": entry.key(),
                "method": entry.method,
                "path": entry.path,
            })
        })
        .collect();
    Json(json!({ "items": items }))
}

async fn decide_step_up(
    State(hub): State<LinkHub>,
    Json(body): Json<StepUpDecideBody>,
) -> Result<Json<Value>, AppError> {
    let mut entry = hub
        .inner
        .step_up
        .get_mut(&body.id)
        .ok_or_else(|| AppError::NotFound("step-up request not found".into()))?;
    if now_ms() > entry.expires_at_ms {
        return Err(AppError::BadRequest("step-up request expired".into()));
    }
    if !body.allow {
        drop(entry);
        hub.inner.step_up.remove(&body.id);
        return Ok(Json(json!({ "status": "denied" })));
    }
    entry.granted = true;
    Ok(Json(json!({ "status": "approved" })))
}

pub async fn link_step_up_middleware(
    State(hub): State<LinkHub>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let path = request.uri().path().to_string();
    let method = request.method().as_str().to_string();
    let needs_confirm = classify_route(&path) == Some(LinkClass::StepUp)
        && request.extensions().get::<LocalTrusted>().is_none();
    if needs_confirm {
        if let Some(token) = bearer(request.headers()) {
            if hub.inner.jwt.verify_link(&token).is_ok() {
                let grant = request
                    .headers()
                    .get("x-flowy-step-up")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let allowed = grant
                    .as_deref()
                    .and_then(|id| hub.inner.step_up.get(id))
                    .is_some_and(|entry| {
                        entry.granted
                            && entry.method == method
                            && entry.path == path
                            && now_ms() <= entry.expires_at_ms
                    });
                if !allowed {
                    return Err(AppError::Forbidden("link step-up required".into()));
                }
                if let Some(id) = grant {
                    hub.inner.step_up.remove(&id);
                }
            }
        }
    }
    Ok(next.run(request).await)
}

async fn authorized_device(hub: &LinkHub, headers: &HeaderMap) -> Result<DeviceRow, AppError> {
    if headers_local_trust_marker(headers) {
        return Err(AppError::Forbidden("device endpoints require a link token".into()));
    }
    let token = bearer(headers).ok_or_else(|| AppError::Unauthorized("link token required".into()))?;
    let claims = hub
        .inner
        .jwt
        .verify_link(&token)
        .map_err(|_| AppError::Unauthorized("link token rejected".into()))?;
    let row = store::find_by_id(&hub.inner.pool, &claims.device_id)
        .await
        .map_err(|error| AppError::Internal(format!("link device lookup failed: {error}")))?
        .ok_or_else(|| AppError::Unauthorized("device missing".into()))?;
    if row.revoked {
        return Err(AppError::Unauthorized("device revoked".into()));
    }
    Ok(row)
}

fn headers_local_trust_marker(headers: &HeaderMap) -> bool {
    headers.get("x-nomi-local-trust").is_some() && bearer(headers).is_none()
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ").map(str::to_owned)
}

fn validate_pair(body: &PairBody) -> Result<(), AppError> {
    if !matches!(body.platform.as_str(), "ios" | "android" | "harmony") {
        return Err(AppError::BadRequest("platform must be ios, android, or harmony".into()));
    }
    let name = body.display_name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(|ch| ch.is_control()) {
        return Err(AppError::BadRequest("display name is invalid".into()));
    }
    if body.public_key_fingerprint.len() != 64
        || !body.public_key_fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::BadRequest("public key fingerprint is invalid".into()));
    }
    Ok(())
}

pub fn classify_wake_event(name: &str, data: &Value) -> Option<&'static str> {
    match name {
        "turn.failed" => Some("turn.failed"),
        "turn.completed" => {
            let waiting = data.pointer("/runtime/state").and_then(Value::as_str) == Some("waiting_confirmation")
                || data
                    .pointer("/runtime/pending_confirmations")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0;
            Some(if waiting { "confirmation.pending" } else { "turn.completed" })
        }
        _ => None,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use nomifun_auth::LocalTrusted;
    use tower::ServiceExt;

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        store::migrate(&pool).await.unwrap();
        pool
    }

    fn hub(pool: SqlitePool) -> LinkHub {
        LinkHub::new(pool, Arc::from("0190f5fe-7c00-7a00-8000-000000000001"), Arc::new(JwtService::new("test-secret".into())))
    }

    async fn json_of(response: axum::response::Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn trust(request: Request<Body>) -> Request<Body> {
        let mut request = request;
        request.extensions_mut().insert(LocalTrusted);
        request
    }

    #[test]
    fn completed_turn_waiting_for_confirmation_wakes_without_text() {
        let data = serde_json::json!({"conversation_id":"c","runtime":{"state":"waiting_confirmation","pending_confirmations":1}});
        assert_eq!(classify_wake_event("turn.completed", &data), Some("confirmation.pending"));
        assert_eq!(
            classify_wake_event("turn.completed", &serde_json::json!({"conversation_id":"c"})),
            Some("turn.completed")
        );
        assert_eq!(classify_wake_event("message.stream", &serde_json::json!({"data":"secret"})), None);
    }

    #[tokio::test]
    async fn desktop_confirm_issues_a_link_token_once() {
        let app = routes(hub(pool().await));
        let opened = app
            .clone()
            .oneshot(trust(
                Request::post("/api/link/pairing-sessions").body(Body::empty()).unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(opened.status(), StatusCode::OK);
        let opened = json_of(opened).await;
        let nonce = opened["nonce"].as_str().unwrap().to_owned();

        let presented = app
            .clone()
            .oneshot(
                Request::post("/api/link/pair")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"nonce":"{nonce}","displayName":"Pocket","platform":"ios","publicKeyFingerprint":"{fp}","appVersion":"0.1"}}"#,
                        fp = "ab".repeat(32)
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(presented.status(), StatusCode::OK);

        let pending = app
            .clone()
            .oneshot(trust(Request::get("/api/link/pair/pending").body(Body::empty()).unwrap()))
            .await
            .unwrap();
        let pending = json_of(pending).await;
        assert_eq!(pending["items"][0]["displayName"], "Pocket");

        let decided = app
            .clone()
            .oneshot(trust(
                Request::post("/api/link/pair/decide")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"nonce":"{nonce}","allow":true}}"#)))
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(decided.status(), StatusCode::OK);

        let status = app
            .clone()
            .oneshot(
                Request::get("/api/link/pair/status")
                    .header("x-flowy-pair-nonce", &nonce)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = json_of(status).await;
        assert_eq!(status["status"], "approved");
        let access = status["accessToken"].as_str().unwrap().to_owned();

        let again = app
            .clone()
            .oneshot(
                Request::get("/api/link/pair/status")
                    .header("x-flowy-pair-nonce", &nonce)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(json_of(again).await["status"], "consumed");

        let me = app
            .oneshot(
                Request::get("/api/link/whoami")
                    .header("authorization", format!("Bearer {access}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(me.status(), StatusCode::OK);
        assert_eq!(json_of(me).await["wakeEnabled"], true);
    }

    #[tokio::test]
    async fn link_change_password_requires_one_desktop_grant() {
        let pool = pool().await;
        let hub = hub(pool.clone());
        let device = store::insert_device(
            &pool,
            "0190f5fe-7c00-7a00-8000-000000000001",
            "Pocket",
            "ios",
            &"ab".repeat(32),
            "hash",
            "0.1",
            1,
        )
        .await
        .unwrap();
        let access = hub
            .inner
            .jwt
            .sign_link("0190f5fe-7c00-7a00-8000-000000000001", "owner", &device)
            .unwrap();
        let app = routes(hub.clone())
            .route(
                "/api/webui/change-password",
                post(|| async { StatusCode::NO_CONTENT }),
            )
            .layer(middleware::from_fn_with_state(hub, link_step_up_middleware));
        let bearer = format!("Bearer {access}");

        let blocked = app
            .clone()
            .oneshot(
                Request::post("/api/webui/change-password")
                    .header("authorization", &bearer)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::FORBIDDEN);

        let opened = app
            .clone()
            .oneshot(
                Request::post("/api/link/step-up")
                    .header("authorization", &bearer)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"method":"POST","path":"/api/webui/change-password"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(opened.status(), StatusCode::OK);
        let id = json_of(opened).await["id"].as_str().unwrap().to_owned();

        let decided = app
            .clone()
            .oneshot(trust(
                Request::post("/api/link/step-up/decide")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"id":"{id}","allow":true}}"#)))
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(decided.status(), StatusCode::OK);

        let allowed = app
            .clone()
            .oneshot(
                Request::post("/api/webui/change-password")
                    .header("authorization", &bearer)
                    .header("x-flowy-step-up", &id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::NO_CONTENT);

        let replay = app
            .oneshot(
                Request::post("/api/webui/change-password")
                    .header("authorization", &bearer)
                    .header("x-flowy-step-up", &id)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::FORBIDDEN);
    }
}
