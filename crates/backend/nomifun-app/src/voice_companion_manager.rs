//! Voice Companion Child-Process Supervisor (`ai-mate`)
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md
//!
//! Supervises the independent `ai-mate` daemon process alongside the Flowy
//! desktop application:
//! - Hidden console window, completely headless with zero extraneous tray icons;
//! - Strict whole-tree lifecycle authority via `nomi_process_runtime::ChildProcessBuilder`;
//! - Windows Job Object / Unix process-group anchoring so children die immediately
//!   when Flowy desktop shuts down or terminates unexpectedly;
//! - Auto-start on application boot when models are ready and user has enabled the companion.

use crate::voice_dialogue::{
    VoiceDialogueBridge, VoiceDialogueCompleter, VoiceDialoguePreflight, VoiceDialogueRecorder,
    VoiceDialogueRecorderError,
};
use crate::voice_tasks::{VoiceTaskRuntime, VoiceTaskService};
use nomifun_api_types::voice::{VoiceExecutionConfig, VoiceExecutionSettings};
use nomifun_api_types::voice::{VoiceTaskDelivery, VoiceTaskSnapshot, VoiceTaskStatus, VoiceTaskSubmitRequest};
use nomifun_api_types::voice::{VoiceTaskControlAction,VoiceTaskControlCommand,VoiceTaskControlRequest};
#[cfg(test)]
use nomifun_api_types::voice::VoiceTaskNotification;
use nomifun_api_types::voice::{VoiceToolConfirmation,VoiceToolConfirmationRequest};
use nomi_process_runtime::{ChildProcessBuilder, ManagedChildProcess};
use nomifun_api_types::voice::{VoiceCompanionEvent, VoiceConnectionState, VoiceSessionError};
use nomifun_api_types::voice::{VoiceDialogueConfig, VoiceDialogueRequest, VoiceDialogueSettings, VoiceModelRef};
use nomifun_api_types::voice::{
    VoiceFeedbackConfig, VoiceFeedbackPatch, VoiceFeedbackPreviewRequest, VoiceFeedbackSettings,
    VoiceTargetConfig, VoiceTargetMode, VoiceTargetSettings, VoiceTargetUpdateRequest,
};
pub use nomifun_api_types::voice::{
    VoiceCallStartRequest, VoiceCallStartResponse, VoiceCallStartStatus, VoiceCallTarget,
    VoiceSessionControlRequest, VoiceSessionPhase, VoiceSessionSnapshot,
};
use nomifun_companion::CompanionService;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::Mutex;
use tokio::sync::{Notify, broadcast};
use tracing::{info, warn};

/// Stable error code returned when the packaged voice daemon is absent.
pub const VOICE_COMPANION_MISSING: &str = "voice_companion_missing";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub models_ready: bool,
    pub autostart: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub acoustic_ready: bool,
    #[serde(default)]
    pub dialogue_ready: bool,
    #[serde(default)]
    pub playback_ready: bool,
    #[serde(default)]
    pub task_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<nomifun_api_types::voice::VoiceCapabilityReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionToggleRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionToggleResponse {
    pub success: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VoiceCompanionPersistedConfig {
    pub autostart: bool,
}

#[derive(Default)]
struct AudioRecovery {
    attempts: u8,
    next_attempt_ms: Option<i64>,
}

/// Bounded automatic daemon restart budget: 2s / 5s / 15s, at most three
/// attempts. A stable run of 60 seconds clears the budget.
#[derive(Default)]
struct RestartBudget {
    attempts: u8,
    next_attempt: Option<tokio::time::Instant>,
    running_since: Option<tokio::time::Instant>,
    exhausted_reported: bool,
}

impl RestartBudget {
    const MAX_ATTEMPTS: u8 = 3;

    fn delay(attempt: u8) -> std::time::Duration {
        match attempt {
            0 => std::time::Duration::from_secs(2),
            1 => std::time::Duration::from_secs(5),
            _ => std::time::Duration::from_secs(15),
        }
    }
}

enum RestartDecision {
    Wait(std::time::Duration),
    StartNow,
    Exhausted,
}

impl AudioRecovery {
    fn due(&mut self, snapshot:&VoiceSessionSnapshot, now_ms:i64)->bool {
        if !snapshot.enabled || snapshot.connection!=VoiceConnectionState::Connected
            || !matches!(snapshot.phase,VoiceSessionPhase::Idle|VoiceSessionPhase::Error)
            || snapshot.ended_reason==Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || snapshot.audio.as_ref().is_some_and(|audio|audio.speech_detected)
        {return false;}
        let capture_failed=snapshot.audio.as_ref().is_some_and(|audio|audio.capture_state=="closed"
            || (audio.capture_state=="no_pcm" && audio.last_frame_at_ms.is_none_or(|last|now_ms.saturating_sub(last)>=5_000)));
        let speaker_failed=snapshot.acoustic_ready && !snapshot.playback_ready;
        if !capture_failed && !speaker_failed {
            if snapshot.acoustic_ready && snapshot.playback_ready {*self=Self::default();}
            return false;
        }
        let due=*self.next_attempt_ms.get_or_insert(now_ms.saturating_add(2_000));
        if now_ms<due {return false;}
        self.attempts=self.attempts.saturating_add(1);
        self.next_attempt_ms=Some(now_ms.saturating_add((2_000_i64<<self.attempts.min(4)).min(30_000)));
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFeedbackDecision {
    None,
    TriggerFeedback {
        command_id: String,
        stage: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct TaskFeedbackTracker {
    pub active_command_id: Option<String>,
    pub accepted_at_ms: i64,
    pub last_feedback_at_ms: i64,
    pub last_stage: Option<String>,
    pub consecutive_same_count: usize,
    pub pending_feedback: Option<String>,
}

impl TaskFeedbackTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(&mut self,task:&VoiceTaskSnapshot,now_ms:i64)->TaskFeedbackDecision {
        self.evaluate_progress(task,None,now_ms)
    }
    pub fn evaluate_progress(&mut self, task: &VoiceTaskSnapshot, progress:Option<&nomifun_api_types::voice::VoiceTaskProgress>, now_ms: i64) -> TaskFeedbackDecision {
        if task.status != VoiceTaskStatus::Accepted {
            self.active_command_id = None;
            self.pending_feedback = None;
            self.consecutive_same_count = 0;
            return TaskFeedbackDecision::None;
        }

        if self.active_command_id.as_deref() != Some(&task.command_id) {
            self.active_command_id = Some(task.command_id.clone());
            self.accepted_at_ms = now_ms;
            self.last_feedback_at_ms = 0;
            self.last_stage = progress.map(|p|format!("{}:{}",p.stage,p.health));
            self.consecutive_same_count = 0;
            self.pending_feedback = None;
        }

        let current_stage = progress.map(|p|format!("{}:{}",p.stage,p.health));
        if current_stage != self.last_stage {
            self.last_stage = current_stage;
            self.consecutive_same_count = 0;
        }

        if self.last_feedback_at_ms > 0 && self.consecutive_same_count >= 2 {
            return TaskFeedbackDecision::None;
        }

        let due = if self.last_feedback_at_ms == 0 {
            now_ms >= self.accepted_at_ms + 15_000
        } else {
            now_ms >= self.last_feedback_at_ms + 45_000
        };

        if due {
            let stage_name = self.last_stage.clone().unwrap_or_else(|| "processing".to_string());
            self.pending_feedback = Some(stage_name.clone());
            TaskFeedbackDecision::TriggerFeedback {
                command_id: task.command_id.clone(),
                stage: stage_name,
            }
        } else {
            TaskFeedbackDecision::None
        }
    }

    pub fn record_dispatched(&mut self, now_ms: i64) {
        self.last_feedback_at_ms = now_ms;
        self.consecutive_same_count += 1;
        self.pending_feedback = None;
    }

    pub fn clear_pending(&mut self) {
        self.pending_feedback = None;
    }
}

mod recorder;
pub use recorder::{ConversationVoiceDialogueRecorder, VoiceWindowContext};

fn wake_rejection_precedes_session(status: axum::http::StatusCode, body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else { return false; };
    if value.get("success").and_then(|value| value.as_bool()) != Some(false) { return false; }
    let reason = value.get("error").and_then(|value| value.as_str());
    matches!(
        (status, reason),
        (axum::http::StatusCode::FORBIDDEN, Some("pairing_required"))
            | (axum::http::StatusCode::SERVICE_UNAVAILABLE, Some("models_unavailable" | "microphone_unavailable"))
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandledStartOutcome {
    Starting {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        conversation_id: String,
        memory_owner: Option<String>,
        reserved_at_epoch_ms: i64,
    },
    Bound {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        conversation_id: String,
        session_id: Option<String>,
        memory_owner: Option<String>,
        bound_at_epoch_ms: i64,
    },
    Stopped {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        conversation_id: String,
        session_id: Option<String>,
        stopped_at_epoch_ms: i64,
    },
    Rejected {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        status_code_u16: u16,
        error_json: String,
        at_epoch_ms: i64,
    },
}

impl HandledStartOutcome {
    fn session_mode(&self) -> &Option<String> {
        match self {
            Self::Starting { session_mode, .. } | Self::Bound { session_mode, .. }
            | Self::Stopped { session_mode, .. } | Self::Rejected { session_mode, .. } => session_mode,
        }
    }
    pub fn target(&self) -> &nomifun_api_types::voice::VoiceCallTarget {
        match self {
            Self::Starting { target, .. }
            | Self::Bound { target, .. }
            | Self::Stopped { target, .. }
            | Self::Rejected { target, .. } => target,
        }
    }
}

#[derive(Clone)]
struct StoredCapabilities {
    report: nomifun_api_types::voice::VoiceCapabilityReport,
    attempt: u64,
    expires_at_ms: i64,
    signature: String,
}

fn readiness_runtime_scope_valid(
    snapshot: &nomifun_api_types::voice::VoiceSessionSnapshot,
    now_ms: i64,
) -> bool {
    let audio = snapshot.audio.as_ref();
    let recent_pcm = audio.is_some_and(|value| {
        value.frames_received > 0
            && value
                .last_frame_at_ms
                .is_some_and(|at| now_ms.saturating_sub(at) <= 2_000)
            && value.capture_state != "closed"
    });
    snapshot.enabled
        && snapshot.connection == VoiceConnectionState::Connected
        && snapshot.acoustic_ready
        && snapshot.playback_ready
        && recent_pcm
        && audio.is_some_and(|value| value.asr_ready && value.vad_ready)
}
fn tts_self_check_languages_valid(
    response: &nomifun_api_types::voice::VoiceTtsSelfCheckResponse,
) -> bool {
    ["zh-CN", "en-US"].iter().all(|language| {
        response.languages.iter().any(|entry| {
            let expected_ms = (entry.samples as u64).saturating_mul(1_000)
                / u64::from(entry.sample_rate.max(1));
            entry.language == *language
                && entry.ok
                && (8_000..=48_000).contains(&entry.sample_rate)
                && entry.samples > 0
                && (100..=30_000).contains(&entry.duration_ms)
                && entry.duration_ms.abs_diff(expected_ms) <= 100
                && (entry.rms_milli >= 5 || entry.peak_milli >= 20)
        })
    })
}
/// Dropping an unfinished readiness check releases the host admission flag and
/// cancels the daemon-side fixed-sample playback/work for this check id.
struct SelfCheckFlight {
    manager: std::sync::Weak<VoiceCompanionProcessManager>,
    check_id: String,
    finished: bool,
}

impl Drop for SelfCheckFlight {
    fn drop(&mut self) {
        let Some(manager) = self.manager.upgrade() else {
            return;
        };
        manager.self_check_active.store(false, Ordering::Release);
        if self.finished {
            return;
        }
        let check_id = self.check_id.clone();
        let origin = manager.daemon_origin.clone();
        let token = manager.control_token.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let client = match reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(2))
                .build()
            {
                Ok(client) => client,
                Err(_) => return,
            };
            if let Err(error) = client
                .post(format!(
                    "{origin}/api/voice/diagnostics/tts-self-check/cancel"
                ))
                .header("x-ai-mate-control-token", token)
                .json(&serde_json::json!({ "check_id": check_id }))
                .send()
                .await
            {
                tracing::warn!(%error, "voice_self_check_cancel_failed");
            }
        });
    }
}
/// Holds the host-side model-write gate. Dropping it re-opens call admission
/// and restarts the daemon only when the user's enable intent is still valid.
pub struct ModelWriteGuard {
    manager: std::sync::Weak<VoiceCompanionProcessManager>,
    resume_daemon: bool,
    released: bool,
}

impl std::fmt::Debug for ModelWriteGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelWriteGuard")
            .field("resume_daemon", &self.resume_daemon)
            .field("released", &self.released)
            .finish()
    }
}

impl Drop for ModelWriteGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let Some(manager) = self.manager.upgrade() else {
            return;
        };
        manager.resource_write_active.store(false, Ordering::Release);
        if !self.resume_daemon {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if let Err(error) = manager.resume_after_model_write().await {
                        warn!(%error, "voice_daemon_restart_after_model_write_failed");
                    }
                });
            }
            Err(error) => warn!(%error, "voice_model_write_guard_dropped_without_runtime"),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub struct ChildExitObservation {
    pub pid: u32,
    pub code: Option<i32>,
    pub observed_at_ms: i64,
}

pub struct VoiceCompanionProcessManager {
    audio_recovery:std::sync::Mutex<AudioRecovery>,
    resource_write_active:AtomicBool,
    self_check_lock:tokio::sync::Mutex<()>,
    self_check_active:AtomicBool,
    self_check_cancel_signal:tokio::sync::Notify,
    self_check_attempt:AtomicU64,
    capabilities:std::sync::RwLock<Option<StoredCapabilities>>,
    restart_budget:std::sync::Mutex<RestartBudget>,
    prompted_approvals:std::sync::Mutex<std::collections::VecDeque<String>>,
    revoked_sessions: RwLock<std::collections::VecDeque<String>>,
    task_feedback: std::sync::Mutex<TaskFeedbackTracker>,
    daemon_origin: String,
    #[cfg(test)]
    test_daemon_port: Option<u16>,
    #[cfg(test)]
    last_child_exit: Arc<std::sync::Mutex<Option<ChildExitObservation>>>,
    #[cfg(test)]
    test_child_pid: Arc<std::sync::atomic::AtomicU32>,
    tasks: Option<Arc<VoiceTaskService>>,
    data_dir: PathBuf,
    task_projection_started: AtomicBool,
    dialogue_completer: Option<Arc<dyn VoiceDialogueCompleter>>,
    dialogue_recorder: Option<Arc<dyn VoiceDialogueRecorder>>,
    target_selection: Arc<RwLock<VoiceTargetConfig>>,
    active_window_candidate: Arc<RwLock<VoiceWindowContext>>,
    companion_service: Option<Arc<CompanionService>>,
    dialogue_bridge: Mutex<Option<VoiceDialogueBridge>>,
    config_write_lock: std::sync::Mutex<()>,
    lifecycle_lock: Mutex<()>,
    feedback_update_lock: Mutex<()>,
    child: Arc<Mutex<Option<ManagedChildProcess>>>,
    models_manager: Arc<nomifun_voice_models::ModelManager>,
    config_path: PathBuf,
    control_token: String,
    snapshot: RwLock<VoiceSessionSnapshot>,
    source: RwLock<Option<(String, u64)>>,
    task_sync_seq: AtomicU64,
    events: broadcast::Sender<VoiceSessionSnapshot>,
    projection_started: AtomicBool,
    generation: AtomicU64,
    process_changed: Notify,
    shutdown_requested: AtomicBool,
    call_start_lock: tokio::sync::Mutex<()>,
    handled_start_requests: std::sync::RwLock<std::collections::HashMap<String, HandledStartOutcome>>,
    viewed_conversation: RwLock<(u64, Option<String>)>,
}

static GLOBAL_COMPANION_MANAGER: std::sync::OnceLock<Arc<VoiceCompanionProcessManager>> =
    std::sync::OnceLock::new();

impl VoiceCompanionProcessManager {
    pub fn register_global(manager: Arc<Self>) {
        manager.install_call_delete_guard();
        let _ = GLOBAL_COMPANION_MANAGER.set(manager);
    }

    fn install_call_delete_guard(self: &Arc<Self>) {
        if let Some(recorder) = &self.dialogue_recorder {
            let weak = Arc::downgrade(self);
            recorder.set_call_delete_guard(Arc::new(move |id| weak.upgrade().is_some_and(|manager| manager.conversation_in_call(id))));
        }
    }

    fn conversation_in_call(&self, id: &str) -> bool {
        let snapshot = self.snapshot();
        (snapshot.session_id.is_some() && snapshot.ended_reason.is_none()
            && snapshot.binding.as_ref().is_some_and(|b| b.conversation_id == id))
            || self.dialogue_recorder.as_ref().and_then(|r| r.current_call_reservation()).is_some_and(|r| r.conversation_id == id)
    }

    async fn push_admission(&self, scope: &str, allowed: bool, expires_at_ms: i64) {
        let client = match reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            Ok(client) => client,
            Err(_) => return,
        };
        if let Err(error) = client
            .post(format!("{}/api/voice/admission", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .json(&serde_json::json!({
                "scope": scope,
                "allowed": allowed,
                "expires_at_ms": expires_at_ms,
            }))
            .send()
            .await
        {
            tracing::warn!(%error, "voice_wake_admission_push_failed");
        }
    }
    async fn refresh_wake_admission(&self) {
        if self.self_check_active.load(Ordering::Acquire) {
            self.push_admission("wake", false, 0).await;
            self.push_admission("call", false, 0).await;
            return;
        }
        let capabilities = self.current_capabilities().await;
        let expires_at_ms = capabilities
            .as_ref()
            .and_then(|report| report.checked_at_ms)
            .map(|checked| checked.saturating_add(60_000))
            .unwrap_or(0);
        let wake = capabilities.as_ref().is_some_and(|report| report.can_wake);
        let call = capabilities
            .as_ref()
            .is_some_and(|report| report.can_start_full_call);
        self.push_admission("wake", wake, if wake { expires_at_ms } else { 0 })
            .await;
        self.push_admission("call", call, if call { expires_at_ms } else { 0 })
            .await;
    }
    fn invalidate_capabilities_if(&self, attempt: u64, generation: u64) {
        let mut guard = self
            .capabilities
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if guard
            .as_ref()
            .is_some_and(|stored| stored.attempt == attempt && stored.report.generation == generation)
        {
            *guard = None;
        }
    }
    fn invalidate_capabilities(&self) {
        *self
            .capabilities
            .write()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }

    async fn capability_signature(&self) -> String {
        let status = self.models_manager.get_status().await;
        let snapshot = self.snapshot();
        let dialogue = match &self.dialogue_completer {
            Some(completer) => completer.configuration_revision().await,
            None => "unconfigured".into(),
        };
        let tts = snapshot
            .tts
            .as_ref()
            .map(|value| {
                format!(
                    "{}:{}:{}",
                    value.provider,
                    value.voice.as_deref().unwrap_or(""),
                    value.online
                )
            })
            .unwrap_or_else(|| "unknown".into());
        let audio = snapshot.audio.as_ref();
        let daemon_instance_id = self
            .source
            .read()
            .ok()
            .and_then(|s| s.as_ref().map(|(id, _)| id.clone()))
            .unwrap_or_else(|| "none".into());
        let worker_epoch = snapshot.worker_epoch.unwrap_or(0);
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            snapshot.instance_id,
            daemon_instance_id,
            worker_epoch,
            self.generation.load(Ordering::Acquire),
            status.state.status_name(),
            status.models_dir.display(),
            status.total_size_bytes,
            self.models_manager.identity_revision(),
            snapshot.dialogue_ready,
            dialogue,
            tts,
            snapshot.playback_ready,
            audio.is_some_and(|value| value.asr_ready),
            audio.is_some_and(|value| value.vad_ready),
            audio
                .map(|value| value.capture_state.as_str())
                .unwrap_or("none")
        )
    }

