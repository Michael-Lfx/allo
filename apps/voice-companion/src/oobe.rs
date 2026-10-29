//! OOBE Pairing & Anti-Brute-Force Rate Limiting Engine.
//!
//! Baseline:
//! - `interaction.zh.md` §3.1: "随机生成 6 位数字动态配对码（例如 839201，有效期 10 分钟）；扬声器自动播报：AI Mate 已开机，请在局域网网页输入配对码：8-3-9-2-0-1；连续 5 次输错配对码锁定 15 分钟。"
//! - `ui-motion.zh.md` §4: "配对 / 进度 Web 服务与配对码 10 分钟有效期解耦，持续服务至 ModelManager 抵达 Ready 或 Failed 终态后才按闲置策略关闭"
//! - `execution/wave-4-hardening.zh.md` §2 (T4-04), §3 (VT-4-03)

use std::sync::Arc;
use std::time::{Duration, Instant};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use nomifun_api_types::voice::ModelInstallState;
use nomifun_voice_models::ModelManager;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

/// Code lifetime: 10 minutes (600 seconds).
pub const CODE_LIFETIME_SECS: u64 = 600;

/// Max consecutive failed attempts before lockout (5 times).
pub const MAX_FAILED_ATTEMPTS: u32 = 5;

/// Lockout duration: 15 minutes (900 seconds).
pub const LOCKOUT_DURATION_SECS: u64 = 900;

/// Outcome of code verification attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerifyOutcome {
    Success,
    Failed { attempts_remaining: u32 },
    Locked { lockout_remaining_secs: u64 },
    Expired,
}

#[derive(Debug)]
struct OobeState {
    code: String,
    created_at: Instant,
    failed_attempts: u32,
    locked_until: Option<Instant>,
    is_paired: bool,
}

/// Coordinates OOBE pairing code generation, vocal announcement, rate-limiting, and lifecycle.
#[derive(Debug, Clone)]
pub struct OobeManager {
    state: Arc<RwLock<OobeState>>,
}

impl Default for OobeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl OobeManager {
    /// Creates a new `OobeManager` with a freshly generated 6-digit pairing code.
    pub fn new() -> Self {
        let code = generate_six_digit_code();
        Self {
            state: Arc::new(RwLock::new(OobeState {
                code,
                created_at: Instant::now(),
                failed_attempts: 0,
                locked_until: None,
                is_paired: false,
            })),
        }
    }

    /// Creates a new `OobeManager` with pairing bypassed (paired by default for local/single-node usage).
    pub fn new_bypassed() -> Self {
        let code = generate_six_digit_code();
        Self {
            state: Arc::new(RwLock::new(OobeState {
                code,
                created_at: Instant::now(),
                failed_attempts: 0,
                locked_until: None,
                is_paired: true,
            })),
        }
    }

    /// Returns the current 6-digit pairing code.
    pub async fn current_code(&self) -> String {
        self.state.read().await.code.clone()
    }

    /// Formats the code for vocal announcement with hyphen separation (e.g. "8-3-9-2-0-1").
    pub async fn vocal_digits(&self) -> String {
        let code = self.current_code().await;
        code.chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Complete vocal prompt string for speaker output.
    pub async fn vocal_announcement_text(&self) -> String {
        if self.is_paired().await {
            "AI Mate 已就绪，请对设备说「Hey Flowy」唤醒体验".to_string()
        } else {
            let digits = self.vocal_digits().await;
            format!("AI Mate 已开机，请在局域网网页输入配对码：{}", digits)
        }
    }

    /// Whether the device has already been successfully paired.
    pub async fn is_paired(&self) -> bool {
        self.state.read().await.is_paired
    }

    /// Regenerates a fresh 6-digit pairing code.
    ///
    /// Respects active lockout (cannot be bypassed by refresh) and resets code.
    pub async fn regenerate_code(&self) -> Result<String, &'static str> {
        let mut guard = self.state.write().await;
        let now = Instant::now();

        if let Some(locked_until) = guard.locked_until {
            if now < locked_until {
                return Err("Device is currently locked out; pairing code cannot be regenerated");
            }
        }

        if guard.is_paired {
            return Err("Device is already paired");
        }

        let new_code = generate_six_digit_code();
        guard.code = new_code.clone();
        guard.created_at = now;
        guard.failed_attempts = 0;
        guard.locked_until = None;
        info!("Regenerated fresh OOBE pairing code.");
        Ok(new_code)
    }