    /// Returns the stored report only while its attempt, version signature and
    /// TTL are still current. Expiry never runs a background check or hangs up
    /// an established call.
    pub async fn current_capabilities(
        &self,
    ) -> Option<nomifun_api_types::voice::VoiceCapabilityReport> {
        let stored = self
            .capabilities
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()?;
        let observed_attempt = stored.attempt;
        let observed_generation = stored.report.generation;
        if nomifun_common::now_ms() > stored.expires_at_ms
            || observed_attempt != self.self_check_attempt.load(Ordering::Acquire)
            || observed_generation != self.generation.load(Ordering::Acquire)
            || stored.signature != self.capability_signature().await
        {
            self.invalidate_capabilities_if(observed_attempt, observed_generation);
            return None;
        }
        // The cached report must still describe the current runtime: a 60s TTL
        // alone cannot hide a dead microphone, sink or engine between reads.
        if !readiness_runtime_scope_valid(&self.snapshot(), nomifun_common::now_ms()) {
            self.invalidate_capabilities_if(observed_attempt, observed_generation);
            return None;
        }
        // A newer check may have published while this read was validating.
        let still_current = self
            .capabilities
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|current| {
                current.attempt == observed_attempt && current.signature == stored.signature
            });
        if !still_current {
            return None;
        }
        Some(stored.report)
    }

    /// Explicit user-triggered full check. Capture comes from fresh PCM; TTS and
    /// playback come from the production daemon provider; the LLM check is a
    /// fixed, tool-free, context-free request and is opt-in because it may cost.
    /// Cancels the current explicit readiness check (including its fixed-sample
    /// playback) without touching an established voice call.
    pub fn cancel_self_check(&self) {
        self.self_check_cancel_signal.notify_waiters();
    }

    pub async fn run_self_check(
        self: &Arc<Self>,
        include_llm: bool,
    ) -> Result<nomifun_api_types::voice::VoiceCapabilityReport, String> {
        self.run_self_check_with_locale_and_stage(include_llm, None, None).await
    }

    pub async fn run_self_check_with_locale_and_stage(
        self: &Arc<Self>,
        include_llm: bool,
        locale: Option<String>,
        stage: Option<String>,
    ) -> Result<nomifun_api_types::voice::VoiceCapabilityReport, String> {
        tokio::select! {
            biased;
            _ = self.self_check_cancel_signal.notified() => Err("self_check_cancelled".into()),
            result = self.run_self_check_inner(include_llm, locale, stage) => result,
        }
    }

    async fn run_self_check_inner(
        self: &Arc<Self>,
        include_llm: bool,
        locale: Option<String>,
        stage: Option<String>,
    ) -> Result<nomifun_api_types::voice::VoiceCapabilityReport, String> {
        use nomifun_api_types::voice::{
            VoiceCapabilityEvidence, VoiceCapabilityReport, VoiceCapabilityStatus,
            VoiceEngineSelfCheckResponse, VoiceTtsSelfCheckResponse,
        };
        let _guard = self.self_check_lock.try_lock().map_err(|_| "self_check_in_progress")?;
        {
            let _admission = self.call_start_lock.lock().await;
            if self.self_check_active.load(Ordering::Acquire) {
                return Err("self_check_in_progress".into());
            }
            if self.voice_suite_in_use() {
                return Err("voice_suite_in_use".into());
            }
            self.self_check_active.store(true, Ordering::Release);
        }
        let attempt = self.self_check_attempt.fetch_add(1, Ordering::AcqRel) + 1;
        let checked_at_ms = nomifun_common::now_ms();
        let generation = self.generation.load(Ordering::Acquire);
        let snapshot = self.snapshot();
        let check_id = format!("{}-{}", snapshot.instance_id, attempt);
        let mut flight = SelfCheckFlight {
            manager: Arc::downgrade(self),
            check_id,
            finished: false,
        };
        self.push_admission("wake", false, 0).await;
        self.push_admission("call", false, 0).await;
        let signature = self.capability_signature().await;
        let evidence = |status: VoiceCapabilityStatus,
                        reason_code: Option<&str>,
                        evidence: &str| VoiceCapabilityEvidence {
            status,
            reason_code: reason_code.map(str::to_owned),
            evidence: evidence.into(),
            checked_at_ms: Some(checked_at_ms),
        };
        let audio = snapshot.audio.as_ref();
        let recent_pcm = audio.is_some_and(|value| {
            value.frames_received > 0
                && value
                    .last_frame_at_ms
                    .is_some_and(|at| checked_at_ms.saturating_sub(at) <= 2_000)
                && value.capture_state != "closed"
        });
        let capture = if recent_pcm {
            evidence(VoiceCapabilityStatus::Ready, None, "recent_pcm")
        } else {
            evidence(
                VoiceCapabilityStatus::Failed,
                Some("capture_no_recent_pcm"),
                "recent_pcm",
            )
        };
        let daemon_source = self.source.read().ok().and_then(|s| s.clone());
        let daemon_instance_id = daemon_source.as_ref().map(|(id, _)| id.clone());
        let expected_worker_epoch = snapshot.worker_epoch.unwrap_or(0);
        let engine_request_id = format!("engine-{}-{}", snapshot.instance_id, attempt);
        let model_revision = self.models_manager.identity_revision();
        let (engine_response, engine_bind_reason): (Option<VoiceEngineSelfCheckResponse>, Option<&str>) = {
            let client = match reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(90))
                .build()
            {
                Ok(client) => client,
                Err(_) => return Err("engine_check_client_failed".into()),
            };
            let sent = client
                .get(format!(
                    "{}/api/voice/diagnostics/engine-self-check?request_id={}&timeout_ms=45000",
                    self.daemon_origin, engine_request_id
                ))
                .header("x-ai-mate-control-token", &self.control_token)
                .header("x-voice-check-request", &engine_request_id)
                .header("x-voice-check-instance", &snapshot.instance_id)
                .header("x-voice-check-model-revision", &model_revision)
                .send()
                .await;
            match sent {
                Ok(response) if response.status().is_success() => {
                    match response.json::<VoiceEngineSelfCheckResponse>().await {
                        Ok(parsed)
                            if parsed.request_id == engine_request_id
                                && daemon_instance_id.as_deref() == Some(&parsed.instance_id)
                                && parsed.worker_epoch > 0
                                && expected_worker_epoch > 0
                                && parsed.worker_epoch == expected_worker_epoch
                                && parsed.model_revision == model_revision =>
                        {
                            (Some(parsed), None)
                        }
                        Ok(_) => (None, Some("engine_response_stale")),
                        Err(_) => (None, Some("engine_response_invalid")),
                    }
                }
                _ => (None, Some("engine_check_unreachable")),
            }
        };        let (kws, vad, asr) = match engine_response {
            Some(response) if response.fixtures_available => (
                if response.kws_ok {
                    evidence(VoiceCapabilityStatus::Ready, None, "fixed_audio_inference")
                } else {
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some("kws_inference_failed"),
                        "fixed_audio_inference",
                    )
                },
                if response.vad_ok {
                    evidence(VoiceCapabilityStatus::Ready, None, "fixed_audio_inference")
                } else {
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some("vad_inference_failed"),
                        "fixed_audio_inference",
                    )
                },
                if response.asr_ok {
                    evidence(VoiceCapabilityStatus::Ready, None, "fixed_audio_inference")
                } else {
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some("asr_inference_failed"),
                        "fixed_audio_inference",
                    )
                },
            ),
            Some(response) => {
                let reason = match response.reason.as_deref() {
                    Some(s) if s.starts_with("fixture_synthesis_failed") => {
                        "fixture_synthesis_failed".to_string()
                    }
                    Some(s) => s.to_string(),
                    None => "fixed_audio_missing".to_string(),
                };
                (
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(&reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(&reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(&reason),
                        "fixed_audio_inference",
                    ),
                )
            }
            None => {
                let reason = engine_bind_reason.unwrap_or("engine_check_unreachable");
                (
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "fixed_audio_inference",
                    ),
                )
            }        };

        let tts_response: Result<VoiceTtsSelfCheckResponse, String> = {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(40))
                .build()
                .map_err(|_| "tts_check_client_failed".to_string())?;
            match client
                .post(format!(
                    "{}/api/voice/diagnostics/tts-self-check",
                    self.daemon_origin
                ))
                .header("x-ai-mate-control-token", &self.control_token)
                .json(&serde_json::json!({ "check_id": flight.check_id, "locale": locale, "stage": stage }))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response
                    .json::<VoiceTtsSelfCheckResponse>()
                    .await
                    .map_err(|_| "tts_bad_response".to_string()),
                Ok(response) => {
                    let status_code = response.status().as_u16();
                    tracing::warn!(status = status_code, "voice_tts_self_check_rejected");
                    Err(format!("tts_http_{status_code}"))
                }
                Err(error) => {
                    tracing::warn!(%error, "voice_tts_self_check_unreachable");
                    Err("tts_check_unreachable".into())
                }
            }
        };
        let (tts, playback) = match tts_response {
            Ok(response) => {
                let languages_valid = tts_self_check_languages_valid(&response);
                let tts_valid = response.ok && languages_valid;
                (
                    if tts_valid {
                        evidence(VoiceCapabilityStatus::Ready, None, "tts_decoded")
                    } else {
                        evidence(
                            VoiceCapabilityStatus::Failed,
                            Some(if response.ok {
                                "tts_inconsistent_response"
                            } else {
                                response.reason.as_deref().unwrap_or("tts_failed")
                            }),
                            "tts_decoded",
                        )
                    },
                    if tts_valid && response.playback_ok {
                        evidence(VoiceCapabilityStatus::Ready, None, "sink_consumed")
                    } else {
                        evidence(
                            VoiceCapabilityStatus::Failed,
                            Some(if tts_valid {
                                "playback_not_consumed"
                            } else {
                                "playback_without_valid_synthesis"
                            }),
                            "sink_consumed",
                        )
                    },
                )
            }
            Err(reason) => (
                evidence(
                    VoiceCapabilityStatus::Failed,
                    Some(&reason),
                    "tts_decoded",
                ),
                evidence(
                    VoiceCapabilityStatus::Failed,
                    Some("playback_not_checked"),
                    "sink_consumed",
                ),
            ),
        };

        let llm = if !include_llm {
            evidence(
                VoiceCapabilityStatus::Unknown,
                Some("llm_check_not_run"),
                "live_response",
            )
        } else if let Some(completer) = &self.dialogue_completer {
            match tokio::time::timeout(
                std::time::Duration::from_secs(12),
                completer.complete("Reply with exactly: OK", None),
            )
            .await
            {
                Ok(Ok(reply)) if !reply.trim().is_empty() => {
                    evidence(VoiceCapabilityStatus::Ready, None, "live_response")
                }
                Ok(Ok(_)) => evidence(
                    VoiceCapabilityStatus::Failed,
                    Some("llm_empty_response"),
                    "live_response",
                ),
                Ok(Err(error)) => {
                    let reason = if error.contains("401") || error.contains("403") {
                        "llm_auth_failed"
                    } else if error.contains("429") {
                        "llm_rate_limited"
                    } else if error.contains("timeout") {
                        "llm_timeout"
                    } else {
                        "llm_failed"
                    };
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "live_response",
                    )
                }
                Err(_) => evidence(
                    VoiceCapabilityStatus::Failed,
                    Some("llm_timeout"),
                    "live_response",
                ),
            }
        } else {
            evidence(
                VoiceCapabilityStatus::Unsupported,
                Some("dialogue_unconfigured"),
                "live_response",
            )
        };

        let ready = |value: &VoiceCapabilityEvidence| {
            value.status == VoiceCapabilityStatus::Ready
        };
        let can_start_full_call = ready(&capture)
            && ready(&vad)
            && ready(&asr)
            && ready(&llm)
            && ready(&tts)
            && ready(&playback);
        let reason_code = if can_start_full_call {
            None
        } else {
            [&capture, &vad, &asr, &llm, &tts, &playback]
                .into_iter()
                .find(|value| !ready(value))
                .and_then(|value| value.reason_code.clone())
                .or_else(|| Some("self_check_incomplete".into()))
        };
        let mut report = VoiceCapabilityReport {
            capture,
            kws,
            vad,
            asr,
            llm,
            tts,
            playback,
            checked_at_ms: Some(checked_at_ms),
            generation,
            can_start_full_call,
            can_wake: false,
            can_transcribe: false,
            reason_code,
            degraded_text_only: false,
        };
        report.can_wake = ready(&report.kws)
            && ready(&report.vad)
            && ready(&report.asr)
            && ready(&report.capture);
        report.can_transcribe =
            ready(&report.vad) && ready(&report.asr) && ready(&report.capture);
        report.degraded_text_only = !report.can_start_full_call && report.can_transcribe;

        {
            let _admission = self.call_start_lock.lock().await;
            let current_signature = self.capability_signature().await;
            let current_daemon_source = self.source.read().ok().and_then(|s| s.clone());
            let current = attempt == self.self_check_attempt.load(Ordering::Acquire)
                && generation == self.generation.load(Ordering::Acquire)
                && signature == current_signature
                && daemon_source.is_some()
                && daemon_source.as_ref().map(|(id, _)| id) == current_daemon_source.as_ref().map(|(id, _)| id)
                && readiness_runtime_scope_valid(&self.snapshot(), nomifun_common::now_ms());
            if current {
                *self
                    .capabilities
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) = Some(StoredCapabilities {
                    report: report.clone(),
                    attempt,
                    expires_at_ms: checked_at_ms.saturating_add(60_000),
                    signature,
                });
            } else {
                report.can_start_full_call = false;
                report.reason_code = Some("self_check_superseded".into());
            }
            self.self_check_active.store(false, Ordering::Release);
            flight.finished = true;
        }
        self.push_admission(
            "wake",
            report.can_wake,
            if report.can_wake {
                checked_at_ms.saturating_add(60_000)
            } else {
                0
            },
        )
        .await;
        self.push_admission(
            "call",
            report.can_start_full_call,
            if report.can_start_full_call {
                checked_at_ms.saturating_add(60_000)
            } else {
                0
            },
        )
        .await;
        Ok(report)
    }

    /// Host authority for model writes: an active call, an unsettled task or a
    /// pending tool approval owns the acoustic suite until it finishes.
    pub fn voice_suite_in_use(&self) -> bool {
        let snapshot = self.snapshot();
        let active_call = snapshot.session_id.is_some()
            && snapshot.ended_reason.is_none()
            && snapshot.phase != VoiceSessionPhase::Idle;
        let unsettled_task = snapshot.task.as_ref().is_some_and(|task| {
            matches!(
                task.status,
                nomifun_api_types::voice::VoiceTaskStatus::Queued
                    | nomifun_api_types::voice::VoiceTaskStatus::Delivering
                    | nomifun_api_types::voice::VoiceTaskStatus::Unknown
                    | nomifun_api_types::voice::VoiceTaskStatus::Accepted
                    | nomifun_api_types::voice::VoiceTaskStatus::Cancelling
            )
        });
        let pending_approval = snapshot
            .confirmation
            .as_ref()
            .is_some_and(|confirmation| !confirmation.approval_id.is_empty());
        let pending_queue = !snapshot.queue.is_empty();
        let pending_start = self
            .dialogue_recorder
            .as_ref()
            .and_then(|recorder| recorder.current_call_reservation())
            .is_some();
        active_call || unsettled_task || pending_approval || pending_queue || pending_start
    }

    /// Atomically closes call/task admission, verifies nothing owns the suite,
    /// then releases the daemon (and its native model handles) before a write.
    pub async fn begin_model_write(self: &Arc<Self>) -> Result<ModelWriteGuard, String> {
        let _call = self.call_start_lock.lock().await;
        if self.resource_write_active.load(Ordering::Acquire) {
            return Err("voice_resource_write_in_progress".into());
        }
        if self.self_check_active.load(Ordering::Acquire) {
            return Err("voice_self_check_in_progress".into());
        }
        if self.voice_suite_in_use() {
            return Err("voice_suite_in_use".into());
        }
        self.resource_write_active.store(true, Ordering::Release);
        self.invalidate_capabilities();
        let resume_daemon =
            !self.shutdown_requested.load(Ordering::Acquire) && self.read_autostart_config();

        let released = {
            let mut child = self.child.lock().await;
            match child.take() {
                Some(mut process) => process.shutdown().await,
                None => Ok(()),
            }
        };
        if let Err(error) = released {
            self.resource_write_active.store(false, Ordering::Release);
            return Err(format!("daemon_release_failed: {error}"));
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.process_changed.notify_one();
        self.disconnected();
        Ok(ModelWriteGuard {
            manager: Arc::downgrade(self),
            resume_daemon,
            released: false,
        })
    }

    async fn resume_after_model_write(self: Arc<Self>) -> Result<(), String> {
        if self.shutdown_requested.load(Ordering::Acquire) || !self.read_autostart_config() {
            return Ok(());
        }
        self.start_child(false).await
    }

    pub fn global() -> Option<Arc<Self>> {
        GLOBAL_COMPANION_MANAGER.get().cloned()
    }

    pub fn new(models_manager: Arc<nomifun_voice_models::ModelManager>, data_dir: &Path) -> Self {
        let config_path = data_dir.join("config").join("voice_companion.json");
        let target_config = std::fs::read(&config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|val| val.get("target").cloned())
            .and_then(|target_val| serde_json::from_value::<VoiceTargetConfig>(target_val).ok())
            .unwrap_or_default();
        let (events, _) = broadcast::channel(128);
        Self {
            audio_recovery:std::sync::Mutex::new(AudioRecovery::default()),
            resource_write_active:AtomicBool::new(false),
            self_check_lock:tokio::sync::Mutex::new(()),
            self_check_active:AtomicBool::new(false),
            self_check_cancel_signal:tokio::sync::Notify::new(),
            self_check_attempt:AtomicU64::new(0),
            capabilities:std::sync::RwLock::new(None),
            restart_budget:std::sync::Mutex::new(RestartBudget::default()),
            prompted_approvals:std::sync::Mutex::new(std::collections::VecDeque::new()),
            revoked_sessions:RwLock::new(std::collections::VecDeque::new()),
            task_feedback: std::sync::Mutex::new(TaskFeedbackTracker::new()),
            daemon_origin: "http://127.0.0.1:18080".into(),
            #[cfg(test)]
            test_daemon_port: None,
            #[cfg(test)]
            last_child_exit: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(test)]
            test_child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            tasks: None,
            data_dir: data_dir.to_path_buf(),
            task_projection_started: AtomicBool::new(false),
            dialogue_completer: None,
            dialogue_recorder: None,
            target_selection: Arc::new(RwLock::new(target_config)),
            active_window_candidate: Arc::new(RwLock::new(VoiceWindowContext::default())),
            companion_service: None,
            dialogue_bridge: Mutex::new(None),
            config_write_lock: std::sync::Mutex::new(()),
            lifecycle_lock: Mutex::new(()),
            feedback_update_lock: Mutex::new(()),
            call_start_lock: tokio::sync::Mutex::new(()),
            handled_start_requests: std::sync::RwLock::new(std::collections::HashMap::new()),
            viewed_conversation: RwLock::new((0, None)),
            child: Arc::new(Mutex::new(None)),
            models_manager,
            config_path,
            control_token: format!(
                "{}{}",
                nomifun_common::generate_id(),
                nomifun_common::generate_id()
            ),
            snapshot: RwLock::new(VoiceSessionSnapshot {
                instance_id: nomifun_common::generate_id(),
                ..Default::default()
            }),
            source: RwLock::new(None),
            task_sync_seq: AtomicU64::new(0),
            events,
            projection_started: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            process_changed: Notify::new(),
            shutdown_requested: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> VoiceSessionSnapshot {
        self.snapshot
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub(crate) fn with_companion_service(mut self, companion_service: Arc<CompanionService>) -> Self {
        self.companion_service = Some(companion_service);
        self
    }

    pub fn target_selection(&self) -> Arc<RwLock<VoiceTargetConfig>> {
        self.target_selection.clone()
    }

    pub fn active_window_candidate(&self) -> Arc<RwLock<VoiceWindowContext>> {
        self.active_window_candidate.clone()
    }

    pub fn set_candidate_conversation(&self, candidate: Option<String>) {
        let mut context = self.active_window_candidate.write().unwrap_or_else(|e| e.into_inner());
        context.conversation_id = candidate;
        context.workspace = None;
    }

    pub fn report_window_context(&self, context: VoiceWindowContext) {
        let mut viewed = self.viewed_conversation.write().unwrap_or_else(|e| e.into_inner());
        // View-only reports participate in the same ordering as full targets.
        if context.sequence <= viewed.0 { return; }
        *viewed = (context.sequence, context.conversation_id.clone());
        let mut current = self.active_window_candidate.write().unwrap_or_else(|e| e.into_inner());
        if context.sequence > current.sequence { *current = context; }
    }

    pub fn report_voice_view(&self, conversation_id: Option<String>, sequence: u64) {
        let mut viewed = self.viewed_conversation.write().unwrap_or_else(|e| e.into_inner());
        if sequence > viewed.0 { *viewed = (sequence, conversation_id); }
    }

    pub fn requested_target(&self) -> VoiceTargetConfig {
        self.target_selection.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn record_handled_start_request(&self, request_id: String, outcome: HandledStartOutcome) {
        let mut map = self.handled_start_requests.write().unwrap_or_else(|e| e.into_inner());
        if map.len() >= 128 {
            let oldest_key = map.iter()
                .min_by_key(|(_, v)| match v {
                    HandledStartOutcome::Starting { reserved_at_epoch_ms, .. } => *reserved_at_epoch_ms,
                    HandledStartOutcome::Bound { bound_at_epoch_ms, .. } => *bound_at_epoch_ms,
                    HandledStartOutcome::Stopped { stopped_at_epoch_ms, .. } => *stopped_at_epoch_ms,
                    HandledStartOutcome::Rejected { at_epoch_ms, .. } => *at_epoch_ms,
                })
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                map.remove(&k);
            }
        }
        map.insert(request_id, outcome);
    }

    pub fn mark_request_bound(&self, request_id: &str, session_id: &str, conversation_id: &str, memory_owner: Option<&str>) {
        let mut map = self.handled_start_requests.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get_mut(request_id) {
            if !matches!(existing, HandledStartOutcome::Starting { conversation_id: expected, .. } if expected == conversation_id) {
                return;
            }
            let target = existing.target().clone();
            let session_mode = existing.session_mode().clone();
            *existing = HandledStartOutcome::Bound {
                target,
                session_mode,
                conversation_id: conversation_id.to_string(),
                session_id: Some(session_id.to_string()),
                memory_owner: memory_owner.map(|s| s.to_string()),
                bound_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
            };
        }
    }

    pub fn mark_active_request_stopped(&self, session_id: Option<&str>, call_request_id: Option<&str>) {
        let Some(sid) = session_id else { return; };
        {
            let mut map = self.handled_start_requests.write().unwrap_or_else(|e| e.into_inner());
            for (request_id, outcome) in map.iter_mut() {
                let matches = match outcome {
                    HandledStartOutcome::Starting { .. } => call_request_id == Some(request_id.as_str()),
                    HandledStartOutcome::Bound { session_id: Some(bound), .. } => bound == sid
                        && call_request_id.is_none_or(|id| id == request_id),
                    _ => false,
                };
                if matches {
                    if let HandledStartOutcome::Starting { conversation_id, target, session_mode, .. }
                        | HandledStartOutcome::Bound { conversation_id, target, session_mode, .. } = outcome {
                        *outcome = HandledStartOutcome::Stopped {
                            target: target.clone(), session_mode: session_mode.clone(),
                            conversation_id: conversation_id.clone(), session_id: Some(sid.into()),
                            stopped_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                        };
                    }
                }
            }
        }
        // Freeze takes the reservation lock before the receipt lock; never invert that order here.
        if let (Some(recorder), Some(request_id)) = (&self.dialogue_recorder, call_request_id) {
            recorder.take_call_reservation(request_id);
        }
    }

    pub async fn target_settings(&self) -> VoiceTargetSettings {
        // Keep the persisted request readable, but do not claim retired pet routing is effective.
        VoiceTargetSettings {
            requested: self.requested_target(),
            effective: VoiceTargetConfig { mode: VoiceTargetMode::VoiceHome, companion_id: None },
            effective_conversation_id: self.snapshot().binding.map(|binding| binding.conversation_id),
            reason: Some("workspace_routing_active".into()),
        }
    }
    pub async fn update_target(&self, update: VoiceTargetUpdateRequest) -> Result<VoiceTargetSettings, String> {
        let new_config = match update.mode {
            VoiceTargetMode::VoiceHome => VoiceTargetConfig {
                mode: VoiceTargetMode::VoiceHome,
                companion_id: None,
            },
            VoiceTargetMode::Companion => {
                let comp_id = update.companion_id.ok_or_else(|| "companion_id_required".to_string())?;
                if let Some(companion_service) = &self.companion_service {
                    if companion_service.get_companion(&comp_id).await.is_err() {
                        return Err("companion_not_found".to_string());
                    }
                }
                VoiceTargetConfig {
                    mode: VoiceTargetMode::Companion,
                    companion_id: Some(comp_id),
                }
            }
        };
        let value = serde_json::to_value(&new_config).map_err(|e| e.to_string())?;
        self.write_config_entry("target", value)?;
        *self.target_selection.write().unwrap_or_else(|e| e.into_inner()) = new_config;
        Ok(self.target_settings().await)
    }

    pub(crate) fn with_dialogue(mut self, completer: Arc<dyn VoiceDialogueCompleter>) -> Self {
        self.dialogue_completer = Some(completer);
        self
    }

    pub fn with_recorder(mut self, recorder: Arc<dyn VoiceDialogueRecorder>) -> Self {
        self.dialogue_recorder = Some(recorder);
        self
    }

    pub(crate) fn with_tasks(mut self, owner: String, runtime: Arc<dyn VoiceTaskRuntime>) -> Self {
        let path = self.data_dir.join("voice").join(format!("tasks-{owner}.json"));
        self.tasks = Some(Arc::new(VoiceTaskService::new(owner, path, runtime)));
        self
    }

    async fn execution_settings(&self) -> VoiceExecutionSettings {
        let session = self.snapshot().session_id;
        let candidate = match &self.dialogue_recorder {
            Some(recorder) => match session {
                Some(session) => recorder.bound_execution(&session).await,
                None => recorder.fallback_execution().await,
            },
            None => Ok(None),
        };
        let resolution = match (candidate, &self.tasks) {
            (Err(code), _) => Err(code),
            (Ok(None), _) => Err("execution_unconfigured".into()),
            (Ok(Some(_)), None) => Err("runtime_unavailable".into()),
            (Ok(Some(config)), Some(tasks)) => tasks.resolve_configuration(&config).await,
        };
        let (effective, reason) = match resolution { Ok(config) => (Some(config),None), Err(code) => (None,Some(code)) };
        let settings = VoiceExecutionSettings {
            effective,
            available: reason.is_none(),
            requested: None,
            reason_code: reason,
        };
        let mut snapshot = self.snapshot.write().unwrap_or_else(|error| error.into_inner());
        if snapshot.task_ready != settings.available {
            snapshot.task_ready = settings.available;
            snapshot.seq += 1;
            let _ = self.events.send(snapshot.clone());
        }
        settings
    }

    fn project_task(&self, task: VoiceTaskSnapshot) {
        let (task,queue) = match &self.tasks {
            Some(tasks) => match (tasks.current_task(),tasks.queue_projection()) {
                (Ok(Some(current)),Ok(queue)) => (current,queue),
                _ => return,
            },
            None => (task,Vec::new()),
        };
        let sync_seq;
        {
            let mut snapshot = self.snapshot.write().unwrap_or_else(|error| error.into_inner());
            // A poll or HTTP read may finish after a newer persisted receipt or
            // command. Publish only the current durable projection under this lock.
            if self.tasks.as_ref().is_some_and(|tasks|tasks.current_task().ok().flatten().as_ref()!=Some(&task)) {return;}
            if snapshot.task.as_ref() == Some(&task) && snapshot.queue == queue { return; }
            let changed_task = snapshot.task.as_ref() != Some(&task);
            snapshot.task = Some(task.clone());
            snapshot.queue = queue;
            if changed_task {snapshot.confirmation=None;}
            snapshot.task_error_code = None;
            snapshot.seq += 1;
            let _ = self.events.send(snapshot.clone());
            sync_seq = self.task_sync_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        }
        let daemon_origin = self.daemon_origin.clone();
        let token = self.control_token.clone();
        let daemon_instance_id = self.source.read().ok().and_then(|s| s.as_ref().map(|(id, _)| id.clone()));
        tokio::spawn(async move {
            let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build();
            if let Ok(c) = client {
                let payload = serde_json::json!({
                    "task": Some(task),
                    "instance_id": daemon_instance_id,
                    "seq": sync_seq,
                });
                let _ = c.post(format!("{daemon_origin}/api/voice/task-status")).header("x-ai-mate-control-token", token).json(&payload).send().await;
            }
        });
    }

    fn project_task_error(&self, code: String) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|error| error.into_inner());
        if snapshot.task_error_code.as_ref() == Some(&code) { return; }
        snapshot.task_error_code = Some(code);
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    fn project_confirmation(&self,task:&VoiceTaskSnapshot,confirmation:Option<VoiceToolConfirmation>) {
        let mut snapshot=self.snapshot.write().unwrap_or_else(|error|error.into_inner());
        if snapshot.task.as_ref()!=Some(task) || snapshot.confirmation==confirmation {return;}
        snapshot.confirmation=confirmation;
        snapshot.seq+=1;
        let _=self.events.send(snapshot.clone());
    }

    fn start_task_projection(self: &Arc<Self>) {
        if self.tasks.is_none() || self.task_projection_started.swap(true, Ordering::AcqRel) { return; }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut speech:Option<tokio::task::JoinHandle<()>>=None;
            if let Some(manager) = weak.upgrade() {manager.execution_settings().await;}
            loop {
                let Some(manager)=weak.upgrade() else {return;};
                let tasks=manager.tasks.as_ref().expect("task projection requires its adapter");
                let mut active=None;
                let mut approval=None;
                match tasks.status().await {
                    Ok(Some(task))=>{
                        manager.project_task(task.clone());
                        let progress=if matches!(task.status,VoiceTaskStatus::Accepted|VoiceTaskStatus::Cancelling) {tasks.progress_for(&task).ok()}else{None};
                        let notification=tasks.notification_for(&task.command_id, manager.snapshot().locale.as_deref()).ok().flatten();
                        {
                            let mut snapshot=manager.snapshot.write().unwrap_or_else(|e|e.into_inner());
                            if snapshot.task_progress!=progress || snapshot.task_notification!=notification {
                                snapshot.task_progress=progress;snapshot.task_notification=notification;snapshot.seq+=1;let _=manager.events.send(snapshot.clone());
                            }
                        }
                        match tasks.confirmations(&task.command_id).await {
                            Ok(confirmations)=>{approval=confirmations.into_iter().next();manager.project_confirmation(&task,approval.clone());},
                            Err(code)=>manager.project_task_error(code),
                        }
                        active=Some(task);
                    },
                    Ok(None)=>{}, Err(code)=>manager.project_task_error(code),
                }
                let _=manager.reconcile_announcements().await;
                // Playback and approval listening never hold up deadline/receipt observation.
                // One owned speech job keeps notices bounded and gives terminal/approval work priority.
                if speech.as_ref().is_none_or(|job|job.is_finished()) {
                    // Recover the binding after a missed SSE frame/reconnect before selecting a notice.
                    if manager.snapshot().binding.is_none() && active.is_some() {
                        let _ = manager.refresh_daemon_snapshot().await;
                    }
                    let active_binding = manager.snapshot().binding;
                    if let Some(binding) = active_binding {
                        let conv_id = binding.conversation_id.clone();
                        if let (Some(task),Some(confirmation))=(active.clone().filter(|t| t.conversation_id.as_deref() == Some(conv_id.as_str())),approval) {
                            let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.notify_confirmation(&task,&confirmation).await {owner.project_task_error(code);}}));
                        } else if let Ok(Some((task,goal)))=tasks.pending_result_for_conversation(&conv_id) {
                            manager.task_feedback.lock().unwrap_or_else(|e|e.into_inner()).clear_pending();
                            let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.notify_task_result(&task,&goal).await {owner.project_task_error(code);}}));
                        } else if let Some(task)=active.filter(|task|task.conversation_id.as_deref() == Some(conv_id.as_str()) && task.status==VoiceTaskStatus::Accepted) {
                            let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.maybe_notify_task_progress(&task).await {owner.project_task_error(code);}}));
                        }
                    }
                }
                drop(manager);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }

    async fn maybe_notify_task_progress(&self, task: &VoiceTaskSnapshot) -> Result<(), String> {
        let now_ms = nomifun_common::now_ms();
        let decision = {
            let mut tracker = self.task_feedback.lock().unwrap_or_else(|e| e.into_inner());
            tracker.evaluate_progress(task,self.snapshot().task_progress.as_ref(),now_ms)
        };
        let TaskFeedbackDecision::TriggerFeedback { command_id: _, stage: _ } = decision else {
            return Ok(());
        };

        let snapshot = self.snapshot();
        if !snapshot.enabled || snapshot.connection != VoiceConnectionState::Connected {
            return Ok(());
        }
        let Some(binding) = &snapshot.binding else { return Ok(()); };
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Ok(()); }

        // Priority 1: Check active tool confirmation
        if snapshot.confirmation.is_some() {
            return Ok(());
        }

        // Priority 2: Check acoustic state (no 抢话 during user utterance)
        if snapshot.audio.as_ref().is_some_and(|audio| audio.speech_detected) {
            return Ok(());
        }

        let source = match self.refresh_daemon_snapshot().await {
            Ok(source) => source,
            Err(_) => return Ok(()),
        };
        if source.session_id.as_deref() != Some(&binding.session_id) {
            return Ok(());
        }
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected)
        {
            return Ok(());
        }

        let tasks=self.tasks.as_ref().ok_or("runtime_unavailable")?;
        if tasks.status_for(&task.command_id).await?.status!=VoiceTaskStatus::Accepted {return Ok(());}
        let progress=tasks.progress_for(task)?;
        let text=match progress.health.as_str() {
            "unverified"=>"暂时无法核实执行侧状态。任务尚未确认结束，你可以查询、取消或查看任务详情。",
            "no_recent_progress"=>"任务九十秒内没有新的业务进展，尚未确认结束。你可以继续等待、查询或取消。",
            _=>match progress.stage.as_str() {
                "thinking"=>"执行侧已报告正在分析任务，结果尚未返回。",
                "tool"=>"执行侧已报告工具调用进展，最终结果尚未返回。",
                "answering"=>"执行侧正在生成文字结果，尚未确认完成。",
                "planning"=>"执行侧已更新计划，任务尚未确认完成。",
                "approval"=>return Ok(()),
                _=>"任务已经接受，尚未收到新的执行进展。你可以查询或取消任务。",
            },
        };
        let request=nomifun_api_types::voice::VoiceTaskResultAnnouncement {
            scope:VoiceSessionControlRequest{instance_id:source.instance_id,session_id:source.session_id,turn_id:source.turn_id},
            command_id:task.command_id.clone(),notification_id:Some(format!("voice-progress:{}",task.command_id)),
            attempt_id:Some(format!("voice-progress:{}:{now_ms}",task.command_id)),goal:None,text:text.into(),
        };
        let response=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"daemon_unavailable")?
            .post(format!("{}/api/voice/task-progress",self.daemon_origin)).header("x-ai-mate-control-token",&self.control_token)
            .json(&request).timeout(std::time::Duration::from_secs(45)).send().await;
        if let Ok(response)=response && response.status().is_success() {
            self.task_feedback.lock().unwrap_or_else(|e|e.into_inner()).record_dispatched(now_ms);
        }
        Ok(())
    }

    async fn notify_task_result(&self, task: &VoiceTaskSnapshot, goal:&str) -> Result<(),String> {
        let snapshot = self.snapshot();
        if !matches!(task.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed)
            || !snapshot.enabled || snapshot.connection!=VoiceConnectionState::Connected
        {return Ok(());}
        let Some(binding) = &snapshot.binding else { return Ok(()); };
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Ok(()); }
        let tasks = self.tasks.as_ref().ok_or("runtime_unavailable")?;
        if tasks.announcement_claimed(&task.command_id)? {return Ok(());}
        let source = match self.refresh_daemon_snapshot().await {Ok(source) => source,Err(_) => return Ok(())};
        if source.session_id.as_deref() != Some(&binding.session_id) { return Ok(()); }
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected) {return Ok(());}
        let text = format_task_result_announcement(task, goal, nomifun_api_types::voice::is_english_locale(source.locale.as_deref()));
        // Claim durably before dispatch: a lost response or host restart must
        // not replay a result as a new announcement. Text remains in the task.
        if !tasks.claim_announcement(&task.command_id,true).await? {return Ok(());}
        let notification=tasks.notification_for(&task.command_id, source.locale.as_deref())?.ok_or("task_announcement_failed")?;
        let request = nomifun_api_types::voice::VoiceTaskResultAnnouncement {
            scope:VoiceSessionControlRequest {instance_id:source.instance_id,session_id:source.session_id,turn_id:source.turn_id},
            command_id:task.command_id.clone(),
            notification_id: Some(notification.notification_id),
            attempt_id: Some(notification.attempt_id.clone()),
            goal:Some(goal.to_owned()),
            text,
        };
        let response = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "task_announcement_failed")?
            .post(format!("{}/api/voice/task-result",self.daemon_origin))
            .header("x-ai-mate-control-token",&self.control_token).json(&request)
            .timeout(std::time::Duration::from_secs(45)).send().await;
        match response {
            Ok(response) if response.status().is_success() => {
                let receipt=match response.json::<nomifun_api_types::voice::VoicePlaybackReceipt>().await {
                    Ok(receipt)=>receipt,
                    Err(_)=>{tasks.finish_announcement(&task.command_id,&notification.attempt_id,"unknown").await?;return Err("task_announcement_failed".into());},
                };
                if receipt.attempt_id!=notification.attempt_id {tasks.finish_announcement(&task.command_id,&notification.attempt_id,"unknown").await?;return Err("task_scope_changed".into());}
                tasks.finish_announcement(&task.command_id,&receipt.attempt_id,&receipt.status).await?;
                if receipt.status=="failed" {return Err("task_announcement_failed".into());}
                Ok(())
            },
            Ok(response) if response.status()==reqwest::StatusCode::CONFLICT => {
                let body=response.json::<serde_json::Value>().await.unwrap_or_default();
                if matches!(body["code"].as_str(),Some("voice_busy"|"task_scope_changed")) {
                    tasks.claim_announcement(&task.command_id,false).await?;
                    Ok(())
                } else {
                    tasks.finish_announcement(&task.command_id,&notification.attempt_id,"failed").await?;
                    Err("task_announcement_failed".into())
                }
            },
            _ => {
                tasks.finish_announcement(&task.command_id,&notification.attempt_id,"unknown").await?;
                Err("task_announcement_failed".into())
            },
        }
    }

    async fn reconcile_announcements(&self)->Result<(),String> {
        let Some(tasks)=&self.tasks else {return Ok(());};
        let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"daemon_unavailable")?;
        for (command,attempt) in tasks.unresolved_announcements()? {
            let mut url=reqwest::Url::parse(&format!("{}/api/voice/notifications/",self.daemon_origin)).map_err(|_|"daemon_unavailable")?;
            url.path_segments_mut().map_err(|_|"daemon_unavailable")?.pop_if_empty().push(&attempt);
            let response=client.get(url).header("x-ai-mate-control-token",&self.control_token).timeout(std::time::Duration::from_millis(250)).send().await;
            if let Ok(response)=response && response.status().is_success() {
                let receipt=response.json::<nomifun_api_types::voice::VoicePlaybackReceipt>().await.map_err(|_|"task_announcement_failed")?;
                if receipt.attempt_id==attempt {tasks.finish_announcement(&command,&attempt,&receipt.status).await?;}
            } else {tasks.finish_announcement(&command,&attempt,"unknown").await?;}
        }
        Ok(())
    }

    pub async fn replay_task_result(&self, command_id: &str,request:&nomifun_api_types::voice::VoiceTaskReplayRequest) -> Result<(), String> {
        let scope=self.snapshot();
        if request.instance_id!=scope.instance_id || scope.session_id.as_deref()!=Some(&request.session_id) || scope.turn_id.as_deref()!=Some(&request.turn_id) {return Err("task_scope_changed".into());}
        if !scope.enabled || scope.connection!=VoiceConnectionState::Connected {return Err("daemon_unavailable".into());}
        if scope.ended_reason==Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop) {return Err("task_scope_changed".into());}
        let Some(binding) = &scope.binding else { return Err("binding_invalid".into()); };
        self.reconcile_announcements().await?;
        let tasks = self.tasks.as_ref().ok_or("runtime_unavailable")?;
        let task = tasks.status_for(command_id).await?;
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Err("task_command_conflict".into()); }
        if !matches!(task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {
            return Err("task_not_terminal".into());
        }
        let goal = tasks.pending_result_for_conversation(&binding.conversation_id).ok().flatten()
            .and_then(|(t, g)| (t.command_id == command_id).then_some(g))
            .unwrap_or_else(|| "先前任务".into());
        // Unclaim so next claim generates a new attempt_id
        if !tasks.request_replay(command_id,&request.replay_id).await? {return Ok(());}
        self.notify_task_result(&task, &goal).await
    }

    async fn notify_confirmation(&self,task:&VoiceTaskSnapshot,confirmation:&VoiceToolConfirmation)->Result<(),String> {
        use nomifun_api_types::voice::{VoiceTaskReference,VoiceToolConfirmationPrompt,VoiceToolConfirmationReply};
        let snapshot=self.snapshot();
        if !snapshot.enabled || snapshot.connection!=VoiceConnectionState::Connected
            || confirmation.expires_at_ms<=nomifun_common::now_ms() {return Ok(());}
        let Some(binding) = &snapshot.binding else { return Ok(()); };
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Ok(()); }
        if self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner()).contains(&confirmation.approval_id){return Ok(());}
        let source=match self.refresh_daemon_snapshot().await {Ok(source)=>source,Err(_)=>return Ok(())};
        if source.session_id.as_deref() != Some(&binding.session_id) { return Ok(()); }
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected) {return Ok(());}
        let summary=nomifun_ai_agent::voice_confirmation_summary(&confirmation.tool_name,&confirmation.arguments).ok_or("task_approval_detail_required")?;
        let text=format!("准备执行工具 {}，参数为 {}。完整参数可在面板查看。提示结束后，请说确认或取消；只允许本次操作。",confirmation.tool_name,summary);
        let request=VoiceToolConfirmationPrompt {
            scope:VoiceSessionControlRequest {instance_id:source.instance_id,session_id:source.session_id,turn_id:source.turn_id},
            task:VoiceTaskReference::from(task),approval_id:confirmation.approval_id.clone(),msg_id:confirmation.msg_id.clone(),call_id:confirmation.call_id.clone(),
            expires_at_ms:confirmation.expires_at_ms,text,
        };
        {
            let mut prompted=self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner());
            if prompted.contains(&confirmation.approval_id){return Ok(());}
            prompted.push_back(confirmation.approval_id.clone());if prompted.len()>64{prompted.pop_front();}
        }
        let remaining=std::time::Duration::from_millis(confirmation.expires_at_ms.saturating_sub(nomifun_common::now_ms()).max(1) as u64);
        let response=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"task_approval_failed")?
            .post(format!("{}/api/voice/confirmation",self.daemon_origin)).header("x-ai-mate-control-token",&self.control_token)
            .json(&request).timeout(remaining).send().await;
        let approved=match response {
            Ok(response) if response.status().is_success()=>response.json::<VoiceToolConfirmationReply>().await.ok()
                .is_some_and(|reply|reply.approval_id==confirmation.approval_id && reply.msg_id==confirmation.msg_id && reply.call_id==confirmation.call_id && reply.approved),
            Ok(response) if response.status()==reqwest::StatusCode::CONFLICT=>{
                self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner()).retain(|id|id!=&confirmation.approval_id);
                return Ok(());
            },
            _=>false,
        };
        let decision=VoiceToolConfirmationRequest {instance_id:snapshot.instance_id,command_id:task.command_id.clone(),conversation_id:task.conversation_id.clone(),message_id:task.message_id.clone(),msg_id:confirmation.msg_id.clone(),call_id:confirmation.call_id.clone(),approved};
        let tasks=self.tasks.as_ref().ok_or("runtime_unavailable")?;
        match tasks.confirm(&confirmation.approval_id,&decision).await {
            Err(code) if code=="task_approval_changed"=>Ok(()),
            result=>result,
        }
    }

    async fn submit_task(self: &Arc<Self>, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
        if self.resource_write_active.load(Ordering::Acquire) {
            return Err("voice_resource_write_in_progress".into());
        }
        let tasks = self.tasks.clone().ok_or("runtime_unavailable")?;
        let snapshot = self.snapshot();
        if request.instance_id != snapshot.instance_id {return Err("task_scope_changed".into());}
        if let Some(delivery)=tasks.replay(&request.command).await? {
            self.project_task(delivery.task.clone());
            return Ok(delivery);
        }
        if snapshot.session_id.as_deref() != Some(request.command.session_id.as_str())
            || snapshot.turn_id.as_deref() != Some(request.command.turn_id.as_str())
            || !matches!(snapshot.phase, VoiceSessionPhase::Deciding | VoiceSessionPhase::Dispatching)
            || !snapshot.enabled || snapshot.connection != VoiceConnectionState::Connected
            || self.revoked_sessions.read().unwrap_or_else(|error|error.into_inner()).contains(&request.command.session_id)
        { return Err("task_scope_changed".into()); }
        let recorder = self.dialogue_recorder.as_ref().ok_or("binding_invalid")?;
        let frozen = recorder.task_configuration(&request.command).await?.ok_or("binding_invalid")?;
        let config = frozen.config;
        // Admission belongs to the host: dropping a voice request cannot cancel
        // an accepted task or abandon its durable delivery record.
        let manager = self.clone();
        let command_id = request.command.command_id.clone();
        let command_id_timeout = command_id.clone();
        let session_id = request.command.session_id.clone();
        let turn_id = request.command.turn_id.clone();
        let tasks_ref = tasks.clone();
        let handoff = frozen.handoff_content;
        let display_text = frozen.display_text;
        let target_conversation = frozen.conversation_id;
        let message_id = frozen.message_id;
        let voice_context_digest = frozen.voice_context_digest;
        let expected_preset = Some(frozen.preset);
        let handle = tokio::spawn(async move {
            match tasks.submit_frozen_with_handoff_queued_with_context(
                request.command,
                config,
                expected_preset,
                handoff,
                display_text,
                target_conversation,
                message_id,
                voice_context_digest,
            ).await {
                Ok(delivery) => { manager.project_task(delivery.task.clone()); Ok(delivery) },
                Err(code) => {
                    if matches!(code.as_str(), "task_creation_unknown" | "task_delivery_unknown") {
                        if let Ok(task) = tasks.status_for(&command_id).await {
                            manager.project_task(task.clone());
                            return Ok(VoiceTaskDelivery { task, freshly_accepted: false });
                        }
                    }
                    if let Ok(task)=tasks.status_for(&command_id).await {manager.project_task(task);}
                    manager.project_task_error(code.clone());
                    Err(code)
                }
            }
        });
        match tokio::time::timeout(std::time::Duration::from_secs(3), handle).await {
            Ok(result) => result.map_err(|_| "task_delivery_unknown".to_string())?,
            Err(_timeout) => {
                let task = tasks_ref.status_for(&command_id_timeout).await.unwrap_or_else(|_| VoiceTaskSnapshot {
                    command_id: command_id_timeout,
                    session_id,
                    acoustic_turn_id: turn_id,
                    conversation_id: None,
                    message_id: None,
                    runtime_turn_id: None,
                    status: VoiceTaskStatus::Unknown,
                    result_ok: None,
                    result_text: None,
                    error_code: Some("task_delivery_pending".into()),
                });
                self.project_task(task.clone());
                Ok(VoiceTaskDelivery { task, freshly_accepted: false })
            }
        }
    }

    async fn submit_keyboard_message(self:&Arc<Self>, request:nomifun_api_types::voice::VoiceKeyboardMessageRequest) -> Result<VoiceTaskDelivery,String> {
        let tasks = self.tasks.clone().ok_or("runtime_unavailable")?;
        let command = nomifun_api_types::voice::VoiceTaskCommand {
            session_id:format!("keyboard-{}",request.conversation_id),turn_id:request.command_id.clone(),
            command_id:request.command_id,goal:request.text,record_id:None,binding_id:None,
        };
        VoiceTaskService::validate_command(&command)?;
        // A retry keeps the original frozen configuration even after a model switch.
        if let Some(delivery) = tasks.replay(&command).await? {return Ok(delivery);}
        let frozen = self.dialogue_recorder.as_ref().ok_or("binding_invalid")?
            .keyboard_configuration(&request.conversation_id).await?;
        let manager = self.clone();
        tokio::spawn(async move {
            let result = tasks.submit_frozen_with_handoff_queued_with_context(command.clone(),frozen.config,Some(frozen.preset),
                Some(command.goal.clone()),Some(command.goal),Some(request.conversation_id),None,None).await;
            if let Ok(Some(task)) = tasks.current_task() {manager.project_task(task);}
            result
        }).await.map_err(|_|"task_delivery_unknown".to_owned())?
    }

    async fn control_task(self:&Arc<Self>,request:VoiceTaskControlRequest,action:VoiceTaskControlAction) -> Result<VoiceTaskSnapshot,String> {
        if action == VoiceTaskControlAction::CancelTask {
            self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        }
        if request.instance_id!=self.snapshot().instance_id {return Err("task_scope_changed".into());}
        let tasks=self.tasks.clone().ok_or("runtime_unavailable")?;
        let current=tasks.task_by_id(&request.task.command_id)?.ok_or("task_not_found")?;
        if !request.task.matches(&current) {return Err("task_scope_changed".into());}
        if action==VoiceTaskControlAction::QueryStatus {
            let task=tasks.status_for(&request.task.command_id).await?;
            self.project_task(task.clone());return Ok(task);
        }
        if !matches!(current.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed)
            && (current.conversation_id.is_none() || current.message_id.is_none())
        {return Err("task_delivery_pending".into());}
        let manager=self.clone();
        tokio::spawn(async move {
            let result=tasks.cancel(&request.task.command_id).await;
            if let Ok(task)=tasks.current_task() && let Some(task)=task {manager.project_task(task);}
            if let Err(code)=&result {manager.project_task_error(code.clone());}
            result
        }).await.map_err(|_|"task_control_unknown".to_owned())?
    }

    async fn dialogue_settings(&self) -> VoiceDialogueSettings {
        let effective = match &self.dialogue_completer {
            Some(completer) => completer.resolve_model(None).await.ok(),
            None => None,
        };
        VoiceDialogueSettings {
            config: VoiceDialogueConfig {
                dialogue_model: None,
            },
            available: effective.is_some(),
            effective_model: effective,
        }
    }

    async fn start_dialogue_bridge(self: &Arc<Self>) -> Result<VoiceDialogueBridge, String> {
        VoiceDialogueBridge::start_with_recorder(
            self.dialogue_completer.clone().ok_or("model_unavailable")?,
            Arc::new(RwLock::new(None)),
            self.control_token.clone(),
            self.private_task_routes(),
            self.dialogue_recorder.clone(),
        ).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<VoiceSessionSnapshot> {
        self.events.subscribe()
    }

    pub(crate) fn private_task_routes(self: &Arc<Self>) -> axum::Router {
        axum::Router::new().route("/tasks", axum::routing::post(private_task_submit).get(private_current_task))
            .route("/task-control",axum::routing::post(private_task_control))
            .with_state(Arc::downgrade(self))
    }

    async fn private_voice_scope(&self, request:&VoiceSessionControlRequest) -> Result<(),String> {
        let source=self.refresh_daemon_snapshot().await?;
        if !request.matches(&source) || !matches!(source.phase,VoiceSessionPhase::Deciding|VoiceSessionPhase::Dispatching)
            || request.session_id.as_ref().is_some_and(|id|self.revoked_sessions.read().unwrap_or_else(|error|error.into_inner()).contains(id))
        {return Err("task_scope_changed".into());}
        Ok(())
    }

    async fn refresh_daemon_snapshot(&self) -> Result<VoiceSessionSnapshot, String> {
        let generation = self.generation.load(Ordering::Acquire);
        let response = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "daemon_unavailable")?
            .get(format!("{}/api/voice/snapshot", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .timeout(std::time::Duration::from_secs(3)).send().await.map_err(|_| "daemon_unavailable")?;
        if !response.status().is_success() {return Err("daemon_unavailable".into());}
        let source: VoiceSessionSnapshot = response.json().await.map_err(|_| "daemon_unavailable")?;
        if generation != self.generation.load(Ordering::Acquire) { return Err("task_scope_changed".into()); }
        self.project_with_binding(source.clone(), generation).await;
        Ok(source)
    }

    async fn project_with_binding(&self, source: VoiceSessionSnapshot, generation: u64) {
        self.project(source.clone(), generation);
        if generation != self.generation.load(Ordering::Acquire) { return; }
        if let (Some(recorder), Some(session)) = (&self.dialogue_recorder, &source.session_id) {
            if matches!(source.phase, VoiceSessionPhase::Listening | VoiceSessionPhase::Transcribing | VoiceSessionPhase::Deciding)
                && self.snapshot().binding.is_none() {
                match recorder.freeze_session(session, source.call_request_id.as_deref()).await {
                    Ok(Some(binding)) => {
                        let mut current = self.snapshot.write().unwrap_or_else(|e|e.into_inner());
                        if generation == self.generation.load(Ordering::Acquire) && current.session_id.as_ref() == Some(session) {
                            current.binding = Some(binding);
                            current.seq += 1;
                            let _ = self.events.send(current.clone());
                        }
                    },
                    Err(_) => self.project_task_error("binding_invalid".into()),
                    _ => {},
                }
            }
        }
    }

    fn requested_feedback(&self) -> Result<VoiceFeedbackConfig, String> {
        if !self.config_path.exists() {
            return Ok(Default::default());
        }
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&self.config_path).map_err(|_| "configuration_read_failed")?,
        )
        .map_err(|_| "configuration_invalid")?;
        let object = value.as_object().ok_or("configuration_invalid")?;
        let config: VoiceFeedbackConfig = match object.get("feedback") {
            Some(value) => {
                serde_json::from_value(value.clone()).map_err(|_| "configuration_invalid")?
            }
            None => Default::default(),
        };
        config.validate()?;
        Ok(config)
    }

    async fn feedback_rpc(
        &self,
        method: reqwest::Method,
        suffix: &str,
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response, String> {
        if self.snapshot().connection != VoiceConnectionState::Connected {
            return Err("daemon_unavailable".into());
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "daemon_unavailable")?;
        let mut request = client
            .request(
                method,
                format!("{}/api/voice/feedback{suffix}",self.daemon_origin),
            )
            .header("x-ai-mate-control-token", &self.control_token)
            .timeout(std::time::Duration::from_secs(3));
        if let Some(body) = body {
            request = request.json(&body);
        }
        request
            .send()
            .await
            .map_err(|_| "daemon_unavailable".into())
    }

    async fn feedback_settings(&self) -> Result<VoiceFeedbackSettings, String> {
        let requested = self.requested_feedback()?;
        let effective = match self.feedback_rpc(reqwest::Method::GET, "", None).await {
            Ok(response) if response.status().is_success() => response.json().await.ok(),
            _ => None,
        };
        Ok(VoiceFeedbackSettings {
            requested,
            effective,
        })
    }

    async fn update_feedback(
        &self,
        patch: VoiceFeedbackPatch,
    ) -> Result<VoiceFeedbackSettings, String> {
        let _guard = self.feedback_update_lock.lock().await;
        let requested = patch.apply(self.requested_feedback()?);
        requested.validate()?;
        let value = serde_json::to_value(requested).map_err(|_| "configuration_invalid")?;
        self.write_config_entry("feedback", value.clone())?;
        let effective = match self
            .feedback_rpc(reqwest::Method::POST, "", Some(value))
            .await
        {
            Ok(response) if response.status().is_success() => response.json().await.ok(),
            _ => None,
        };
        Ok(VoiceFeedbackSettings {
            requested,
            effective,
        })
    }

    #[cfg(test)]
    pub(crate) fn replay_acoustic_source(&self, snapshot: VoiceSessionSnapshot) {
        self.projection_started.store(true, Ordering::Release);
        self.project(snapshot, self.generation.load(Ordering::Acquire));
    }

    #[cfg(test)]
    #[cfg(test)]
    fn with_test_daemon_port(mut self, port: u16) -> Self {
        self.daemon_origin = format!("http://127.0.0.1:{port}");
        self.test_daemon_port = Some(port);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_replay_daemon(mut self, origin: String) -> Self {
        self.daemon_origin = origin;
        self.projection_started.store(true, Ordering::Release);
        self
    }

    #[cfg(test)]
    pub(crate) fn replay_control_credential(&self) -> String { self.control_token.clone() }

    #[cfg(test)]
    pub(crate) fn replay_task_projection(&self, task: VoiceTaskSnapshot) {self.project_task(task);}

    #[doc(hidden)]
    pub fn project_snapshot_for_test(&self, source_snapshot: VoiceSessionSnapshot, generation: u64) -> VoiceSessionSnapshot {
        if generation != self.generation.load(Ordering::Acquire) {
            return self.snapshot();
        }
        self.apply_host_projection(source_snapshot)
    }

    #[doc(hidden)]
    pub async fn project_with_binding_for_test(&self, source_snapshot: VoiceSessionSnapshot, generation: u64) {
        self.project_with_binding(source_snapshot, generation).await;
    }

    #[doc(hidden)]
    pub fn project_task_for_test(&self, task: VoiceTaskSnapshot) {
        self.project_task(task);
    }

    fn apply_host_projection(&self, mut source_snapshot: VoiceSessionSnapshot) -> VoiceSessionSnapshot {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        source_snapshot.instance_id = snapshot.instance_id.clone();
        source_snapshot.seq = snapshot.seq + 1;
        source_snapshot.enabled = self.read_autostart_config();
        source_snapshot.connection = VoiceConnectionState::Connected;
        source_snapshot.task = snapshot.task.clone();
        source_snapshot.queue = snapshot.queue.clone();
        source_snapshot.binding = snapshot.binding.clone().filter(|binding|Some(&binding.session_id)==source_snapshot.session_id.as_ref());
        source_snapshot.task_progress = snapshot.task_progress.clone();
        source_snapshot.task_notification = snapshot.task_notification.clone();
        source_snapshot.confirmation=snapshot.confirmation.clone();
        source_snapshot.task_ready = snapshot.task_ready;
        source_snapshot.task_error_code = snapshot.task_error_code.clone();
        source_snapshot.conversation.retain(|turn| turn.session_id.is_some() && turn.session_id == source_snapshot.session_id);
        source_snapshot.projected_turn_ids.clear();
        if let Some(binding) = &source_snapshot.binding {
            for turn in &mut source_snapshot.conversation {
                if let Some(recorder) = &self.dialogue_recorder {
                    if recorder.is_projected(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                        source_snapshot.projected_turn_ids.push(turn.turn_id.clone());
                        turn.error = None;
                    } else if turn.error.is_none() {
                        if let Some(error) = recorder.turn_error(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                            turn.error = Some(error);
                        }
                    }
                    if let Some((user, assistant)) = recorder.projected_message_ids(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                        turn.user_message_id = user;
                        turn.assistant_message_id = Some(assistant);
                    }
                }
            }
            if let Some(recorder) = &self.dialogue_recorder {
                if let Some(curr_turn) = source_snapshot.turn_id.as_deref() {
                    if recorder.is_projected(&binding.session_id, &binding.conversation_id, curr_turn) {
                        source_snapshot.error = None;
                    } else if let Some(error) = recorder.turn_error(&binding.session_id, &binding.conversation_id, curr_turn) {
                        source_snapshot.error = Some(error);
                    }
                }
            }
        }
        // Fresh daemon audio snapshots must not erase the last readable reply.
        // Session/turn identities still come exclusively from the new daemon.
        if source_snapshot.session_id.is_none() && source_snapshot.conversation.is_empty() {
            source_snapshot.conversation=snapshot.conversation.clone();
            source_snapshot.user_text=snapshot.user_text.clone();
            source_snapshot.understood_request=snapshot.understood_request.clone();
            source_snapshot.assistant_text=snapshot.assistant_text.clone();
        }
        *snapshot = source_snapshot.clone();
        let _ = self.events.send(snapshot.clone());
        source_snapshot
    }

    fn project(&self, source_snapshot: VoiceSessionSnapshot, generation: u64) {
        if generation != self.generation.load(Ordering::Acquire) {
            return;
        }
        let mut source = self.source.write().unwrap_or_else(|err| err.into_inner());
        if source.as_ref().is_some_and(|(id, seq)| {
            *id == source_snapshot.instance_id
                && (*seq > source_snapshot.seq
                    || (*seq == source_snapshot.seq
                        && self.snapshot().connection == VoiceConnectionState::Connected))
        }) {
            return;
        }
        *source = Some((source_snapshot.instance_id.clone(), source_snapshot.seq));
        if source_snapshot.ended_reason.is_some() {
            self.mark_active_request_stopped(source_snapshot.session_id.as_deref(), source_snapshot.call_request_id.as_deref());
        }
        self.apply_host_projection(source_snapshot);
    }

    fn disconnected(&self) {
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        let enabled = self.read_autostart_config();
        let connection = if enabled {
            VoiceConnectionState::Offline
        } else {
            VoiceConnectionState::Stopped
        };
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        if snapshot.enabled == enabled && snapshot.connection == connection {
            return;
        }
        snapshot.enabled = enabled;
        snapshot.connection = connection;
        snapshot.acoustic_ready = false;
        snapshot.dialogue_ready = false;
        snapshot.playback_ready = false;
        snapshot.audio = None;
        snapshot.phase = if enabled {
            VoiceSessionPhase::Error
        } else {
            VoiceSessionPhase::Idle
        };
        snapshot.error = enabled.then(|| VoiceSessionError {
            stage: "connection".into(),
            code: "daemon_unavailable".into(),
        });
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    fn mark_daemon_running(&self, now: tokio::time::Instant) {
        let mut budget = self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(since) = budget.running_since else {
            budget.running_since = Some(now);
            budget.next_attempt = None;
            return;
        };
        if now.duration_since(since) >= std::time::Duration::from_secs(60) {
            *budget = RestartBudget {
                running_since: Some(now),
                ..RestartBudget::default()
            };
        }
    }

    fn restart_decision(&self, now: tokio::time::Instant) -> RestartDecision {
        let mut budget = self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        budget.running_since = None;
        if let Some(at) = budget.next_attempt {
            if now < at {
                return RestartDecision::Wait(at.duration_since(now));
            }
            budget.next_attempt = None;
            return RestartDecision::StartNow;
        }
        if budget.attempts >= RestartBudget::MAX_ATTEMPTS {
            return RestartDecision::Exhausted;
        }
        let delay = RestartBudget::delay(budget.attempts);
        budget.attempts += 1;
        budget.next_attempt = Some(now + delay);
        RestartDecision::Wait(delay)
    }

    fn note_restart_exhausted(&self) -> bool {
        let mut budget = self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if budget.exhausted_reported {
            return false;
        }
        budget.exhausted_reported = true;
        true
    }

    fn reset_restart_budget(&self) {
        *self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = RestartBudget::default();
    }

    fn starting(&self) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        snapshot.enabled = self.read_autostart_config();
        snapshot.connection = VoiceConnectionState::Starting;
        snapshot.phase = VoiceSessionPhase::Idle;
        snapshot.error = None;
        snapshot.acoustic_ready = false;
        snapshot.dialogue_ready = false;
        snapshot.playback_ready = false;
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    /// Surface a missing packaged component as its own state instead of a
    /// generic daemon connection failure. Repeats are coalesced.
    fn note_missing_voice_binary(&self) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        if snapshot
            .error
            .as_ref()
            .is_some_and(|error| error.code == VOICE_COMPANION_MISSING)
        {
            return;
        }
        snapshot.enabled = self.read_autostart_config();
        snapshot.connection = VoiceConnectionState::Offline;
        snapshot.phase = VoiceSessionPhase::Error;
        snapshot.error = Some(VoiceSessionError {
            stage: "connection".into(),
            code: VOICE_COMPANION_MISSING.into(),
        });
        snapshot.acoustic_ready = false;
        snapshot.dialogue_ready = false;
        snapshot.playback_ready = false;
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    /// One upstream stream per host, independent of any renderer/page lifecycle.
    fn start_projection(self: &Arc<Self>) {
        self.install_call_delete_guard();
        if self.projection_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let manager = self.clone();
        tokio::spawn(async move {
            use futures_util::StreamExt;
            let client = reqwest::Client::builder()
                .no_proxy().redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("fixed loopback client configuration");
            loop {
                if manager.shutdown_requested.load(Ordering::Acquire) {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
                let status = manager.status().await;
                if !status.running {
                    manager.disconnected();
                    if manager.shutdown_requested.load(Ordering::Acquire)
                        || !manager.read_autostart_config()
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                    // Missing assets are not a crash loop: do not consume the
                    // restart budget while the suite still needs installation.
                    if !status.models_ready {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    // A missing installer component is also not a crash loop.
                    // Report it once with an actionable code instead of letting
                    // reconnects masquerade as a daemon restart failure.
                    if Self::resolve_voice_companion_binary().is_none() {
                        manager.note_missing_voice_binary();
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    match manager.restart_decision(tokio::time::Instant::now()) {
                        RestartDecision::Wait(delay) => {
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        RestartDecision::Exhausted => {
                            if manager.note_restart_exhausted() {
                                let mut snapshot = manager
                                    .snapshot
                                    .write()
                                    .unwrap_or_else(|err| err.into_inner());
                                snapshot.error = Some(VoiceSessionError {
                                    stage: "connection".into(),
                                    code: "daemon_restart_exhausted".into(),
                                });
                                snapshot.seq += 1;
                                let _ = manager.events.send(snapshot.clone());
                                warn!(
                                    "Voice companion restart budget exhausted; awaiting explicit retry"
                                );
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                        RestartDecision::StartNow => {
                            if let Err(error) = manager.start_child(false).await {
                                warn!(%error, "Voice companion automatic restart failed");
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            continue;
                        }
                    }
                }
                manager.mark_daemon_running(tokio::time::Instant::now());
                let generation = manager.generation.load(Ordering::Acquire);
                let response = client
                    .get(format!("{}/api/voice/events",manager.daemon_origin))
                    .header("x-ai-mate-control-token", &manager.control_token)
                    .send()
                    .await;
                if let Ok(response) = response
                    && response.status().is_success()
                {
                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    let mut recovery_tick=tokio::time::interval(std::time::Duration::from_secs(1));
                    recovery_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    let mut admission_ticks = 0u8;
                    loop {
                        let chunk = tokio::select! {
                            biased;
                            _ = manager.process_changed.notified() => break,
                            _ = recovery_tick.tick() => {
                                if manager.shutdown_requested.load(Ordering::Acquire)
                                    || generation!=manager.generation.load(Ordering::Acquire)
                                    || manager.recover_audio_if_needed().await {break;}
                                if let Err(error) = manager.reconcile_empty_placeholders().await {
                                    tracing::debug!(%error, "voice placeholder cleanup deferred");
                                }
                                admission_ticks = admission_ticks.wrapping_add(1);
                                if admission_ticks % 2 == 0 {
                                    manager.refresh_wake_admission().await;
                                }
                                continue;
                            },
                            next = stream.next() => next,
                        };
                        let Some(Ok(chunk)) = chunk else {
                            break;
                        };
                        buffer.extend_from_slice(&chunk);
                        if buffer.len() > 262_144 {
                            break;
                        }
                        while let Some(end) = buffer.windows(2).position(|bytes| bytes == b"\n\n") {
                            let frame: Vec<_> = buffer.drain(..end + 2).collect();
                            if let Ok(frame) = std::str::from_utf8(&frame) {
                                let data = frame
                                    .lines()
                                    .filter_map(|line| line.strip_prefix("data:"))
                                    .map(str::trim)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if let Ok(VoiceCompanionEvent::Snapshot { snapshot }) =
                                    serde_json::from_str(&data)
                                {
                                    manager.project_with_binding(snapshot, generation).await;
                                }
                            }
                        }
                    }
                }
                manager.disconnected();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });
    }

    pub async fn stop_session(&self, request: VoiceSessionControlRequest) -> Result<(), String> {
        let _start_lock = self.call_start_lock.lock().await;
        let snap = self.snapshot();
        if !request.matches(&snap) || request.session_id.is_none()
            || snap.ended_reason.is_some() || snap.phase == VoiceSessionPhase::Idle {
            return Err("session_scope_changed".into());
        }
        if let Some(recorder) = &self.dialogue_recorder {
            if recorder.current_call_reservation().is_some_and(|reservation| {
                Some(reservation.request_id.as_str()) != snap.call_request_id.as_deref()
            }) { return Err("session_scope_changed".into()); }
        }
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        if let Some(session_id) = &request.session_id {
            let mut revoked = self.revoked_sessions.write().unwrap_or_else(|error| error.into_inner());
            if !revoked.contains(session_id) {
                revoked.push_back(session_id.clone());
            }
            if revoked.len() > 64 {
                revoked.pop_front();
            }
        }
        let source = self
            .source
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .ok_or_else(|| "daemon_unavailable".to_string())?;
        let request = VoiceSessionControlRequest {
            instance_id: source.0,
            ..request
        };
        let response = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "daemon_unavailable".to_owned())?
            .post(format!("{}/api/voice/stop", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .json(&request)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|_| "daemon_unavailable".to_string())?;
        if response.status().is_success() {
            self.mark_active_request_stopped(request.session_id.as_deref(), snap.call_request_id.as_deref());
            self.refresh_daemon_snapshot().await?;
            Ok(())
        } else {
            Err("session_scope_changed".into())
        }
    }

    async fn reconcile_empty_placeholders(&self) -> Result<(), String> {
        let _guard = self.call_start_lock.lock().await;
        let snapshot = self.snapshot();
        if snapshot.connection != VoiceConnectionState::Connected { return Ok(()); }
        let Some(recorder) = &self.dialogue_recorder else { return Ok(()); };
        let mut protected: Vec<String> = match &self.tasks {
            Some(tasks) => tasks.queue_projection()?.into_iter().filter_map(|item| item.task.conversation_id).collect(),
            None => Vec::new(),
        };
        if let Some(reservation) = recorder.current_call_reservation() { protected.push(reservation.conversation_id); }
        if let Some(binding) = &snapshot.binding {
            if snapshot.ended_reason.is_none() { protected.push(binding.conversation_id.clone()); }
            else if snapshot.session_id.as_deref() == Some(&binding.session_id)
                && let Some(request_id) = snapshot.call_request_id.as_deref() {
                recorder.finish_call_placeholder(&binding.conversation_id, request_id).await.map_err(|e| e.to_string())?;
            }
        }
        let (view_sequence, selected) = self.viewed_conversation.read().unwrap_or_else(|e| e.into_inner()).clone();
        if view_sequence == 0 { return Ok(()); }
        recorder.cleanup_ended_placeholders(selected.as_deref(), &protected).await.map_err(|e| e.to_string())
    }

    pub async fn start_call(
        &self,
        request: nomifun_api_types::voice::VoiceCallStartRequest,
    ) -> Result<nomifun_api_types::voice::VoiceCallStartResponse, (axum::http::StatusCode, String)> {
        let _start_lock = self.call_start_lock.lock().await;
        if self.resource_write_active.load(Ordering::Acquire) {
            return Err((
                axum::http::StatusCode::CONFLICT,
                serde_json::json!({
                    "code": "voice_resource_write_in_progress",
                    "message": "Voice models are being installed or removed; wait for the write to finish"
                })
                .to_string(),
            ));
        }
        if self.self_check_active.load(Ordering::Acquire) {
            return Err((
                axum::http::StatusCode::CONFLICT,
                serde_json::json!({
                    "code": "voice_self_check_in_progress",
                    "reason_code": "self_check_in_progress",
                    "requires_check": false,
                    "message": "A readiness check is using the acoustic suite; wait for it to finish"
                })
                .to_string(),
            ));
        }

        let status = self.status().await;
        if !status.running {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                "voice_companion_not_running".to_string(),
            ));
        }

        let recorder = self.dialogue_recorder.as_ref().ok_or_else(|| {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dialogue_recorder_unavailable".to_string(),
            )
        })?;

        let snap = self.snapshot();
        let is_active = snap.session_id.is_some()
            && snap.ended_reason.is_none()
            && snap.phase != VoiceSessionPhase::Idle;

        // Check idempotency against handled_start_requests cache
        let existing_outcome = {
            self.handled_start_requests
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(&request.request_id)
                .cloned()
        };
        if let Some(existing) = existing_outcome {
            if existing.target() != &request.target || existing.session_mode() != &request.session_mode {
                return Err((
                    axum::http::StatusCode::BAD_REQUEST,
                    serde_json::json!({
                        "code": "request_target_conflict",
                        "message": "Cannot change target for the same request_id"
                    })
                    .to_string(),
                ));
            }
            match existing {
                HandledStartOutcome::Starting {
                    conversation_id,
                    memory_owner,
                    reserved_at_epoch_ms,
                    ..
                } => {
                    let now = chrono::Utc::now().timestamp_millis();
                    if snap.session_id.is_some()
                        && snap.call_request_id.as_deref() == Some(request.request_id.as_str())
                        && is_active
                        && snap.binding.as_ref().map(|b| &b.conversation_id) == Some(&conversation_id)
                    {
                        self.mark_request_bound(
                            &request.request_id,
                            snap.session_id.as_deref().unwrap_or(""),
                            &conversation_id,
                            memory_owner.as_deref(),
                        );
                        return Ok(nomifun_api_types::voice::VoiceCallStartResponse {
                            request_id: request.request_id,
                            status: if snap.acoustic_ready {
                                nomifun_api_types::voice::VoiceCallStartStatus::Ready
                            } else {
                                nomifun_api_types::voice::VoiceCallStartStatus::Active
                            },
                            conversation_id,
                            session_id: snap.session_id,
                            memory_owner_companion_id: memory_owner,
                            binding: snap.binding,
                        });
                    }
                    if now - reserved_at_epoch_ms < 10_000 {
                        return Ok(nomifun_api_types::voice::VoiceCallStartResponse {
                            request_id: request.request_id,
                            status: nomifun_api_types::voice::VoiceCallStartStatus::Starting,
                            conversation_id,
                            session_id: snap.session_id,
                            memory_owner_companion_id: memory_owner,
                            binding: snap.binding,
                        });
                    }
                    return Err((axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        serde_json::json!({"code":"voice_call_start_unknown", "conversation_id":conversation_id}).to_string()));
                }
                HandledStartOutcome::Bound {
                    conversation_id,
                    session_id,
                    memory_owner,
                    ..
                } => {
                    if !is_active || snap.session_id != session_id || snap.call_request_id.as_deref() != Some(request.request_id.as_str()) {
                        return Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, serde_json::json!({"code":"voice_call_start_unknown"}).to_string()));
                    }
                    return Ok(nomifun_api_types::voice::VoiceCallStartResponse {
                        request_id: request.request_id,
                        status: if snap.acoustic_ready
                            && snap.binding.as_ref().map(|b| &b.conversation_id) == Some(&conversation_id)
                        {
                            nomifun_api_types::voice::VoiceCallStartStatus::Ready
                        } else {
                            nomifun_api_types::voice::VoiceCallStartStatus::Active
                        },
                        conversation_id,
                        session_id: snap.session_id.or(session_id),
                        memory_owner_companion_id: memory_owner,
                        binding: snap.binding,
                    });
                }
                HandledStartOutcome::Stopped { conversation_id, .. } => {
                    return Err((
                        axum::http::StatusCode::CONFLICT,
                        serde_json::json!({
                            "code": "voice_call_already_stopped",
                            "active_conversation_id": Some(conversation_id),
                            "message": "This request has already concluded and stopped"
                        })
                        .to_string(),
                    ));
                }
                HandledStartOutcome::Rejected {
                    status_code_u16,
                    error_json,
                    ..
                } => {
                    let status = axum::http::StatusCode::from_u16(status_code_u16)
                        .unwrap_or(axum::http::StatusCode::BAD_REQUEST);
                    return Err((status, error_json));
                }
            }
        }

        // Check if an existing reservation from another request is pending
        if let Some(res) = recorder.current_call_reservation() {
            let elapsed_ms = chrono::Utc::now().timestamp_millis() - res.reserved_at_epoch_ms;
            if elapsed_ms < 10_000 {
                if res.request_id != request.request_id && !is_active {
                    return Err((
                        axum::http::StatusCode::CONFLICT,
                        serde_json::json!({
                            "code": "voice_call_starting",
                            "active_conversation_id": Some(res.conversation_id),
                            "message": "A voice call is currently starting"
                        })
                        .to_string(),
                    ));
                }
            } else {
                recorder.take_call_reservation(&res.request_id);
            }
        }

        // Resolving a target may create a Conversation; arbitrate active calls first.
        if is_active {
            let active_id = snap.binding.as_ref().map(|binding| binding.conversation_id.as_str());
            let same_target = match &request.target {
                VoiceCallTarget::VoiceConversation { conversation_id } => Some(conversation_id.as_str()) == active_id,
                VoiceCallTarget::Workspace { workspace } => snap.binding.as_ref().and_then(|binding| binding.workspace.as_deref())
                    .is_some_and(|active| nomifun_common::paths::paths_equivalent(Path::new(workspace), Path::new(active))),
                VoiceCallTarget::CurrentSelection => active_id.is_some(),
                VoiceCallTarget::DefaultWorkspace => false,
                VoiceCallTarget::NewVoiceConversation { .. } => false,
            };
            if same_target && active_id.is_some() {
                return Ok(VoiceCallStartResponse {
                    request_id: request.request_id,
                    status: VoiceCallStartStatus::Active,
                    conversation_id: active_id.unwrap().to_owned(),
                    session_id: snap.session_id,
                    memory_owner_companion_id: snap.binding.as_ref().and_then(|b| b.memory_owner_companion_id.clone()),
                    binding: snap.binding,
                });
            }
            return Err((axum::http::StatusCode::CONFLICT, serde_json::json!({
                "code":"voice_call_conflict", "active_conversation_id":active_id,
            }).to_string()));
        }

        // A connected daemon authorizes a new full call only with a current,
        // attributed self-check report. Missing/expired/old reports reject
        // before any Conversation placeholder is created.
        if snap.enabled {
            let capabilities = self.current_capabilities().await;
            let can_start = capabilities
                .as_ref()
                .is_some_and(|report| report.can_start_full_call);
            if !can_start {
                let reason_code = capabilities
                    .as_ref()
                    .and_then(|report| report.reason_code.clone())
                    .unwrap_or_else(|| "self_check_required".into());
                let degraded = capabilities
                    .as_ref()
                    .is_some_and(|report| report.degraded_text_only);
                return Err((
                    axum::http::StatusCode::CONFLICT,
                    serde_json::json!({
                        "code": "voice_suite_not_ready",
                        "reason_code": reason_code,
                        "requires_check": capabilities.is_none(),
                        "degraded_text_only": degraded,
                        "capabilities": capabilities,
                        "message": "The voice suite has not passed its current local capability checks for a full call"
                    })
                    .to_string(),
                ));
            }
        }

        let (conversation_id, memory_owner) = recorder
            .resolve_call_target_with_locale(&request.target, request.session_mode.as_deref(), Some(&request.request_id), request.locale.as_deref())
            .await
            .map_err(|e| match e {
                VoiceDialogueRecorderError::WriteFailed(msg) if msg.contains("workspace_directory_invalid") => {
                    (axum::http::StatusCode::BAD_REQUEST, "workspace_directory_invalid".to_string())
                }
                VoiceDialogueRecorderError::BindingInvalid(msg) if matches!(msg.as_str(), "conversation_already_discarded" | "voice_call_already_stopped" | "voice_start_request_conflict") => {
                    (axum::http::StatusCode::CONFLICT, serde_json::json!({
                        "code": msg,
                    }).to_string())
                }
                VoiceDialogueRecorderError::BindingInvalid(msg) => {
                    (axum::http::StatusCode::BAD_REQUEST, msg)
                }
                _ => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "target_resolution_failed".to_string()),
            })?;

        let now_epoch_ms = chrono::Utc::now().timestamp_millis();
        let reservation = crate::voice_dialogue::VoiceCallReservation {
            request_id: request.request_id.clone(),
            target: request.target.clone(),
            conversation_id: conversation_id.clone(),
            memory_owner: memory_owner.clone(),
            reserved_at_epoch_ms: now_epoch_ms,
        };
        recorder.set_call_reservation(Some(reservation));
        self.set_candidate_conversation(Some(conversation_id.clone()));
        self.record_handled_start_request(
            request.request_id.clone(),
            HandledStartOutcome::Starting {
                target: request.target.clone(),
                session_mode: request.session_mode.clone(),
                conversation_id: conversation_id.clone(),
                memory_owner: memory_owner.clone(),
                reserved_at_epoch_ms: now_epoch_ms,
            },
        );

        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                recorder.take_call_reservation(&request.request_id);
                (axum::http::StatusCode::SERVICE_UNAVAILABLE, e.to_string())
            })?;

        let wake_result = client
            .post(format!("{}/api/voice/wake", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .timeout(std::time::Duration::from_secs(3))
            .json(&{
                let mut body = serde_json::json!({ "request_id": request.request_id });
                if let Some(locale) = &request.locale {
                    body["locale"] = serde_json::Value::String(locale.clone());
                }
                body
            })
            .send()
            .await;

        let resp = match wake_result {
            Ok(r) => r,
            Err(e) => {
                // Unknown transport result: wake may have been delivered to daemon before network drop.
                // Do NOT immediately discard reservation; keep it alive for late binding / idempotent retry.
                return Err((
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    serde_json::json!({
                        "code": "daemon_wake_timeout",
                        "conversation_id": Some(&conversation_id),
                        "message": format!("Failed to receive wake confirmation from voice daemon: {e}. Reservation preserved for retry.")
                    })
                    .to_string(),
                ));
            }
        };

        let resp_status = resp.status();
        if !resp_status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            if !wake_rejection_precedes_session(resp_status, &body_text) {
                return Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, serde_json::json!({
                    "code": "daemon_wake_unknown",
                    "conversation_id": conversation_id,
                    "message": "Wake outcome is unknown; reservation preserved for retry"
                }).to_string()));
            }
            // Only the daemon's explicit pre-wake failures prove there is no session.
            recorder.take_call_reservation(&request.request_id);
            let error_json = serde_json::json!({
                "code": "daemon_wake_rejected",
                "status": resp_status.as_u16(),
                "request_id": request.request_id,
                "terminal": true,
                "message": format!("Daemon rejected wake request: {body_text}"),
            }).to_string();
            self.record_handled_start_request(
                request.request_id.clone(),
                HandledStartOutcome::Rejected {
                    target: request.target.clone(),
                    session_mode: request.session_mode.clone(),
                    status_code_u16: resp_status.as_u16(),
                    error_json: error_json.clone(),
                    at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                },
            );
            return Err((resp_status, error_json));
        }

        Ok(nomifun_api_types::voice::VoiceCallStartResponse {
            request_id: request.request_id,
            status: nomifun_api_types::voice::VoiceCallStartStatus::Starting,
            conversation_id,
            session_id: None,
            memory_owner_companion_id: memory_owner,
            binding: None,
        })
    }

    /// Resolves the compiled `voice-companion` executable path on disk (with fallback to `ai-mate`).
    pub fn resolve_voice_companion_binary() -> Option<PathBuf> {
        let exe_names = if cfg!(windows) {
            ["voice-companion.exe", "ai-mate.exe"]
        } else {
            ["voice-companion", "ai-mate"]
        };

        // 1. Explicit environment variable override
        if let Some(env_path) = std::env::var_os("VOICE_COMPANION_BIN")
            .or_else(|| std::env::var_os("AI_MATE_BIN"))
            .map(PathBuf::from)
        {
            if env_path.is_file() {
                return Some(env_path);
            }
        }

        // 2. Tauri's resource bin is authoritative across Windows/macOS/Linux.
        let bundled_bin=std::env::var_os("NOMIFUN_BUNDLED_BIN_DIR").map(PathBuf::from);
        let current_exe=std::env::current_exe().ok();
        if let Some(binary)=Self::installed_voice_companion_binary(bundled_bin.as_deref(),current_exe.as_deref()) {return Some(binary);}

        // 3. Known development build target directories (prefer newest binary)
        for exe_name in exe_names {
            let mut candidates = vec![
                Path::new("target/debug").join(exe_name),
                Path::new("target/release").join(exe_name),
                Path::new("build.noindex/debug").join(exe_name),
                Path::new("build.noindex/release").join(exe_name),
            ];

            // 4. Resolve relative to workspace root if cwd is a sub-crate
            if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                if let Some(r) = Path::new(&manifest_dir).ancestors().nth(2) {
                    for dir in [
                        "target/debug",
                        "target/release",
                        "build.noindex/debug",
                        "build.noindex/release",
                    ] {
                        candidates.push(r.join(dir).join(exe_name));
                    }
                }
            }

            let mut existing: Vec<PathBuf> = candidates.into_iter().filter(|c| c.is_file()).collect();
            existing.sort_by_key(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });
            if let Some(newest) = existing.pop() {
                let resolved = std::fs::canonicalize(&newest).unwrap_or(newest);
                return Some(resolved);
            }
        }

        None
    }

    /// Backwards-compatible alias for resolving the companion binary.
    #[inline]
    pub fn resolve_ai_mate_binary() -> Option<PathBuf> {
        Self::resolve_voice_companion_binary()
    }

    fn installed_voice_companion_binary(bundled_bin:Option<&Path>,current_exe:Option<&Path>)->Option<PathBuf> {
        let names = if cfg!(windows) {
            ["voice-companion.exe", "ai-mate.exe"]
        } else {
            ["voice-companion", "ai-mate"]
        };
        for name in names {
            if let Some(candidate) = bundled_bin.into_iter().chain(current_exe.and_then(Path::parent))
                .map(|directory| directory.join(name)).find(|candidate| candidate.is_file()) {
                return Some(candidate);
            }
        }
        None
    }

    #[inline]
    #[cfg(test)]
    fn installed_ai_mate_binary(bundled_bin:Option<&Path>,current_exe:Option<&Path>)->Option<PathBuf> {
        Self::installed_voice_companion_binary(bundled_bin, current_exe)
    }

    fn read_autostart_config(&self) -> bool {
        if self.config_path.is_file() {
            if let Ok(raw) = std::fs::read_to_string(&self.config_path) {
                if let Ok(cfg) = serde_json::from_str::<VoiceCompanionPersistedConfig>(&raw) {
                    return cfg.autostart;
                }
            }
        }
        false
    }

    fn write_autostart_config(&self, autostart: bool) -> Result<(), String> {
        self.write_config_entry("autostart", autostart.into())
    }

    fn write_config_entry(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
        let _guard = self
            .config_write_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut cfg = if self.config_path.exists() {
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(&self.config_path)
                    .map_err(|_| "configuration_read_failed".to_string())?,
            )
            .map_err(|_| "configuration_invalid".to_string())?
        } else {
            serde_json::json!({})
        };
        let object = cfg
            .as_object_mut()
            .ok_or_else(|| "configuration_invalid".to_string())?;
        object.insert(key.into(), value);
        let bytes =
            serde_json::to_vec_pretty(&cfg).map_err(|_| "configuration_invalid".to_string())?;
        nomifun_common::dir_config::write_atomic_replace(&self.config_path, &bytes)
            .map_err(|_| "configuration_write_failed".to_string())
    }

    /// Queries the real running status of the background companion daemon.
    #[cfg(test)]
    pub(crate) fn last_child_exit(&self) -> Option<ChildExitObservation> {
        self.last_child_exit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub async fn status(&self) -> VoiceCompanionStatus {
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        let autostart = self.read_autostart_config();

        let mut lock = self.child.lock().await;
        let mut running = false;
        let mut pid = None;

        if let Some(ref mut child) = *lock {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    #[cfg(test)]
                    {
                        *self.last_child_exit.lock().unwrap_or_else(|error| error.into_inner()) = Some(ChildExitObservation {
                            pid: self.test_child_pid.load(Ordering::Acquire),
                            code: exit_status.code(),
                            observed_at_ms: nomifun_common::now_ms(),
                        });
                    }
                    info!(
                        exit_code = exit_status.code(),
                        ?exit_status,
                        "Voice companion child process has terminated"
                    );
                    *lock = None;
                }
                Ok(None) => {
                    running = true;
                    pid = child.id();
                }
                Err(err) => {
                    warn!(error = %err, "Failed to check status of voice companion process");
                    *lock = None;
                }
            }
        }

        drop(lock);
        let capabilities = self.current_capabilities().await;
        VoiceCompanionStatus {
            running,
            pid,
            models_ready,
            autostart,
            error: self.snapshot().error.map(|error| error.code),
            acoustic_ready: running && self.snapshot().acoustic_ready,
            dialogue_ready: running && self.snapshot().dialogue_ready,
            playback_ready: running && self.snapshot().playback_ready,
            task_ready: self.snapshot().task_ready,
            capabilities,
        }
    }

    /// Launches the `ai-mate` companion daemon under whole-tree supervision.
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        self.shutdown_requested.store(false, Ordering::Release);
        self.reset_restart_budget();
        self.start_child(true).await
    }

    async fn start_child(self:&Arc<Self>, explicit:bool)->Result<(),String> {
        let mut lock = self.child.lock().await;
        if !explicit && (self.shutdown_requested.load(Ordering::Acquire) || !self.read_autostart_config()) {
            return Err("voice_stopped".into());
        }

        // Check if already active
        if let Some(ref mut child) = *lock {
            if let Ok(None) = child.try_wait() {
                info!(
                    pid = child.id(),
                    "Voice companion process is already running"
                );
                return Ok(());
            }
        }

        // Verify models are Ready before attempting launch
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        if !models_ready {
            return Err("Voice acoustic models are not installed yet".to_string());
        }

        let bin_path = match Self::resolve_voice_companion_binary() {
            Some(path) => path,
            None => {
                warn!(
                    bundled_bin = ?std::env::var_os("NOMIFUN_BUNDLED_BIN_DIR"),
                    current_exe = ?std::env::current_exe().ok(),
                    "voice_companion_binary_missing; the installation is missing the voice companion component"
                );
                return Err(VOICE_COMPANION_MISSING.into());
            }
        };

        let feedback_config = serde_json::to_string(&self.requested_feedback()?)
            .map_err(|_| "configuration_invalid")?;
        info!(binary = %bin_path.display(), "Spawning Voice Companion daemon...");
        self.starting();

        let mut builder = ChildProcessBuilder::new(&bin_path);
        builder.arg("--bypass-pairing");
        #[cfg(test)]
        let host = self.test_daemon_port.map(|port| format!("127.0.0.1:{port}")).unwrap_or_else(|| "127.0.0.1:18080".into());
        #[cfg(not(test))]
        let host = "127.0.0.1:18080".to_string();
        builder.arg("--host").arg(host);
        builder.env("VOICE_COMPANION_MODEL_DIR", self.models_manager.models_dir());
        builder.env("AI_MATE_MODEL_DIR", self.models_manager.models_dir());
        builder.env("VOICE_COMPANION_SUPERVISED", "1");
        #[cfg(test)]
        for key in ["NOMIFUN_VOICE_FAULT_BLOCK_ASR", "NOMIFUN_VOICE_FAULT_HEADLESS", "VOICE_COMPANION_NATIVE_STUCK_GRACE_MS", "NOMIFUN_VOICE_NATIVE_LEASE_GRACE_MS", "NOMIFUN_VOICE_SELF_CHECK_AUDIO_DIR"] {
            if let Some(value) = std::env::var_os(key) { builder.env(key, value); }
        }
        builder.env("VOICE_COMPANION_CONTROL_TOKEN", &self.control_token);
        builder.env("AI_MATE_CONTROL_TOKEN", &self.control_token);
        builder.env("VOICE_COMPANION_FEEDBACK_CONFIG", &feedback_config);
        builder.env("AI_MATE_FEEDBACK_CONFIG", feedback_config);
        if self.dialogue_completer.is_some() {
            let mut bridge = self.dialogue_bridge.lock().await;
            if bridge.is_none() {
                *bridge = Some(
                    self.start_dialogue_bridge()
                    .await?,
                );
            }
            builder.env(
                "AI_MATE_DIALOGUE_ENDPOINT",
                &bridge.as_ref().unwrap().endpoint,
            );
            builder.env("AI_MATE_TASK_ENDPOINT", bridge.as_ref().unwrap().endpoint.replace("/complete", "/tasks"));
        }
        // Daemon tracing writes stdout. Keep it in the actual host log root
        // rather than discarding the diagnostics needed for microphone/ASR triage.
        let log_root = std::env::var_os("NOMIFUN_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.config_path
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(Path::new("."))
                    .join("logs")
            });
        let log_dir = log_root.join("voice");
        std::fs::create_dir_all(&log_dir)
            .map_err(|err| format!("Cannot create voice log directory: {err}"))?;
        let log_path = log_dir.join(format!("ai-mate-{}.log", nomifun_common::now_ms()));
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|err| format!("Cannot open voice log: {err}"))?;
        let stderr = log
            .try_clone()
            .map_err(|err| format!("Cannot clone voice log handle: {err}"))?;
        info!(path = %log_path.display(), "Voice companion diagnostics log");
        builder.stdout(std::process::Stdio::from(log));
        builder.stderr(std::process::Stdio::from(stderr));

        if self.shutdown_requested.load(Ordering::Acquire) || (!explicit && !self.read_autostart_config()) {
            return Err("voice_stopped".into());
        }
        let managed = builder
            .spawn_managed()
            .map_err(|e| format!("Failed to spawn ai-mate process: {e}"))?;

        let pid = managed.id();
        #[cfg(test)]
        self.test_child_pid.store(pid.unwrap_or(0), Ordering::Release);
        info!(?pid, "AI Mate voice companion daemon successfully started");

        *lock = Some(managed);
        drop(lock);

        self.generation.fetch_add(1, Ordering::AcqRel);
        *self.source.write().unwrap_or_else(|err| err.into_inner()) = None;
        self.process_changed.notify_one();
        Ok(())
    }

    async fn recover_audio_if_needed(self:&Arc<Self>)->bool {
        if self.shutdown_requested.load(Ordering::Acquire) {return false;}
        let generation=self.generation.load(Ordering::Acquire);
        let snapshot=self.snapshot();
        if !self.audio_recovery.lock().unwrap_or_else(|error|error.into_inner()).due(&snapshot,nomifun_common::now_ms()) {return false;}
        let mut child=self.child.lock().await;
        if generation!=self.generation.load(Ordering::Acquire) || self.shutdown_requested.load(Ordering::Acquire)
            || !self.read_autostart_config() {return false;}
        warn!(capture_state=?snapshot.audio.as_ref().map(|audio|&audio.capture_state),playback_ready=snapshot.playback_ready,"voice_audio_device_recovery");
        self.generation.fetch_add(1,Ordering::AcqRel);
        *self.source.write().unwrap_or_else(|error|error.into_inner())=None;
        if let Some(mut process)=child.take() {
            if let Err(error)=process.shutdown().await {warn!(%error,"voice_audio_recovery_shutdown_failed");}
        }
        drop(child);
        self.disconnected();
        if let Err(error)=self.start_child(false).await {warn!(%error,"voice_audio_recovery_start_failed");}
        true
    }

    /// Gracefully shuts down and reaps the companion child process tree.
    pub async fn stop(&self) -> Result<(), String> {
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        self.shutdown_requested.store(true, Ordering::Release);
        self.reset_restart_budget();
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.process_changed.notify_one();

        let mut lock = self.child.lock().await;
        if let Some(mut child) = lock.take() {
            info!(
                pid = child.id(),
                "Stopping AI Mate voice companion daemon..."
            );
            if let Err(e) = child.shutdown().await {
                warn!(error = %e, "Error shutting down ai-mate process tree");
            } else {
                info!("AI Mate voice companion daemon stopped cleanly");
            }
        }
        self.disconnected();
        Ok(())
    }

    /// Toggles the companion daemon on or off.
    pub async fn toggle(self: &Arc<Self>, enabled: bool) -> Result<bool, String> {
        // Serialize the user's enable/disable choice with prepare's
        // check-start-recheck cycle. A prepare can never stop a newer enable.
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.write_autostart_config(enabled)?;
        if enabled {
            self.start().await?;
            Ok(true)
        } else {
            self.stop().await?;
            Ok(false)
        }
    }

    /// Automatically starts the companion daemon on Flowy startup if enabled and models are Ready.
    pub async fn auto_start_if_enabled(self: &Arc<Self>) {
        if !self.shutdown_requested.load(Ordering::Acquire) && self.read_autostart_config() {
            let models_ready = self.models_manager.get_status().await.state.is_ready();
            if models_ready {
                info!("Auto-starting AI Mate voice companion on application boot...");
                if let Err(err) = self.start_child(false).await {
                    warn!(error = %err, "Failed to auto-start AI Mate companion on boot");
                }
            } else {
                info!(
                    "Voice companion autostart is enabled, but acoustic models are not yet ready; standing by"
                );
            }
        }
    }
}


async fn private_task_submit(
    axum::extract::State(weak): axum::extract::State<std::sync::Weak<VoiceCompanionProcessManager>>,
    headers: axum::http::HeaderMap,
    axum::Json(mut request): axum::Json<VoiceTaskSubmitRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(manager) = weak.upgrade() else { return task_api_error("runtime_unavailable".into()); };
    if headers.get("x-ai-mate-control-token").and_then(|value| value.to_str().ok()) != Some(manager.control_token.as_str()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    // Read the current daemon state before admitting a command. SSE delivery may
    // lag behind this authenticated request; no delayed event replay is needed.
    let scope=VoiceSessionControlRequest {instance_id:request.instance_id.clone(),session_id:Some(request.command.session_id.clone()),turn_id:Some(request.command.turn_id.clone())};
    if let Err(code)=manager.private_voice_scope(&scope).await {return task_api_error(code);}
    request.instance_id = manager.snapshot().instance_id;
    match manager.submit_task(request).await {
        Ok(delivery) => axum::Json(delivery).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn private_current_task(
    axum::extract::State(weak):axum::extract::State<std::sync::Weak<VoiceCompanionProcessManager>>,
    headers:axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(manager)=weak.upgrade() else {return task_api_error("runtime_unavailable".into());};
    if headers.get("x-ai-mate-control-token").and_then(|value|value.to_str().ok())!=Some(manager.control_token.as_str()) {return axum::http::StatusCode::UNAUTHORIZED.into_response();}
    match manager.tasks.as_ref().map(|tasks|tasks.current_task()).transpose() {
        Ok(task) => {
            let task=task.flatten().map(|mut task|{task.result_text=None;task});
            axum::Json(task).into_response()
        },
        Err(code) => task_api_error(code),
    }
}

async fn private_task_control(
    axum::extract::State(weak):axum::extract::State<std::sync::Weak<VoiceCompanionProcessManager>>,
    headers:axum::http::HeaderMap,
    axum::Json(request):axum::Json<VoiceTaskControlCommand>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(manager)=weak.upgrade() else {return task_api_error("runtime_unavailable".into());};
    if headers.get("x-ai-mate-control-token").and_then(|value|value.to_str().ok())!=Some(manager.control_token.as_str()) {return axum::http::StatusCode::UNAUTHORIZED.into_response();}
    if let Err(code)=manager.private_voice_scope(&request.scope).await {return task_api_error(code);}
    let raw_user_text = request.raw_user_text.as_deref().map(str::trim).filter(|value| !value.is_empty());
    // An older daemon may query without the new provenance field. Reads remain
    // compatible; cancellation still requires the explicit affirmative text.
    if request.action == VoiceTaskControlAction::CancelTask && raw_user_text.is_none() {
        return task_api_error("task_control_scope_required".into());
    }
    let Some(recorder)=manager.dialogue_recorder.clone() else {
        return task_api_error("binding_invalid".into());
    };
    let conversation=match recorder.task_scope_conversation(&request.scope).await {
        Ok(Some(conversation))=>conversation,
        Ok(None)=>return task_api_error("binding_invalid".into()),
        Err(error)=>return task_api_error(crate::voice_dialogue::record_error_code(&error).into()),
    };
    if request.task.conversation_id.as_deref()!=Some(conversation.as_str()) {
        return task_api_error("task_scope_changed".into());
    }
    if request.action==VoiceTaskControlAction::CancelTask
        && !nomifun_api_types::voice::is_affirmative_cancel_command(raw_user_text.unwrap_or_default()) {
        return task_api_error("task_control_ambiguous".into());
    }
    let request_scope=VoiceTaskControlRequest {instance_id:manager.snapshot().instance_id,task:request.task.clone()};
    let result=manager.control_task(request_scope,request.action).await;
    let persistence=match raw_user_text {
        Some(raw)=>recorder.record_task_control_result(
            &request.scope,
            &request.task,
            Some(raw),
            request.locale.as_deref(),
            request.action,
            &result,
        ).await,
        None=>{
            // Compatibility read-only query: no provenance means the user
            // question cannot be projected, but the task state is still valid.
            tracing::warn!("voice_task_control_missing_raw_text");
            Ok(())
        }
    };
    match result {
        Ok(task) => match persistence {
            Ok(()) => axum::Json(task).into_response(),
            Err(error) => task_api_error(crate::voice_dialogue::record_error_code(&error).into()),
        },
        Err(code) => {
            if let Err(error)=persistence {
                tracing::warn!(code=crate::voice_dialogue::record_error_code(&error),"voice_task_control_result_not_persisted");
            }
            task_api_error(code)
        }
    }
}

/// Formats spoken/display task status response text across languages and terminal outcomes.
fn format_task_status_text(task: &nomifun_api_types::voice::VoiceTaskSnapshot, is_en: bool, is_cancelled: bool) -> String {
    use nomifun_api_types::voice::VoiceTaskStatus;
    if is_cancelled && task.status == VoiceTaskStatus::Failed {
        if is_en { "Task cancelled." } else { "任务已取消。" }.to_string()
    } else if is_en {
        match task.status {
            VoiceTaskStatus::Queued => "The task is queued, waiting for previous task to finish.".into(),
            VoiceTaskStatus::Delivering => "Submitting the task, receipt verification pending.".into(),
            VoiceTaskStatus::Unknown => "Task status not verified yet, checking receipt.".into(),
            VoiceTaskStatus::Accepted => "Task accepted, waiting for execution results.".into(),
            VoiceTaskStatus::Cancelling => "Cancelling the task, awaiting execution confirmation.".into(),
            VoiceTaskStatus::Completed => format!(
                "Execution finished. {}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("Please check task details.").chars().take(220).collect::<String>()
            ),
            VoiceTaskStatus::Failed => format!(
                "Task ended, execution was unsuccessful. {}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("Please check task details for reasons.").chars().take(220).collect::<String>()
            ),
        }
    } else {
        match task.status {
            VoiceTaskStatus::Queued => "任务已排队，等待上一任务结束。".into(),
            VoiceTaskStatus::Delivering => "任务正在提交，尚未核实接单。".into(),
            VoiceTaskStatus::Unknown => "任务状态尚未核实，正在核对原回执。".into(),
            VoiceTaskStatus::Accepted => "任务已接受，正在等待执行结果。".into(),
            VoiceTaskStatus::Cancelling => "正在取消任务，等待执行侧确认。".into(),
            VoiceTaskStatus::Completed => format!(
                "执行已结束。{}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("请查看任务详情。").chars().take(220).collect::<String>()
            ),
            VoiceTaskStatus::Failed => format!(
                "任务已结束，执行结果未成功。{}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("请查看任务详情了解原因。").chars().take(220).collect::<String>()
            ),
        }
    }
}

fn format_task_result_announcement(task: &VoiceTaskSnapshot, goal: &str, is_en: bool) -> String {
    let summary = extract_spoken_summary(task.result_text.as_deref().unwrap_or(""), task.status, is_en);
    let spoken_goal: String = goal.chars().take(25).collect();
    if spoken_goal.trim().is_empty() { summary }
    else if is_en { format!("Result for ‘{spoken_goal}’: {summary}") }
    else { format!("任务「{spoken_goal}」的结果：{summary}") }
}

/// Extracts a concise, high-signal spoken summary (50-80 chars) suitable for TTS playback.
///
/// Preserves:
/// - Explicit conclusion sentences (e.g. "已完成", "成功", "未找到", "失败");
/// - Vital parameters (temperatures, numbers, dates, locations, units);
/// - Negations ("不", "未", "无", "失败", "没有", "无法") - never drops or inverts polarity;
/// - Strips markdown noise, code blocks, and stage directions.
pub fn extract_spoken_summary(raw: &str, status: VoiceTaskStatus, is_en: bool) -> String {
    let text = strip_markdown_and_noise(raw);
    let text = text.trim();
    if text.is_empty() {
        return match (status, is_en) {
            (VoiceTaskStatus::Completed, true) => "Execution finished. See conversation for details.".into(),
            (VoiceTaskStatus::Completed, false) => "执行已结束，详情请在会话中查看。".into(),
            (_, true) => "Execution was unsuccessful. See conversation for details.".into(),
            (_, false) => "任务未完成，详情请在会话中查看。".into(),
        };
    }
    let details = if is_en { ". See conversation for details." } else { "，详情请在会话中查看。" };
    let restrictions = if is_en { "Do not act on a shortened result. Review the full details and restrictions." }
        else { "任务执行结果包含未完成事项与重要说明，详情请在会话中查看。" };
    // If it's already concise enough (under 80 chars), speak as is.
    if text.chars().count() <= 80 {
        return text.to_string();
    }

    let has_failure = is_negation_or_failure(text) || status == VoiceTaskStatus::Failed;
    const MAX_TARGET_CHARS: usize = 65;

    let sentences = split_sentences(text);
    // English restrictions need their whole sentence: a success prefix is not
    // a safe substitute when the failure/condition cannot fit the speech budget.
    if has_english_failure(text) {
        let critical = sentences
            .iter()
            .copied()
            .filter(|s| is_negation_or_failure(s))
            .collect::<Vec<_>>()
            .join(". ");
        return if critical.is_empty() || critical.chars().count() > 60 {
            restrictions.into()
        } else {
            format!("{critical}{details}")
        };
    }
    if sentences.is_empty() {
        let truncated: String = text.chars().take(MAX_TARGET_CHARS).collect();
        let res = format!("{truncated}{details}");
        if has_failure && !is_negation_or_failure(&res) {
            return restrictions.to_string();
        }
        return res;
    }

    // Score sentences:
    // Conclusive words, parameters, negation words
    let mut scored: Vec<(usize, i32, &str)> = Vec::new();
    for (idx, &s) in sentences.iter().enumerate() {
        let mut score = 0;
        // Prioritize early sentences
        if idx == 0 {
            score += 15;
        } else if idx == 1 {
            score += 8;
        }
        // Conclusive keywords
        if contains_any(s, &["已完成", "成功", "完成", "查询到", "查到", "结果为", "总计", "共计", "天气为", "温度为", "状态为"]) {
            score += 30;
        } else if contains_any(s, &["天气", "温度", "状态", "结果"]) {
            score += 15;
        }
        // Negation keywords - critical! Never drop negation
        if is_negation_or_failure(s) {
            score += 60;
        }
        // Parameter signals: digits and units
        if s.chars().any(|c| c.is_ascii_digit()) {
            score += 15;
        }
        if contains_any(s, &["℃", "度", "元", "个", "条", "次", "点", "分", "%", "米", "日", "号"]) {
            score += 15;
        }
        scored.push((idx, score, s));
    }

    // Sort by score descending to find best candidates
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let mut selected_indices = Vec::new();
    let mut accumulated_chars = 0;

    for (idx, _, s) in &scored {
        let s_chars = s.chars().count();
        if accumulated_chars + s_chars <= MAX_TARGET_CHARS {
            selected_indices.push(*idx);
            accumulated_chars += s_chars;
        } else if selected_indices.is_empty() {
            // Even the single best sentence is longer than MAX_TARGET_CHARS.
            let clauses = split_clauses(s);
            let mut scored_clauses: Vec<(usize, i32, &str)> = Vec::new();
            for (c_idx, &c) in clauses.iter().enumerate() {
                let mut c_score = 0;
                if c_idx == 0 {
                    c_score += 15;
                }
                if is_negation_or_failure(c) {
                    c_score += 60;
                }
                if contains_any(c, &["已完成", "成功", "完成", "查询到", "查到", "结果为", "但是", "但"]) {
                    c_score += 30;
                }
                if c.chars().any(|ch| ch.is_ascii_digit()) {
                    c_score += 15;
                }
                scored_clauses.push((c_idx, c_score, c));
            }
            scored_clauses.sort_by(|a, b| b.1.cmp(&a.1));

            let mut selected_clause_indices = Vec::new();
            let mut acc_clause_chars = 0;
            for (c_idx, _, c) in &scored_clauses {
                let c_len = c.chars().count();
                if acc_clause_chars + c_len <= MAX_TARGET_CHARS {
                    selected_clause_indices.push(*c_idx);
                    acc_clause_chars += c_len;
                }
            }

            if selected_clause_indices.is_empty() {
                let target_clause = if has_failure {
                    clauses.iter().find(|&&c| is_negation_or_failure(c)).unwrap_or(&clauses[0])
                } else {
                    &clauses[0]
                };
                let first_part: String = target_clause.chars().take(MAX_TARGET_CHARS).collect();
                let trimmed = first_part.trim_end_matches(['，', ',']);
                let res = format!("{trimmed}{details}");
                if has_failure && !is_negation_or_failure(&res) {
                    return restrictions.to_string();
                }
                return res;
            }

            selected_clause_indices.sort_unstable(); // keep chronological order
            let mut clause_summary = String::new();
            for &c_idx in &selected_clause_indices {
                let c = clauses[c_idx].trim();
                if !c.is_empty() {
                    if !clause_summary.is_empty() && !clause_summary.ends_with(['，', ',']) {
                        clause_summary.push_str(if is_en { ", " } else { "，" });
                    }
                    clause_summary.push_str(c);
                }
            }
            let trimmed = clause_summary.trim_end_matches(['，', ',']);
            let res = format!("{trimmed}{details}");
            if has_failure && !is_negation_or_failure(&res) {
                return restrictions.to_string();
            }
            return res;
        }
    }

    selected_indices.sort_unstable(); // keep chronological order
    let mut summary = String::new();
    for &idx in &selected_indices {
        let s = sentences[idx].trim();
        if !s.is_empty() {
            if !summary.is_empty() && !summary.ends_with(['。', '！', '!', '？', '?', '；', ';', '，', ',']) {
                summary.push_str(if is_en { ", " } else { "，" });
            }
            summary.push_str(s);
        }
    }

    let summary = summary.trim_end_matches(['。', '！', '!', '？', '?', '；', ';', '，', ',']);
    let result = if summary.chars().count() < text.chars().count() {
        format!("{summary}{details}")
    } else {
        summary.to_string()
    };
    if has_failure && !is_negation_or_failure(&result) {
        return restrictions.to_string();
    }
    result
}

fn is_negation_or_failure(text: &str) -> bool {
    contains_any(
        text,
        &[
            "未找到", "失败", "无法", "未完成", "未成功", "未能", "没有", "不存在",
            "无数据", "错误", "未查到", "不成功", "不能", "未复制", "仍然为空", "异常",
        ],
    ) || has_english_failure(text)
        || (text.contains('未') && !text.contains("未来"))
        || (text.contains('无') && !text.contains("无论"))
        || (text.contains('不') && !text.contains("不但") && !text.contains("不仅"))
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}

fn has_english_failure(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '\'' && c != '’')
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "no" | "not"
                    | "never"
                    | "cannot"
                    | "can't"
                    | "can’t"
                    | "couldn't"
                    | "couldn’t"
                    | "don't"
                    | "don’t"
                    | "doesn't"
                    | "doesn’t"
                    | "didn't"
                    | "didn’t"
                    | "wasn't"
                    | "wasn’t"
                    | "weren't"
                    | "weren’t"
                    | "won't"
                    | "won’t"
                    | "failed"
                    | "failure"
                    | "failures"
                    | "unable"
                    | "unsuccessful"
                    | "denied"
                    | "error"
                    | "errors"
            )
        })
}

fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let after = index + ch.len_utf8();
        let boundary = matches!(ch, '。' | '！' | '!' | '？' | '?' | '\n' | '；' | ';')
            || (ch == '.' && text[after..].chars().next().is_none_or(char::is_whitespace));
        if boundary {
            let sentence = text[start..index].trim();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = after;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

fn split_clauses(text: &str) -> Vec<&str> {
    text.split(|c| matches!(c, '，' | ',' | '、'))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_markdown_and_noise(raw: &str) -> String {
    let stage_cleared = nomifun_common::stage_direction::strip_stage_directions(raw);
    let mut result = String::with_capacity(stage_cleared.len());
    let mut in_code_block = false;

    for line in stage_cleared.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-' || c == '*' || c == '=') {
            continue;
        }
        let mut line_str = trimmed;
        while line_str.starts_with('#') {
            line_str = line_str.trim_start_matches('#').trim_start();
        }
        if line_str.starts_with('>') {
            line_str = line_str.trim_start_matches('>').trim_start();
        }
        if line_str.starts_with("- ") || line_str.starts_with("* ") {
            line_str = line_str[2..].trim_start();
        } else if let Some(rest) = line_str.strip_prefix(|c: char| c.is_ascii_digit()) {
            if let Some(rest2) = rest.strip_prefix(". ") {
                line_str = rest2.trim_start();
            }
        }
        if line_str.is_empty() {
            continue;
        }

        let cleaned_line = clean_inline_markdown(line_str);
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&cleaned_line);
    }
    result
}