    /// Verifies an input pairing code against the current dynamic code.
    pub async fn verify_code(&self, candidate: &str) -> VerifyOutcome {
        let mut guard = self.state.write().await;
        let now = Instant::now();

        // 1. Check if currently locked out
        if let Some(locked_until) = guard.locked_until {
            if now < locked_until {
                let remaining = locked_until.duration_since(now).as_secs();
                return VerifyOutcome::Locked {
                    lockout_remaining_secs: remaining.max(1),
                };
            } else {
                // Lockout elapsed; reset failed attempts
                guard.locked_until = None;
                guard.failed_attempts = 0;
            }
        }

        // 2. Check code expiry (10 minutes)
        if now.duration_since(guard.created_at) > Duration::from_secs(CODE_LIFETIME_SECS) {
            return VerifyOutcome::Expired;
        }

        // 3. Compare code
        if candidate.trim() == guard.code {
            info!("OOBE pairing code successfully verified.");
            guard.is_paired = true;
            guard.failed_attempts = 0;
            VerifyOutcome::Success
        } else {
            guard.failed_attempts += 1;
            warn!(
                failed_attempts = guard.failed_attempts,
                "Failed OOBE pairing code verification attempt."
            );

            if guard.failed_attempts >= MAX_FAILED_ATTEMPTS {
                guard.locked_until = Some(now + Duration::from_secs(LOCKOUT_DURATION_SECS));
                warn!("OOBE anti-brute-force triggered: device locked for 15 minutes.");
                VerifyOutcome::Locked {
                    lockout_remaining_secs: LOCKOUT_DURATION_SECS,
                }
            } else {
                let remaining = MAX_FAILED_ATTEMPTS - guard.failed_attempts;
                VerifyOutcome::Failed {
                    attempts_remaining: remaining,
                }
            }
        }
    }

    /// Determines whether the pairing HTTP service should remain alive.
    ///
    /// Per `ui-motion.zh.md` §4:
    /// "配对 / 进度 Web 服务与配对码 10 分钟有效期解耦，持续服务至 ModelManager 抵达 Ready 或 Failed 终态后才按闲置策略关闭"
    pub async fn should_keep_web_service_alive(&self, model_manager: &Arc<ModelManager>) -> bool {
        let status = model_manager.get_status().await;
        match status.state {
            ModelInstallState::Ready { .. } => false,
            ModelInstallState::Failed { .. } => false,
            _ => true,
        }
    }
}

fn generate_six_digit_code() -> String {
    let mut rng = rand::rng();
    let num: u32 = rng.random_range(100_000..=999_999);
    num.to_string()
}

/// Request DTO for `/api/oobe/verify`.
#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
    pub code: String,
}

/// Response DTO for `/api/oobe/verify`.
#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempts_remaining: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockout_remaining_secs: Option<u64>,
}

/// Axum routes for OOBE verification.
pub fn oobe_routes(oobe: OobeManager) -> Router {
    Router::new()
        .route("/api/oobe/verify", post(verify_handler))
        .route("/api/oobe/status", get(status_handler))
        .route("/api/oobe/refresh", post(refresh_handler))
        .with_state(oobe)
}

async fn verify_handler(
    State(oobe): State<OobeManager>,
    Json(payload): Json<VerifyRequest>,
) -> impl IntoResponse {
    match oobe.verify_code(&payload.code).await {
        VerifyOutcome::Success => (
            StatusCode::OK,
            Json(VerifyResponse {
                status: "verified".into(),
                attempts_remaining: None,
                lockout_remaining_secs: None,
            }),
        ),
        VerifyOutcome::Failed { attempts_remaining } => (
            StatusCode::BAD_REQUEST,
            Json(VerifyResponse {
                status: "failed".into(),
                attempts_remaining: Some(attempts_remaining),
                lockout_remaining_secs: None,
            }),
        ),
        VerifyOutcome::Locked {
            lockout_remaining_secs,
        } => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(VerifyResponse {
                status: "locked".into(),
                attempts_remaining: Some(0),
                lockout_remaining_secs: Some(lockout_remaining_secs),
            }),
        ),
        VerifyOutcome::Expired => (
            StatusCode::GONE,
            Json(VerifyResponse {
                status: "expired".into(),
                attempts_remaining: None,
                lockout_remaining_secs: None,
            }),
        ),
    }
}

async fn status_handler(State(oobe): State<OobeManager>) -> impl IntoResponse {
    let paired = oobe.is_paired().await;
    Json(serde_json::json!({
        "paired": paired
    }))
}

async fn refresh_handler(State(oobe): State<OobeManager>) -> impl IntoResponse {
    match oobe.regenerate_code().await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "refreshed"
            })),
        ),
        Err(err) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "status": "error",
                "message": err
            })),
        ),
    }
}