fn clean_inline_markdown(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' || c == '*' || c == '_' || c == '~' {
            i += 1;
            continue;
        }
        if c == '[' {
            if let Some(close_bracket) = chars[i..].iter().position(|&x| x == ']') {
                let text_end = i + close_bracket;
                if text_end + 1 < chars.len() && chars[text_end + 1] == '(' {
                    if let Some(close_paren) = chars[text_end + 1..].iter().position(|&x| x == ')') {
                        let link_text: String = chars[i + 1..text_end].iter().collect();
                        out.push_str(&link_text);
                        i = text_end + 1 + close_paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Host-side gate for the model install/delete routes. It runs before the
/// models crate so the daemon's native file handles are released before a
/// writer touches the canonical directory.
pub async fn model_write_guard_middleware(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let method = request.method().clone();
    let path = request.uri().path();
    let is_model_write = (method == axum::http::Method::POST
        && path == "/api/voice/models/install")
        || (method == axum::http::Method::DELETE
            && (path == "/api/voice/models" || path == "/api/voice/models/"));
    if !is_model_write {
        return next.run(request).await;
    }
    match manager.begin_model_write().await {
        Ok(guard) => {
            let response = next.run(request).await;
            drop(guard);
            response
        }
        Err(code) => {
            let message = match code.as_str() {
                "voice_suite_in_use" => "A voice call, task or approval is using the models",
                "voice_resource_write_in_progress" => "Another voice model write is in progress",
                _ => "The voice models are busy",
            };
            (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({
                    "error": format!("{code}: {message}"),
                    "code": code,
                })),
            )
                .into_response()
        }
    }
}

async fn companion_capabilities_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<Option<nomifun_api_types::voice::VoiceCapabilityReport>> {
    axum::Json(manager.current_capabilities().await)
}

async fn companion_prepare_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<nomifun_api_types::voice::VoiceSelfCheckRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let mut started = false;
    {
        // Serialize with toggle(): a disable that lands after this check cannot
        // be overwritten, and a newer enable cannot be stopped by this prepare.
        let _lifecycle = manager.lifecycle_lock.lock().await;
        // A prepare is never an enable action. The user's explicit switch in
        // Settings owns that transition; an unknown/disabled choice must not be
        // repaired into autostart here.
        if !manager.read_autostart_config() {
            return (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
            )
                .into_response();
        }
        if !manager.status().await.running {
            if !manager.models_manager.get_status().await.state.is_ready() {
                return (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({
                        "code": "models_not_installed",
                        "error": "models_not_installed"
                    })),
                )
                    .into_response();
            }
            if let Err(error) = manager.start_child(true).await {
                return (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({ "code": "daemon_start_failed", "error": error })),
                )
                    .into_response();
            }
            started = true;
            // The user may have disabled voice while the child was starting.
            if !manager.read_autostart_config() {
                let _ = manager.stop().await;
                return (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
                )
                    .into_response();
            }
        }
    }
    if !manager.read_autostart_config() {
        return (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
        )
            .into_response();
    }
    if started {
        for _ in 0..60 {
            let snapshot = manager.snapshot();
            let audio = snapshot.audio.as_ref();
            let ready = snapshot.connection == VoiceConnectionState::Connected
                && audio.is_some_and(|value| {
                    value.frames_received > 0
                        && value
                            .last_frame_at_ms
                            .is_some_and(|at| nomifun_common::now_ms().saturating_sub(at) <= 2_000)
                });
            if ready {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if !manager.read_autostart_config() {
            return (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
            )
                .into_response();
        }
    }
    match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        manager.run_self_check_with_locale_and_stage(
            request.include_llm,
            request.locale,
            request.stage.or_else(|| Some("preparing".into())),
        ),
    )
    .await
    {
        Ok(Ok(report)) => axum::Json(report).into_response(),
        Ok(Err(code)) => (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "code": code, "error": code })),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            axum::Json(serde_json::json!({
                "code": "self_check_budget_exhausted",
                "error": "self_check_budget_exhausted"
            })),
        )
            .into_response(),
    }
}
async fn companion_self_check_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<nomifun_api_types::voice::VoiceSelfCheckRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        manager.run_self_check_with_locale_and_stage(
            request.include_llm,
            request.locale,
            request.stage,
        ),
    )
    .await
    {
        Ok(Ok(report)) => axum::Json(report).into_response(),
        Ok(Err(code)) => (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "code": code, "error": code })),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            axum::Json(serde_json::json!({
                "code": "self_check_budget_exhausted",
                "error": "self_check_budget_exhausted"
            })),
        )
            .into_response(),
    }
}

pub fn voice_companion_routes(manager: Arc<VoiceCompanionProcessManager>) -> axum::Router {
    use axum::routing::{get, post};
    manager.start_projection();
    manager.start_task_projection();
    axum::Router::new()
        .route("/api/voice/companion/execution", get(companion_execution_handler))
        .route("/api/voice/companion/tasks", post(companion_submit_task_handler))
        .route("/api/voice/companion/messages", post(companion_keyboard_message_handler))
        .route("/api/voice/companion/tasks/{command_id}", get(companion_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/cancel",post(companion_cancel_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/confirmations",get(companion_task_confirmations_handler))
        .route("/api/voice/companion/tasks/{command_id}/confirmations/{approval_id}",post(companion_confirm_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/replay", post(companion_replay_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/notification", get(companion_task_notification_handler))
        .route(
            "/api/voice/companion/config",
            get(companion_config_handler),
        )
        .route(
            "/api/voice/companion/feedback",
            get(companion_feedback_handler).post(companion_update_feedback_handler),
        )
        .route(
            "/api/voice/companion/feedback/preview",
            post(companion_preview_feedback_handler),
        )
        .route("/api/voice/companion/status", get(companion_status_handler))
        .route(
            "/api/voice/companion/capabilities",
            get(companion_capabilities_handler),
        )
        .route(
            "/api/voice/companion/self-check",
            post(companion_self_check_handler),
        )
        .route(
            "/api/voice/companion/prepare",
            post(companion_prepare_handler),
        )
        .route(
            "/api/voice/companion/toggle",
            post(companion_toggle_handler),
        )
        .route("/api/voice/companion/events", get(companion_events_handler))
        .route("/api/voice/companion/wake", post(companion_wake_handler))
        .route(
            "/api/voice/companion/session",
            get(companion_snapshot_handler),
        )
        .route(
            "/api/voice/companion/target",
            get(companion_target_handler).post(companion_update_target_handler),
        )
        .route(
            "/api/voice/companion/candidate",
            post(companion_candidate_handler),
        )
        .route(
            "/api/voice/companion/session/stop",
            post(companion_stop_session_handler),
        )
        .route(
            "/api/voice/companion/session/start",
            post(companion_start_call_handler),
        )
        .with_state(manager)
}

async fn companion_target_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceTargetSettings> {
    axum::Json(manager.target_settings().await)
}

async fn companion_update_target_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(requested): axum::Json<VoiceTargetUpdateRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.update_target(requested).await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => {
            let status = match code.as_str() {
                "companion_not_found" => axum::http::StatusCode::NOT_FOUND,
                "companion_id_required" => axum::http::StatusCode::BAD_REQUEST,
                _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, axum::Json(serde_json::json!({ "code": code }))).into_response()
        }
    }
}

async fn companion_candidate_handler() -> axum::response::Response {
    use axum::response::IntoResponse;
    // A browser request cannot attest native window focus or current navigation.
    (axum::http::StatusCode::FORBIDDEN, axum::Json(serde_json::json!({ "code": "native_context_required" }))).into_response()
}

async fn companion_execution_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceExecutionSettings> {
    axum::Json(manager.execution_settings().await)
}

fn task_api_error(code: String) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};
    let status = match code.as_str() {
        "runtime_unavailable" | "daemon_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        "task_scope_changed" | "task_command_conflict" | "task_already_active" | "task_queue_full" | "task_delivery_pending" | "task_approval_changed" | "voice_busy" | "binding_busy" | "voice_record_conflict" | "voice_generation_unknown" | "task_execution_config_conflict" => StatusCode::CONFLICT,
        "task_creation_rejected" | "task_delivery_rejected" => StatusCode::UNPROCESSABLE_ENTITY,
        "task_not_found" => StatusCode::NOT_FOUND,
        "binding_invalid" | "execution_unconfigured" | "task_scope_invalid" | "task_goal_invalid" | "task_workspace_invalid" | "task_execution_model_unconfigured" | "model_unavailable" | "task_preset_unavailable" | "task_preset_requires_nomi" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, axum::Json(serde_json::json!({"code":code}))).into_response()
}

async fn companion_keyboard_message_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<nomifun_api_types::voice::VoiceKeyboardMessageRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.submit_keyboard_message(request).await {
        Ok(delivery) => axum::Json(delivery).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn companion_submit_task_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<VoiceTaskSubmitRequest>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};
    match manager.submit_task(request).await {
        Ok(delivery) => {
            let status = if matches!(delivery.task.status, VoiceTaskStatus::Delivering | VoiceTaskStatus::Unknown) {StatusCode::ACCEPTED} else {StatusCode::OK};
            (status, axum::Json(delivery)).into_response()
        }
        Err(code) => task_api_error(code),
    }
}

async fn companion_task_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(tasks) = &manager.tasks else {return task_api_error("runtime_unavailable".into());};
    match tasks.status_for(&command_id).await {
        Ok(task) => {manager.project_task(task.clone()); axum::Json(task).into_response()},
        Err(code) => task_api_error(code),
    }
}

async fn companion_cancel_task_handler(
    axum::extract::State(manager):axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id):axum::extract::Path<String>,
    axum::Json(request):axum::Json<VoiceTaskControlRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if command_id!=request.task.command_id {return task_api_error("task_scope_changed".into());}
    match manager.control_task(request,VoiceTaskControlAction::CancelTask).await {
        Ok(task) => axum::Json(task).into_response(),Err(code) => task_api_error(code),
    }
}

async fn companion_task_confirmations_handler(
    axum::extract::State(manager):axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id):axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(tasks)=&manager.tasks else{return task_api_error("runtime_unavailable".into());};
    match tasks.confirmations(&command_id).await {Ok(pending)=>axum::Json(pending).into_response(),Err(code)=>task_api_error(code)}
}

async fn companion_confirm_task_handler(
    axum::extract::State(manager):axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path((command_id,approval_id)):axum::extract::Path<(String,String)>,
    axum::Json(request):axum::Json<VoiceToolConfirmationRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if request.instance_id!=manager.snapshot().instance_id || request.command_id!=command_id {return task_api_error("task_scope_changed".into());}
    let Some(tasks)=&manager.tasks else{return task_api_error("runtime_unavailable".into());};
    match tasks.confirm(&approval_id,&request).await {
        Ok(())=>axum::Json(serde_json::json!({"success":true})).into_response(),Err(code)=>task_api_error(code),
    }
}

async fn companion_replay_task_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
    axum::Json(request):axum::Json<nomifun_api_types::voice::VoiceTaskReplayRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.replay_task_result(&command_id,&request).await {
        Ok(()) => axum::Json(serde_json::json!({"success": true})).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn companion_task_notification_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(tasks) = &manager.tasks else {return task_api_error("runtime_unavailable".into());};
    match tasks.notification_for(&command_id, manager.snapshot().locale.as_deref()) {
        Ok(Some(notif)) => axum::Json(notif).into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, axum::Json(serde_json::json!({"code": "notification_not_found"}))).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn companion_config_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceDialogueSettings> {
    axum::Json(manager.dialogue_settings().await)
}

async fn companion_feedback_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.feedback_settings().await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"code":code})),
        )
            .into_response(),
    }
}

async fn companion_update_feedback_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(patch): axum::Json<VoiceFeedbackPatch>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.update_feedback(patch).await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"code":code})),
        )
            .into_response(),
    }
}

async fn companion_preview_feedback_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<VoiceFeedbackPreviewRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager
        .feedback_rpc(
            reqwest::Method::POST,
            "/preview",
            serde_json::to_value(request).ok(),
        )
        .await
    {
        Ok(response) if response.status().is_success() => {
            axum::Json(serde_json::json!({"success":true})).into_response()
        }
        Ok(response) => {
            let code = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|value| value["code"].as_str().map(str::to_owned))
                .filter(|code| {
                    matches!(
                        code.as_str(),
                        "voice_busy" | "feedback_muted" | "cue_output_failed"
                    )
                })
                .unwrap_or_else(|| "daemon_unavailable".into());
            (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({"code":code})),
            )
                .into_response()
        }
        Err(code) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"code":code})),
        )
            .into_response(),
    }
}

async fn companion_status_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceCompanionStatus> {
    axum::Json(manager.status().await)
}

async fn companion_toggle_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<VoiceCompanionToggleRequest>,
) -> axum::Json<VoiceCompanionToggleResponse> {
    match manager.toggle(payload.enabled).await {
        Ok(running) => axum::Json(VoiceCompanionToggleResponse {
            success: true,
            running,
            error: None,
        }),
        Err(err) => axum::Json(VoiceCompanionToggleResponse {
            success: false,
            running: false,
            error: Some(err),
        }),
    }
}

async fn companion_events_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::response::Response {
    use axum::response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    };
    let rx = manager.subscribe();
    let first = Some(manager.snapshot());
    let stream = futures_util::stream::unfold(
        (rx, first, manager),
        |(mut rx, first, manager)| async move {
            let snapshot = if let Some(first) = first {
                first
            } else {
                match rx.recv().await {
                    Ok(snapshot) => snapshot,
                    Err(broadcast::error::RecvError::Lagged(_)) => manager.snapshot(),
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            };
            let event = VoiceCompanionEvent::Snapshot { snapshot };
            let data = serde_json::to_string(&event).expect("voice DTO serializes");
            Some((
                Ok::<_, std::convert::Infallible>(Event::default().data(data)),
                (rx, None, manager),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn companion_snapshot_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceSessionSnapshot> {
    axum::Json(manager.snapshot())
}

async fn companion_stop_session_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<VoiceSessionControlRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.stop_session(request).await {
        Ok(()) => axum::Json(serde_json::json!({"success": true})).into_response(),
        Err(code) => (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({"success": false, "error": code})),
        )
            .into_response(),
    }
}

async fn companion_start_call_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<nomifun_api_types::voice::VoiceCallStartRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.start_call(payload).await {
        Ok(res) => axum::Json(res).into_response(),
        Err((status, err_str)) => {
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&err_str) {
                (status, axum::Json(json_val)).into_response()
            } else {
                (status, axum::Json(serde_json::json!({ "code": err_str, "message": err_str }))).into_response()
            }
        }
    }
}

async fn companion_wake_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let status = manager.status().await;
    if !status.running {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "success": false,
                "error": "Voice companion is not running"
            })),
        )
            .into_response();
    }

    let client = match reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build() {
        Ok(client)=>client,
        Err(_)=>return (StatusCode::SERVICE_UNAVAILABLE,axum::Json(serde_json::json!({"success":false,"error":"companion_control_unavailable"}))).into_response(),
    };
    match client
        .post(format!("{}/api/voice/wake",manager.daemon_origin))
        .header("x-ai-mate-control-token", &manager.control_token)
        .timeout(std::time::Duration::from_secs(3))
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let json = resp.json::<serde_json::Value>().await.unwrap_or_default();
            (status, axum::Json(json)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to proxy wake to companion: {e}")
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests;
