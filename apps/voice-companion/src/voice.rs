//! AI Mate Voice Companion Runtime & Pipeline Assembly
//!
//! Ties together:
//! - `nomifun_audio::voice_pipeline::VoiceCapturePipeline`
//! - `nomifun_voice_terminal::playback::PlaybackPipeline`
//! - `nomifun_voice_terminal::driver::VoiceSessionDriver`
//! - `nomifun_voice_terminal::router::QwenVoiceRouter`
//! - `nomifun_voice_models::ModelManager`
//! - `crate::oobe::OobeManager`

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tracing::{debug, info, warn};
use crate::self_check_audio::SelfCheckAudio;

use nomifun_audio::EarconKind;
use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::kws::KwsEngine;
#[cfg(any(test, feature = "voice-skeleton"))]
use nomifun_audio::vad::{EnergyVad, VadConfig};
use nomifun_audio::vad::{SherpaVad, VadBackend};
use nomifun_audio::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};
use nomifun_audio::{frame::AudioChannel, recorder::SttCallback};
use nomifun_voice_models::ModelManager;
use nomifun_voice_terminal::contract::{AcousticEvent, AcousticState, IntentType};
use nomifun_voice_terminal::driver::{VoiceRouter, VoiceSessionDriver};
#[cfg(any(test, feature = "voice-skeleton"))]
use nomifun_voice_terminal::playback::ChannelAudioSink;
use nomifun_voice_terminal::playback::{AudioSinkError, PhysicalAudioSink, PlaybackPipeline};
use nomifun_voice_terminal::router::QwenVoiceRouter;
use nomifun_voice_terminal::tts::{
    EdgeTtsProvider, FallbackTtsProvider, SherpaOfflineTtsProvider, TtsProvider,
};

use crate::oobe::OobeManager;
use crate::voice_events::VoiceEventHub;
use crate::voice_feedback;
mod confirmation;
use confirmation::PendingConfirmation;
pub mod asr_lifecycle;
pub use nomifun_api_types::voice::VoiceCompanionEvent;
use nomifun_api_types::voice::{
    VoiceAudioDiagnostics, VoiceCueKind, VoiceDecision, VoiceSessionControlRequest,
    VoiceSessionEndReason, VoiceSessionSnapshot,
};

#[derive(Debug, thiserror::Error)]
pub enum VoiceWakeError {
    #[error("pairing_required")]
    PairingRequired,
    #[error("models_unavailable")]
    ModelsUnavailable,
    #[error("microphone_unavailable")]
    MicrophoneUnavailable,
    #[error("session_busy")]
    Busy,
}

#[derive(Clone, Debug)]
struct AnnouncementRecord {
    command_id: String,
    notification_id: String,
    attempt_id: String,
    scope: nomifun_api_types::voice::VoiceSessionControlRequest,
    text: String,
    status: String,
    reason_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PendingUtterance {
    pub pcm: Vec<f32>,
    pub session_id: Option<String>,
    pub generation_id: u64,
}

struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

fn detect_task_control(input: &str) -> Option<nomifun_api_types::voice::VoiceTaskControlAction> {
    nomifun_api_types::voice::classify_voice_task_control(input)
}

async fn local_task_control_action(
    input: &str,
    client: &dyn nomifun_voice_terminal::task_client::VoiceTaskClient,
) -> Option<nomifun_api_types::voice::VoiceTaskControlAction> {
    let action = detect_task_control(input)?;
    let explicit = input.contains("任务")
        || input.to_ascii_lowercase().contains("task")
        || input.contains("进度")
        || input.to_ascii_lowercase().contains("progress");
    if explicit {
        return Some(action);
    }
    let has_active = tokio::time::timeout(std::time::Duration::from_millis(250), client.current())
        .await
        .ok()
        .and_then(|result| result.ok())
        .flatten()
        .is_some();
    has_active.then_some(action)
}

fn task_status_for_prompt(
    task: Option<&nomifun_api_types::voice::VoiceTaskSnapshot>,
    scope: &nomifun_api_types::voice::VoiceSessionSnapshot,
) -> Option<String> {
    let session = scope.session_id.as_deref()?;
    let task = task?;
    if task.session_id != session {
        return None;
    }
    Some(serde_json::json!({
        "command_id": task.command_id,
        "status": task.status,
    }).to_string())
}

/// Pre-roll sample buffer (500ms @ 16kHz) preserved during active playback
/// to ensure the leading onset of user speech is not clipped upon barge-in interruption.
const BARGE_IN_PREROLL_SAMPLES: usize = 8_000;

struct WorkerEngineCheck {
    request_id: String,
    deadline: std::time::Instant,
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    native_busy: Arc<AtomicBool>,
    native_job_epoch: Arc<AtomicU64>,
    fixtures: SelfCheckAudio,
    reply: oneshot::Sender<Result<WorkerEngineCheckResult, String>>,
}

struct NativeBusyGuard {
    busy: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    #[cfg_attr(not(feature = "native-fault-injection"), allow(dead_code))]
    job: u64,
}

impl NativeBusyGuard {
    #[cfg(feature = "native-fault-injection")]
    fn new(busy: Arc<AtomicBool>, epoch: Arc<AtomicU64>) -> Self {
        let job = epoch.fetch_add(1, Ordering::AcqRel) + 1;
        Self::attach(busy, epoch, job)
    }

    fn attach(busy: Arc<AtomicBool>, epoch: Arc<AtomicU64>, job: u64) -> Self {
        busy.store(true, Ordering::Release);
        #[cfg(feature = "native-fault-injection")]
        {
            FAULT_NATIVE_BUSY.store(true, Ordering::Release);
            FAULT_CURRENT_EPOCH.store(job, Ordering::Release);
        }
        Self { busy, epoch, job }
    }
}
impl Drop for NativeBusyGuard {
    fn drop(&mut self) {
        // Retire the old generation before opening the reservation window.
        // Clear the observation while the old owner still excludes newcomers;
        // otherwise A could clear B's freshly attached busy flag.
        self.epoch.fetch_add(1, Ordering::AcqRel);
        #[cfg(feature = "native-fault-injection")]
        if FAULT_CURRENT_EPOCH.load(Ordering::Acquire) == self.job {
            FAULT_NATIVE_BUSY.store(false, Ordering::Release);
        }
        self.busy.store(false, Ordering::Release);
    }
}

/// Reserves the native lease for a user utterance before maintenance work is
/// enqueued. A reservation is a promise that the next free lease goes to ASR,
/// not a lease itself: capture and native frame work remain free to finish.
#[derive(Default)]
struct NativeAdmission {
    pending: AtomicUsize,
    notify: Notify,
    arbitration: std::sync::Mutex<()>,
    #[cfg(test)]
    before_background_claim: std::sync::Mutex<Option<Arc<AdmissionClaimTestGate>>>,
}

#[cfg(test)]
struct AdmissionClaimTestGate {
    reached: Notify,
    resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl NativeAdmission {
    fn reserve(self: &Arc<Self>) -> NativeAdmissionGuard {
        let _arbitration = self.arbitration.lock().unwrap_or_else(|error| error.into_inner());
        self.pending.fetch_add(1, Ordering::AcqRel);
        NativeAdmissionGuard {
            admission: self.clone(),
        }
    }

    fn has_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire) > 0
    }
}

struct NativeAdmissionGuard {
    admission: Arc<NativeAdmission>,
}

impl Drop for NativeAdmissionGuard {
    fn drop(&mut self) {
        {
            let _arbitration = self.admission.arbitration.lock().unwrap_or_else(|error| error.into_inner());
            self.admission.pending.fetch_sub(1, Ordering::AcqRel);
        }
        self.admission.notify.notify_waiters();
    }
}

#[derive(Default)]
struct NativeHealth {
    state: std::sync::Mutex<NativeHealthState>,
}

#[derive(Default)]
struct NativeHealthState {
    job: u64,
    degraded: bool,
}

impl NativeHealth {
    fn publish(&self, job: u64, degraded: bool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        // A job may refine its own result (for example an inner loader error),
        // but an older job must never overwrite a newer job's health.
        if job >= state.job {
            state.job = job;
            state.degraded = degraded;
        }
    }

    fn publish_result<T, E>(&self, job: u64, result: &Result<T, E>) {
        self.publish(job, result.is_err());
    }

    fn is_degraded(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .degraded
    }
}

#[derive(Clone)]
struct NativeCallRunner {
    busy: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    health: Option<Arc<NativeHealth>>,
    admission: Option<Arc<NativeAdmission>>,
    priority: bool,
}

impl NativeCallRunner {
    #[cfg(test)]
    fn new(busy: Arc<AtomicBool>, epoch: Arc<AtomicU64>) -> Self {
        Self {
            busy,
            epoch,
            health: None,
            admission: None,
            priority: false,
        }
    }

    fn with_health(
        busy: Arc<AtomicBool>,
        epoch: Arc<AtomicU64>,
        health: Arc<NativeHealth>,
    ) -> Self {
        Self {
            busy,
            epoch,
            health: Some(health),
            admission: None,
            priority: false,
        }
    }

    fn with_admission(
        busy: Arc<AtomicBool>,
        epoch: Arc<AtomicU64>,
        health: Arc<NativeHealth>,
        admission: Arc<NativeAdmission>,
        priority: bool,
    ) -> Self {
        Self {
            busy,
            epoch,
            health: Some(health),
            admission: Some(admission),
            priority,
        }
    }

    fn publish_health(&self, job: u64, degraded: bool) {
        if let Some(health) = &self.health {
            health.publish(job, degraded);
        }
    }

    fn publish_error(&self, job: u64, reason: &str) {
        if native_error_requires_degraded(reason) {
            self.publish_health(job, true);
        }
    }

    async fn acquire(
        &self,
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
    ) -> Result<NativeBusyGuard, String> {
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("engine_check_cancelled".into());
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err("engine_check_timeout".into());
            }
            let acquired = {
                // Reserve and background admission share one linearization point.
                let _arbitration = self.admission.as_ref().map(|admission| {
                    admission.arbitration.lock().unwrap_or_else(|error| error.into_inner())
                });
                let reserved = !self.priority && self.admission.as_ref().is_some_and(|admission| admission.has_pending());
                #[cfg(test)]
                if !self.priority && !reserved {
                    if let Some(gate) = self.admission.as_ref().and_then(|admission| admission.before_background_claim.lock().unwrap_or_else(|error| error.into_inner()).take()) {
                        gate.reached.notify_one();
                        gate.resume.lock().unwrap_or_else(|error| error.into_inner()).recv_timeout(std::time::Duration::from_secs(2)).expect("test must release the admission gate");
                    }
                }
                if !reserved && self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                    let job = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
                    Some(NativeBusyGuard::attach(self.busy.clone(), self.epoch.clone(), job))
                } else {
                    None
                }
            };
            if let Some(guard) = acquired { return Ok(guard); }
            if !wait_for_lease {
                return Err("engine_check_native_busy".into());
            }
            let pause = tokio::time::sleep(std::cmp::min(
                deadline.saturating_duration_since(now),
                std::time::Duration::from_millis(10),
            ));
            tokio::pin!(pause);
            if let Some(admission) = &self.admission {
                tokio::select! {
                    _ = notify.notified() => {},
                    _ = admission.notify.notified() => {},
                    _ = &mut pause => {},
                }
            } else {
                tokio::select! {
                    _ = notify.notified() => {},
                    _ = &mut pause => {},
                }
            }
        }
    }

    fn schedule_watchdog(&self, job: u64, started_at: std::time::Instant) {
        if std::env::var("VOICE_COMPANION_SUPERVISED").ok().as_deref() != Some("1") {
            return;
        }
        let busy = self.busy.clone();
        let epoch = self.epoch.clone();
        let grace_ms = std::env::var("NOMIFUN_VOICE_NATIVE_LEASE_GRACE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(30_000)
            .clamp(250, 60_000);
        let grace = std::time::Duration::from_millis(grace_ms);
        let delay = grace.saturating_sub(started_at.elapsed());
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            if native_job_needs_recovery(
                true,
                busy.load(Ordering::Acquire),
                epoch.load(Ordering::Acquire),
                job,
            ) {
                tracing::error!(grace_ms, job, "voice_native_lease_stuck_exiting_for_supervised_restart");
                std::process::exit(75);
            }
        });
    }

    async fn run<T, F>(
        &self,
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
        watchdog: bool,
        #[cfg(feature = "native-fault-injection")] context: Option<&NativeCallContext>,
        call: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.run_and_publish(
            deadline,
            cancel,
            notify,
            wait_for_lease,
            watchdog,
            #[cfg(feature = "native-fault-injection")]
            context,
            call,
            |_, _| {},
        )
        .await
    }

    async fn run_and_publish<T, F, P>(
        &self,
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
        watchdog: bool,
        #[cfg(feature = "native-fault-injection")] context: Option<&NativeCallContext>,
        call: F,
        publish: P,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
        P: FnOnce(&T, u64) + Send + 'static,
    {
        let guard = self
            .acquire(deadline, cancel, notify, wait_for_lease)
            .await?;
        if cancel.load(Ordering::Acquire) {
            return Err("engine_check_cancelled".into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("engine_check_timeout".into());
        }
        let job = guard.job;
        #[cfg(feature = "native-fault-injection")]
        if let Some(context) = context {
            bind_fault_job(job, context);
        }
        let _watchdog = NativeJobWatchdog::new(self, job, watchdog);
        #[cfg(feature = "native-fault-injection")]
        let context = context.cloned();
        let mut task = tokio::task::spawn_blocking(move || {
            #[cfg(feature = "native-fault-injection")]
            if let Some(context) = context.as_ref() {
                record_fault_job_entry(job, Some(context));
            }
            let value = call();
            (guard, value)
        });
        tokio::select! {
            _ = notify.notified() => Err("engine_check_cancelled".into()),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                self.publish_health(job, true);
                Err("engine_check_timeout".into())
            }
            result = &mut task => match result {
                Ok((guard, value)) => {
                    // Keep the lease through publication. Otherwise a successor
                    // could start before this result is visible and then be
                    // overwritten by the older caller.
                    self.publish_health(job, false);
                    publish(&value, job);
                    drop(guard);
                    Ok(value)
                }
                Err(error) => {
                    let reason = format!("native_call_join_failed:{error}");
                    self.publish_error(job, &reason);
                    Err(reason)
                }
            },
        }
    }

    async fn load<T, E, F>(
        &self,
        budget: std::time::Duration,
        call: F,
    ) -> Result<Result<T, E>, String>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce() -> Result<T, E> + Send + 'static,
    {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.load_until(
            std::time::Instant::now() + budget,
            &cancel,
            &notify,
            true,
            call,
        )
        .await
    }

    async fn load_until<T, E, F>(
        &self,
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
        call: F,
    ) -> Result<Result<T, E>, String>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce() -> Result<T, E> + Send + 'static,
    {
        let health = self.health.clone();
        self.run_and_publish(
            deadline,
            cancel,
            notify,
            wait_for_lease,
            true,
            #[cfg(feature = "native-fault-injection")]
            None,
            call,
            move |result, job| {
                if let Some(health) = health {
                    health.publish_result(job, result);
                }
            },
        )
        .await
    }

    #[cfg(test)]
    async fn run_uncancelable<T, F>(
        &self,
        budget: std::time::Duration,
        wait_for_lease: bool,
        call: F,
    ) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.run(
            std::time::Instant::now() + budget,
            &cancel,
            &notify,
            wait_for_lease,
            true,
            #[cfg(feature = "native-fault-injection")]
            None,
            call,
        )
        .await
    }
}

struct NativeJobWatchdog {
    runner: NativeCallRunner,
    job: u64,
    started_at: std::time::Instant,
    armed: bool,
}

impl NativeJobWatchdog {
    fn new(runner: &NativeCallRunner, job: u64, armed: bool) -> Self {
        Self {
            runner: runner.clone(),
            job,
            started_at: std::time::Instant::now(),
            armed,
        }
    }
}

impl Drop for NativeJobWatchdog {
    fn drop(&mut self) {
        if !self.armed
            || !self.runner.busy.load(Ordering::Acquire)
            || self.runner.epoch.load(Ordering::Acquire) != self.job
        {
            return;
        }
        if std::env::var("VOICE_COMPANION_SUPERVISED").ok().as_deref() == Some("1") {
            self.runner.schedule_watchdog(self.job, self.started_at);
        }
    }
}

fn native_job_needs_recovery(supervised: bool, busy: bool, current_job: u64, job: u64) -> bool {
    supervised && busy && current_job == job
}

fn native_error_requires_degraded(reason: &str) -> bool {
    reason == "engine_check_timeout"
        || reason == "worker_asr_failed"
        || reason.starts_with("native_call_join_failed:")
        || reason.starts_with("worker_asr_join_failed:")
}

fn asr_failure_code(reason: &str) -> &'static str {
    match reason {
        "engine_check_native_busy" => "asr_busy",
        "engine_check_timeout" => "asr_timeout",
        _ => "asr_failed",
    }
}

trait KwsBackend: Send {
    fn process_pcm_chunk(&mut self, chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult>;
    fn reset(&mut self);
    fn is_initialized(&self) -> bool;
}

impl KwsBackend for KwsEngine {
    fn process_pcm_chunk(&mut self, chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
        KwsEngine::process_pcm_chunk(self, chunk)
    }

    fn reset(&mut self) {
        KwsEngine::reset(self);
    }

    fn is_initialized(&self) -> bool {
        KwsEngine::is_initialized(self)
    }
}

struct NativeKws {
    inner: Arc<std::sync::Mutex<Box<dyn KwsBackend>>>,
    initialized: Arc<AtomicBool>,
    runner: NativeCallRunner,
}

impl NativeKws {
    fn new(kws: Box<dyn KwsBackend>, runner: NativeCallRunner) -> Self {
        let initialized = kws.is_initialized();
        Self {
            inner: Arc::new(std::sync::Mutex::new(kws)),
            initialized: Arc::new(AtomicBool::new(initialized)),
            runner,
        }
    }

    fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    async fn process_pcm_chunk(
        &self,
        chunk: &[f32],
    ) -> Result<Option<nomifun_audio::kws::KwsResult>, String> {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.process_pcm_chunk_until(
            chunk,
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            &cancel,
            &notify,
            true,
        )
        .await
    }

    async fn process_pcm_chunk_until(
        &self,
        chunk: &[f32],
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
    ) -> Result<Option<nomifun_audio::kws::KwsResult>, String> {
        let inner = self.inner.clone();
        let chunk = chunk.to_vec();
        self.runner
            .run(
                deadline,
                cancel,
                notify,
                wait_for_lease,
                true,
                #[cfg(feature = "native-fault-injection")]
                None,
                move || {
                    inner
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .process_pcm_chunk(&chunk)
                },
            )
            .await
    }

    async fn reset(&self) -> Result<(), String> {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.reset_until(
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            &cancel,
            &notify,
            true,
        )
        .await
    }

    async fn reset_until(
        &self,
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
    ) -> Result<(), String> {
        let inner = self.inner.clone();
        self.runner
            .run(
                deadline,
                cancel,
                notify,
                wait_for_lease,
                true,
                #[cfg(feature = "native-fault-injection")]
                None,
                move || {
                    inner
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .reset();
                },
            )
            .await
    }

    async fn replace(&self, next: Box<dyn KwsBackend>) -> Result<(), String> {
        self.replace_until(
            next,
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        )
        .await
    }

    async fn replace_until(
        &self,
        next: Box<dyn KwsBackend>,
        deadline: std::time::Instant,
    ) -> Result<(), String> {
        let initialized = next.is_initialized();
        let inner = self.inner.clone();
        let initialized_state = self.initialized.clone();
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.runner
            .run(
                deadline,
                &cancel,
                &notify,
                true,
                true,
                #[cfg(feature = "native-fault-injection")]
                None,
                move || {
                    let mut current = inner.lock().unwrap_or_else(|error| error.into_inner());
                    let previous = std::mem::replace(&mut *current, next);
                    // Publish with the actual replacement, while this job still
                    // excludes successors. A timed-out waiter cannot leave the
                    // cached readiness describing the replaced engine.
                    initialized_state.store(initialized, Ordering::Release);
                    drop(previous);
                },
            )
            .await
    }
}

struct NativeVad {
    inner: Arc<std::sync::Mutex<Box<dyn VadBackend>>>,
    active: Arc<AtomicBool>,
    runner: NativeCallRunner,
}

impl NativeVad {
    fn new(vad: Box<dyn VadBackend>, runner: NativeCallRunner) -> Self {
        Self {
            inner: Arc::new(std::sync::Mutex::new(vad)),
            active: Arc::new(AtomicBool::new(false)),
            runner,
        }
    }

    fn is_speech_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    async fn process_frame(&self, frame: &[f32]) -> Result<bool, String> {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.process_frame_until(
            frame,
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            &cancel,
            &notify,
            true,
        )
        .await
    }

    async fn process_frame_until(
        &self,
        frame: &[f32],
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
    ) -> Result<bool, String> {
        let inner = self.inner.clone();
        let frame = frame.to_vec();
        let active = self.active.clone();
        self.runner
            .run_and_publish(
                deadline,
                cancel,
                notify,
                wait_for_lease,
                true,
                #[cfg(feature = "native-fault-injection")]
                None,
                move || {
                    inner
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .process_frame(&frame)
                },
                move |result, _job| active.store(*result, Ordering::Release),
            )
            .await
    }

    async fn reset(&self) -> Result<(), String> {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        self.reset_until(
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            &cancel,
            &notify,
            true,
        )
        .await
    }

    async fn reset_until(
        &self,
        deadline: std::time::Instant,
        cancel: &AtomicBool,
        notify: &Notify,
        wait_for_lease: bool,
    ) -> Result<(), String> {
        let inner = self.inner.clone();
        let active = self.active.clone();
        self.runner
            .run_and_publish(
                deadline,
                cancel,
                notify,
                wait_for_lease,
                true,
                #[cfg(feature = "native-fault-injection")]
                None,
                move || {
                    inner
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .reset();
                },
                move |_, _job| active.store(false, Ordering::Release),
            )
            .await
    }
}

#[derive(Debug)]
struct WorkerEngineCheckResult {
    kws_ok: bool,
    vad_ok: bool,
    asr_ok: bool,
    kws_hits: u32,
    asr_text: Option<String>,
}

#[cfg(feature = "native-fault-injection")]
struct FaultBlockingStt;

#[cfg(feature = "native-fault-injection")]
#[async_trait::async_trait]
impl SttCallback for FaultBlockingStt {
    async fn transcribe(
        &self,
        _channel: AudioChannel,
        _pcm: Vec<f32>,
        _sample_rate: u32,
    ) -> Option<String> {
        // The lease-bound wrapper records this job before entry. Here we also
        // record inside the blocking thread to bind the worker thread identity.
        let _ = tokio::task::spawn_blocking(|| {
            FAULT_ASR_ENTERED.fetch_add(1, Ordering::AcqRel);
            record_fault_job_entry(1, None);
            std::thread::sleep(std::time::Duration::from_secs(120));
        })
        .await;
        Some("fault-blocking-asr".into())
    }
}

#[cfg(feature = "native-fault-injection")]
struct BoundStt {
    inner: Arc<dyn SttCallback>,
    job: u64,
    context: Option<NativeCallContext>,
    entered: AtomicBool,
}

#[cfg(feature = "native-fault-injection")]
#[async_trait::async_trait]
impl SttCallback for BoundStt {
    async fn transcribe(
        &self,
        channel: AudioChannel,
        pcm: Vec<f32>,
        sample_rate: u32,
    ) -> Option<String> {
        if !self.entered.swap(true, Ordering::AcqRel) {
            record_fault_job_entry(self.job, self.context.as_ref());
        }
        self.inner.transcribe(channel, pcm, sample_rate).await
    }

    async fn transcribe_checked(&self, channel: AudioChannel, pcm: Vec<f32>, sample_rate: u32) -> Result<Option<String>, String> {
        if !self.entered.swap(true, Ordering::AcqRel) {
            record_fault_job_entry(self.job, self.context.as_ref());
        }
        self.inner.transcribe_checked(channel, pcm, sample_rate).await
    }
}

#[cfg(feature = "native-fault-injection")]
#[derive(Clone)]
struct FaultBoundary {
    request_id: String,
    worker_epoch: u64,
    instance_id: String,
}

#[cfg(feature = "native-fault-injection")]
#[derive(Clone)]
struct FaultJobRecord {
    request_id: String,
    worker_epoch: u64,
    instance_id: String,
    pid: u32,
    job_epoch: u64,
    entered_unix_ms: u128,
}

#[cfg(feature = "native-fault-injection")]
#[derive(Clone)]
struct NativeCallContext {
    request_id: String,
    worker_epoch: u64,
    instance_id: String,
}

#[cfg(feature = "native-fault-injection")]
static FAULT_BOUNDARY: std::sync::Mutex<Option<FaultBoundary>> = std::sync::Mutex::new(None);
#[cfg(feature = "native-fault-injection")]
static FAULT_JOB_BOUNDARIES: std::sync::Mutex<Vec<(u64, FaultBoundary)>> =
    std::sync::Mutex::new(Vec::new());
#[cfg(feature = "native-fault-injection")]
static FAULT_JOB_RECORD: std::sync::Mutex<Option<FaultJobRecord>> = std::sync::Mutex::new(None);
#[cfg(feature = "native-fault-injection")]
pub static FAULT_NATIVE_BUSY: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "native-fault-injection")]
static FAULT_CURRENT_EPOCH: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "native-fault-injection")]
fn bind_fault_boundary(request_id: &str, worker_epoch: u64, instance_id: &str) {
    *FAULT_BOUNDARY.lock().unwrap_or_else(|error| error.into_inner()) = Some(FaultBoundary {
        request_id: request_id.to_owned(),
        worker_epoch,
        instance_id: instance_id.to_owned(),
    });
}

#[cfg(feature = "native-fault-injection")]
fn bind_fault_job(job: u64, context: &NativeCallContext) {
    FAULT_JOB_BOUNDARIES
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push((
            job,
            FaultBoundary {
                request_id: context.request_id.clone(),
                worker_epoch: context.worker_epoch,
                instance_id: context.instance_id.clone(),
            },
        ));
}

#[cfg(feature = "native-fault-injection")]
fn record_fault_job_entry(job_epoch: u64, context: Option<&NativeCallContext>) {
    let boundary = context
        .map(|context| FaultBoundary {
            request_id: context.request_id.clone(),
            worker_epoch: context.worker_epoch,
            instance_id: context.instance_id.clone(),
        })
        .or_else(|| {
            FAULT_JOB_BOUNDARIES
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .find(|(job, _)| *job == job_epoch)
                .map(|(_, boundary)| boundary.clone())
        })
        .or_else(|| {
            FAULT_BOUNDARY
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        });
    let Some(boundary) = boundary else { return };
    let entered_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    *FAULT_JOB_RECORD.lock().unwrap_or_else(|error| error.into_inner()) = Some(FaultJobRecord {
        request_id: boundary.request_id,
        worker_epoch: boundary.worker_epoch,
        instance_id: boundary.instance_id,
        pid: std::process::id(),
        job_epoch,
        entered_unix_ms,
    });
}

#[derive(Clone)]
struct EngineCheckControl {
    id: String,
    epoch: u64,
    cancel: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
}

enum ReplyPlayback {
    Played,
    Interrupted,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupMode {
    Default,
    HeadlessTest,
}

/// Decides the startup path before any device assembly. Any retained fault or
/// headless marker is fail-closed unless this is the explicit test feature and
/// the headless marker is present. Production defaults never fall through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupMarkers {
    pub headless_present: bool,
    pub headless_valid: bool,
    pub fault_present: bool,
    pub exit_code_present: bool,
}

pub fn parse_startup_markers(
    headless: Option<&std::ffi::OsStr>,
    fault: Option<&std::ffi::OsStr>,
    exit_code: Option<&std::ffi::OsStr>,
) -> StartupMarkers {
    StartupMarkers {
        headless_present: headless.is_some(),
        headless_valid: headless.is_some_and(|value| value == "1"),
        fault_present: fault.is_some(),
        exit_code_present: exit_code.is_some(),
    }
}

pub fn startup_mode(
    feature_enabled: bool,
    markers: StartupMarkers,
) -> Result<StartupMode, &'static str> {
    if markers.headless_present || markers.fault_present || markers.exit_code_present {
        if !feature_enabled {
            return Err("native_fault_marker_requires_feature_and_headless");
        }
        if !markers.headless_valid {
            return Err("native_fault_feature_requires_valid_headless");
        }
        return Ok(StartupMode::HeadlessTest);
    }
    if feature_enabled {
        return Err("native_fault_feature_requires_headless");
    }
    Ok(StartupMode::Default)
}

pub static HARDWARE_MIC_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
pub static HARDWARE_SPEAKER_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
pub static FAULT_ASR_ENTERED: AtomicU64 = AtomicU64::new(0);
pub static FAULT_INIT_ENTERED: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "native-fault-injection")]
pub fn fault_job_observation() -> serde_json::Value {
    let job = FAULT_JOB_RECORD
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let busy = FAULT_NATIVE_BUSY.load(Ordering::Acquire);
    let current_epoch = FAULT_CURRENT_EPOCH.load(Ordering::Acquire);
    serde_json::json!({
        "fault_asr_entered": FAULT_ASR_ENTERED.load(Ordering::Acquire),
        "fault_init_entered": FAULT_INIT_ENTERED.load(Ordering::Acquire),
        "native_busy": busy,
        "job": job.map(|job| serde_json::json!({
            "request_id": job.request_id,
            "worker_epoch": job.worker_epoch,
            "instance_id": job.instance_id,
            "pid": job.pid,
            "job_epoch": job.job_epoch,
            "entered_unix_ms": job.entered_unix_ms,
            "still_blocking": busy && current_epoch == job.job_epoch,
        })),
    })
}
#[cfg(not(feature = "native-fault-injection"))]
pub fn fault_job_observation() -> serde_json::Value {
    serde_json::json!({ "fault_asr_entered": FAULT_ASR_ENTERED.load(Ordering::Acquire),
        "fault_init_entered": FAULT_INIT_ENTERED.load(Ordering::Acquire), "native_busy": false, "job": null })
}
pub fn hardware_attempts() -> (u64, u64) {
    (
        HARDWARE_MIC_ATTEMPTS.load(Ordering::Acquire),
        HARDWARE_SPEAKER_ATTEMPTS.load(Ordering::Acquire),
    )
}

#[cfg(any(test, feature = "voice-skeleton"))]
#[derive(Default)]
pub struct SpeechEndTestGate {
    pub reached: Notify,
    pub resume: Notify,
}

/// Production voice companion pipeline assembly and runtime controller.
pub struct VoiceCompanionRuntime {
    #[cfg(test)]
    confirmation_speech_end_gate: Option<Arc<SpeechEndTestGate>>,
    #[cfg(any(test, feature = "voice-skeleton"))]
    pub ordinary_speech_end_gate: Option<Arc<SpeechEndTestGate>>,
    pending_confirmation:Arc<std::sync::Mutex<Option<PendingConfirmation>>>,
    announced_tasks: Mutex<std::collections::VecDeque<AnnouncementRecord>>,
    task_client: Option<Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>>,
    model_manager: Arc<ModelManager>,
    oobe_manager: Arc<OobeManager>,
    driver: Arc<Mutex<VoiceSessionDriver>>,
    playback: Arc<PlaybackPipeline>,
    tts: Arc<dyn TtsProvider>,
    router: Arc<dyn VoiceRouter>,
    transcriber: Option<Arc<dyn SttCallback>>,
    capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
    acoustic_ready: Arc<AtomicBool>,
    self_check_active: Arc<AtomicBool>,
    self_check_cancel: Arc<AtomicBool>,
    self_check_id: Arc<std::sync::Mutex<Option<String>>>,
    engine_check_active: Arc<AtomicBool>,
    engine_check_native_busy: Arc<AtomicBool>,
    native_admission: Arc<NativeAdmission>,
    native_degraded: Arc<NativeHealth>,
    native_recovery_scheduled: Arc<AtomicU64>,
    native_job_epoch: Arc<AtomicU64>,
    engine_check_control: Arc<std::sync::Mutex<Option<EngineCheckControl>>>,
    worker_epoch: Arc<AtomicU64>,
    engine_check_tx: std::sync::Mutex<Option<mpsc::Sender<WorkerEngineCheck>>>,
    engine_check_rx: std::sync::Mutex<Option<mpsc::Receiver<WorkerEngineCheck>>>,
    wake_admission_until_ms: Arc<AtomicU64>,
    call_admission_until_ms: Arc<AtomicU64>,
    event_tx: Arc<VoiceEventHub>,
    active_turn: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    input_generation: Arc<AtomicU64>,
    pending_utterances: Arc<std::sync::Mutex<std::collections::VecDeque<PendingUtterance>>>,
    /// Active RAII guard holding the cpal input stream open. Dropping this closes the mic.
    #[allow(dead_code)]
    mic_source: Option<Arc<nomifun_audio::MicSource>>,
    #[cfg(any(test, feature = "voice-skeleton"))]
    replay_capture: bool,
    #[cfg(any(test, feature = "voice-skeleton"))]
    replay_barge_in: bool,
}

impl Drop for VoiceCompanionRuntime {
    fn drop(&mut self) {
        if let Some(turn) = self
            .active_turn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            turn.abort();
        }
        self.pending_utterances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.pending_utterances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

fn validate_self_check_pcm(pcm: &[f32], sample_rate: u32) -> Result<(u64, u16, u16), String> {
    if !(8_000..=48_000).contains(&sample_rate) {
        return Err("tts_invalid_sample_rate".into());
    }
    if pcm.is_empty() {
        return Err("tts_empty_pcm".into());
    }
    if pcm.iter().any(|sample| !sample.is_finite()) {
        return Err("tts_non_finite_pcm".into());
    }
    let duration_ms = (pcm.len() as u64).saturating_mul(1_000) / u64::from(sample_rate);
    if !(100..=30_000).contains(&duration_ms) {
        return Err("tts_duration_out_of_bounds".into());
    }
    let peak = pcm.iter().map(|sample| sample.abs()).fold(0.0_f32, f32::max);
    let rms = (pcm.iter().map(|sample| sample * sample).sum::<f32>() / pcm.len() as f32).sqrt();
    let peak_milli = (peak * 1_000.0).round().clamp(0.0, 65_535.0) as u16;
    let rms_milli = (rms * 1_000.0).round().clamp(0.0, 65_535.0) as u16;
    if peak < 0.02 && rms < 0.005 {
        return Err("tts_silent_pcm".into());
    }
    Ok((duration_ms, rms_milli, peak_milli))
}
struct SelfCheckGuard {
    active: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    id: Arc<std::sync::Mutex<Option<String>>>,
}

impl Drop for SelfCheckGuard {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.cancel.store(false, Ordering::Release);
        *self.id.lock().unwrap_or_else(|error| error.into_inner()) = None;
    }
}
fn admission_valid_for(until: &Arc<AtomicU64>) -> bool {
    let now = nomifun_common::now_ms().max(0) as u64;
    now < until.load(Ordering::Acquire)
}
impl VoiceCompanionRuntime {
    fn report_tts_source(events: &VoiceEventHub, tts: &dyn TtsProvider, scope: &VoiceSessionControlRequest) {
        if let Some(diagnostic) = tts.diagnostic("") {
            let _ = events.send_scoped(VoiceCompanionEvent::TtsSource {
                session_id: scope.session_id.clone(), turn_id: scope.turn_id.clone(), diagnostic,
            }, scope);
        }
    }
    /// Assembles the complete voice pipeline with capture, playback, session driver, and router.
    pub fn assemble(model_manager: Arc<ModelManager>, oobe_manager: Arc<OobeManager>) -> Self {
        HARDWARE_MIC_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
        let (capture_pipeline, mic_source) = match nomifun_audio::MicSource::new(16_000) {
            Ok(mic) => {
                info!("Hardware microphone successfully initialized for AI Mate");
                let pipeline = mic.pipeline();
                let mic_arc = Arc::new(mic);
                (pipeline, Some(mic_arc))
            }
            Err(err) => {
                warn!(error = %err, "No hardware microphone available; using standalone capture pipeline");
                let (pipeline, _) = VoiceCapturePipeline::new(PipelineConfig::default());
                (Arc::new(Mutex::new(pipeline)), None)
            }
        };

        HARDWARE_SPEAKER_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
        let sink = Arc::new(PhysicalAudioSink::open_default());
        let playback = Arc::new(PlaybackPipeline::new(sink));

        let driver = Arc::new(Mutex::new(VoiceSessionDriver::new()));
        let router = Arc::new(QwenVoiceRouter::new());
        let event_tx = Arc::new(VoiceEventHub::new());
        let (engine_check_tx, engine_check_rx) = mpsc::channel(1);

        Self {
            #[cfg(test)]
            confirmation_speech_end_gate: None,
            #[cfg(any(test, feature = "voice-skeleton"))]
            ordinary_speech_end_gate: None,
            pending_confirmation:Arc::new(std::sync::Mutex::new(None)),
            announced_tasks: Mutex::new(std::collections::VecDeque::new()),
            task_client: std::env::var("AI_MATE_TASK_ENDPOINT").ok().and_then(|endpoint| {
                nomifun_voice_terminal::task_client::HostTaskClient::new(endpoint, std::env::var("AI_MATE_CONTROL_TOKEN").unwrap_or_default())
                    .map(|client| Arc::new(client) as Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>).ok()
            }),
            model_manager: model_manager.clone(),
            oobe_manager,
            driver,
            playback,
            tts: {
                let edge = Arc::new(EdgeTtsProvider::new());
                let model_dir = std::env::var("NOMIFUN_VOICE_TTS_MODEL_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| model_manager.models_dir().join("tts"));
                let sherpa = Arc::new(SherpaOfflineTtsProvider::from_dir(model_dir));
                let prefer_offline = std::env::var("NOMIFUN_VOICE_TTS_OFFLINE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                if prefer_offline {
                    Arc::new(FallbackTtsProvider::new(sherpa, edge))
                } else {
                    Arc::new(FallbackTtsProvider::new(edge, sherpa))
                }
            },
            router,
            transcriber: None,
            capture_pipeline,
            acoustic_ready: Arc::new(AtomicBool::new(false)),
            self_check_active: Arc::new(AtomicBool::new(false)),
            self_check_cancel: Arc::new(AtomicBool::new(false)),
            self_check_id: Arc::new(std::sync::Mutex::new(None)),
            engine_check_active: Arc::new(AtomicBool::new(false)),
            engine_check_native_busy: Arc::new(AtomicBool::new(false)),
            native_admission: Arc::new(NativeAdmission::default()),
            native_degraded: Arc::new(NativeHealth::default()),
            native_recovery_scheduled: Arc::new(AtomicU64::new(0)),
            native_job_epoch: Arc::new(AtomicU64::new(0)),
            engine_check_control: Arc::new(std::sync::Mutex::new(None)),
            worker_epoch: Arc::new(AtomicU64::new(0)),
            engine_check_tx: std::sync::Mutex::new(Some(engine_check_tx)),
            engine_check_rx: std::sync::Mutex::new(Some(engine_check_rx)),
            wake_admission_until_ms: Arc::new(AtomicU64::new(0)),
            call_admission_until_ms: Arc::new(AtomicU64::new(0)),
            event_tx,
            active_turn: Arc::new(std::sync::Mutex::new(None)),
            input_generation: Arc::new(AtomicU64::new(0)),
            pending_utterances: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
            mic_source,
            #[cfg(any(test, feature = "voice-skeleton"))]
            replay_capture: false,
            #[cfg(any(test, feature = "voice-skeleton"))]
            replay_barge_in: false,
        }
    }

    /// Test-only headless assembly: no microphone, speaker, device enumeration or
    /// hardware probe. Fixed owned PCM feeds the real capture pipeline; the
    /// worker still loads real KWS/VAD and the injected non-cooperative STT.
    #[cfg(feature = "native-fault-injection")]
    pub fn assemble_headless(
        model_manager: Arc<ModelManager>,
        oobe_manager: Arc<OobeManager>,
    ) -> Self {
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        tokio::spawn(async move {
            let mut phase = 0.0_f32;
            loop {
                let mut pcm = [0.0_f32; 160];
                for sample in &mut pcm {
                    *sample = (phase * std::f32::consts::TAU).sin() * 0.02;
                    phase = (phase + 440.0 / 16_000.0) % 1.0;
                }
                producer.push(&pcm);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
        let headless_transcriber: Arc<dyn SttCallback> =
            if std::env::var("NOMIFUN_VOICE_FAULT_BLOCK_ASR").ok().as_deref() == Some("1") {
                Arc::new(FaultBlockingStt)
            } else {
                Arc::new(nomifun_audio::stt::NullSttCallback)
            };
        let (mut runtime, _) = Self::from_capture(
            model_manager,
            oobe_manager,
            Arc::new(Mutex::new(capture)),
            headless_transcriber,
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.replay_capture = false;
        runtime.replay_barge_in = false;
        runtime.acoustic_ready.store(false, Ordering::Release);
        runtime
    }

    /// Assemble the same worker with an owned capture source and STT/router dependencies.
    /// Replay consumers feed the supplied pipeline rather than a disconnected producer.
    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn from_capture(
        model_manager: Arc<ModelManager>,
        oobe_manager: Arc<OobeManager>,
        capture_pipeline: Arc<Mutex<VoiceCapturePipeline>>,
        transcriber: Arc<dyn SttCallback>,
        router: Arc<dyn VoiceRouter>,
    ) -> (Self, tokio::sync::mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let event_tx = Arc::new(VoiceEventHub::new());
        let (engine_check_tx, engine_check_rx) = mpsc::channel(1);
        (
            Self {
                #[cfg(test)]
                confirmation_speech_end_gate: None,
                #[cfg(any(test, feature = "voice-skeleton"))]
                ordinary_speech_end_gate: None,
                pending_confirmation:Arc::new(std::sync::Mutex::new(None)),
                announced_tasks: Mutex::new(std::collections::VecDeque::new()),
                task_client: None,
                model_manager: model_manager.clone(),
                oobe_manager,
                driver: Arc::new(Mutex::new(VoiceSessionDriver::new())),
                playback: Arc::new(PlaybackPipeline::new(Arc::new(ChannelAudioSink::new(tx)))),
                tts: Arc::new(nomifun_voice_terminal::tts::NullTts::new()),
                router,
                transcriber: Some(transcriber),
                capture_pipeline,
                event_tx,
                active_turn: Arc::new(std::sync::Mutex::new(None)),
                input_generation: Arc::new(AtomicU64::new(0)),
                pending_utterances: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
                mic_source: None,
                acoustic_ready: Arc::new(AtomicBool::new(true)),
                self_check_active: Arc::new(AtomicBool::new(false)),
                self_check_cancel: Arc::new(AtomicBool::new(false)),
                self_check_id: Arc::new(std::sync::Mutex::new(None)),
                engine_check_active: Arc::new(AtomicBool::new(false)),
                engine_check_native_busy: Arc::new(AtomicBool::new(false)),
                native_admission: Arc::new(NativeAdmission::default()),
                native_degraded: Arc::new(NativeHealth::default()),
                native_recovery_scheduled: Arc::new(AtomicU64::new(0)),
                native_job_epoch: Arc::new(AtomicU64::new(0)),
                engine_check_control: Arc::new(std::sync::Mutex::new(None)),
                worker_epoch: Arc::new(AtomicU64::new(0)),
                engine_check_tx: std::sync::Mutex::new(Some(engine_check_tx)),
                engine_check_rx: std::sync::Mutex::new(Some(engine_check_rx)),
                wake_admission_until_ms: Arc::new(AtomicU64::new(u64::MAX)),
                call_admission_until_ms: Arc::new(AtomicU64::new(u64::MAX)),
                replay_capture: true,
                replay_barge_in: false,
            },
            rx,
        )
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn with_test_playback(
        mut self,
        playback: Arc<PlaybackPipeline>,
        tts: Arc<dyn TtsProvider>,
    ) -> Self {
        self.playback = playback;
        self.tts = tts;
        self
    }

    /// Controlled echo-free PCM only; never grants production AEC readiness.
    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn with_test_barge_in(mut self) -> Self {
        self.replay_barge_in = true;
        self
    }

    pub fn event_sender(&self) -> tokio::sync::broadcast::Sender<VoiceCompanionEvent> {
        self.event_tx.sender()
    }

    pub fn with_tasks(mut self, client: Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>) -> Self {
        self.task_client = Some(client);
        self
    }

    pub fn update_task_status(&self, task: Option<nomifun_api_types::voice::VoiceTaskSnapshot>) {
        self.event_tx.update_task(task);
    }

    pub fn update_task_status_versioned(
        &self,
        task: Option<nomifun_api_types::voice::VoiceTaskSnapshot>,
        instance_id: Option<&str>,
        seq: Option<u64>,
    ) {
        let is_terminal = task.as_ref().is_some_and(|t| {
            matches!(t.status, nomifun_api_types::voice::VoiceTaskStatus::Completed | nomifun_api_types::voice::VoiceTaskStatus::Failed)
        });
        let expected = task.as_ref().map(|task| {
            (
                task.command_id.clone(),
                task.session_id.clone(),
                task.acoustic_turn_id.clone(),
            )
        });
        let accepted = self.event_tx.update_task_versioned(task, instance_id, seq);
        if is_terminal && accepted {
            let Some((command_id, session_id, turn_id)) = expected else { return; };
            let driver = self.driver.clone();
            let event_tx = self.event_tx.clone();
            let playback = self.playback.clone();
            let generation = playback.generation_id();
            tokio::spawn(async move {
                let snapshot = event_tx.snapshot();
                let still_owned = snapshot.task.as_ref().is_some_and(|task| {
                    task.command_id == command_id
                        && task.session_id == session_id
                        && task.acoustic_turn_id == turn_id
                })
                    && snapshot.session_id.as_ref().is_none_or(|value| value == &session_id)
                    && snapshot.turn_id.as_ref().is_none_or(|value| value == &turn_id);
                if !still_owned || playback.generation_id() != generation {
                    return;
                }
                if playback.generation_id() != generation
                    || event_tx.snapshot().task.as_ref().is_none_or(|task| {
                        task.command_id != command_id
                            || task.session_id != session_id
                            || task.acoustic_turn_id != turn_id
                    })
                {
                    return;
                }
                // When a task is interrupted or cancelled, clean up DispatchingTask so voice can recover
                let is_failure = event_tx.snapshot().task.as_ref().is_some_and(|t| {
                    t.status == nomifun_api_types::voice::VoiceTaskStatus::Failed
                        || t.error_code.is_some()
                });
                if is_failure {
                    let mut d = driver.lock().await;
                    if d.current_state() == AcousticState::DispatchingTask {
                        let _ = d.handle_event(AcousticEvent::TaskAccepted);
                        let _ = d.handle_event(AcousticEvent::PlaybackDone);
                        if d.current_state() == AcousticState::Idle {
                            let _ = d.handle_event(AcousticEvent::WakeHit);
                            if d.begin_listening_window() {
                                let _ = event_tx.send(VoiceCompanionEvent::Listening);
                                let _ = event_tx.send(VoiceCompanionEvent::ListeningReady { timeout_secs: 180 });
                            }
                        }
                    }
                }
            });
        }
    }

    pub async fn announce_task_result(&self, request: nomifun_api_types::voice::VoiceTaskResultAnnouncement) -> Result<(), &'static str> {
        if request.text.trim().is_empty() || request.text.len()>16_384 || request.command_id.is_empty() || request.command_id.len()>128 {return Err("task_result_invalid");}
        let notification_id = request.notification_id.clone().unwrap_or_else(|| request.command_id.clone());
        let attempt = request.attempt_id.clone().unwrap_or_else(|| notification_id.clone());
        let mut announced = self.announced_tasks.lock().await;

        // 1. Exact attempt deduplication
        if let Some(existing) = announced.iter().find(|r| r.attempt_id == attempt) {
            if existing.scope == request.scope && existing.text == request.text {
                return Ok(());
            } else {
                return Err("task_result_conflict");
            }
        }

        // 2. Logical notification checks
        if let Some(existing) = announced.iter().find(|r| r.notification_id == notification_id) {
            if existing.text != request.text {
                return Err("task_result_conflict");
            }
            if existing.status == "started" {
                return Err("voice_busy");
            }
        }

        let mut driver = self.driver.lock().await;
        let snapshot = self.snapshot();
        if !request.scope.matches(&snapshot) {return Err("task_scope_changed");}
        if snapshot.ended_reason==Some(VoiceSessionEndReason::UserStop)
            || snapshot.audio.as_ref().is_some_and(|audio|audio.speech_detected)
            || !self.pending_utterances.lock().unwrap_or_else(|e|e.into_inner()).is_empty()
            || !driver.yield_for_notification() {return Err("voice_busy");}
        driver.handle_event(AcousticEvent::TaskSummaryReady).map_err(|_| "voice_busy")?;
        let generation = self.playback.generation_id();
        announced.push_back(AnnouncementRecord {
            command_id: request.command_id.clone(),
            notification_id: notification_id.clone(),
            attempt_id: attempt.clone(),
            scope: request.scope.clone(),
            text: request.text.clone(),
            status: "started".into(),
            reason_code: None,
        });
        if announced.len()>64 {announced.pop_front();}
        let _ = self.event_tx.send(VoiceCompanionEvent::TaskResultReady {
            command_id: request.command_id,
            notification_id: request.notification_id,
            attempt_id: request.attempt_id,
            goal: request.goal,
            text: request.text.clone(),
        });
        let playback_snapshot = self.event_tx.snapshot();
        let playback_scope = VoiceSessionControlRequest {
            instance_id: playback_snapshot.instance_id,
            session_id: playback_snapshot.session_id,
            turn_id: playback_snapshot.turn_id,
        };
        drop(driver);
        drop(announced);
        voice_feedback::play(&self.playback, &self.event_tx, VoiceCueKind::Recognized, generation).await;
        let play_result = Self::play_response(&request.text,&self.driver,&self.playback,&self.event_tx,&self.tts,generation,&playback_scope).await;
        let mut announced = self.announced_tasks.lock().await;
        if let Some(record) = announced.iter_mut().find(|r| r.attempt_id == attempt) {
            match play_result {
                Ok(ReplyPlayback::Played) => {
                    record.status = "played".into();
                    record.reason_code = None;
                }
                Ok(ReplyPlayback::Interrupted) => {
                    record.status = "interrupted".into();
                    record.reason_code = Some("playback_interrupted".into());
                }
                Ok(ReplyPlayback::Muted) => {
                    record.status = "muted".into();
                    record.reason_code = Some("tts_muted".into());
                }
                Err(code) => {
                    record.status = "failed".into();
                    record.reason_code = Some(code.into());
                }
            }
        }
        Ok(())
    }

    pub async fn notification_status(&self, attempt: &str) -> Option<nomifun_api_types::voice::VoicePlaybackReceipt> {
        let announced = self.announced_tasks.lock().await;
        announced
            .iter()
            .rev()
            .find(|r| r.attempt_id == attempt || r.notification_id == attempt || r.command_id == attempt)
            .map(|r| nomifun_api_types::voice::VoicePlaybackReceipt {
                attempt_id: r.attempt_id.clone(),
                status: r.status.clone(),
                reason_code: r.reason_code.clone(),
            })
    }

    pub fn set_admission(&self, scope: &str, allowed: bool, expires_at_ms: i64) {
        let value = if allowed {
            u64::try_from(expires_at_ms.max(0)).unwrap_or(0)
        } else {
            0
        };
        match scope {
            "call" => self.call_admission_until_ms.store(value, Ordering::Release),
            _ => self.wake_admission_until_ms.store(value, Ordering::Release),
        }
    }

    pub fn set_wake_admission(&self, allowed: bool, expires_at_ms: i64) {
        self.set_admission("wake", allowed, expires_at_ms);
    }

    fn admission_valid(&self, scope: &str) -> bool {
        let now = nomifun_common::now_ms().max(0) as u64;
        let until = match scope {
            "call" => self.call_admission_until_ms.load(Ordering::Acquire),
            _ => self.wake_admission_until_ms.load(Ordering::Acquire),
        };
        now < until
    }

    /// Explicit current-instance inference check for KWS/VAD/ASR. The work is
    /// scheduled onto the live capture worker so a transient diagnostic loader
    /// cannot stand in for the engines that actually consume capture.
    pub async fn self_check_engines(
        &self,
    ) -> nomifun_api_types::voice::VoiceEngineSelfCheckResponse {
        self.self_check_engines_with_request(None, std::time::Duration::from_secs(45))
            .await
    }

    pub async fn self_check_engines_with_request(
        &self,
        request_id: Option<String>,
        timeout: std::time::Duration,
    ) -> nomifun_api_types::voice::VoiceEngineSelfCheckResponse {
        use nomifun_api_types::voice::VoiceEngineSelfCheckResponse;
        let request_id = request_id.unwrap_or_else(nomifun_common::generate_id);
        let deadline = std::time::Instant::now() + timeout;
        let snapshot = self.snapshot();
        let instance_id = snapshot.instance_id.clone();
        let worker_epoch = self.worker_epoch.load(Ordering::Acquire);
        let model_revision = self.model_manager.identity_revision();
        let failure = |reason: &str| VoiceEngineSelfCheckResponse {
            kws_ok: false,
            vad_ok: false,
            asr_ok: false,
            fixtures_available: false,
            reason: Some(reason.into()),
            kws_hits: 0,
            asr_text: None,
            request_id: request_id.clone(),
            instance_id: instance_id.clone(),
            worker_epoch,
            model_revision: model_revision.clone(),
            fixtures_source: None,
            fixtures_online: false,
            fixtures_generated_at_ms: None,
        };
        let session_active = snapshot.session_id.is_some()
            && snapshot.ended_reason.is_none()
            && snapshot.phase != nomifun_api_types::voice::VoiceSessionPhase::Idle;
        if session_active {
            return failure("voice_busy");
        }
        if self.self_check_active.load(Ordering::Acquire) {
            return failure("self_check_in_progress");
        }
        if self.engine_check_native_busy.load(Ordering::Acquire)
            || self.native_admission.has_pending()
        {
            return failure("engine_check_native_busy");
        }
        if self
            .engine_check_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return failure("engine_check_in_progress");
        }
        let epoch = self
            .worker_epoch
            .load(Ordering::Acquire)
            .wrapping_mul(1_000_000)
            .saturating_add(nomifun_common::now_ms().max(0) as u64);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_notify = Arc::new(Notify::new());
        *self
            .engine_check_control
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(EngineCheckControl {
            id: request_id.clone(),
            epoch,
            cancel: cancel.clone(),
            cancel_notify: cancel_notify.clone(),
        });
        let release = || {
            *self
                .engine_check_control
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = None;
            self.engine_check_active.store(false, Ordering::Release);
        };

        if worker_epoch == 0 {
            release();
            return failure("worker_not_ready");
        }
        let Some(tx) = self
            .engine_check_tx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        else {
            release();
            return failure("worker_unavailable");
        };
        let fixtures = match crate::self_check_audio::load_or_generate_until(
            &model_revision,
            self.tts.as_ref(),
            cancel.as_ref(),
            cancel_notify.as_ref(),
            deadline,
        )
        .await
        {
            Ok(fixtures) => fixtures,
            Err(reason) => {
                release();
                let mut response = failure(&reason);
                response.fixtures_source = Some("unavailable".into());
                return response;
            }
        };
        let fixtures_source = fixtures.source.clone();
        let fixtures_online = fixtures.online;
        let fixtures_generated_at_ms = fixtures.generated_at_ms;
        if let Some(reason) = if cancel.load(Ordering::Acquire) {
            Some("engine_check_cancelled")
        } else if std::time::Instant::now() >= deadline {
            Some("engine_check_timeout")
        } else {
            None
        } {
            release();
            let mut response = failure(reason);
            response.fixtures_available = true;
            response.fixtures_source = Some(fixtures_source);
            response.fixtures_online = fixtures_online;
            response.fixtures_generated_at_ms = fixtures_generated_at_ms;
            return response;
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        let command = WorkerEngineCheck {
            request_id: request_id.clone(),
            deadline,
            cancel: cancel.clone(),
            cancel_notify: cancel_notify.clone(),
            native_busy: self.engine_check_native_busy.clone(),
            native_job_epoch: self.native_job_epoch.clone(),
            fixtures,
            reply: reply_tx,
        };
        if tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tx.send(command),
        )
        .await
        .is_err()
        {
            release();
            return failure("worker_unavailable");
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let result = match tokio::time::timeout(remaining, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("worker_dropped_check".to_string()),
            Err(_) => {
                cancel.store(true, Ordering::Release);
                cancel_notify.notify_waiters();
                Err("engine_check_timeout".to_string())
            }
        };
        let current_epoch = self
            .engine_check_control
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|control| control.id == request_id && control.epoch == epoch);
        let released = {
            let mut control = self
                .engine_check_control
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if control.as_ref().is_some_and(|value| value.id == request_id) {
                *control = None;
            }
            self.engine_check_active.store(false, Ordering::Release);
            true
        };
        debug_assert!(released);
        if !current_epoch {
            let mut response = failure("engine_check_superseded");
            response.fixtures_available = true;
            response.fixtures_source = Some(fixtures_source);
            response.fixtures_online = fixtures_online;
            response.fixtures_generated_at_ms = fixtures_generated_at_ms;
            return response;
        }
        if self.engine_check_native_busy.load(Ordering::Acquire) {
            self.schedule_supervised_native_recovery();
        }
        match result {
            Ok(result) if !cancel.load(Ordering::Acquire) => VoiceEngineSelfCheckResponse {
                kws_ok: result.kws_ok,
                vad_ok: result.vad_ok,
                asr_ok: result.asr_ok,
                fixtures_available: true,
                reason: if result.kws_ok && result.vad_ok && result.asr_ok {
                    None
                } else {
                    Some("engine_inference_failed".into())
                },
                kws_hits: result.kws_hits,
                asr_text: result.asr_text,
                request_id,
                instance_id,
                worker_epoch,
                model_revision,
                fixtures_source: Some(fixtures_source),
                fixtures_online,
                fixtures_generated_at_ms,
            },
            Ok(_) => {
                let mut response = failure("engine_check_cancelled");
                response.fixtures_available = true;
                response.fixtures_source = Some(fixtures_source);
                response.fixtures_online = fixtures_online;
                response.fixtures_generated_at_ms = fixtures_generated_at_ms;
                response
            }
            Err(reason) => {
                let mut response = failure(&reason);
                response.fixtures_available = true;
                response.fixtures_source = Some(fixtures_source);
                response.fixtures_online = fixtures_online;
                response.fixtures_generated_at_ms = fixtures_generated_at_ms;
                response
            }
        }
    }

    fn schedule_supervised_native_recovery(&self) {
        if std::env::var("VOICE_COMPANION_SUPERVISED").ok().as_deref() != Some("1") {
            return;
        }
        let job_epoch = self.native_job_epoch.clone();
        let job = job_epoch.load(Ordering::Acquire);
        if job == 0 || self.native_recovery_scheduled.swap(job, Ordering::AcqRel) == job {
            return;
        }
        let busy = self.engine_check_native_busy.clone();
        let scheduled = self.native_recovery_scheduled.clone();
        let grace_ms = std::env::var("VOICE_COMPANION_NATIVE_STUCK_GRACE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5_000)
            .clamp(250, 30_000);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(grace_ms)).await;
            if busy.load(Ordering::Acquire) && job_epoch.load(Ordering::Acquire) == job {
                tracing::error!(
                    grace_ms,
                    "voice_engine_native_stuck_exiting_for_supervised_restart"
                );
                std::process::exit(75);
            }
            let _ = scheduled.compare_exchange(job, 0, Ordering::AcqRel, Ordering::Acquire);
        });
    }

    pub async fn cancel_engine_check(&self, request_id: &str) -> bool {
        let control = self
            .engine_check_control
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        match control {
            Some(control) if control.id == request_id => {
                control.cancel.store(true, Ordering::Release);
                control.cancel_notify.notify_waiters();
                true
            }
            _ => false,
        }
    }    /// Cancels the matching explicit TTS self-check and interrupts its output.
    pub async fn cancel_self_check(&self, check_id: &str) -> bool {
        let matches = self
            .self_check_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_deref()
            == Some(check_id);
        if !matches {
            return false;
        }
        self.self_check_cancel.store(true, Ordering::Release);
        let _ = self.playback.interrupt().await;
        true
    }
    async fn worker_engine_check(
        kws: &NativeKws,
        vad: &NativeVad,
        transcriber: &Option<Arc<dyn SttCallback>>,
        fixtures: &SelfCheckAudio,
        cancel: &AtomicBool,
        cancel_notify: &Notify,
        native_busy: &Arc<AtomicBool>,
        native_degraded: &Arc<NativeHealth>,
        native_job_epoch: &Arc<AtomicU64>,
        deadline: std::time::Instant,
    ) -> Result<WorkerEngineCheckResult, String> {
        let stopped = || {
            cancel.load(Ordering::Acquire)
                || std::time::Instant::now() >= deadline
        };
        if stopped() {
            return Err("engine_check_cancelled".into());
        }
        kws.reset_until(deadline, cancel, cancel_notify, false).await?;
        let mut wake_hits = 0u32;
        for chunk in fixtures.wake.chunks(320) {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if kws
                .process_pcm_chunk_until(chunk, deadline, cancel, cancel_notify, false)
                .await?
                .is_some()
            {
                wake_hits = wake_hits.saturating_add(1);
            }
        }
        let mut negative_hits = 0u32;
        for pcm in [&fixtures.command, &fixtures.non_wake] {
            kws.reset_until(deadline, cancel, cancel_notify, false).await?;
            for chunk in pcm.chunks(320) {
                if stopped() {
                    return Err("engine_check_cancelled".into());
                }
                if kws
                    .process_pcm_chunk_until(chunk, deadline, cancel, cancel_notify, false)
                    .await?
                    .is_some()
                {
                    negative_hits = negative_hits.saturating_add(1);
                }
            }
        }
        let kws_ok = kws.is_initialized() && wake_hits > 0 && negative_hits == 0;

        vad.reset_until(deadline, cancel, cancel_notify, false).await?;
        let silence = vec![0.0f32; 512];
        let mut silent_start = true;
        for _ in 0..30 {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if vad
                .process_frame_until(&silence, deadline, cancel, cancel_notify, false)
                .await?
            {
                silent_start = false;
                break;
            }
        }
        let mut speech = false;
        for frame in fixtures.command.chunks(512) {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if frame.len() < 512 {
                break;
            }
            if vad
                .process_frame_until(frame, deadline, cancel, cancel_notify, false)
                .await?
            {
                speech = true;
                break;
            }
        }
        let mut ended = false;
        for _ in 0..120 {
            if stopped() {
                return Err("engine_check_cancelled".into());
            }
            if !vad
                .process_frame_until(&silence, deadline, cancel, cancel_notify, false)
                .await?
            {
                ended = true;
                break;
            }
        }
        let vad_ok = silent_start && speech && ended;
        vad.reset_until(deadline, cancel, cancel_notify, false).await?;

        let (asr_ok, asr_text) = match transcriber {
            Some(asr) => {
                let zh = Self::transcribe_cancelable(
                    asr.clone(),
                    fixtures.command.clone(),
                    cancel,
                    cancel_notify,
                    native_busy,
                    native_degraded,
                    native_job_epoch,
                    deadline,
                    true,
                    None,
                    kws.runner.admission.as_ref(),
                    #[cfg(feature = "native-fault-injection")]
                    None,
                )
                .await?
                .unwrap_or_default();
                if stopped() {
                    return Err("engine_check_cancelled".into());
                }
                let en = Self::transcribe_cancelable(
                    asr.clone(),
                    fixtures.non_wake.clone(),
                    cancel,
                    cancel_notify,
                    native_busy,
                    native_degraded,
                    native_job_epoch,
                    deadline,
                    true,
                    None,
                    kws.runner.admission.as_ref(),
                    #[cfg(feature = "native-fault-injection")]
                    None,
                )
                .await?
                .unwrap_or_default();
                let lowered = en.to_lowercase();
                (
                    zh.contains("天气") && lowered.contains("hello") && lowered.contains("friend"),
                    Some(format!("zh={zh};en={en}")),
                )
            }
            None => (false, None),
        };
        if stopped() {
            return Err("engine_check_cancelled".into());
        }
        Ok(WorkerEngineCheckResult {
            kws_ok,
            vad_ok,
            asr_ok,
            kws_hits: wake_hits,
            asr_text,
        })
    }

    async fn load_worker_asr(
        transcriber: &mut Option<Arc<dyn SttCallback>>,
        models: &Arc<ModelManager>,
        cancel: &AtomicBool,
        notify: &Notify,
        native_busy: &Arc<AtomicBool>,
        native_degraded: &Arc<NativeHealth>,
        native_job_epoch: &Arc<AtomicU64>,
        native_admission: &Arc<NativeAdmission>,
        deadline: std::time::Instant,
    ) -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("engine_check_cancelled".into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("engine_check_timeout".into());
        }
        let dir = models.models_dir().to_path_buf();
        let runner = NativeCallRunner::with_admission(
            native_busy.clone(),
            native_job_epoch.clone(),
            native_degraded.clone(),
            native_admission.clone(),
            false,
        );
        let loaded = runner
            .load_until(
                deadline,
                cancel,
                notify,
                false,
                move || nomifun_audio::stt::try_load_sherpa(Some(&dir)),
            )
            .await?;
        match loaded {
            Ok(Some(stt)) => *transcriber = Some(Arc::new(stt)),
            Ok(None) => {}
            Err(error) => return Err(format!("worker_asr_load_failed:{error}")),
        }
        Ok(())
    }

    async fn transcribe_with_lease(
        asr: Arc<dyn SttCallback>,
        pcm: Vec<f32>,
        native_busy: &Arc<AtomicBool>,
        native_degraded: &Arc<NativeHealth>,
        native_job_epoch: &Arc<AtomicU64>,
        budget: std::time::Duration,
        reservation: Option<NativeAdmissionGuard>,
        admission: Option<&Arc<NativeAdmission>>,
        #[cfg(feature = "native-fault-injection")] context: Option<&NativeCallContext>,
    ) -> Result<Option<String>, String> {
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        Self::transcribe_cancelable(
            asr,
            pcm,
            &cancel,
            &notify,
            native_busy,
            native_degraded,
            native_job_epoch,
            std::time::Instant::now() + budget,
            true,
            reservation,
            admission,
            #[cfg(feature = "native-fault-injection")]
            context,
        )
        .await
    }

    async fn transcribe_cancelable(
        asr: Arc<dyn SttCallback>,
        pcm: Vec<f32>,
        cancel: &AtomicBool,
        notify: &Notify,
        native_busy: &Arc<AtomicBool>,
        native_degraded: &Arc<NativeHealth>,
        native_job_epoch: &Arc<AtomicU64>,
        deadline: std::time::Instant,
        watchdog: bool,
        reservation: Option<NativeAdmissionGuard>,
        admission: Option<&Arc<NativeAdmission>>,
        #[cfg(feature = "native-fault-injection")] context: Option<&NativeCallContext>,
    ) -> Result<Option<String>, String> {
        if cancel.load(Ordering::Acquire) {
            return Err("engine_check_cancelled".into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("engine_check_timeout".into());
        }
        let priority = reservation.is_some();
        let target_admission = reservation
            .as_ref()
            .map(|r| r.admission.clone())
            .or_else(|| admission.cloned());
        let runner = match target_admission {
            Some(adm) => NativeCallRunner::with_admission(
                native_busy.clone(),
                native_job_epoch.clone(),
                native_degraded.clone(),
                adm,
                priority,
            ),
            None => NativeCallRunner::with_health(
                native_busy.clone(),
                native_job_epoch.clone(),
                native_degraded.clone(),
            ),
        };
        let guard = runner
            .acquire(deadline, cancel, notify, priority)
            .await?;
        drop(reservation);
        if cancel.load(Ordering::Acquire) {
            return Err("engine_check_cancelled".into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("engine_check_timeout".into());
        }
        let job = guard.job;
        #[cfg(feature = "native-fault-injection")]
        if let Some(context) = context {
            bind_fault_job(job, context);
        }
        let _watchdog = NativeJobWatchdog::new(&runner, job, watchdog);
        #[cfg(feature = "native-fault-injection")]
        let asr: Arc<dyn SttCallback> = Arc::new(BoundStt {
            inner: asr,
            job,
            context: context.cloned(),
            entered: AtomicBool::new(false),
        });
        let mut task = tokio::spawn(async move {
            let text = asr.transcribe_checked(AudioChannel::Mic, pcm, 16_000).await;
            (guard, text)
        });
        let result = tokio::select! {
            _ = notify.notified() => Err("engine_check_cancelled".into()),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                Err("engine_check_timeout".into())
            }
            result = &mut task => match result {
                Ok((guard, Ok(text))) => {
                    native_degraded.publish(job, false);
                    drop(guard);
                    Ok(text)
                }
                Ok((guard, Err(_))) => {
                    drop(guard);
                    Err("worker_asr_failed".into())
                }
                Err(error) => Err(format!("worker_asr_join_failed:{error}")),
            },
        };
        if let Err(reason) = &result
            && native_error_requires_degraded(reason)
        {
            native_degraded.publish(job, true);
        }
        result
    }

    pub fn snapshot(&self) -> VoiceSessionSnapshot {
        self.event_tx.snapshot()
    }

    /// Explicit user-triggered TTS check. It uses the production provider and
    /// fixed, non-private bilingual sentences; no user text or session context.
    pub async fn self_check_tts(
        &self,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        self.self_check_tts_inner(None, None, None).await
    }

    pub async fn self_check_tts_with_id(
        &self,
        check_id: &str,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        self.self_check_tts_inner(Some(check_id), None, None).await
    }

    pub async fn self_check_tts_with_options(
        &self,
        check_id: Option<&str>,
        locale: Option<&str>,
        stage: Option<&str>,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        self.self_check_tts_inner(check_id, locale, stage).await
    }

    async fn self_check_tts_inner(
        &self,
        check_id: Option<&str>,
        locale: Option<&str>,
        stage: Option<&str>,
    ) -> Result<nomifun_api_types::voice::VoiceTtsSelfCheckResponse, String> {
        if self.engine_check_active.load(Ordering::Acquire) {
            return Err("engine_check_in_progress".into());
        }
        if self.self_check_active.swap(true, Ordering::AcqRel) {
            return Err("self_check_in_progress".into());
        }
        let _self_check_guard = SelfCheckGuard {
            active: self.self_check_active.clone(),
            cancel: self.self_check_cancel.clone(),
            id: self.self_check_id.clone(),
        };
        self.self_check_cancel.store(false, Ordering::Release);
        *self
            .self_check_id
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = check_id.map(str::to_owned);
        let snapshot = self.snapshot();
        let active = snapshot.session_id.is_some()
            && snapshot.ended_reason.is_none()
            && snapshot.phase != nomifun_api_types::voice::VoiceSessionPhase::Idle;
        if active {
            return Err("voice_busy".into());
        }
        let is_en = nomifun_api_types::voice::is_english_locale(locale);
        let (zh_text, en_text) = match stage {
            Some("task") | Some("create_task") => ("创建语音任务中", "Creating voice task..."),
            Some("prepare") | Some("preparing") => ("正在准备语音通话。", "Preparing your voice call."),
            Some("connect") | Some("connecting") => ("正在连接语音通话。", "Connecting voice call."),
            Some("start") | Some("starting") => ("正在启动语音服务。", "Starting voice service."),
            _ => ("正在启动语音服务。", "Starting voice service."),
        };
        let cases = [
            ("zh-CN", "zh-CN-XiaoxiaoNeural", zh_text),
            ("en-US", "en-US-JennyNeural", en_text),
        ];
        let mut languages = Vec::new();
        let mut playback_ok = false;
        let mut first_error = None;
        let is_muted = !self.event_tx.feedback().speak_replies;
        for (language, voice, text) in cases {
            if self.self_check_cancel.load(Ordering::Acquire) {
                return Err("self_check_cancelled".into());
            }
            let play_this = if is_en { language == "en-US" } else { language == "zh-CN" };
            let result: Result<(u32, usize, u64, u16, u16), String> = async {
                let mut stream = self
                    .tts
                    .synthesize_stream(text, voice)
                    .await
                    .map_err(|error| error.to_string())?;
                let wav = tokio::time::timeout(
                    std::time::Duration::from_secs(25),
                    stream.recv(),
                )
                .await
                .map_err(|_| "tts_timeout".to_string())?
                .ok_or_else(|| "tts_no_audio".to_string())?;
                let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
                let (duration_ms, rms_milli, peak_milli) = validate_self_check_pcm(&pcm, rate)?;
                if self.self_check_cancel.load(Ordering::Acquire) {
                    return Err("self_check_cancelled".into());
                }
                if play_this {
                    if is_muted {
                        let silent_pcm = vec![0.0f32; pcm.len()];
                        let silent_wav = nomifun_audio::pcm_to_wav(&silent_pcm, rate);
                        self.playback
                            .play_wav_for_self_check(silent_wav)
                            .await
                            .map_err(|error| error.to_string())?;
                    } else {
                        self.playback
                            .play_wav_for_self_check(wav)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                    playback_ok = true;
                }
                Ok((rate, pcm.len(), duration_ms, rms_milli, peak_milli))
            }
            .await;
            match result {
                Ok((sample_rate, samples, duration_ms, rms_milli, peak_milli)) => {
                    languages.push(nomifun_api_types::voice::VoiceTtsSelfCheckLanguage {
                        language: language.into(),
                        voice: voice.into(),
                        ok: true,
                        sample_rate,
                        samples,
                        duration_ms,
                        rms_milli,
                        peak_milli,
                        error: None,
                    });
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error.clone());
                    }
                    languages.push(nomifun_api_types::voice::VoiceTtsSelfCheckLanguage {
                        language: language.into(),
                        voice: voice.into(),
                        ok: false,
                        sample_rate: 0,
                        samples: 0,
                        duration_ms: 0,
                        rms_milli: 0,
                        peak_milli: 0,
                        error: Some(error),
                    });
                }
            }
        }
        let diagnostic = self.tts.diagnostic("");
        let provider = diagnostic
            .as_ref()
            .map(|value| value.provider.clone())
            .unwrap_or_else(|| "unknown".into());
        let online = diagnostic.as_ref().is_some_and(|value| value.online);
        let ok = languages.iter().all(|language| language.ok);
        Ok(nomifun_api_types::voice::VoiceTtsSelfCheckResponse {
            ok,
            provider,
            online,
            playback_ok,
            languages,
            reason: if ok { None } else { first_error },
        })
    }

    pub fn feedback(&self) -> nomifun_api_types::voice::VoiceFeedbackConfig {
        self.event_tx.feedback()
    }

    pub fn update_feedback(
        &self,
        patch: nomifun_api_types::voice::VoiceFeedbackPatch,
    ) -> Result<(), &'static str> {
        let config = patch.apply(self.feedback());
        config.validate()?;
        self.event_tx.set_feedback(config);
        Ok(())
    }

    pub async fn preview_feedback(&self, kind: VoiceCueKind) -> Result<(), &'static str> {
        let scope = self.snapshot();
        if scope.phase != nomifun_api_types::voice::VoiceSessionPhase::Idle {
            return Err("voice_busy");
        }
        let config = self.feedback();
        if config.volume == 0 || config.mode == nomifun_api_types::voice::VoiceFeedbackMode::Off {
            return Err("feedback_muted");
        }
        let frame = voice_feedback::preview_frame(kind, config.volume);
        let generation = self.playback.generation_id();
        self.playback
            .play_optional_frame(
                frame,
                generation,
                || {
                    let now = self.snapshot();
                    now.phase == nomifun_api_types::voice::VoiceSessionPhase::Idle
                        && now.session_id == scope.session_id
                },
                || {},
            )
            .await
            .map_err(|_| "cue_output_failed")?
            .then_some(())
            .ok_or("voice_busy")
    }

    pub async fn stop_session(
        &self,
        request: &VoiceSessionControlRequest,
    ) -> Result<(), VoiceWakeError> {
        let mut driver = self.driver.lock().await;
        if !request.matches(&self.snapshot()) {
            return Err(VoiceWakeError::Busy);
        }
        driver.reset();
        self.pending_confirmation.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.input_generation.fetch_add(1, Ordering::SeqCst);
        self.playback
            .interrupt()
            .await
            .map_err(|_| VoiceWakeError::Busy)?;
        if let Some(turn) = self
            .active_turn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            turn.abort();
        }
        self.pending_utterances
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let generation = self.playback.generation_id();
        let _ = self.event_tx.send(VoiceCompanionEvent::SessionEnded {
            reason: VoiceSessionEndReason::UserStop,
        });
        drop(driver);
        voice_feedback::schedule(
            self.playback.clone(),
            self.event_tx.clone(),
            VoiceCueKind::Stopped,
            generation,
        );
        Ok(())
    }

    pub fn driver(&self) -> Arc<Mutex<VoiceSessionDriver>> {
        Arc::clone(&self.driver)
    }

    pub fn playback(&self) -> Arc<PlaybackPipeline> {
        Arc::clone(&self.playback)
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn enqueue_pending_utterance(&self, pcm: Vec<f32>, session_id: Option<String>) {
        let mut queue = self.pending_utterances.lock().unwrap_or_else(|e| e.into_inner());
        queue.push_back(PendingUtterance {
            pcm,
            session_id,
            generation_id: self.playback.generation_id(),
        });
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn pending_utterance_count(&self) -> usize {
        self.pending_utterances.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn router(&self) -> Arc<dyn VoiceRouter> {
        Arc::clone(&self.router)
    }

    pub fn capture_pipeline(&self) -> Arc<Mutex<VoiceCapturePipeline>> {
        Arc::clone(&self.capture_pipeline)
    }

    /// Check if device is paired and models are ready.
    pub async fn is_operational(&self) -> bool {
        let paired = self.oobe_manager.is_paired().await;
        let models_ready = self.models_ready().await;
        paired
            && models_ready
            && self.capture_available()
            && self.acoustic_ready.load(Ordering::Acquire)
    }

    fn capture_available(&self) -> bool {
        #[cfg(any(test, feature = "voice-skeleton"))]
        if self.replay_capture {
            return true;
        }
        self.mic_source.is_some()
    }

    async fn models_ready(&self) -> bool {
        #[cfg(any(test, feature = "voice-skeleton"))]
        if self.replay_capture {
            return true;
        }
        self.model_manager.get_status().await.state.is_ready()
    }

    /// Handle far-field wake event ("Hey Flowy").
    pub async fn handle_wake(&self) -> Result<(), VoiceWakeError> {
        self.handle_wake_keyword("Hey Flowy").await
    }

    /// Handle far-field wake event with specific matched keyword.
    #[cfg(any(test, feature = "voice-skeleton"))]
    pub async fn advance_time(&self, elapsed: std::time::Duration) {
        self.driver.lock().await.advance_time(elapsed);
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
    pub async fn expire_listening_deadline(&self) {
        self.driver.lock().await.expire_listening_deadline();
    }

    pub async fn handle_wake_keyword(&self, keyword: &str) -> Result<(), VoiceWakeError> {
        self.handle_wake_request(keyword, None).await
    }

    pub async fn handle_wake_request(&self, keyword: &str, request_id: Option<&str>) -> Result<(), VoiceWakeError> {
        self.handle_wake_request_with_locale(keyword, request_id, None).await
    }

    pub async fn handle_wake_request_with_locale(
        &self,
        keyword: &str,
        request_id: Option<&str>,
        locale: Option<&str>,
    ) -> Result<(), VoiceWakeError> {
        if !self.oobe_manager.is_paired().await {
            warn!("Wake received but device is not yet paired via OOBE");
            let _ = self.playback.play_earcon(EarconKind::Alert).await;
            return Err(VoiceWakeError::PairingRequired);
        }

        if !self.models_ready().await {
            warn!("Wake received but voice models are not yet installed or verified");
            let _ = self.playback.play_earcon(EarconKind::Alert).await;
            return Err(VoiceWakeError::ModelsUnavailable);
        }

        if self.self_check_active.load(Ordering::Acquire)
            || self.engine_check_active.load(Ordering::Acquire)
            || self.engine_check_native_busy.load(Ordering::Acquire)
        {
            return Err(VoiceWakeError::Busy);
        }
        if !self.admission_valid("call") {
            return Err(VoiceWakeError::Busy);
        }
        if !self.capture_available() {
            return Err(VoiceWakeError::MicrophoneUnavailable);
        }
        if !self.acoustic_ready.load(Ordering::Acquire) {
            return Err(VoiceWakeError::ModelsUnavailable);
        }

        Self::accept_wake(&self.driver, &self.playback, &self.event_tx, keyword, request_id, locale, true).await
    }

    async fn accept_wake(
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        events: &Arc<VoiceEventHub>,
        keyword: &str,
        request_id: Option<&str>,
        locale: Option<&str>,
        play_feedback: bool,
    ) -> Result<(), VoiceWakeError> {
        let mut d = driver.lock().await;
        let next = d
            .handle_event(AcousticEvent::WakeHit)
            .map_err(|_| VoiceWakeError::Busy)?;
        if play_feedback { d.pause_listening_for_feedback(); }
        playback
            .interrupt()
            .await
            .map_err(|_| VoiceWakeError::Busy)?;
        info!(state = ?next, "Wake accepted");
        let _ = events.send(VoiceCompanionEvent::WakeHit {
            keyword: keyword.to_string(),
            request_id: request_id.map(str::to_owned),
            locale: locale.map(str::to_owned),
        });
        let _ = events.send(VoiceCompanionEvent::Listening);
        if !play_feedback { return Ok(()); }
        drop(d);
        let driver = driver.clone();
        let playback = playback.clone();
        let events = events.clone();
        let generation = playback.generation_id();
        tokio::spawn(async move {
            voice_feedback::play(&playback, &events, VoiceCueKind::Wake, generation).await;
            let mut d = driver.lock().await;
            if playback.generation_id() == generation && d.begin_listening_window() {
                let _ = events.send(VoiceCompanionEvent::ListeningReady { timeout_secs: 180 });
            }
        });
        Ok(())
    }

    /// One playback/follow-up path for chat, controls and task results.
    async fn play_response(
        reply_text:&str,
        driver:&Arc<Mutex<VoiceSessionDriver>>,
        playback:&Arc<PlaybackPipeline>,
        event_tx:&Arc<VoiceEventHub>,
        tts:&Arc<dyn TtsProvider>,
        generation:u64,
        scope:&VoiceSessionControlRequest,
    ) -> Result<ReplyPlayback, &'static str> {
        let guard=driver.lock().await;
        let current = event_tx.snapshot();
        if playback.generation_id()!=generation || !scope.matches(&current) || current.ended_reason.is_some() {
            // A stale turn must not advance the current turn's FSM, even when
            // both happen to be in the same Speaking/Dispatching state.
            drop(guard);
            return Ok(ReplyPlayback::Interrupted);
        }
        info!("voice_response_playback_started");
        let _=event_tx.send_scoped(VoiceCompanionEvent::ResponseReady {text:reply_text.to_owned()}, scope);
        drop(guard);
        if !event_tx.feedback().speak_replies {
            Self::finish_muted_playback(driver, playback, event_tx, generation, scope).await;
            return Ok(ReplyPlayback::Muted);
        }
        let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSynthesizing {
            text: reply_text.to_string(),
        }, scope);
        let started = std::time::Instant::now();
        let is_en = nomifun_api_types::voice::is_english_locale(event_tx.snapshot().locale.as_deref());
        let voice_name = if is_en { "en-US-JennyNeural" } else { "" };
        let result = playback
            .speak_text_for_generation(tts.as_ref(), reply_text, voice_name, generation, || {
                Self::report_tts_source(event_tx, tts.as_ref(), scope);
                let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSpeaking {
                    text: reply_text.to_string(),
                }, scope);
            })
            .await;
        info!(elapsed_ms=started.elapsed().as_millis() as u64,ok=result.is_ok(),"voice_playback_finished");
        if playback.generation_id() != generation {
            let mut d = driver.lock().await;
            let current = event_tx.snapshot();
            if !scope.matches(&current) || current.ended_reason.is_some() {
                return Ok(ReplyPlayback::Interrupted);
            }
            if matches!(d.current_state(), AcousticState::SpeakingFast | AcousticState::SpeakingSummary | AcousticState::SpeakingControl) {
                let _ = d.handle_event(AcousticEvent::PlaybackDone);
                if d.current_state() == AcousticState::FollowUpListening && d.begin_listening_window() {
                    let _ = event_tx.send_scoped(VoiceCompanionEvent::Listening, scope);
                }
            }
            return Ok(ReplyPlayback::Interrupted);
        }
        Self::finish_response_playback(result,driver,playback,event_tx,generation,scope).await?;
        Ok(ReplyPlayback::Played)
    }

    async fn finish_muted_playback(
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &Arc<VoiceEventHub>,
        generation: u64,
        scope: &VoiceSessionControlRequest,
    ) {
        let mut d = driver.lock().await;
        let current = event_tx.snapshot();
        if playback.generation_id() != generation || !scope.matches(&current) || current.ended_reason.is_some() {
            return;
        }
        if d.current_state() == AcousticState::Deciding {
            let _ = d.handle_event(AcousticEvent::IntentChat);
        }
        let _ = d.handle_event(AcousticEvent::PlaybackDone);
        drop(d);
        let _ = event_tx.send_scoped(VoiceCompanionEvent::FollowUpStarted { text_only: true }, scope);
        voice_feedback::play(
            playback,
            event_tx,
            VoiceCueKind::FollowUp,
            generation,
        )
        .await;
        let mut d = driver.lock().await;
        let current = event_tx.snapshot();
        if playback.generation_id() != generation || !scope.matches(&current) || current.ended_reason.is_some() {
            return;
        }
        if d.current_state() == AcousticState::FollowUpListening && d.begin_listening_window() {
            let _ = event_tx.send_scoped(VoiceCompanionEvent::TextFollowUpWindow { timeout_secs: 180 }, scope);
        } else if d.current_state() != AcousticState::FollowUpListening {
            d.reset();
        }
    }

    async fn finish_response_playback(
        result: Result<(), AudioSinkError>,
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &Arc<VoiceEventHub>,
        generation: u64,
        scope: &VoiceSessionControlRequest,
    ) -> Result<(), &'static str> {
        let mut d2 = driver.lock().await;
        let current = event_tx.snapshot();
        if playback.generation_id() != generation || !scope.matches(&current) || current.ended_reason.is_some() {
            return Ok(());
        }
        if d2.current_state() == AcousticState::Deciding {
            let _ = d2.handle_event(AcousticEvent::IntentChat);
        }
        let _ = d2.handle_event(AcousticEvent::PlaybackDone);
        drop(d2);
        if let Err(err) = result {
            let code = match err {
                AudioSinkError::RateLimited { .. } => "tts_rate_limited",
                AudioSinkError::Synthesis(_) => "tts_failed",
                AudioSinkError::NoAudio => "tts_empty_audio",
                _ => "speaker_failed",
            };
            warn!(code, "voice_playback_failed");
            let _ = playback.interrupt().await;
            let fallback_generation = playback.generation_id();
            let guard = driver.lock().await;
            let current = event_tx.snapshot();
            if !scope.matches(&current) || current.ended_reason.is_some() {
                return Err(code);
            }
            drop(guard);
            let _ = event_tx.send_scoped(VoiceCompanionEvent::Error {
                stage: "playback".into(),
                code: code.into(),
            }, scope);
            let _ = event_tx.send_scoped(VoiceCompanionEvent::FollowUpStarted { text_only: true }, scope);
            voice_feedback::play(
                playback,
                event_tx,
                VoiceCueKind::Error,
                fallback_generation,
            )
            .await;
            let current = event_tx.snapshot();
            if playback.generation_id() != fallback_generation || !scope.matches(&current) || current.ended_reason.is_some() {
                return Err(code);
            }
            voice_feedback::play(
                playback,
                event_tx,
                VoiceCueKind::FollowUp,
                fallback_generation,
            )
            .await;
            let mut d = driver.lock().await;
            let current = event_tx.snapshot();
            if playback.generation_id() != fallback_generation || !scope.matches(&current) || current.ended_reason.is_some() {
                return Err(code);
            }
            if d.current_state() == AcousticState::FollowUpListening
                && d.begin_listening_window()
            {
                let _ = event_tx.send_scoped(VoiceCompanionEvent::TextFollowUpWindow { timeout_secs: 180 }, scope);
            } else if d.current_state() != AcousticState::FollowUpListening {
                d.reset();
            }
            Err(code)
        } else {
            let _ = event_tx.send_scoped(VoiceCompanionEvent::FollowUpStarted { text_only: false }, scope);
            voice_feedback::play(
                playback,
                event_tx,
                VoiceCueKind::FollowUp,
                generation,
            )
            .await;
            let mut d = driver.lock().await;
            let current = event_tx.snapshot();
            if playback.generation_id() != generation || !scope.matches(&current) || current.ended_reason.is_some() {
                return Ok(());
            }
            if d.current_state() == AcousticState::FollowUpListening
                && d.begin_listening_window()
            {
                let _ = event_tx.send_scoped(VoiceCompanionEvent::FollowUpWindow { timeout_secs: 180 }, scope);
            } else if d.current_state() != AcousticState::FollowUpListening {
                d.reset();
            }
            Ok(())
        }
    }

    async fn discard_utterance(
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        events: &Arc<VoiceEventHub>,
        generation: u64,
        scope: &VoiceSessionControlRequest,
    ) {
        let mut d = driver.lock().await;
        let current = events.snapshot();
        if playback.generation_id() != generation || !scope.matches(&current) || current.ended_reason.is_some() { return; }
        if d.handle_event(AcousticEvent::UtteranceIgnored) == Ok(AcousticState::Listening) {
            let _ = events.send_scoped(VoiceCompanionEvent::IntentDecided { intent: "Ignore".into(), reply: None, understood_request: None }, scope);
            let _ = events.send_scoped(VoiceCompanionEvent::Listening, scope);
            let _ = events.send_scoped(VoiceCompanionEvent::ListeningReady { timeout_secs: 180 }, scope);
        }
    }

    async fn report_asr_failure(
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        playback: &Arc<PlaybackPipeline>,
        events: &Arc<VoiceEventHub>,
        generation: u64,
        scope: &VoiceSessionControlRequest,
        code: &str,
    ) {
        let mut driver = driver.lock().await;
        let snapshot = events.snapshot();
        if playback.generation_id() != generation || !scope.matches(&snapshot) || snapshot.ended_reason.is_some() {
            return;
        }
        if driver.handle_event(AcousticEvent::UtteranceIgnored) == Ok(AcousticState::Listening) {
            // Remove the empty turn first: Ignore clears the previous error.
            let _ = events.send_scoped(VoiceCompanionEvent::IntentDecided { intent: "Ignore".into(), reply: None, understood_request: None }, scope);
            let _ = events.send_scoped(VoiceCompanionEvent::Error { stage: "asr".into(), code: code.into() }, scope);
            let _ = events.send_scoped(VoiceCompanionEvent::Listening, scope);
            let _ = events.send_scoped(VoiceCompanionEvent::ListeningReady { timeout_secs: 180 }, scope);
        }
    }

    /// Internal helper to run ASR transcription, Qwen intent routing, TTS synthesis and FSM dispatch.
    async fn process_utterance(
        utterance_pcm: &[f32],
        driver: &Arc<Mutex<VoiceSessionDriver>>,
        router: &Arc<dyn VoiceRouter>,
        transcriber: &Option<Arc<dyn SttCallback>>,
        playback: &Arc<PlaybackPipeline>,
        event_tx: &Arc<VoiceEventHub>,
        tts: &Arc<dyn TtsProvider>,
        task_client: &Option<Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>>,
        generation: u64,
        scope: VoiceSessionSnapshot,
        pending_utterances: &Arc<std::sync::Mutex<std::collections::VecDeque<PendingUtterance>>>,
        native_busy: &Arc<AtomicBool>,
        native_degraded: &Arc<NativeHealth>,
        native_job_epoch: &Arc<AtomicU64>,
        admission: NativeAdmissionGuard,
        _native_request_id: &str,
        recognized_text: Option<String>,
    ) {
        let control_scope = VoiceSessionControlRequest { instance_id: scope.instance_id.clone(), session_id: scope.session_id.clone(), turn_id: scope.turn_id.clone() };
        let current = event_tx.snapshot();
        if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
            return;
        }
        let started = std::time::Instant::now();
        let mut task_context:Result<Option<nomifun_api_types::voice::VoiceTaskSnapshot>,String>=Ok(None);
        info!(session_id = ?scope.session_id, turn_id = ?scope.turn_id, samples = utterance_pcm.len(), sample_rate = 16000, "voice_asr_started");
        let already_recognized = recognized_text.is_some();
        let text_opt = if let Some(text) = recognized_text {
            drop(admission);
            Some(text)
        } else if let Some(stt) = transcriber {
            #[cfg(feature = "native-fault-injection")]
            let call_context = NativeCallContext {
                request_id: _native_request_id.to_owned(),
                worker_epoch: event_tx.worker_epoch(),
                instance_id: scope.instance_id.clone(),
            };
            match Self::transcribe_with_lease(
                stt.clone(),
                utterance_pcm.to_vec(),
                native_busy,
                native_degraded,
                native_job_epoch,
                std::time::Duration::from_secs(30),
                Some(admission),
                None,
                #[cfg(feature = "native-fault-injection")]
                Some(&call_context),
            )
            .await
            {
                Ok(text) => text,
                Err(reason) => {
                    warn!(reason, "voice_ordinary_asr_failed");
                    if reason != "engine_check_cancelled" {
                        Self::report_asr_failure(driver, playback, event_tx, generation, &control_scope, asr_failure_code(&reason)).await;
                    }
                    return;
                }
            }
        } else {
            drop(admission);
            Self::report_asr_failure(driver, playback, event_tx, generation, &control_scope, "asr_unavailable").await;
            return;
        }
        .filter(|text| !text.trim().is_empty());
        let current = event_tx.snapshot();
        if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
            return;
        }
        info!(
            session_id = ?scope.session_id, turn_id = ?scope.turn_id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            text_chars = text_opt.as_ref().map_or(0, |text| text.chars().count()),
            "voice_asr_finished"
        );
        if text_opt.as_ref().is_none_or(|text| text.trim().is_empty()) {
            warn!("voice_asr_empty_result");
            Self::report_asr_failure(driver, playback, event_tx, generation, &control_scope, "asr_empty_result").await;
            return;
        }
        let clean_input = if already_recognized {
            text_opt.clone().unwrap_or_default()
        } else {
            text_opt.as_deref().map(nomifun_audio::strip_wake_phrase).unwrap_or_default()
        };
        let decision = if let Some(ref text) = text_opt {
            info!("ASR transcription produced text");
            if matches!(event_tx.send_scoped(VoiceCompanionEvent::SpeechRecognized { text: text.clone() }, &control_scope), Ok(0)) {
                return;
            }
            if !clean_input.is_empty() {
                voice_feedback::play(playback, event_tx, VoiceCueKind::Recognized, generation)
                    .await;
                let current = event_tx.snapshot();
                if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                    return;
                }
            }
            let is_local_task_control = match task_client {
                Some(client) => local_task_control_action(&clean_input, client.as_ref()).await,
                None => None,
            };
            let current = event_tx.snapshot();
            if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() { return; }
            if clean_input.is_empty() {
                Self::discard_utterance(driver, playback, event_tx, generation, &control_scope).await;
                return;
            } else if let Some(action) = is_local_task_control {
                // Local task control fast-path: query and cancellation remain local,
                // zero dialogue model calls, fully functional even when model is offline.
                let dec = VoiceDecision {
                    intent_type: IntentType::TaskControl,
                    understood_request: Some(if action == nomifun_api_types::voice::VoiceTaskControlAction::CancelTask {
                        "取消当前后台任务".to_string()
                    } else {
                        "查询当前后台任务进度与状态".to_string()
                    }),
                    task_control_action: Some(action),
                    reply_tts: None,
                    spoken_ack: None,
                    task_goal: None,
                    requires_confirmation: None,
                };
                Some(dec)
            } else if !router.is_configured() {
                let _ = event_tx.send_scoped(VoiceCompanionEvent::Error {
                    stage: "dialogue".into(),
                    code: "model_unavailable".into(),
                }, &control_scope);
                None
            } else {
                let started = std::time::Instant::now();
                let context = event_tx.context();
                if let Some(client) = task_client {
                    task_context = tokio::time::timeout(std::time::Duration::from_millis(250), client.current()).await
                        .unwrap_or_else(|_| Err("task_status_unavailable".into()));
                }
                let current = event_tx.snapshot();
                if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                    return;
                }
                let task_status = task_status_for_prompt(
                    task_context.as_ref().ok().and_then(|task| task.as_ref()),
                    &scope,
                );
                let (sentence_tx, mut sentence_rx) = tokio::sync::mpsc::channel::<nomifun_api_types::voice::VoiceDialogueSentence>(4);
                let (speech_tx, speech_rx) = tokio::sync::mpsc::channel::<String>(4);
                let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

                let mut req = nomifun_api_types::voice::VoiceDialogueRequest::scoped(
                    scope.session_id.as_deref().unwrap_or(""),
                    scope.turn_id.as_deref().unwrap_or(""),
                    &clean_input,
                );
                req.locale = scope.locale.clone();
                req.call_request_id = scope.call_request_id.clone();

                let mut router_guard = AbortOnDrop(Some(tokio::spawn({
                    let router = router.clone();
                    let clean = clean_input.clone();
                    let task_status = task_status.clone();
                    let context = context.clone();
                    async move {
                        router.route_scoped_stream(&req, &clean, task_status.as_deref(), &context, sentence_tx).await
                    }
                })));

                let speak_replies = event_tx.feedback().speak_replies;
                let first_speech_text = Arc::new(std::sync::Mutex::new(None::<String>));
                let latency_scope=(scope.session_id.clone(),scope.turn_id.clone());
                let is_en = nomifun_api_types::voice::is_english_locale(scope.locale.as_deref());
                let voice_name = if is_en { "en-US-JennyNeural" } else { "" };
                let mut playback_guard = if speak_replies {
                    let playback = playback.clone();
                    let tts = tts.clone();
                    let expected_generation = generation;
                    let on_started = {
                        let event_tx = event_tx.clone();
                        let first_speech_text = first_speech_text.clone();
                        let source_tts = tts.clone();
                        let stream_start = started;
                        let control_scope = control_scope.clone();
                        move || {
                            let text = first_speech_text.lock().unwrap().clone().unwrap_or_default();
                            let ttfa_ms = stream_start.elapsed().as_millis() as u64;
                            info!(session_id=?latency_scope.0,turn_id=?latency_scope.1,from_model_start_ms=ttfa_ms,
                                text_chars = text.chars().count(), "voice_first_audio_started");
                            if let Some(diagnostic) = source_tts.diagnostic("") {
                                let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSource {
                                    session_id: control_scope.session_id.clone(), turn_id: control_scope.turn_id.clone(), diagnostic,
                                }, &control_scope);
                            }
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSpeaking { text }, &control_scope);
                        }
                    };
                    AbortOnDrop(Some(tokio::spawn(async move {
                        playback.speak_sentence_stream_with_voice(tts.as_ref(), speech_rx, expected_generation, cancel_rx, voice_name, on_started).await
                    })))
                } else {
                    AbortOnDrop(None)
                };

                let mut router_result = None;
                let mut raw_text = None;
                let mut chat_streamed = false;
                let mut sentence_rx_closed = false;

                loop {
                    tokio::select! {
                        biased;
                        _ = playback.interrupted(generation) => {
                            let _ = cancel_tx.send(true);
                            return;
                        }
                        sentence_opt = sentence_rx.recv(), if !sentence_rx_closed => {
                            match sentence_opt {
                                Some(sentence) => {
                                    let current = event_tx.snapshot();
                                    if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                                        let _ = cancel_tx.send(true);
                                        break;
                                    }
                                    if !chat_streamed {
                                        let mut d = driver.lock().await;
                                        if d.current_state() == AcousticState::Deciding {
                                            let _ = d.handle_event(AcousticEvent::IntentChat);
                                        }
                                        chat_streamed = true;
                                    }
                                    {
                                        let mut guard = first_speech_text.lock().unwrap();
                                        if guard.is_none() {
                                            *guard = Some(sentence.text.clone());
                                            if speak_replies {
                                                let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSynthesizing {
                                                    text: sentence.text.clone(),
                                                }, &control_scope);
                                            }
                                        }
                                    }
                                    let _ = event_tx.send_scoped(VoiceCompanionEvent::ReplySentence {
                                        text: sentence.text.clone(),
                                        understood_request: sentence.understood_request.clone(),
                                    }, &control_scope);
                                    if speak_replies {
                                        if speech_tx.send(sentence.text).await.is_err() {
                                            let _ = cancel_tx.send(true);
                                            break;
                                        }
                                    }
                                }
                                None => {
                                    sentence_rx_closed = true;
                                    if router_result.is_some() {
                                        break;
                                    }
                                }
                            }
                        }
                        res = router_guard.0.as_mut().unwrap(), if router_result.is_none() => {
                            let (res, raw) = match res {
                                Ok((res, raw)) => (res, raw),
                                Err(_) => (
                                    Err(nomifun_voice_terminal::driver::RouteError::Generation("model_panicked".into())),
                                    None,
                                ),
                            };
                            raw_text = raw;
                            match res {
                                Ok(dec) => {
                                    router_result = Some(Ok(dec));
                                    if sentence_rx_closed {
                                        break;
                                    }
                                }
                                Err(err) => {
                                    if !matches!(err, nomifun_voice_terminal::driver::RouteError::Record { .. }) {
                                        let _ = cancel_tx.send(true);
                                    }
                                    router_result = Some(Err(err));
                                    break;
                                }
                            }
                        }
                        else => {
                            break;
                        }
                    }
                }
                drop(speech_tx);

                let current = event_tx.snapshot();
                if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() { return; }

                if router_result.is_none() {
                    if let Some(mut handle) = router_guard.0.take() {
                        if let Ok((res, raw)) = (&mut handle).await {
                            raw_text = raw;
                            router_result = Some(res);
                        }
                    }
                } else {
                    router_guard.0 = None;
                }

                let current = event_tx.snapshot();
                if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                    let _ = cancel_tx.send(true);
                    return;
                }

                if let Some(Err(err)) = router_result {
                    use nomifun_voice_terminal::driver::RouteError;
                    match err {
                        RouteError::Record { code, content } => {
                            raw_text = Some(content.clone());
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::Error {
                                stage: "history".into(),
                                code: code.clone(),
                            }, &control_scope);
                            if code == "binding_invalid" || code == "voice_record_conflict" {
                                let _ = playback.interrupt().await;
                                let _ = playback_guard.0.take();
                                let mut d = driver.lock().await;
                                let current = event_tx.snapshot();
                                if !control_scope.matches(&current) || current.ended_reason.is_some() { return; }
                                d.reset();
                                drop(d);
                                let _ = event_tx.send_scoped(VoiceCompanionEvent::SessionEnded {
                                    reason: VoiceSessionEndReason::Failed,
                                }, &control_scope);
                                return;
                            }
                            if let Ok(dec) = nomifun_api_types::voice::parse_voice_decision(&content) {
                                router_result = Some(Ok(dec));
                            } else {
                                let _ = playback.interrupt().await;
                                let _ = playback_guard.0.take();
                                let mut d = driver.lock().await;
                                let current = event_tx.snapshot();
                                if !control_scope.matches(&current) || current.ended_reason.is_some() { return; }
                                d.reset();
                                drop(d);
                                let _ = event_tx.send_scoped(VoiceCompanionEvent::SessionEnded {
                                    reason: VoiceSessionEndReason::Failed,
                                }, &control_scope);
                                return;
                            }
                        }
                        other_err => {
                            let mut failed_driver = driver.lock().await;
                            if playback.generation_id() != generation { return; }
                            let _ = playback.interrupt().await;
                            let _ = playback_guard.0.take();
                            let (stage, code) = match other_err {
                                RouteError::Network(_) => ("dialogue", "model_network_failed"),
                                RouteError::Generation(ref detail)
                                    if matches!(
                                        detail.as_str(),
                                        "model_unavailable"
                                            | "model_auth_failed"
                                            | "model_network_failed"
                                            | "model_rejected"
                                            | "model_response_invalid"
                                            | "model_timeout"
                                    ) =>
                                {
                                    ("dialogue", detail.as_str())
                                }
                                RouteError::Generation(ref detail)
                                    if matches!(
                                        detail.as_str(),
                                        "binding_invalid"
                                            | "voice_record_conflict"
                                            | "binding_busy"
                                            | "voice_projection_write_failed"
                                            | "voice_record_write_failed"
                                            | "voice_generation_unknown"
                                    ) =>
                                {
                                    ("history", detail.as_str())
                                }
                                RouteError::Validation(_) => ("dialogue", "model_response_invalid"),
                                RouteError::Generation(ref detail)
                                    if detail.contains("401") || detail.contains("403") =>
                                {
                                    ("dialogue", "model_auth_failed")
                                }
                                RouteError::Generation(ref detail)
                                    if detail.starts_with("HTTP status") =>
                                {
                                    ("dialogue", "model_rejected")
                                }
                                RouteError::Generation(ref detail)
                                    if detail.contains("timeout") || detail.contains("timed out") =>
                                {
                                    ("dialogue", "model_timeout")
                                }
                                RouteError::Record { ref code, .. } => {
                                    ("history", code.as_str())
                                }
                                _ => ("dialogue", "model_response_invalid"),
                            };
                            warn!(stage, code, "voice_model_failed");
                            let _ = event_tx.send(VoiceCompanionEvent::Error {
                                stage: stage.into(),
                                code: code.into(),
                            });
                            failed_driver.reset();
                            let has_pending = {
                                let p = pending_utterances.lock().unwrap_or_else(|e| e.into_inner());
                                p.front().is_some_and(|item| item.generation_id == generation && item.session_id == scope.session_id)
                            };
                            if !has_pending {
                                let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                    reason: VoiceSessionEndReason::Failed,
                                });
                            }
                            return;
                        }
                    }
                }

                let dec = match router_result {
                    Some(Ok(d)) => d,
                    _ => {
                        let _ = playback_guard.0.take();
                        return;
                    }
                };

                if let Some(text) = raw_text {
                    let _ = event_tx.send_scoped(VoiceCompanionEvent::ModelResponse { text }, &control_scope);
                }
                info!(
                    session_id = ?scope.session_id, turn_id = ?scope.turn_id,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    ok = true,
                    "voice_model_finished"
                );

                if chat_streamed {
                    let _ = event_tx.send_scoped(VoiceCompanionEvent::IntentDecided {
                        intent: "Chat".into(),
                        reply: dec.reply_tts.clone(),
                        understood_request: dec.understood_request.clone(),
                    }, &control_scope);
                    info!(intent = ?dec.intent_type, has_understanding = dec.understood_request.is_some(), "voice_intent_decided");

                    if !speak_replies {
                        Self::finish_muted_playback(driver, playback, event_tx, generation, &control_scope).await;
                        return;
                    }

                    let play_res = if let Some(h) = playback_guard.0.take() {
                        match h.await {
                            Ok(res) => res,
                            Err(_) => Err(nomifun_voice_terminal::playback::AudioSinkError::Synthesis(
                                "tts_task_panicked".into(),
                            )),
                        }
                    } else {
                        Ok(())
                    };
                    if playback.generation_id() != generation {
                        return;
                    }

                    let _ = Self::finish_response_playback(play_res, driver, playback, event_tx, generation, &control_scope).await;
                    return;
                }

                let _ = playback_guard.0.take();
                Some(dec)
            }
        } else {
            None
        };

        if playback.generation_id() != generation {
            return;
        }

        let mut d = driver.lock().await;
        let current = event_tx.snapshot();
        if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
            return;
        }
        if let Some(dec) = decision {
            if dec.intent_type != IntentType::Ignore {
                let _ = event_tx.send_scoped(VoiceCompanionEvent::IntentDecided {
                    intent: format!("{:?}", dec.intent_type),
                    reply: dec.reply_tts.clone().filter(|_| dec.intent_type == IntentType::Chat),
                    understood_request: dec.understood_request.clone(),
                }, &control_scope);
                info!(intent = ?dec.intent_type, has_understanding = dec.understood_request.is_some(), "voice_intent_decided");
            }
            if playback.generation_id()!=generation {
                drop(d);
                return;
            }
            match dec.intent_type {
                IntentType::Ignore => {
                    drop(d);
                    Self::discard_utterance(driver, playback, event_tx, generation, &control_scope).await;
                }
                IntentType::Chat => {
                    let _ = d.handle_event(AcousticEvent::IntentChat);
                    drop(d);
                    let Some(reply_text) = dec
                        .reply_tts
                        .as_deref()
                        .filter(|text| !text.trim().is_empty())
                    else {
                        let mut d = driver.lock().await;
                        d.reset();
                        let _ = event_tx.send(VoiceCompanionEvent::Error {
                            stage: "dialogue".into(),
                            code: "model_response_invalid".into(),
                        });
                        let _ = event_tx.send(VoiceCompanionEvent::Idle);
                        return;
                    };
                    let _ = Self::play_response(reply_text,driver,playback,event_tx,tts,generation,&control_scope).await;
                }
                IntentType::TaskControl if task_client.is_some() => {
                    use nomifun_api_types::voice::{VoiceSessionControlRequest,VoiceTaskControlCommand,VoiceTaskReference,VoiceTaskStatus};
                    let snapshot=event_tx.snapshot();
                    if task_context.as_ref().ok().and_then(|t| t.as_ref()).is_none() {
                        if let Some(client) = task_client {
                            task_context = tokio::time::timeout(std::time::Duration::from_millis(250), client.current()).await
                                .unwrap_or_else(|_| Err("task_status_unavailable".into()));
                        }
                    }
                    let current = event_tx.snapshot();
                    if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() { return; }
                    let request=match (task_context,dec.task_control_action) {
                        (Ok(Some(task)),Some(action)) => Ok(VoiceTaskControlCommand {
                            scope:VoiceSessionControlRequest {instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id},
                            task:VoiceTaskReference::from(&task),action,
                            raw_user_text: Some(clean_input.clone()),
                            locale: snapshot.locale.clone(),
                        }),
                        (Err(_),_) => Err("task_status_unavailable".to_owned()),
                        (Ok(None),_) => Err("task_not_found".into()),
                        (_,None) => Err("model_response_invalid".into()),
                    };
                    let _=d.handle_event(AcousticEvent::IntentTaskControl);
                    drop(d);
                    let result=match request {
                        Ok(request) => task_client.as_ref().unwrap().control(request).await,
                        Err(code) => Err(code),
                    };
                    let current = event_tx.snapshot();
                    if playback.generation_id()!=generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                        return;
                    }
                    match result {
                        Ok(task) => {
                            let is_en = nomifun_api_types::voice::is_english_locale(event_tx.snapshot().locale.as_deref());
                            let cancelled = dec.task_control_action == Some(nomifun_api_types::voice::VoiceTaskControlAction::CancelTask)
                                && task.status == VoiceTaskStatus::Failed
                                && task.error_code.as_deref() == Some("task_cancelled");
                            let text = if cancelled {
                                if is_en { "Task cancelled." } else { "任务已取消。" }.to_string()
                            } else if is_en {
                                match task.status {
                                    VoiceTaskStatus::Queued => "The task is queued, waiting for previous task to finish.".into(),
                                    VoiceTaskStatus::Delivering => "Submitting the task, receipt verification pending.".into(),
                                    VoiceTaskStatus::Unknown => "Task status not verified yet, checking receipt.".into(),
                                    VoiceTaskStatus::Accepted => "Task accepted, waiting for execution results.".into(),
                                    VoiceTaskStatus::Cancelling => "Cancelling the task, awaiting execution confirmation.".into(),
                                    VoiceTaskStatus::Completed => format!("Execution finished. {}", task.result_text.as_deref().unwrap_or("Please check task details.").chars().take(220).collect::<String>()),
                                    VoiceTaskStatus::Failed => format!("Task ended, execution was unsuccessful. {}", task.result_text.as_deref().unwrap_or("Please check task details for reasons.").chars().take(220).collect::<String>()),
                                }
                            } else {
                                match task.status {
                                    VoiceTaskStatus::Queued => "任务已排队，等待上一任务结束。".into(),
                                    VoiceTaskStatus::Delivering => "任务正在提交，尚未核实接单。".into(),
                                    VoiceTaskStatus::Unknown => "任务状态尚未核实，正在核对原回执。".into(),
                                    VoiceTaskStatus::Accepted => "任务已接受，正在等待执行结果。".into(),
                                    VoiceTaskStatus::Cancelling => "正在取消任务，等待执行侧确认。".into(),
                                    VoiceTaskStatus::Completed => format!("执行已结束。{}",task.result_text.as_deref().unwrap_or("请查看任务详情。").chars().take(220).collect::<String>()),
                                    VoiceTaskStatus::Failed => format!("任务已结束，执行结果未成功。{}",task.result_text.as_deref().unwrap_or("请查看任务详情了解原因。").chars().take(220).collect::<String>()),
                                }
                            };
                            let _ = Self::play_response(&text,driver,playback,event_tx,tts,generation,&control_scope).await;
                        },
                        Err(code) => {
                            let is_en = nomifun_api_types::voice::is_english_locale(event_tx.snapshot().locale.as_deref());
                            let is_recoverable = matches!(
                                code.as_str(),
                                "task_not_found"
                                    | "task_status_unavailable"
                                    | "task_scope_changed"
                                    | "task_control_ambiguous"
                                    | "task_control_scope_required"
                                    | "voice_projection_write_failed"
                                    | "voice_record_write_failed"
                                    | "voice_record_conflict"
                                    | "binding_busy"
                                    | "target_revision_changed"
                            );
                            if is_recoverable {
                                let _ = event_tx.send(VoiceCompanionEvent::Error { stage: "task".into(), code: code.clone() });
                                let text = match code.as_str() {
                                    "task_not_found" => {
                                        if is_en {
                                            "No task in this voice conversation is currently running."
                                        } else {
                                            "当前语音会话没有正在运行的后台任务。"
                                        }
                                    }
                                    "task_scope_changed" => {
                                        if is_en {
                                            "That task belongs to another conversation, so I did not control it."
                                        } else {
                                            "该任务属于其他会话，未执行操作。"
                                        }
                                    }
                                    "task_control_ambiguous" => {
                                        if is_en {
                                            "I did not cancel anything. Please state the task to cancel clearly."
                                        } else {
                                            "没有执行取消，请明确要取消的任务。"
                                        }
                                    }
                                    "task_control_scope_required" => {
                                        if is_en {
                                            "I cannot confirm which task to control."
                                        } else {
                                            "无法确认要操作的任务。"
                                        }
                                    }
                                    "voice_projection_write_failed"
                                    | "voice_record_write_failed"
                                    | "voice_record_conflict" => {
                                        if is_en {
                                            "I couldn't save the task result. Please retry or check the conversation."
                                        } else {
                                            "任务结果暂时无法保存，请稍后重试或在会话中查看。"
                                        }
                                    }
                                    "binding_busy" | "target_revision_changed" => {
                                        if is_en {
                                            "The task state changed. Please try again."
                                        } else {
                                            "任务状态已变化，请稍后重试。"
                                        }
                                    }
                                    _ => {
                                        if is_en {
                                            "Task status is temporarily unavailable."
                                        } else {
                                            "暂时无法获取任务状态。"
                                        }
                                    }
                                };
                                let _ = Self::play_response(text, driver, playback, event_tx, tts, generation, &control_scope).await;
                            } else {
                                let mut driver = driver.lock().await;
                                if playback.generation_id() != generation { return; }
                                driver.reset();
                                let _ = event_tx.send(VoiceCompanionEvent::Error { stage: "task".into(), code });
                                drop(driver);
                                voice_feedback::play(playback, event_tx, VoiceCueKind::Error, generation).await;
                                if playback.generation_id() == generation {
                                    let _ = event_tx.send(VoiceCompanionEvent::SessionEnded { reason: VoiceSessionEndReason::Failed });
                                }
                            }
                        },
                    }
                }
                IntentType::AgentTask if task_client.is_some() => {
                    use nomifun_api_types::voice::{VoiceTaskCommand, VoiceTaskStatus, VoiceTaskSubmitRequest};
                    let _ = d.handle_event(AcousticEvent::IntentAgentTask);
                    drop(d);
                    let _ = event_tx.send_scoped(VoiceCompanionEvent::TaskDispatching, &control_scope);
                    let scope = event_tx.snapshot();
                    if !control_scope.matches(&scope) || scope.ended_reason.is_some() { return; }
                    let command = match (scope.session_id, scope.turn_id, dec.task_goal.filter(|goal| !goal.trim().is_empty())) {
                        (Some(session_id),Some(turn_id),Some(goal)) => VoiceTaskCommand {
                            command_id: format!("voice-{turn_id}"),
                            record_id: Some(format!("voice-{turn_id}")),
                            binding_id: Some(format!("voice-binding-{session_id}")),
                            session_id,
                            turn_id,
                            goal,
                        },
                        _ => {
                            let mut d = driver.lock().await;
                            let current = event_tx.snapshot();
                            if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() { return; }
                            d.reset();
                            drop(d);
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::Error {stage:"task".into(),code:"task_goal_invalid".into()}, &control_scope);
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::SessionEnded {reason:VoiceSessionEndReason::Failed}, &control_scope);
                            return;
                        }
                    };
                    let result = task_client.as_ref().unwrap().submit(VoiceTaskSubmitRequest {
                        instance_id:scope.instance_id,command:command.clone(),
                    }).await;
                    let current = event_tx.snapshot();
                    if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                        return;
                    }
                    let is_en = nomifun_api_types::voice::is_english_locale(event_tx.snapshot().locale.as_deref());
                    let voice_name = if is_en { "en-US-JennyNeural" } else { "" };
                    let code = match result {
                        Ok(delivery) if delivery.task.command_id != command.command_id
                            || delivery.task.session_id != command.session_id
                            || delivery.task.acoustic_turn_id != command.turn_id => Some("task_receipt_scope_changed".into()),
                        Ok(delivery) if delivery.task.status == VoiceTaskStatus::Failed => Some(delivery.task.error_code.unwrap_or_else(||"task_failed".into())),
                        Ok(delivery) if delivery.freshly_accepted && delivery.task.status == VoiceTaskStatus::Accepted
                            && delivery.task.conversation_id.is_some() && delivery.task.message_id.is_some() => {
                            let text = if is_en { "Task accepted, executing now." } else { "任务已接单，正在执行。" };
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::ResponseReady { text: text.into() }, &control_scope);
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSynthesizing {text:text.into()}, &control_scope);
                            if event_tx.feedback().speak_replies {
                                playback.speak_text_for_generation(tts.as_ref(),text,voice_name, generation, || {
                                    let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSpeaking {text:text.into()}, &control_scope);
                                }).await.err().map(|error| match error {AudioSinkError::Synthesis(_) => "tts_failed", AudioSinkError::NoAudio => "tts_empty_audio", _ => "speaker_failed"}.to_owned())
                            } else {None}
                        }
                        Ok(delivery) if delivery.task.status == VoiceTaskStatus::Queued
                            && delivery.task.conversation_id.is_some() && delivery.task.message_id.is_some() => {
                            let text = if is_en { "A task is currently running, the new task is queued." } else { "当前有任务正在执行，新任务已排队。" };
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::ResponseReady { text: text.into() }, &control_scope);
                            let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSynthesizing { text: text.into() }, &control_scope);
                            if event_tx.feedback().speak_replies {
                                playback.speak_text_for_generation(tts.as_ref(), text, voice_name, generation, || {
                                    let _ = event_tx.send_scoped(VoiceCompanionEvent::TtsSpeaking { text: text.into() }, &control_scope);
                                }).await.err().map(|error| match error {
                                    AudioSinkError::Synthesis(_) => "tts_failed",
                                    AudioSinkError::NoAudio => "tts_empty_audio",
                                    _ => "speaker_failed",
                                }.to_owned())
                            } else {
                                None
                            }
                        }
                        Ok(delivery) if delivery.task.conversation_id.is_none() || delivery.task.message_id.is_none()
                            || matches!(delivery.task.status, VoiceTaskStatus::Delivering | VoiceTaskStatus::Unknown) => Some("task_delivery_unknown".into()),
                        Ok(_) => None,
                        Err(code) => Some(code),
                    };
                    let mut driver_guard = driver.lock().await;
                    let current = event_tx.snapshot();
                    if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                        return;
                    }
                    let _ = driver_guard.handle_event(AcousticEvent::TaskAccepted);
                    drop(driver_guard);
                    let failed = code.is_some();
                    if let Some(code) = code {
                        let _ = event_tx.send_scoped(VoiceCompanionEvent::Error {stage:"task".into(),code}, &control_scope);
                        voice_feedback::play(playback,event_tx,VoiceCueKind::Error,generation).await;
                    }
                    let current = event_tx.snapshot();
                    if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                        return;
                    }
                    if failed {
                        let _ = event_tx.send_scoped(VoiceCompanionEvent::SessionEnded {reason:VoiceSessionEndReason::Failed}, &control_scope);
                    } else {
                        let has_pending = {
                            let p = pending_utterances.lock().unwrap_or_else(|e| e.into_inner());
                            p.front().is_some_and(|item| item.generation_id == generation)
                        };
                        if !has_pending {
                            let mut d = driver.lock().await;
                            let current = event_tx.snapshot();
                            if playback.generation_id() != generation || !control_scope.matches(&current) || current.ended_reason.is_some() {
                                return;
                            }
                            if d.current_state() == AcousticState::Idle {
                                let _ = d.handle_event(AcousticEvent::WakeHit);
                                if d.begin_listening_window() {
                                    let _ = event_tx.send_scoped(VoiceCompanionEvent::Listening, &control_scope);
                                    let _ = event_tx.send_scoped(VoiceCompanionEvent::ListeningReady { timeout_secs: 180 }, &control_scope);
                                }
                            }
                        }
                    }
                }
                IntentType::TaskControl | IntentType::AgentTask => {
                    // Only a real runtime receipt may authorize an acceptance announcement.
                    let _ = d.handle_event(AcousticEvent::IntentIgnore);
                    drop(d);
                    let _ = event_tx.send(VoiceCompanionEvent::Error {
                        stage: "task".into(),
                        code: "runtime_unavailable".into(),
                    });
                    voice_feedback::play(playback, event_tx, VoiceCueKind::Error, generation).await;
                    if playback.generation_id() == generation {
                        let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                            reason: VoiceSessionEndReason::Failed,
                        });
                    }
                }
            }
        } else {
            let _ = d.handle_event(AcousticEvent::IntentIgnore);
            drop(d);
            voice_feedback::play(playback, event_tx, VoiceCueKind::Error, generation).await;
            let has_pending = {
                let p = pending_utterances.lock().unwrap_or_else(|e| e.into_inner());
                p.front().is_some_and(|item| item.generation_id == generation)
            };
            if playback.generation_id() == generation && !has_pending {
                let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                    reason: VoiceSessionEndReason::Failed,
                });
            }
        }
    }

    /// Feature-only ordinary caller entry for reverse acceptance. It uses the
    /// real start_utterance/process_utterance/ASR path, not a diagnostic helper.
    #[cfg(feature = "native-fault-injection")]
    pub fn test_start_ordinary_utterance(&self, request_id: &str) -> bool {
        let Some(stt) = self.transcriber.clone() else {
            return false;
        };
        Self::start_utterance(
            vec![0.1f32; 16_000],
            self.driver.clone(),
            self.router.clone(),
            Some(stt),
            self.playback.clone(),
            self.event_tx.clone(),
            self.tts.clone(),
            self.task_client.clone(),
            &self.active_turn,
            self.input_generation.clone(),
            self.playback.generation_id(),
            self.pending_utterances.clone(),
            self.event_tx.snapshot().session_id.clone(),
            self.engine_check_native_busy.clone(),
            self.native_degraded.clone(),
            self.native_job_epoch.clone(),
            self.native_admission.clone(),
            Some(request_id.to_owned()),
            None,
        );
        true
    }

    #[cfg(feature = "native-fault-injection")]
    pub fn test_abort_ordinary_turn(&self) -> bool {
        let turn = self
            .active_turn
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(turn) = turn {
            turn.abort();
            true
        } else {
            false
        }
    }

    /// Keeps capture consuming frames while a turn awaits the model or speaker.
    fn start_utterance(
        pcm: Vec<f32>,
        driver: Arc<Mutex<VoiceSessionDriver>>,
        router: Arc<dyn VoiceRouter>,
        transcriber: Option<Arc<dyn SttCallback>>,
        playback: Arc<PlaybackPipeline>,
        events: Arc<VoiceEventHub>,
        tts: Arc<dyn TtsProvider>,
        task_client: Option<Arc<dyn nomifun_voice_terminal::task_client::VoiceTaskClient>>,
        active_turn: &Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
        input_generation: Arc<AtomicU64>,
        generation: u64,
        pending_utterances: Arc<std::sync::Mutex<std::collections::VecDeque<PendingUtterance>>>,
        session_id: Option<String>,
        native_busy: Arc<AtomicBool>,
        native_degraded: Arc<NativeHealth>,
        native_job_epoch: Arc<AtomicU64>,
        native_admission: Arc<NativeAdmission>,
        native_request_id: Option<String>,
        recognized_text: Option<String>,
    ) {
        let mut active = active_turn.lock().unwrap_or_else(|e| e.into_inner());
        if playback.generation_id() != generation {
            return;
        }
        let active_turn_clone = active_turn.clone();
        let pending_clone = pending_utterances.clone();
        let input_id = input_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let admission = native_admission.reserve();
        let scope = events.snapshot();
        *active = Some(tokio::spawn(async move {
            use futures_util::FutureExt;
            if playback.generation_id() != generation {
                return;
            }
            let result = std::panic::AssertUnwindSafe(Self::process_utterance(
                &pcm,
                &driver,
                &router,
                &transcriber,
                &playback,
                &events,
                &tts,
                &task_client,
                generation,
                scope,
                &pending_clone,
                &native_busy,
                &native_degraded,
                &native_job_epoch,
                admission,
                native_request_id.as_deref().unwrap_or("ordinary-turn"),
                recognized_text,
            ))
            .catch_unwind()
            .await;
            if result.is_err() {
                let mut d = driver.lock().await;
                if playback.generation_id() != generation || input_generation.load(Ordering::SeqCst) != input_id {
                    return;
                }
                d.reset();
                let _ = playback.interrupt().await;
                warn!(generation, "voice_turn_panicked");
                let _ = events.send(VoiceCompanionEvent::Error {
                    stage: "turn".into(),
                    code: "voice_turn_failed".into(),
                });
                let _ = events.send(VoiceCompanionEvent::Idle);
            } else {
                let next_to_start = {
                    let mut act = active_turn_clone.lock().unwrap_or_else(|e| e.into_inner());
                    let mut pending = pending_clone.lock().unwrap_or_else(|e| e.into_inner());
                    if input_generation.load(Ordering::SeqCst) == input_id
                        && events.snapshot().session_id == session_id
                        && events.snapshot().ended_reason.is_none()
                    {
                        let next_generation = playback.generation_id();
                        pending.retain(|item| item.generation_id == generation && item.session_id == session_id);
                        for item in pending.iter_mut() {
                            item.generation_id = next_generation;
                        }
                        let next = pending.pop_front();
                        if next.is_none() {
                            *act = None;
                        }
                        next
                    } else {
                        *act = None;
                        None
                    }
                };
                if let Some(next_item) = next_to_start {
                    let mut d = driver.lock().await;
                    let can_decide = match d.current_state() {
                        AcousticState::FollowUpListening => {
                            d.handle_event(AcousticEvent::FollowUpSpeech) == Ok(AcousticState::Deciding)
                        }
                        AcousticState::Listening => {
                            d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding)
                        }
                        AcousticState::Idle => {
                            d.handle_event(AcousticEvent::WakeHit).is_ok()
                                && d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding)
                        }
                        AcousticState::DispatchingTask => {
                            let _ = d.handle_event(AcousticEvent::TaskAccepted);
                            let _ = d.handle_event(AcousticEvent::PlaybackDone);
                            d.handle_event(AcousticEvent::WakeHit).is_ok()
                                && d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding)
                        }
                        _ => false,
                    };
                    if can_decide {
                        drop(d);
                        let _ = events.send(VoiceCompanionEvent::Transcribing);
                        let next_gen = playback.generation_id();
                        Self::start_utterance(
                            next_item.pcm,
                            driver.clone(),
                            router.clone(),
                            transcriber.clone(),
                            playback.clone(),
                            events.clone(),
                            tts.clone(),
                            task_client.clone(),
                            &active_turn_clone,
                            input_generation.clone(),
                            next_gen,
                            pending_clone.clone(),
                            next_item.session_id,
                            native_busy.clone(),
                            native_degraded.clone(),
                            native_job_epoch.clone(),
                            native_admission.clone(),
                            None,
                            None,
                        );
                    } else {
                        let _ = events.send(VoiceCompanionEvent::Error {
                            stage: "queue".into(),
                            code: "pending_utterance_dropped".into(),
                        });
                        let mut act = active_turn_clone.lock().unwrap_or_else(|e| e.into_inner());
                        *act = None;
                    }
                }
            }
        }));
    }

    fn note_native_worker_error(operation: &str, reason: &str, acoustic_ready: &AtomicBool) {
        acoustic_ready.store(false, Ordering::Release);
        warn!(operation, reason, "voice_native_call_failed");
    }

    #[cfg(any(test, feature = "voice-skeleton"))]
    pub fn set_ordinary_speech_end_gate(&mut self, gate: Option<Arc<SpeechEndTestGate>>) {
        self.ordinary_speech_end_gate = gate;
    }

    pub fn start_worker(&self) -> tokio::task::JoinHandle<()> {
        #[cfg(test)]
        let confirmation_speech_end_gate = self.confirmation_speech_end_gate.clone();
        #[cfg(any(test, feature = "voice-skeleton"))]
        let mut ordinary_speech_end_gate = self.ordinary_speech_end_gate.clone();
        let driver = Arc::clone(&self.driver);
        let capture = Arc::clone(&self.capture_pipeline);
        let playback = Arc::clone(&self.playback);
        let tts = Arc::clone(&self.tts);
        let router = Arc::clone(&self.router);
        let task_client = self.task_client.clone();
        let pending_confirmation=self.pending_confirmation.clone();
        let mut transcriber = self.transcriber.clone();
        let acoustic_ready = self.acoustic_ready.clone();
        let self_check_active = self.self_check_active.clone();
        let engine_check_active = self.engine_check_active.clone();
        let native_busy = self.engine_check_native_busy.clone();
        let native_admission = self.native_admission.clone();
        // Healthy ASR ownership is not an outage; only actual native errors
        // degrade readiness, and a later native success clears the state.
        let native_degraded = self.native_degraded.clone();
        let native_job_epoch = self.native_job_epoch.clone();
        let mut engine_check_rx = self
            .engine_check_rx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        let worker_epoch = self.worker_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.event_tx.set_worker_epoch(worker_epoch);
        let wake_admission_until_ms = self.wake_admission_until_ms.clone();
        let capture_available = self.capture_available();
        let replay = {
            #[cfg(any(test, feature = "voice-skeleton"))]
            {
                self.replay_capture
            }
            #[cfg(not(any(test, feature = "voice-skeleton")))]
            {
                false
            }
        };
        let oobe = Arc::clone(&self.oobe_manager);
        let models = Arc::clone(&self.model_manager);
        let event_tx = self.event_tx.clone();
        let active_turn = self.active_turn.clone();
        let pending_utterances = Arc::clone(&self.pending_utterances);
        let input_generation = self.input_generation.clone();
        // Native capture has no render reference/AEC yet. Raw VAD is not safe barge-in.
        let barge_in_ready = {
            #[cfg(any(test, feature = "voice-skeleton"))]
            { self.replay_capture && self.replay_barge_in }
            #[cfg(not(any(test, feature = "voice-skeleton")))]
            { false }
        };

        tokio::spawn(async move {
            // Native engines must not open an unverified offline copy. Keep
            // waiting so first-boot installation can complete without restart.
            if !replay {
                while !models.get_status().await.state.is_ready() {
                    acoustic_ready.store(false, Ordering::Release);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            #[cfg(feature = "native-fault-injection")]
            if std::env::var("NOMIFUN_VOICE_FAULT_BLOCK_ASR").ok().as_deref() == Some("1") {
                transcriber = Some(Arc::new(FaultBlockingStt));
            }
            let native_runner = NativeCallRunner::with_admission(
                native_busy.clone(),
                native_job_epoch.clone(),
                native_degraded.clone(),
                native_admission.clone(),
                false,
            );
            let kws = loop {
                let dir = models.models_dir().to_path_buf();
                match native_runner
                    .load(std::time::Duration::from_secs(30), move || {
                        KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
                            model_dir: Some(dir),
                            ..Default::default()
                        })
                    })
                    .await
                {
                    Ok(Ok(kws)) => break kws,
                    Ok(Err(error)) => panic!("the fixed 16 kHz KWS configuration is valid: {error}"),
                    Err(reason) => {
                        Self::note_native_worker_error("kws_init", &reason, &acoustic_ready);
                    }
                }
            };
            let vad: Box<dyn VadBackend> = loop {
                let dir = models.models_dir().to_path_buf();
                match native_runner
                    .load(std::time::Duration::from_secs(30), move || {
                        SherpaVad::load(&dir)
                    })
                    .await
                {
                    Ok(Ok(vad)) => break Box::new(vad),
                    // Synthetic worker tests without installed assets retain their test detector.
                    #[cfg(any(test, feature = "voice-skeleton"))]
                    Ok(Err(_)) if replay && !models.models_dir().join("vad/silero_vad.onnx").is_file() => {
                        break Box::new(EnergyVad::new(VadConfig {
                            threshold: 0.01,
                            ..Default::default()
                        }));
                    }
                    Ok(Err(code)) => {
                        acoustic_ready.store(false, Ordering::Release);
                        warn!(code, "voice_vad_load_failed");
                        let _ = event_tx.send(VoiceCompanionEvent::Error {
                            stage: "capture".into(),
                            code,
                        });
                        let _ = event_tx.send(VoiceCompanionEvent::Availability {
                            acoustic_ready: false,
                            dialogue_ready: router.is_configured(),
                            playback_ready: playback.is_available(),
                        });
                        return;
                    }
                    Err(reason) => {
                        Self::note_native_worker_error("vad_init", &reason, &acoustic_ready);
                    }
                }
            };
            let kws = NativeKws::new(Box::new(kws), native_runner.clone());
            let vad = NativeVad::new(vad, native_runner.clone());
            info!(silence_ms = 800, "voice_vad_loaded");
            macro_rules! native_worker_call {
                ($operation:expr, $call:expr, $default:expr) => {{
                    match $call {
                        Ok(value) => value,
                        Err(reason) => {
                            Self::note_native_worker_error($operation, &reason, &acoustic_ready);
                            $default
                        }
                    }
                }};
            }
            macro_rules! native_worker_frame {
                ($operation:expr, $call:expr) => {{
                    match $call {
                        Ok(value) => value,
                        Err(reason) => {
                            Self::note_native_worker_error($operation, &reason, &acoustic_ready);
                            continue;
                        }
                    }
                }};
            }
            let mut utterance_pcm = Vec::<f32>::new();
            let mut speech_started = false;
            let mut listen_start: Option<std::time::Instant> = None;
            let mut last_model_check: Option<std::time::Instant> = None;
            let mut last_availability = None;
            let mut observed_session = None;
            let mut audio = VoiceAudioDiagnostics::default();
            audio.vad_ready = true;
            audio.barge_in_ready = barge_in_ready;
            audio.barge_in_reason = Some(if barge_in_ready { "test_echo_free_pcm" } else { "aec_unavailable" }.into());
            let mut last_audio_publish = std::time::Instant::now();
            let mut last_audio_log = std::time::Instant::now();
            let mut last_pcm = std::time::Instant::now();
            let mut last_asr_load_warning: Option<std::time::Instant> = None;
            let mut processing_feedback: Option<(String, std::time::Instant, u8)> = None;
            let quiet_input = Arc::new(AtomicBool::new(false));
            let mut quiet_since: Option<std::time::Instant> = None;
            let mut reminded_session: Option<String> = None;

            loop {
                if let Some(receiver) = engine_check_rx.as_mut() {
                    while let Ok(command) = receiver.try_recv() {
                        let WorkerEngineCheck {
                            request_id,
                            deadline,
                            cancel,
                            cancel_notify,
                            native_busy,
                            native_job_epoch,
                            fixtures,
                            reply,
                        } = command;
                        #[cfg(feature = "native-fault-injection")]
                        if std::env::var("NOMIFUN_VOICE_FAULT_BLOCK_ASR").ok().as_deref() == Some("1") {
                            bind_fault_boundary(&request_id, worker_epoch, &event_tx.snapshot().instance_id);
                        }
                        let load_result = if transcriber.is_none() {
                            Self::load_worker_asr(
                                &mut transcriber,
                                &models,
                                cancel.as_ref(),
                                cancel_notify.as_ref(),
                                &native_busy,
                                &native_degraded,
                                &native_job_epoch,
                                &native_admission,
                                deadline,
                            )
                            .await
                        } else {
                            Ok(())
                        };
                        tracing::info!(
                            request_id = %request_id,
                            worker_epoch = worker_epoch,
                            "voice_engine_check_started"
                        );
                        let result = match load_result {
                            Ok(()) => Self::worker_engine_check(
                                &kws,
                                &vad,
                                &transcriber,
                                &fixtures,
                                cancel.as_ref(),
                                cancel_notify.as_ref(),
                                &native_busy,
                                &native_degraded,
                                &native_job_epoch,
                                deadline,
                            )
                            .await,
                            Err(reason) => Err(reason),
                        };
                        let kws_reset = kws
                            .reset_until(deadline, cancel.as_ref(), cancel_notify.as_ref(), false)
                            .await;
                        let vad_reset = vad
                            .reset_until(deadline, cancel.as_ref(), cancel_notify.as_ref(), false)
                            .await;
                        if let Err(reason) = &kws_reset
                            && native_error_requires_degraded(reason)
                        {
                            Self::note_native_worker_error("kws_reset", reason, &acoustic_ready);
                        }
                        if let Err(reason) = &vad_reset
                            && native_error_requires_degraded(reason)
                        {
                            Self::note_native_worker_error("vad_reset", reason, &acoustic_ready);
                        }
                        let result = match (result, kws_reset.and(vad_reset)) {
                            (Err(reason), _) => Err(reason),
                            (Ok(_), Err(reason)) => Err(reason),
                            (Ok(result), Ok(())) => Ok(result),
                        };
                        utterance_pcm.clear();
                        speech_started = false;
                        listen_start = None;
                        pending_utterances
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .clear();
                        while let CaptureReadResult::Chunk(_) = {
                            let mut cap = capture.lock().await;
                            cap.try_next_chunk()
                        } {}
                        let _ = reply.send(result);
                        continue;
                    }
                }
                if engine_check_active.load(Ordering::Acquire) {
                    let _ = {
                        let mut cap = capture.lock().await;
                        cap.try_next_chunk()
                    };
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                }
                let session = event_tx.snapshot().session_id;
                if session != observed_session {
                    observed_session = session.clone();
                    utterance_pcm.clear();
                    pending_utterances.lock().unwrap_or_else(|e| e.into_inner()).clear();
                    speech_started = false;
                    listen_start = None;
                    audio.utterance_samples = 0;
                    audio.utterance_peak_milli = 0;
                    native_worker_call!("vad_reset", vad.reset().await, ());
                    native_worker_call!("kws_reset", kws.reset().await, ());
                }
                if !replay
                    && last_model_check
                        .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(1))
                {
                    last_model_check = Some(std::time::Instant::now());
                    if models.get_status().await.state.is_ready() {
                        if !kws.is_initialized() {
                            let dir = models.models_dir().to_path_buf();
                            let loaded = native_runner
                                .load(std::time::Duration::from_secs(30), move || {
                                    KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
                                        model_dir: Some(dir),
                                        ..Default::default()
                                    })
                                })
                                .await;
                            match loaded {
                                Ok(Ok(next)) => {
                                    native_worker_call!("kws_reload", kws.replace(Box::new(next)).await, ());
                                }
                                Ok(Err(error)) => panic!("fixed KWS configuration: {error}"),
                                Err(reason) => {
                                    Self::note_native_worker_error(
                                        "kws_reload_load",
                                        &reason,
                                        &acoustic_ready,
                                    );
                                }
                            }
                        }
                        if transcriber.is_none() {
                            let dir = models.models_dir().to_path_buf();
                            let loaded = native_runner
                                .load(std::time::Duration::from_secs(30), move || {
                                    nomifun_audio::stt::try_load_sherpa(Some(&dir))
                                })
                                .await;
                            if let Err(reason) = &loaded {
                                Self::note_native_worker_error("asr_reload", reason, &acoustic_ready);
                            }
                            let report = last_asr_load_warning.is_none_or(|last| {
                                last.elapsed() >= std::time::Duration::from_secs(5)
                            });
                            match loaded {
                                Ok(Ok(Some(stt))) => {
                                    transcriber = Some(Arc::new(stt));
                                }
                                Ok(Err(err)) if report => {
                                    warn!(error = %err, "voice_asr_load_failed");
                                    last_asr_load_warning = Some(std::time::Instant::now());
                                }
                                Err(err) if report => {
                                    warn!(error = %err, "voice_asr_loader_failed");
                                    last_asr_load_warning = Some(std::time::Instant::now());
                                }
                                Ok(Ok(None)) if report => {
                                    warn!(models_dir = %models.models_dir().display(), "voice_asr_model_not_found");
                                    last_asr_load_warning = Some(std::time::Instant::now());
                                }
                                _ => {}
                            }
                        }
                        acoustic_ready.store(
                            kws.is_initialized()
                                && transcriber.is_some()
                                && !native_degraded.is_degraded()
                                && audio.capture_state != "closed",
                            Ordering::Release,
                        );
                    } else {
                        acoustic_ready.store(false, Ordering::Release);
                    }
                }
                let availability = (
                    acoustic_ready.load(Ordering::Acquire),
                    router.is_configured(),
                    playback.is_available(),
                );
                audio.asr_ready = transcriber.is_some();
                audio.kws_ready = kws.is_initialized();
                if last_availability != Some(availability) {
                    info!(
                        microphone_open = capture_available,
                        kws_ready = kws.is_initialized(),
                        asr_ready = transcriber.is_some(),
                        dialogue_ready = availability.1,
                        "voice_engines_ready"
                    );
                    last_availability = Some(availability);
                    let _ = event_tx.send(VoiceCompanionEvent::Availability {
                        acoustic_ready: availability.0,
                        dialogue_ready: availability.1,
                        playback_ready: availability.2,
                    });
                    let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                        diagnostics: audio.clone(),
                    });
                }
                // 1. Check FSM watchdog timers (e.g. 8-second confirmation timeout)
                let snapshot = event_tx.snapshot();
                if matches!(
                    snapshot.phase,
                    nomifun_api_types::voice::VoiceSessionPhase::Transcribing
                        | nomifun_api_types::voice::VoiceSessionPhase::Deciding
                        | nomifun_api_types::voice::VoiceSessionPhase::Synthesizing
                ) {
                    if let Some((turn, start, count)) = &mut processing_feedback {
                        if snapshot.turn_id.as_deref() == Some(turn.as_str())
                            && *count < 3
                            && start.elapsed()
                                >= std::time::Duration::from_secs(3 + u64::from(*count) * 6)
                        {
                            *count += 1;
                            voice_feedback::schedule_optional(
                                playback.clone(),
                                event_tx.clone(),
                                VoiceCueKind::Processing,
                                playback.generation_id(),
                                quiet_input.clone(),
                                None,
                            );
                        }
                    }
                } else {
                    processing_feedback = None;
                }
                {
                    let mut d = driver.lock().await;
                    let timed_state = d.current_state();
                    let native_owned = native_busy.load(Ordering::Acquire);
                    if native_owned
                        && matches!(timed_state, AcousticState::Listening | AcousticState::FollowUpListening)
                    {
                        d.begin_listening_window();
                    }
                    if matches!(timed_state, AcousticState::Listening | AcousticState::FollowUpListening) {
                        let task_active = snapshot.task.as_ref().is_some_and(|t| {
                            !matches!(t.status, nomifun_api_types::voice::VoiceTaskStatus::Completed | nomifun_api_types::voice::VoiceTaskStatus::Failed)
                        });
                        if task_active {
                            d.begin_listening_window();
                        }
                    }
                    if timed_state == AcousticState::Listening
                        && !native_owned
                        && quiet_input.load(Ordering::Acquire)
                        && snapshot.session_id.is_some()
                        && reminded_session != snapshot.session_id
                        && d.remaining_listening_time().is_some_and(|remaining| {
                            !remaining.is_zero() && remaining <= std::time::Duration::from_secs(177)
                        })
                    {
                        reminded_session = snapshot.session_id.clone();
                        voice_feedback::schedule_optional(
                            playback.clone(),
                            event_tx.clone(),
                            VoiceCueKind::ListeningReminder,
                            playback.generation_id(),
                            quiet_input.clone(),
                            Some(driver.clone()),
                        );
                    }
                    if !native_owned
                        && let Ok(Some(next)) = d.check_watchdog()
                    {
                        info!(state = ?next, pcm_frames = audio.frames_received, rms_milli = audio.rms_milli, speech_detected = speech_started, "voice_listen_timeout");
                        if next == AcousticState::Idle {
                            let code = if timed_state==AcousticState::ListeningConfirm {
                                "confirmation_silence"
                            } else if timed_state == AcousticState::FollowUpListening {
                                "follow_up_silence"
                            } else if last_pcm.elapsed() > std::time::Duration::from_secs(2) {
                                "capture_no_pcm"
                            } else {
                                "initial_silence"
                            };
                            info!(
                                code,
                                samples = audio.utterance_samples,
                                peak_milli = audio.utterance_peak_milli,
                                "voice_listening_ended"
                            );
                            if code == "capture_no_pcm" {
                                let _ = event_tx.send(VoiceCompanionEvent::Error {
                                    stage: "capture".into(),
                                    code: code.into(),
                                });
                            }
                            utterance_pcm.clear();
                            speech_started = false;
                            listen_start = None;
                            native_worker_call!("vad_reset", vad.reset().await, ());
                            native_worker_call!("kws_reset", kws.reset().await, ());
                            let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                reason: match code {
                                    "follow_up_silence" => VoiceSessionEndReason::FollowUpSilence,
                                    "initial_silence" => VoiceSessionEndReason::InitialSilence,
                                    "confirmation_silence"=>VoiceSessionEndReason::NoIntent,
                                    _ => VoiceSessionEndReason::Failed,
                                },
                            });
                            voice_feedback::schedule(
                                playback.clone(),
                                event_tx.clone(),
                                if code == "capture_no_pcm" {
                                    VoiceCueKind::Error
                                } else {
                                    VoiceCueKind::Sleep
                                },
                                playback.generation_id(),
                            );
                        }
                    }
                }

                // 2. Fetch next PCM audio chunk from capture pipeline
                let chunk_result = {
                    let mut cap = capture.lock().await;
                    cap.try_next_chunk()
                };

                let chunk = match chunk_result {
                    CaptureReadResult::Chunk(samples) => samples,
                    CaptureReadResult::TemporarilyEmpty => {
                        if last_pcm.elapsed() > std::time::Duration::from_secs(2)
                            && last_audio_publish.elapsed() >= std::time::Duration::from_secs(1)
                        {
                            audio.capture_state = "no_pcm".into();
                            audio.rms_milli = 0;
                            audio.peak_milli = 0;
                            audio.speech_detected = false;
                            let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                                diagnostics: audio.clone(),
                            });
                            warn!(frames = audio.frames_received, "voice_capture_no_pcm");
                            last_audio_publish = std::time::Instant::now();
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        continue;
                    }
                    CaptureReadResult::StreamClosed | CaptureReadResult::DeviceError(_) => {
                        if audio.capture_state != "closed" {
                            let mut d = driver.lock().await;
                            d.reset();
                            acoustic_ready.store(false, Ordering::Release);
                            let _ = event_tx.send(VoiceCompanionEvent::Availability {
                                acoustic_ready: false,
                                dialogue_ready: router.is_configured(),
                                playback_ready: playback.is_available(),
                            });
                            let _ = event_tx.send(VoiceCompanionEvent::Error {
                                stage: "capture".into(),
                                code: "capture_closed".into(),
                            });
                            let _ = event_tx.send(VoiceCompanionEvent::SessionEnded {
                                reason: VoiceSessionEndReason::Failed,
                            });
                            let _ = playback.interrupt().await;
                            if let Some(turn) =
                                active_turn.lock().unwrap_or_else(|e| e.into_inner()).take()
                            {
                                turn.abort();
                            }
                            drop(d);
                            processing_feedback = None;
                            quiet_input.store(false, Ordering::Release);
                            utterance_pcm.clear();
                            speech_started = false;
                            listen_start = None;
                            native_worker_call!("vad_reset", vad.reset().await, ());
                            native_worker_call!("kws_reset", kws.reset().await, ());
                            audio.capture_state = "closed".into();
                            audio.rms_milli = 0;
                            audio.peak_milli = 0;
                            audio.speech_detected = false;
                            voice_feedback::schedule(
                                playback.clone(),
                                event_tx.clone(),
                                VoiceCueKind::Error,
                                playback.generation_id(),
                            );
                            warn!("voice_capture_closed_or_failed");
                            let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                                diagnostics: audio.clone(),
                            });
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };

                last_pcm = std::time::Instant::now();
                audio.frames_received += 1;
                audio.last_frame_at_ms = Some(chrono::Utc::now().timestamp_millis());
                audio.capture_state = "receiving".into();
                audio.asr_ready = transcriber.is_some();
                audio.kws_ready = kws.is_initialized();
                let rms = if chunk.is_empty() {
                    0.0
                } else {
                    (chunk.iter().map(|sample| sample * sample).sum::<f32>() / chunk.len() as f32)
                        .sqrt()
                };
                audio.rms_milli = (rms.clamp(0.0, 1.0) * 1000.0).round() as u16;
                audio.peak_milli = (chunk
                    .iter()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
                    .clamp(0.0, 1.0)
                    * 1000.0)
                    .round() as u16;
                if rms >= 0.001 || vad.is_speech_active() {
                    quiet_since = None;
                    quiet_input.store(false, Ordering::Release);
                } else {
                    let since = quiet_since.get_or_insert_with(std::time::Instant::now);
                    quiet_input.store(
                        since.elapsed() >= std::time::Duration::from_millis(500),
                        Ordering::Release,
                    );
                }
                if last_audio_publish.elapsed() >= std::time::Duration::from_millis(200) {
                    let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                        diagnostics: audio.clone(),
                    });
                    last_audio_publish = std::time::Instant::now();
                }
                if last_audio_log.elapsed() >= std::time::Duration::from_secs(5) {
                    info!(
                        frames = audio.frames_received,
                        rms_milli = audio.rms_milli,
                        peak_milli = audio.peak_milli,
                        vad_active = audio.speech_detected,
                        "voice_capture_heartbeat"
                    );
                    last_audio_log = std::time::Instant::now();
                }

                let current_state = {
                    let d = driver.lock().await;
                    d.current_state()
                };

                match current_state {
                    AcousticState::Idle => {
                        // Standby: feed audio chunk to Keyword Spotter
                        let mut hit = native_worker_frame!("kws_process", kws.process_pcm_chunk(&chunk).await).map(|r| r.keyword);
                        let mut wake_command = None;

                        // Stage-2: Cascade VAD + SenseVoice check if streaming KWS was borderline
                        if hit.is_none() {
                            let is_active = native_worker_frame!("vad_process", vad.process_frame(&chunk).await);
                            if is_active {
                                utterance_pcm.extend_from_slice(&chunk);
                                if utterance_pcm.len() > 16_000 * 4 {
                                    utterance_pcm.drain(0..16_000);
                                }
                            } else if !utterance_pcm.is_empty() {
                                if utterance_pcm.len() >= 4_000 {
                                    let snippet = std::mem::take(&mut utterance_pcm);
                                    #[cfg(feature = "native-fault-injection")]
                                    let cascade_context = NativeCallContext {
                                        request_id: "standby-cascade".into(),
                                        worker_epoch: event_tx.worker_epoch(),
                                        instance_id: event_tx.snapshot().instance_id,
                                    };
                                    if let Some(stt) = &transcriber {
                                        let transcribe_res = Self::transcribe_with_lease(
                                            stt.clone(),
                                            snippet,
                                            &native_busy,
                                            &native_degraded,
                                            &native_job_epoch,
                                            std::time::Duration::from_secs(30),
                                            None,
                                            Some(&native_admission),
                                            #[cfg(feature = "native-fault-injection")]
                                            Some(&cascade_context),
                                        )
                                        .await;
                                        match transcribe_res {
                                            Ok(Some(text)) => {
                                                if let Some(matched) = nomifun_audio::match_wake_phrase(&text) {
                                                    info!(matched = %matched.matched_phrase, "Cascade wake hit accepted via SenseVoice");
                                                    wake_command = matched.remainder;
                                                    hit = Some(matched.matched_phrase);
                                                } else {
                                                    debug!(stage = "text_no_match", chars = text.chars().count(), "Cascade ASR transcription did not match wake phrase");
                                                }
                                            }
                                            Ok(None) => {
                                                debug!(stage = "asr_empty", "Cascade ASR returned no speech");
                                            }
                                            Err(err) => {
                                                warn!(stage = "asr_error", error = %err, "Cascade ASR transcription failed");
                                            }
                                        }
                                    }
                                } else {
                                    utterance_pcm.clear();
                                }
                                native_worker_call!("vad_reset", vad.reset().await, ());
                            }
                        }

                        if let Some(keyword) = hit {
                            info!(keyword = %keyword, "Wake hit detected");
                            if !oobe.is_paired().await {
                                warn!(stage = "host_rejected", reason = "not_paired", "Wake received but device is not yet paired via OOBE");
                                let _ = playback.play_earcon(EarconKind::Alert).await;
                            } else if !acoustic_ready.load(Ordering::Acquire) {
                                warn!(stage = "host_rejected", reason = "acoustic_not_ready", "Wake received but voice models are not yet installed");
                                let _ = playback.play_earcon(EarconKind::Alert).await;
                            } else if self_check_active.load(Ordering::Acquire)
                                || engine_check_active.load(Ordering::Acquire)
                                || native_busy.load(Ordering::Acquire)
                            {
                                info!(stage = "host_rejected", reason = "exclusively_owned", "Wake ignored while the runtime is exclusively owned");
                            } else if !admission_valid_for(&wake_admission_until_ms) {
                                info!(stage = "host_rejected", reason = "admission_expired", "Wake hit ignored because host readiness admission is absent or expired");
                            } else {
                                if Self::accept_wake(&driver, &playback, &event_tx, &keyword, None, event_tx.snapshot().locale.as_deref(), wake_command.is_none())
                                    .await
                                    .is_ok()
                                {
                                    info!(stage = "wake_success", keyword = %keyword, "Wake successfully accepted");
                                    observed_session = event_tx.snapshot().session_id;
                                    let wake_generation = playback.generation_id();
                                    native_worker_call!("vad_reset", vad.reset().await, ());
                                    utterance_pcm.clear();
                                    speech_started = false;
                                    listen_start = Some(std::time::Instant::now());
                                    audio.utterance_samples = 0;
                                    audio.utterance_peak_milli = 0;
                                    if let Some(command) = wake_command {
                                        let mut d = driver.lock().await;
                                        if event_tx.snapshot().session_id == observed_session
                                            && event_tx.snapshot().ended_reason.is_none()
                                            && playback.generation_id() == wake_generation
                                            && d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding) {
                                            let scope = event_tx.snapshot();
                                            let generation = playback.generation_id();
                                            drop(d);
                                            let _ = event_tx.send(VoiceCompanionEvent::Transcribing);
                                            Self::start_utterance(
                                                Vec::new(), driver.clone(), router.clone(), transcriber.clone(),
                                                playback.clone(), event_tx.clone(), tts.clone(), task_client.clone(),
                                                &active_turn, input_generation.clone(), generation,
                                                pending_utterances.clone(), scope.session_id,
                                                native_busy.clone(), native_degraded.clone(), native_job_epoch.clone(),
                                                native_admission.clone(), None, Some(command),
                                            );
                                            listen_start = None;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    AcousticState::ListeningConfirm=>{
                        if !barge_in_ready && playback.has_pending_audio() {
                            utterance_pcm.clear();
                            speech_started = false;
                            native_worker_call!("vad_reset", vad.reset().await, ());
                            continue;
                        }
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len()>8000 {let excess=utterance_pcm.len()-8000;utterance_pcm.drain(..excess);}
                        audio.utterance_samples=utterance_pcm.len() as u32;
                        audio.utterance_peak_milli=audio.utterance_peak_milli.max(audio.peak_milli);
                        let active=native_worker_frame!("vad_process", vad.process_frame(&chunk).await);speech_started|=active;
                        if speech_started && !active {
                            #[cfg(test)]
                            if let Some(gate) = &confirmation_speech_end_gate {
                                gate.reached.notify_one();
                                gate.resume.notified().await;
                            }
                            let response={let mut pending=pending_confirmation.lock().unwrap_or_else(|error|error.into_inner());pending.as_mut().and_then(|pending|pending.response.take().map(|response|(response,pending.generation,pending.deadline_ms,pending.approval_id.clone(),pending.session_id.clone())))};
                            if let Some((response,generation,deadline_ms,approval_id,session_id))=response {
                                let pcm=std::mem::take(&mut utterance_pcm);let stt=transcriber.clone();let output=playback.clone();let events=event_tx.clone();let states=driver.clone();let lease_busy=native_busy.clone();let lease_degraded=native_degraded.clone();let lease_epoch=native_job_epoch.clone();let lease_admission=native_admission.clone();
                                let reservation = lease_admission.reserve();
                                let pending_for_turn = pending_confirmation.clone();
                                let snapshot = events.snapshot();
                                let scope = VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id };
                                let turn=tokio::spawn(async move {
                                    let result=match stt {
                                        Some(stt)=>{
                                            #[cfg(feature = "native-fault-injection")]
                                            let confirmation_context = NativeCallContext {
                                                request_id: "confirmation".into(),
                                                worker_epoch: events.worker_epoch(),
                                                instance_id: events.snapshot().instance_id,
                                            };
                                            Self::transcribe_with_lease(
                                                stt,
                                                pcm,
                                                &lease_busy,
                                                &lease_degraded,
                                                &lease_epoch,
                                                std::time::Duration::from_millis(deadline_ms.saturating_sub(chrono::Utc::now().timestamp_millis()).max(0) as u64).min(std::time::Duration::from_secs(30)),
                                                Some(reservation),
                                                None,
                                                #[cfg(feature = "native-fault-injection")]
                                                Some(&confirmation_context),
                                            ).await.map_err(|reason| asr_failure_code(&reason).to_owned())
                                        },
                                        None=>Err("asr_unavailable".into()),
                                    };
                                    let guard=states.lock().await;
                                    let current=events.snapshot();
                                    let owned=pending_for_turn.lock().unwrap_or_else(|error|error.into_inner()).as_ref().is_some_and(|pending|pending.approval_id==approval_id && pending.session_id==session_id && pending.generation==generation);
                                    if owned && scope.matches(&current) && current.ended_reason.is_none() && output.generation_id()==generation && chrono::Utc::now().timestamp_millis()<deadline_ms && guard.current_state()==AcousticState::ListeningConfirm {
                                        if let Ok(Some(text))=&result {if !text.trim().is_empty() {let _=events.send_scoped(VoiceCompanionEvent::ConfirmationHeard {text:text.clone()}, &scope);}}
                                        let _=response.send(result);
                                    }
                                });
                                let mut active=active_turn.lock().unwrap_or_else(|error|error.into_inner());if let Some(previous)=active.replace(turn){previous.abort();}
                            }
                            speech_started=false;utterance_pcm.clear();native_worker_call!("vad_reset", vad.reset().await, ());
                        }
                    }
                    AcousticState::Listening => {
                        // Match follow-up listening: retain VAD onset, not the
                        // preceding silence, in the 30-second utterance budget.
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len() > 8_000 {
                            let excess = utterance_pcm.len() - 8_000;
                            utterance_pcm.drain(..excess);
                        }
                        audio.utterance_samples = utterance_pcm.len() as u32;
                        audio.utterance_peak_milli =
                            audio.utterance_peak_milli.max(audio.peak_milli);

                        // Feed frame to VAD detector
                        let is_active = native_worker_frame!("vad_process", vad.process_frame(&chunk).await);
                        if is_active {
                            if !speech_started {
                                info!(session_id = ?event_tx.snapshot().session_id, rms_milli = audio.rms_milli, "voice_speech_started");
                                driver.lock().await.mark_speech_started();
                                let _ = event_tx.send(VoiceCompanionEvent::SpeechStarted);
                            }
                            speech_started = true;
                        }
                        if speech_started && (!is_active || utterance_pcm.len() >= 16_000 * 30) {
                            #[cfg(any(test, feature = "voice-skeleton"))]
                            if let Some(gate) = ordinary_speech_end_gate.take() {
                                gate.reached.notify_one();
                                gate.resume.notified().await;
                            }
                            // User paused / end of utterance detected
                            info!(
                                samples = utterance_pcm.len(),
                                "SpeechEnd detected by VAD, transitioning to Deciding"
                            );
                            speech_started = false;
                            listen_start = None;

                            let mut d = driver.lock().await;
                            if let Ok(AcousticState::Deciding) =
                                d.handle_event(AcousticEvent::SpeechEnd)
                            {
                                let generation = playback.generation_id();
                                drop(d);
                                let _ = event_tx.send(VoiceCompanionEvent::Transcribing);
                                processing_feedback = event_tx
                                    .snapshot()
                                    .turn_id
                                    .map(|id| (id, std::time::Instant::now(), 0));
                                Self::start_utterance(
                                    std::mem::take(&mut utterance_pcm),
                                    driver.clone(),
                                    router.clone(),
                                    transcriber.clone(),
                                    playback.clone(),
                                    event_tx.clone(),
                                    tts.clone(),
                                    task_client.clone(),
                                    &active_turn,
                                    input_generation.clone(),
                                    generation,
                                    pending_utterances.clone(),
                                    session.clone(),
                                    native_busy.clone(),
                                    native_degraded.clone(),
                                    native_job_epoch.clone(),
                                    native_admission.clone(),
                                    None,
                                    None,
                                );
                                utterance_pcm.clear();
                                native_worker_call!("kws_reset", kws.reset().await, ());
                                native_worker_call!("vad_reset", vad.reset().await, ());
                            }
                        }
                    }

                    AcousticState::FollowUpListening => {
                        // 180-second follow-up window (seamless continuous dialogue without wake phrase)
                        if listen_start.is_none() {
                            listen_start = Some(std::time::Instant::now());
                            native_worker_call!("vad_reset", vad.reset().await, ());
                            utterance_pcm.clear();
                            speech_started = false;
                        }

                        // Keep a bounded onset window while VAD accumulates its
                        // evidence; speech begins before the active flag does.
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len() > 8_000 {
                            let excess = utterance_pcm.len() - 8_000;
                            utterance_pcm.drain(..excess);
                        }
                        audio.utterance_samples = utterance_pcm.len() as u32;
                        audio.utterance_peak_milli =
                            audio.utterance_peak_milli.max(audio.peak_milli);
                        let is_active = native_worker_frame!("vad_process", vad.process_frame(&chunk).await);
                        if is_active {
                            if !speech_started {
                                driver.lock().await.mark_speech_started();
                                let _ = event_tx.send(VoiceCompanionEvent::SpeechStarted);
                            }
                            speech_started = true;
                        }
                        if speech_started && (!is_active || utterance_pcm.len() >= 16_000 * 30) {
                            // User followed up with another utterance during the 4s window!
                            info!(
                                samples = utterance_pcm.len(),
                                "Follow-up speech ended, transitioning to Deciding"
                            );
                            speech_started = false;
                            listen_start = None;

                            let mut d = driver.lock().await;
                            if let Ok(AcousticState::Deciding) =
                                d.handle_event(AcousticEvent::FollowUpSpeech)
                            {
                                let generation = playback.generation_id();
                                drop(d);
                                let _ = event_tx.send(VoiceCompanionEvent::Transcribing);
                                processing_feedback = event_tx
                                    .snapshot()
                                    .turn_id
                                    .map(|id| (id, std::time::Instant::now(), 0));
                                Self::start_utterance(
                                    std::mem::take(&mut utterance_pcm),
                                    driver.clone(),
                                    router.clone(),
                                    transcriber.clone(),
                                    playback.clone(),
                                    event_tx.clone(),
                                    tts.clone(),
                                    task_client.clone(),
                                    &active_turn,
                                    input_generation.clone(),
                                    generation,
                                    pending_utterances.clone(),
                                    session.clone(),
                                    native_busy.clone(),
                                    native_degraded.clone(),
                                    native_job_epoch.clone(),
                                    native_admission.clone(),
                                    None,
                                    None,
                                );
                                utterance_pcm.clear();
                                native_worker_call!("kws_reset", kws.reset().await, ());
                                native_worker_call!("vad_reset", vad.reset().await, ());
                            }
                        }
                    }

                    AcousticState::SpeakingFast
                    | AcousticState::SpeakingControl
                    | AcousticState::SpeakingSummary => {
                        if !barge_in_ready || !playback.has_pending_audio() { continue; }
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len() > BARGE_IN_PREROLL_SAMPLES {
                            let excess = utterance_pcm.len() - BARGE_IN_PREROLL_SAMPLES;
                            utterance_pcm.drain(..excess);
                        }
                        audio.utterance_samples = utterance_pcm.len() as u32;
                        audio.utterance_peak_milli =
                            audio.utterance_peak_milli.max(audio.peak_milli);

                        let is_active = native_worker_frame!("vad_process", vad.process_frame(&chunk).await);
                        if is_active {
                            info!(
                                state = ?current_state,
                                rms_milli = audio.rms_milli,
                                "barge_in_playback_interrupted"
                            );
                            let mut d = driver.lock().await;
                            if d.current_state() != current_state { continue; }
                            if playback.interrupt().await.is_err() {
                                d.reset();
                                let _ = event_tx.send(VoiceCompanionEvent::Error { stage:"playback".into(), code:"speaker_failed".into() });
                                let _ = event_tx.send(VoiceCompanionEvent::SessionEnded { reason:VoiceSessionEndReason::Failed });
                                continue;
                            }
                            if let Ok(AcousticState::Listening) = d.handle_event(AcousticEvent::BargeIn) {
                                input_generation.fetch_add(1, Ordering::SeqCst);
                                d.mark_speech_started();
                                speech_started = true;
                                let _ = event_tx.send(VoiceCompanionEvent::SpeechStarted);
                                let _ = event_tx.send(VoiceCompanionEvent::Listening);
                                let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                                    diagnostics: audio.clone(),
                                });
                                last_audio_publish = std::time::Instant::now();
                            }
                        }
                    }

                    AcousticState::AwaitingConfirmation => {
                        if !barge_in_ready || !playback.has_pending_audio() { continue; }
                        utterance_pcm.extend_from_slice(&chunk);
                        if !speech_started && utterance_pcm.len() > BARGE_IN_PREROLL_SAMPLES {
                            let excess = utterance_pcm.len() - BARGE_IN_PREROLL_SAMPLES;
                            utterance_pcm.drain(..excess);
                        }
                        audio.utterance_samples = utterance_pcm.len() as u32;
                        audio.utterance_peak_milli =
                            audio.utterance_peak_milli.max(audio.peak_milli);

                        let is_active = native_worker_frame!("vad_process", vad.process_frame(&chunk).await);
                        if is_active {
                            info!(
                                rms_milli = audio.rms_milli,
                                "barge_in_confirmation_prompt_interrupted"
                            );
                            let mut d = driver.lock().await;
                            if d.current_state() != AcousticState::AwaitingConfirmation { continue; }
                            if playback.interrupt().await.is_err() {
                                d.reset();
                                let _ = event_tx.send(VoiceCompanionEvent::Error { stage:"playback".into(), code:"speaker_failed".into() });
                                let _ = event_tx.send(VoiceCompanionEvent::SessionEnded { reason:VoiceSessionEndReason::Failed });
                                continue;
                            }
                            if let Ok(AcousticState::ListeningConfirm) = d.handle_event(AcousticEvent::BargeIn) {
                                if let Some(pending) = pending_confirmation.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                                    pending.begin_listening();
                                }
                                d.mark_speech_started();
                                speech_started = true;
                                let _ = event_tx.send(VoiceCompanionEvent::SpeechStarted);
                                let _ = event_tx.send(VoiceCompanionEvent::AudioDiagnostics {
                                    diagnostics: audio.clone(),
                                });
                                last_audio_publish = std::time::Instant::now();
                            }
                        }
                    }

                    AcousticState::Deciding | AcousticState::DispatchingTask => {
                        let is_active = native_worker_frame!("vad_process", vad.process_frame(&chunk).await);
                        if is_active {
                            utterance_pcm.extend_from_slice(&chunk);
                            speech_started = true;
                        }
                        if speech_started && (!is_active || utterance_pcm.len() >= 16_000 * 30) {
                            #[cfg(any(test, feature = "voice-skeleton"))]
                            if let Some(gate) = PARENT_DISPATCH_QUEUE_GATE.get() {
                                gate.reached.notify_one();
                                gate.resume.notified().await;
                            }
                            speech_started = false;
                            let pcm = std::mem::take(&mut utterance_pcm);

                            // Atomic boundary: lock active_turn then pending_utterances in a pure synchronous scope
                            let (should_alert, next_to_start) = {
                                let act = active_turn.lock().unwrap_or_else(|e| e.into_inner());
                                let mut queue = pending_utterances.lock().unwrap_or_else(|e| e.into_inner());
                                let is_busy = act.as_ref().is_some_and(|h| !h.is_finished());
                                if queue.len() < 2 {
                                    queue.push_back(PendingUtterance {
                                        pcm,
                                        session_id: session.clone(),
                                        generation_id: playback.generation_id(),
                                    });
                                    let next = if !is_busy {
                                        queue.pop_front()
                                    } else {
                                        None
                                    };
                                    (false, next)
                                } else {
                                    (true, None)
                                }
                            };

                            if should_alert {
                                let _ = playback.play_earcon(EarconKind::Alert).await;
                                native_worker_call!("vad_reset", vad.reset().await, ());
                                continue;
                            }

                            if let Some(next_item) = next_to_start {
                                let mut d = driver.lock().await;
                                let can_decide = match d.current_state() {
                                    AcousticState::FollowUpListening => {
                                        d.handle_event(AcousticEvent::FollowUpSpeech) == Ok(AcousticState::Deciding)
                                    }
                                    AcousticState::Listening => {
                                        d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding)
                                    }
                                    AcousticState::Idle => {
                                        d.handle_event(AcousticEvent::WakeHit).is_ok()
                                            && d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding)
                                    }
                                    AcousticState::DispatchingTask => {
                                        let _ = d.handle_event(AcousticEvent::TaskAccepted);
                                        let _ = d.handle_event(AcousticEvent::PlaybackDone);
                                        d.handle_event(AcousticEvent::WakeHit).is_ok()
                                            && d.handle_event(AcousticEvent::SpeechEnd) == Ok(AcousticState::Deciding)
                                    }
                                    _ => false,
                                };
                                if can_decide {
                                    drop(d);
                                    let _ = event_tx.send(VoiceCompanionEvent::Transcribing);
                                    let next_gen = playback.generation_id();
                                    Self::start_utterance(
                                        next_item.pcm,
                                        driver.clone(),
                                        router.clone(),
                                        transcriber.clone(),
                                        playback.clone(),
                                        event_tx.clone(),
                                        tts.clone(),
                                        task_client.clone(),
                                        &active_turn,
                                        input_generation.clone(),
                                        next_gen,
                                        pending_utterances.clone(),
                                        next_item.session_id,
                                        native_busy.clone(),
                                        native_degraded.clone(),
                                        native_job_epoch.clone(),
                                        native_admission.clone(),
                                        None,
                                        None,
                                    );
                                } else {
                                    let mut queue = pending_utterances.lock().unwrap_or_else(|e| e.into_inner());
                                    queue.push_front(next_item);
                                }
                            }
                            native_worker_call!("vad_reset", vad.reset().await, ());
                        }
                    }
                }
                audio.speech_detected = vad.is_speech_active();
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomifun_voice_terminal::contract::AcousticState;
    use nomifun_voice_terminal::task_client::VoiceTaskClient;
    use tempfile::tempdir;

    #[derive(Default)]
    struct BlockingGateState {
        entered: bool,
        released: bool,
    }

    #[derive(Clone, Default)]
    struct BlockingGate {
        state: Arc<(std::sync::Mutex<BlockingGateState>, std::sync::Condvar)>,
        entries: Arc<AtomicU64>,
    }

    impl BlockingGate {
        fn block(&self) {
            let (lock, wake) = &*self.state;
            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.entered = true;
            self.entries.fetch_add(1, Ordering::AcqRel);
            wake.notify_all();
            while !state.released {
                state = wake.wait(state).unwrap_or_else(|error| error.into_inner());
            }
        }

        fn is_entered(&self) -> bool {
            self.state
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entered
        }

        fn rearm(&self) {
            let mut state = self
                .state
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.entered = false;
            state.released = false;
        }

        fn release(&self) {
            let (lock, wake) = &*self.state;
            let mut state = lock.lock().unwrap_or_else(|error| error.into_inner());
            state.released = true;
            wake.notify_all();
        }
    }

    struct ReleaseBlockingGateOnDrop(BlockingGate);
    impl Drop for ReleaseBlockingGateOnDrop {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    async fn wait_for_gate_entry(gate: &BlockingGate) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !gate.is_entered() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("blocking native test double must be entered");
    }

    async fn wait_for_native_release(busy: &AtomicBool) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while busy.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("released native job must relinquish its lease");
    }

    struct BlockingKws {
        gate: BlockingGate,
        calls: u64,
    }

    impl KwsBackend for BlockingKws {
        fn process_pcm_chunk(&mut self, _chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
            self.gate.block();
            self.calls += 1;
            Some(nomifun_audio::kws::KwsResult {
                keyword: format!("hit-{}", self.calls),
                score: 1.0,
                timestamp_ms: self.calls,
            })
        }

        fn reset(&mut self) {
            self.gate.block();
        }

        fn is_initialized(&self) -> bool {
            true
        }
    }

    struct DropBlockingKws {
        gate: BlockingGate,
    }

    impl KwsBackend for DropBlockingKws {
        fn process_pcm_chunk(&mut self, _chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
            None
        }

        fn reset(&mut self) {}

        fn is_initialized(&self) -> bool {
            false
        }
    }

    impl Drop for DropBlockingKws {
        fn drop(&mut self) {
            self.gate.block();
        }
    }

    struct HealthyKws;
    impl KwsBackend for HealthyKws {
        fn process_pcm_chunk(&mut self, _chunk: &[f32]) -> Option<nomifun_audio::kws::KwsResult> {
            None
        }

        fn reset(&mut self) {}

        fn is_initialized(&self) -> bool {
            true
        }
    }

    struct BlockingVad {
        gate: BlockingGate,
        calls: u64,
        active: bool,
    }

    impl VadBackend for BlockingVad {
        fn process_frame(&mut self, _samples: &[f32]) -> bool {
            self.gate.block();
            self.calls += 1;
            self.active = self.calls % 2 == 1;
            self.active
        }

        fn reset(&mut self) {
            self.gate.block();
            self.active = false;
        }

        fn is_speech_active(&self) -> bool {
            self.active
        }
    }

    #[test]
    fn startup_guard_fails_closed_before_any_device_assembly() {
        use std::ffi::OsStr;
        let absent = parse_startup_markers(None, None, None);
        assert_eq!(startup_mode(false, absent), Ok(StartupMode::Default));
        for value in ["", "0", "false", "unknown"] {
            let marker = parse_startup_markers(Some(OsStr::new(value)), None, None);
            assert!(marker.headless_present && !marker.headless_valid);
            assert!(startup_mode(false, marker).is_err(), "default must reject {value:?}");
            assert!(startup_mode(true, marker).is_err(), "feature must require headless=1");
            let block = parse_startup_markers(None, Some(OsStr::new(value)), None);
            assert!(startup_mode(false, block).is_err(), "default must reject block marker {value:?}");
            let exit_code = parse_startup_markers(None, None, Some(OsStr::new(value)));
            assert!(startup_mode(false, exit_code).is_err(), "default must reject exit marker {value:?}");
        }
        let valid = parse_startup_markers(
            Some(OsStr::new("1")),
            Some(OsStr::new("1")),
            Some(OsStr::new("1")),
        );
        assert_eq!(startup_mode(true, valid), Ok(StartupMode::HeadlessTest));
        assert!(startup_mode(false, valid).is_err());
        let feature_no_marker = parse_startup_markers(None, None, None);
        assert!(startup_mode(true, feature_no_marker).is_err());
        let combined = parse_startup_markers(None, Some(OsStr::new("1")), Some(OsStr::new("1")));
        assert!(startup_mode(false, combined).is_err());
    }

    #[test]
    fn sequential_reservation_retires_old_generation_before_new_owner_can_claim() {
        for _ in 0..100_000 {
            let busy = Arc::new(AtomicBool::new(false));
            let epoch = Arc::new(AtomicU64::new(0));
            assert!(busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok());
            let a_job = epoch.fetch_add(1, Ordering::AcqRel) + 1;
            let a = NativeBusyGuard::attach(busy.clone(), epoch.clone(), a_job);
            assert!(busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err(), "B must not acquire A's lease");
            drop(a);
            assert!(busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok());
            let b_job = epoch.fetch_add(1, Ordering::AcqRel) + 1;
            assert_eq!(epoch.load(Ordering::Acquire), b_job, "old drop must not retire B generation");
            let b = NativeBusyGuard::attach(busy.clone(), epoch.clone(), b_job);
            assert_eq!(epoch.load(Ordering::Acquire), b_job);
            drop(b);
        }
    }

    #[test]
    fn native_recovery_requires_the_same_supervised_active_job() {
        assert!(native_job_needs_recovery(true, true, 7, 7));
        assert!(!native_job_needs_recovery(false, true, 7, 7));
        assert!(!native_job_needs_recovery(true, false, 7, 7));
        assert!(!native_job_needs_recovery(true, true, 9, 7));
        assert!(native_error_requires_degraded("engine_check_timeout"));
        assert!(native_error_requires_degraded("native_call_join_failed:panic"));
        assert!(native_error_requires_degraded("worker_asr_join_failed:panic"));
        assert!(!native_error_requires_degraded("engine_check_cancelled"));
        assert!(!native_error_requires_degraded("engine_check_native_busy"));
    }

    #[test]
    fn native_health_rejects_stale_jobs_and_refines_the_current_job() {
        let health = NativeHealth::default();
        health.publish(8, false);
        health.publish(7, true);
        assert!(!health.is_degraded(), "old failure must not degrade healthy B");

        health.publish(9, true);
        health.publish(8, false);
        assert!(health.is_degraded(), "old recovery must not clear degraded B");

        health.publish(10, false);
        health.publish_result(10, &Err::<(), _>("loader_failed"));
        assert!(health.is_degraded(), "inner loader errors refine the same job");
        health.publish(9, false);
        assert!(health.is_degraded());
    }

    #[tokio::test]
    async fn native_load_timeout_retains_ownership_and_fences_late_result() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let health = Arc::new(NativeHealth::default());
        let runner = NativeCallRunner::with_health(busy.clone(), epoch.clone(), health.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let first = {
            let runner = runner.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                runner
                    .load(std::time::Duration::from_millis(500), move || {
                        gate.block();
                        Ok::<_, String>("late-a")
                    })
                    .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let first_job = epoch.load(Ordering::Acquire);
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(2), first)
            .await
            .expect("first native load must have a bounded caller")
            .unwrap();
        assert_eq!(first_result.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(health.is_degraded());
        assert!(native_job_needs_recovery(true, true, epoch.load(Ordering::Acquire), first_job));
        assert!(!native_job_needs_recovery(false, true, epoch.load(Ordering::Acquire), first_job));

        let rejected = runner
            .load_until(
                std::time::Instant::now() + std::time::Duration::from_millis(20),
                cancel.as_ref(),
                notify.as_ref(),
                false,
                || Ok::<_, String>("must-not-run"),
            )
            .await;
        assert_eq!(rejected.unwrap_err(), "engine_check_native_busy");
        assert_eq!(gate.entries.load(Ordering::Acquire), 1);

        gate.release();
        wait_for_native_release(&busy).await;
        assert!(health.is_degraded(), "late A must not publish recovery");
        gate.rearm();
        let second = {
            let runner = runner.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                runner
                    .load(std::time::Duration::from_secs(1), move || {
                        gate.block();
                        Ok::<_, String>("healthy-b")
                    })
                    .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let second_job = epoch.load(Ordering::Acquire);
        assert_ne!(second_job, first_job);
        assert!(busy.load(Ordering::Acquire));
        assert!(!native_job_needs_recovery(true, true, second_job, first_job));
        gate.release();
        let second_result = tokio::time::timeout(std::time::Duration::from_secs(2), second)
            .await
            .expect("healthy successor must finish")
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(second_result, "healthy-b");
        assert!(!health.is_degraded());
        assert_eq!(gate.entries.load(Ordering::Acquire), 2);
        wait_for_native_release(&busy).await;
    }

    #[tokio::test]
    async fn native_detached_caller_cannot_release_running_job() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let runner = NativeCallRunner::new(busy.clone(), epoch.clone());
        let first = {
            let runner = runner.clone();
            let gate = gate.clone();
            tokio::spawn(async move {
                let cancel = AtomicBool::new(false);
                let notify = Notify::new();
                runner
                    .run(
                        std::time::Instant::now() + std::time::Duration::from_secs(30),
                        &cancel,
                        &notify,
                        true,
                        true,
                        #[cfg(feature = "native-fault-injection")]
                        None,
                        move || gate.block(),
                    )
                    .await
            })
        };
        wait_for_gate_entry(&gate).await;
        first.abort();
        let _ = first.await;
        assert!(busy.load(Ordering::Acquire));
        gate.release();
        wait_for_native_release(&busy).await;
        runner
            .run_uncancelable(std::time::Duration::from_secs(1), false, || "healthy-b")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn native_kws_frame_timeout_does_not_deliver_late_wake_to_successor() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let kws = Arc::new(NativeKws::new(
            Box::new(BlockingKws { gate: gate.clone(), calls: 0 }),
            NativeCallRunner::new(busy.clone(), epoch.clone()),
        ));
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let first = {
            let kws = kws.clone();
            let cancel = cancel.clone();
            let notify = notify.clone();
            tokio::spawn(async move {
                kws.process_pcm_chunk_until(
                    &[0.1; 160],
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    cancel.as_ref(),
                    notify.as_ref(),
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let first_job = epoch.load(Ordering::Acquire);
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .expect("KWS caller must be bounded")
            .unwrap();
        assert_eq!(first_result.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));

        gate.release();
        wait_for_native_release(&busy).await;
        gate.rearm();
        let second = {
            let kws = kws.clone();
            tokio::spawn(async move { kws.process_pcm_chunk(&[0.1; 160]).await })
        };
        wait_for_gate_entry(&gate).await;
        assert_ne!(epoch.load(Ordering::Acquire), first_job);
        assert!(!native_job_needs_recovery(
            true,
            true,
            epoch.load(Ordering::Acquire),
            first_job,
        ));
        gate.release();
        let second_hit = tokio::time::timeout(std::time::Duration::from_secs(1), second)
            .await
            .expect("healthy KWS successor must finish")
            .unwrap()
            .unwrap()
            .expect("healthy KWS successor returns its own hit");
        assert_eq!(second_hit.keyword, "hit-2");
        assert_eq!(gate.entries.load(Ordering::Acquire), 2);
        wait_for_native_release(&busy).await;
    }

    #[tokio::test]
    async fn native_vad_frame_timeout_does_not_publish_late_active_state() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let vad = Arc::new(NativeVad::new(
            Box::new(BlockingVad { gate: gate.clone(), calls: 0, active: false }),
            NativeCallRunner::new(busy.clone(), epoch.clone()),
        ));
        let first = {
            let vad = vad.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let notify = Arc::new(Notify::new());
            tokio::spawn(async move {
                vad.process_frame_until(
                    &[0.1; 160],
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    cancel.as_ref(),
                    notify.as_ref(),
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let first_result = tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .expect("VAD caller must be bounded")
            .unwrap();
        assert_eq!(first_result.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(!vad.is_speech_active());

        gate.release();
        wait_for_native_release(&busy).await;
        gate.rearm();
        let second = {
            let vad = vad.clone();
            tokio::spawn(async move { vad.process_frame(&[0.1; 160]).await })
        };
        wait_for_gate_entry(&gate).await;
        gate.release();
        let second_active = tokio::time::timeout(std::time::Duration::from_secs(1), second)
            .await
            .expect("healthy VAD successor must finish")
            .unwrap()
            .unwrap();
        assert!(!second_active, "successor returns its own second-call result, not late A");
        assert!(!vad.is_speech_active());
        assert_eq!(gate.entries.load(Ordering::Acquire), 2);
        wait_for_native_release(&busy).await;
    }

    #[tokio::test]
    async fn native_kws_and_vad_reset_keep_ownership_until_actual_return() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let runner = NativeCallRunner::new(busy.clone(), epoch.clone());

        let kws_gate = BlockingGate::default();
        let _kws_cleanup = ReleaseBlockingGateOnDrop(kws_gate.clone());
        let kws = Arc::new(NativeKws::new(
            Box::new(BlockingKws { gate: kws_gate.clone(), calls: 0 }),
            runner.clone(),
        ));
        let kws_reset = {
            let kws = kws.clone();
            tokio::spawn(async move {
                let cancel = AtomicBool::new(false);
                let notify = Notify::new();
                kws.reset_until(
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    &cancel,
                    &notify,
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&kws_gate).await;
        let timed = tokio::time::timeout(std::time::Duration::from_secs(1), kws_reset)
            .await
            .expect("reset deadline must bound the caller")
            .unwrap();
        assert_eq!(timed.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        kws_gate.release();
        wait_for_native_release(&busy).await;
        kws.reset().await.unwrap();

        let vad_gate = BlockingGate::default();
        let _vad_cleanup = ReleaseBlockingGateOnDrop(vad_gate.clone());
        let vad = Arc::new(NativeVad::new(
            Box::new(BlockingVad { gate: vad_gate.clone(), calls: 0, active: true }),
            runner,
        ));
        vad.active.store(true, Ordering::Release);
        let vad_reset = {
            let vad = vad.clone();
            tokio::spawn(async move {
                let cancel = AtomicBool::new(false);
                let notify = Notify::new();
                vad.reset_until(
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                    &cancel,
                    &notify,
                    true,
                )
                .await
            })
        };
        wait_for_gate_entry(&vad_gate).await;
        let timed = tokio::time::timeout(std::time::Duration::from_secs(1), vad_reset)
            .await
            .expect("reset deadline must bound the caller")
            .unwrap();
        assert_eq!(timed.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(vad.is_speech_active(), "a late reset must not be published as completed");
        vad_gate.release();
        wait_for_native_release(&busy).await;
        vad.reset().await.unwrap();
        assert!(!vad.is_speech_active());
    }

    #[tokio::test]
    async fn native_kws_reload_installs_a_healthy_successor() {
        let gate = BlockingGate::default();
        gate.release();
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let kws = NativeKws::new(
            Box::new(BlockingKws { gate, calls: 0 }),
            NativeCallRunner::new(busy, epoch),
        );
        kws.replace(Box::new(HealthyKws)).await.unwrap();
        assert!(kws.is_initialized());
        assert!(kws.process_pcm_chunk(&[0.1; 160]).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn native_kws_reload_timeout_keeps_ownership_through_old_engine_drop() {
        let gate = BlockingGate::default();
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let kws = Arc::new(NativeKws::new(
            Box::new(DropBlockingKws { gate: gate.clone() }),
            NativeCallRunner::new(busy.clone(), epoch),
        ));
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        assert!(!kws.is_initialized());
        let reload = {
            let kws = kws.clone();
            tokio::spawn(async move {
                kws.replace_until(
                    Box::new(HealthyKws),
                    std::time::Instant::now() + std::time::Duration::from_millis(500),
                )
                .await
            })
        };
        wait_for_gate_entry(&gate).await;
        let timed = tokio::time::timeout(std::time::Duration::from_secs(1), reload)
            .await
            .expect("reload caller must be bounded by the old engine destructor")
            .unwrap();
        assert_eq!(timed.unwrap_err(), "engine_check_timeout");
        assert!(busy.load(Ordering::Acquire));
        assert!(kws.is_initialized(), "the installed engine and cache must agree");

        gate.release();
        wait_for_native_release(&busy).await;
        assert!(kws.process_pcm_chunk(&[0.1; 160]).await.unwrap().is_none());
    }

    #[cfg(feature = "native-fault-injection")]
    #[test]
    fn concurrent_completion_never_clears_new_owner_observation() {
        use std::sync::Barrier;
        const ITERATIONS: usize = 100_000;
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let start = Arc::new(Barrier::new(2));
        let done = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let a_busy = busy.clone();
            let a_epoch = epoch.clone();
            let a_start = start.clone();
            let a_done = done.clone();
            let a = scope.spawn(move || {
                for _ in 0..ITERATIONS {
                    let job = a_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                    let owner = NativeBusyGuard::attach(a_busy.clone(), a_epoch.clone(), job);
                    a_start.wait();
                    drop(owner);
                    a_done.wait();
                }
            });
            let b_busy = busy.clone();
            let b_epoch = epoch.clone();
            let b_start = start.clone();
            let b_done = done.clone();
            let b = scope.spawn(move || {
                for _ in 0..ITERATIONS {
                    b_start.wait();
                    while b_busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                        std::hint::spin_loop();
                    }
                    let job = b_epoch.fetch_add(1, Ordering::AcqRel) + 1;
                    let owner = NativeBusyGuard::attach(b_busy.clone(), b_epoch.clone(), job);
                    assert_eq!(FAULT_NATIVE_BUSY.load(Ordering::Acquire), true, "old owner cleared B observation");
                    assert_eq!(FAULT_CURRENT_EPOCH.load(Ordering::Acquire), job);
                    drop(owner);
                    b_done.wait();
                }
            });
            a.join().unwrap();
            b.join().unwrap();
        });
    }

    #[cfg(feature = "native-fault-injection")]
    #[test]
    fn fault_observation_tracks_current_lease_not_stale_history() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        bind_fault_boundary("lease-a", 1, "instance-a");
        let first = NativeBusyGuard::new(busy.clone(), epoch.clone());
        record_fault_job_entry(1, None);
        let active = fault_job_observation();
        assert_eq!(active["native_busy"], serde_json::json!(true));
        assert_eq!(active["job"]["request_id"], serde_json::json!("lease-a"));
        assert_eq!(active["job"]["still_blocking"], serde_json::json!(true));
        drop(first);
        let completed = fault_job_observation();
        assert_eq!(completed["native_busy"], serde_json::json!(false));
        assert_eq!(completed["job"]["still_blocking"], serde_json::json!(false), "completed history must not remain active");
        let second = NativeBusyGuard::new(busy.clone(), epoch.clone());
        let stale_a_new_b = fault_job_observation();
        assert_eq!(stale_a_new_b["native_busy"], serde_json::json!(true));
        assert_eq!(stale_a_new_b["job"]["request_id"], serde_json::json!("lease-a"));
        assert_eq!(stale_a_new_b["job"]["still_blocking"], serde_json::json!(false), "A history must not claim B ownership");
        drop(second);
    }

    #[tokio::test]
    async fn test_voice_runtime_assembly() {
        let dir = tempdir().unwrap();
        let model_manager = Arc::new(ModelManager::new(dir.path()));
        let oobe_manager = Arc::new(OobeManager::new_bypassed());

        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, mut playback_rx) = VoiceCompanionRuntime::from_capture(
            model_manager.clone(),
            oobe_manager.clone(),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );

        // Verify driver state initialized to Idle
        {
            let d = runtime.driver.lock().await;
            assert_eq!(d.current_state(), AcousticState::Idle);
        }

        // Verify audio pipeline playback sends frames into rx
        runtime
            .playback()
            .play_earcon(EarconKind::Wake)
            .await
            .unwrap();
        let frame = playback_rx.recv().await;
        assert!(frame.is_some());
        assert!(!frame.unwrap().is_empty());

        // Verify worker task spawns and pumps cleanly
        let handle = runtime.start_worker();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert!(!handle.is_finished());

        handle.abort();
    }

    struct FailingStt;
    #[async_trait::async_trait]
    impl SttCallback for FailingStt {
        async fn transcribe(
            &self,
            _channel: AudioChannel,
            _pcm: Vec<f32>,
            _sample_rate: u32,
        ) -> Option<String> {
            None
        }
    }

    fn fixture_pcm(name: &str) -> Vec<f32> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/architecture/voice-agent/execution/audio-fixtures").join(name);
        let bytes = std::fs::read(path).unwrap();
        let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&bytes);
        assert_eq!(rate, 16_000, "fixture must be 16 kHz");
        pcm
    }

    #[tokio::test]
    #[ignore = "requires the real D:/tmp/flowy-dev/models suite and repository audio fixtures"]
    async fn actual_worker_engine_failure_is_not_masked_by_a_fresh_reload() {
        let models_dir = std::path::PathBuf::from("D:/tmp/flowy-dev/models");
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        assert!(models_dir.join("kws/encoder.int8.onnx").is_file());
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(&models_dir)),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(FailingStt),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime);
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(runtime.worker_epoch.load(Ordering::Acquire) > 0);

        let fixtures = SelfCheckAudio {
            wake: fixture_pcm("wake-jenny-16k.wav"),
            command: fixture_pcm("command-16k.wav"),
            non_wake: fixture_pcm("non-wake-16k.wav"),
            source: "test_fixture".into(),
            online: false,
            generated_at_ms: None,
        };
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime
            .engine_check_tx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .unwrap()
            .send(WorkerEngineCheck {
                request_id: "actual-worker-failure".into(),
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(20),
                cancel: Arc::new(AtomicBool::new(false)),
                cancel_notify: Arc::new(Notify::new()),
                native_busy: runtime.engine_check_native_busy.clone(),
                native_job_epoch: runtime.native_job_epoch.clone(),
                fixtures,
                reply: reply_tx,
            })
            .await
            .unwrap();
        let result = reply_rx.await.unwrap().expect("worker ran the check");
        assert!(result.kws_ok, "the actual worker KWS must be exercised");
        assert!(result.vad_ok, "the actual worker VAD must be exercised");
        assert!(!result.asr_ok, "the failing actual worker transcriber must not be replaced");
        let reload = tokio::task::spawn_blocking(move || {
            nomifun_audio::stt::try_load_sherpa(Some(&models_dir))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(reload.is_some(), "a transient reload would have succeeded");
        worker.abort();
        let _ = worker.await;
        producer.close();
    }

    #[tokio::test]
    async fn voice_worker_waits_for_verified_assets_before_native_initialization() {
        let dir = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(dir.path()));
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            models, Arc::new(OobeManager::new_bypassed()), Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback), Arc::new(QwenVoiceRouter::new()),
        );
        runtime.replay_capture = false;
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let waiting = !worker.is_finished();
        let ready = runtime.snapshot().acoustic_ready;
        worker.abort(); let _ = worker.await; producer.close();
        assert!(waiting, "missing assets must wait for provisioning, not open native engines and exit");
        assert!(!ready);
    }

    struct MockSpeech(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for MockSpeech {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    struct CountingStt(Arc<AtomicUsize>);
    #[async_trait::async_trait]
    impl SttCallback for CountingStt {
        async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some("你好".into())
        }
    }

    #[tokio::test]
    async fn worker_transcribes_two_turns_after_transient_native_contention() {
        use nomifun_voice_terminal::driver::StubVoiceRouter;
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(CountingStt(calls.clone())),
            Arc::new(StubVoiceRouter::chat("你好呀")),
        );
        let gate = Arc::new(SpeechEndTestGate::default());
        runtime.ordinary_speech_end_gate = Some(gate.clone());
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        let worker = runtime.start_worker();
        runtime.handle_wake().await.unwrap();
        let session = runtime.snapshot().session_id;
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), gate.reached.notified()).await.unwrap();
        let runner = NativeCallRunner::with_admission(runtime.engine_check_native_busy.clone(), runtime.native_job_epoch.clone(), runtime.native_degraded.clone(), runtime.native_admission.clone(), false);
        let owner = runner.acquire(std::time::Instant::now() + std::time::Duration::from_secs(1), &AtomicBool::new(false), &Notify::new(), false).await.unwrap();
        gate.resume.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(owner);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while runtime.snapshot().assistant_text.as_deref() != Some("你好呀") { tokio::time::sleep(std::time::Duration::from_millis(10)).await; }
        }).await.unwrap();
        let first = runtime.snapshot();
        assert_eq!(first.user_text.as_deref(), Some("你好"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while runtime.snapshot().conversation.iter().filter(|turn| turn.user_text.as_deref() == Some("你好")).count() < 2 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        let second = runtime.snapshot();
        worker.abort();
        producer.close();
        assert_eq!(second.session_id, session);
        assert_ne!(second.turn_id, first.turn_id);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(second.conversation.iter().filter(|turn| turn.user_text.as_deref() == Some("你好")).count() >= 2);
    }

    #[tokio::test]
    async fn confirmation_speech_waits_for_a_transient_native_owner() {
        use nomifun_api_types::voice::{VoiceSessionControlRequest, VoiceSessionPhase, VoiceTaskReference, VoiceToolConfirmationPrompt};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink};

        for (answer, approved) in [("确认", true), ("取消", false)] {
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always(answer)),
            Arc::new(StubVoiceRouter::chat("unused")),
        );
        let gate = Arc::new(SpeechEndTestGate::default());
        runtime.confirmation_speech_end_gate = Some(gate.clone());
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        ));
        let worker = runtime.start_worker();
        let snapshot = runtime.snapshot();
        let prompt = VoiceToolConfirmationPrompt {
            scope: VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id },
            task: VoiceTaskReference { command_id: "command".into(), conversation_id: Some("conversation".into()), message_id: Some("message".into()) },
            approval_id: "approval".into(), msg_id: "tool-message".into(), call_id: "call".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 8000,
            text: "请确认。".into(),
        };
        let request = { let runtime = runtime.clone(); tokio::spawn(async move { runtime.request_tool_confirmation(prompt).await }) };
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.expect("confirmation must start listening");
        for frame in 0..75 {
            let pcm: Vec<f32> = (0..320).map(|sample| if frame < 25 {
                0.03 * (sample as f32 * std::f32::consts::TAU * 200.0 / 16000.0).sin()
            } else { 0.0 }).collect();
            assert_eq!(producer.push(&pcm), pcm.len());
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(3), gate.reached.notified()).await.expect("worker must reach confirmation speech end");
        let runner = NativeCallRunner::with_admission(
            runtime.engine_check_native_busy.clone(), runtime.native_job_epoch.clone(),
            runtime.native_degraded.clone(), runtime.native_admission.clone(), false,
        );
        let owner = runner.acquire(
            std::time::Instant::now() + std::time::Duration::from_secs(1),
            &AtomicBool::new(false), &Notify::new(), true,
        ).await.unwrap();
        gate.resume.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        drop(owner);
        let reply = tokio::time::timeout(std::time::Duration::from_secs(8), request).await.unwrap().unwrap().unwrap();
        let snapshot = runtime.snapshot();
        worker.abort();
        assert_eq!(reply.approved, approved, "the spoken confirmation decision must survive transient contention");
        assert_eq!(snapshot.user_text.as_deref(), Some(answer));
        }
    }

    #[tokio::test]
    async fn empty_confirmation_is_denied_with_a_visible_recognition_error() {
        use nomifun_api_types::voice::{VoiceSessionControlRequest, VoiceSessionPhase, VoiceTaskReference, VoiceToolConfirmationPrompt};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink};
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(nomifun_audio::stt::FakeSttCallback::always("")),
            Arc::new(StubVoiceRouter::chat("unused")),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        ));
        let worker = runtime.start_worker();
        let snapshot = runtime.snapshot();
        let prompt = VoiceToolConfirmationPrompt {
            scope: VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id },
            task: VoiceTaskReference { command_id: "command".into(), conversation_id: Some("conversation".into()), message_id: Some("message".into()) },
            approval_id: "approval".into(), msg_id: "tool-message".into(), call_id: "call".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 4000, text: "请确认。".into(),
        };
        let request = { let runtime = runtime.clone(); tokio::spawn(async move { runtime.request_tool_confirmation(prompt).await }) };
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let reply = tokio::time::timeout(std::time::Duration::from_secs(5), request).await.unwrap().unwrap().unwrap();
        let snapshot = runtime.snapshot();
        worker.abort();
        assert!(!reply.approved);
        assert_eq!(snapshot.error.as_ref().map(|error| error.code.as_str()), Some("asr_empty_result"));
    }

    #[tokio::test]
    async fn stopping_at_confirmation_speech_end_cannot_authorize_a_new_session() {
        use nomifun_api_types::voice::{VoiceSessionControlRequest, VoiceSessionPhase, VoiceTaskReference, VoiceToolConfirmationPrompt};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink};
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (mut runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())), Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)), Arc::new(nomifun_audio::stt::FakeSttCallback::always("确认")),
            Arc::new(StubVoiceRouter::chat("unused")),
        );
        let gate = Arc::new(SpeechEndTestGate::default());
        runtime.confirmation_speech_end_gate = Some(gate.clone());
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        ));
        let worker = runtime.start_worker();
        runtime.handle_wake().await.unwrap();
        let snapshot = runtime.snapshot();
        let prompt = VoiceToolConfirmationPrompt {
            scope: VoiceSessionControlRequest { instance_id: snapshot.instance_id, session_id: snapshot.session_id, turn_id: snapshot.turn_id },
            task: VoiceTaskReference { command_id: "command".into(), conversation_id: Some("conversation".into()), message_id: Some("message".into()) },
            approval_id: "approval".into(), msg_id: "tool-message".into(), call_id: "call".into(),
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 8000, text: "请确认。".into(),
        };
        let request = { let runtime = runtime.clone(); tokio::spawn(async move { runtime.request_tool_confirmation(prompt).await }) };
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while runtime.snapshot().phase != VoiceSessionPhase::ListeningConfirm { tokio::time::sleep(std::time::Duration::from_millis(5)).await; }
        }).await.unwrap();
        for index in 0..75 {
            assert_eq!(producer.push(&vec![if index < 25 { 0.1 } else { 0.0 }; 320]), 320);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.reached.notified()).await.unwrap();
        let old = runtime.snapshot();
        runtime.stop_session(&VoiceSessionControlRequest { instance_id: old.instance_id, session_id: old.session_id.clone(), turn_id: old.turn_id }).await.unwrap();
        let reply = tokio::time::timeout(std::time::Duration::from_secs(2), request).await.unwrap().unwrap().unwrap();
        assert!(!reply.approved);
        runtime.handle_wake().await.unwrap();
        let new = runtime.snapshot();
        assert_ne!(new.session_id, old.session_id);
        gate.resume.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let after = runtime.snapshot();
        worker.abort();
        producer.close();
        assert_eq!(after.session_id, new.session_id);
        assert!(after.error.is_none());
        assert!(after.user_text.is_none());
    }

    #[tokio::test]
    async fn stale_muted_playback_completion_cannot_advance_a_new_turn() {
        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("问题")),
            Arc::new(nomifun_voice_terminal::driver::StubVoiceRouter::chat("回答")),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        assert_eq!(runtime.driver.lock().await.handle_event(AcousticEvent::SpeechEnd), Ok(AcousticState::Deciding));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let old = runtime.snapshot();
        let old_scope = VoiceSessionControlRequest {
            instance_id: old.instance_id,
            session_id: old.session_id,
            turn_id: old.turn_id,
        };
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let new = runtime.snapshot();
        assert_ne!(new.turn_id, old_scope.turn_id);
        let generation = runtime.playback.generation_id();

        VoiceCompanionRuntime::finish_muted_playback(
            &runtime.driver,
            &runtime.playback,
            &runtime.event_tx,
            generation,
            &old_scope,
        ).await;

        let after = runtime.snapshot();
        assert_eq!(after.turn_id, new.turn_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Transcribing);
        assert_eq!(runtime.driver.lock().await.current_state(), AcousticState::Deciding);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_task_acceptance_playback_cannot_advance_a_new_turn() {
        use nomifun_api_types::voice::{VoiceTaskDelivery, VoiceTaskSnapshot, VoiceTaskStatus, VoiceTaskSubmitRequest};
        use nomifun_voice_terminal::{driver::StubVoiceRouter, playback::TestAudioSink, task_client::VoiceTaskClient};

        struct Gate {
            reached: Notify,
            resume: Notify,
        }
        struct GatedSpeech(Arc<Gate>);
        #[async_trait::async_trait]
        impl nomifun_voice_terminal::tts::TtsProvider for GatedSpeech {
            async fn synthesize_stream(
                &self,
                _: &str,
                _: &str,
            ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
                self.0.reached.notify_one();
                self.0.resume.notified().await;
                let (tx, rx) = tokio::sync::mpsc::channel(1);
                tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
                Ok(rx)
            }
        }
        struct TaskClient;
        #[async_trait::async_trait]
        impl VoiceTaskClient for TaskClient {
            async fn current(&self) -> Result<Option<VoiceTaskSnapshot>, String> { Ok(None) }
            async fn control(&self, _: nomifun_api_types::voice::VoiceTaskControlCommand) -> Result<VoiceTaskSnapshot, String> {
                Err("unused".into())
            }
            async fn submit(&self, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
                Ok(VoiceTaskDelivery {
                    freshly_accepted: true,
                    task: VoiceTaskSnapshot {
                        command_id: request.command.command_id,
                        session_id: request.command.session_id,
                        acoustic_turn_id: request.command.turn_id,
                        conversation_id: Some("conversation".into()),
                        message_id: Some("message".into()),
                        runtime_turn_id: Some("runtime-turn".into()),
                        status: VoiceTaskStatus::Accepted,
                        result_ok: None,
                        result_text: None,
                        error_code: None,
                    },
                })
            }
        }

        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("读取工作区文件")),
            Arc::new(StubVoiceRouter::new(serde_json::from_value(serde_json::json!({
                "intent_type":"agent_task", "task_goal":"读取工作区文件", "requires_confirmation":false
            })).unwrap())),
        );
        let gate = Arc::new(Gate { reached: Notify::new(), resume: Notify::new() });
        let runtime = runtime
            .with_tasks(Arc::new(TaskClient))
            .with_test_playback(
                Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
                Arc::new(GatedSpeech(gate.clone())),
            );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        assert_eq!(runtime.driver.lock().await.handle_event(AcousticEvent::SpeechEnd), Ok(AcousticState::Deciding));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let old = runtime.snapshot();
        let generation = runtime.playback.generation_id();
        VoiceCompanionRuntime::start_utterance(
            vec![0.1; 1600],
            runtime.driver.clone(),
            runtime.router.clone(),
            runtime.transcriber.clone(),
            runtime.playback.clone(),
            runtime.event_tx.clone(),
            runtime.tts.clone(),
            runtime.task_client.clone(),
            &runtime.active_turn,
            runtime.input_generation.clone(),
            generation,
            runtime.pending_utterances.clone(),
            old.session_id.clone(),
            runtime.engine_check_native_busy.clone(),
            runtime.native_degraded.clone(),
            runtime.native_job_epoch.clone(),
            runtime.native_admission.clone(),
            None,
            None,
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.reached.notified()).await.unwrap();

        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let new = runtime.snapshot();
        assert_ne!(new.turn_id, old.turn_id);
        gate.resume.notify_one();
        let turn = runtime.active_turn.lock().unwrap().take().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), turn).await.unwrap().unwrap();

        let after = runtime.snapshot();
        assert_eq!(after.turn_id, new.turn_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Transcribing);
        assert!(after.assistant_text.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_dialogue_error_cannot_change_a_new_session() {
        use nomifun_voice_terminal::{contract::VoiceDecision, driver::{RouteError, VoiceRouter}};

        struct Gate {
            reached: Notify,
            resume: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
        }
        struct UnavailableRouter(Arc<Gate>);
        #[async_trait::async_trait]
        impl VoiceRouter for UnavailableRouter {
            fn is_configured(&self) -> bool {
                if let Some(resume) = self.0.resume.lock().unwrap().take() {
                    self.0.reached.notify_one();
                    resume.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                }
                false
            }

            async fn route_with_status(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<VoiceDecision, RouteError> {
                panic!("an unavailable router must not be called")
            }
        }

        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        let gate = Arc::new(Gate {
            reached: Notify::new(),
            resume: std::sync::Mutex::new(Some(resume_rx)),
        });
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("旧会话问题")),
            Arc::new(UnavailableRouter(gate.clone())),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let generation = runtime.playback.generation_id();
        VoiceCompanionRuntime::start_utterance(
            vec![0.1; 160],
            runtime.driver.clone(),
            runtime.router.clone(),
            runtime.transcriber.clone(),
            runtime.playback.clone(),
            runtime.event_tx.clone(),
            runtime.tts.clone(),
            runtime.task_client.clone(),
            &runtime.active_turn,
            runtime.input_generation.clone(),
            generation,
            runtime.pending_utterances.clone(),
            runtime.snapshot().session_id,
            runtime.engine_check_native_busy.clone(),
            runtime.native_degraded.clone(),
            runtime.native_job_epoch.clone(),
            runtime.native_admission.clone(),
            None,
            None,
        );
        tokio::time::timeout(std::time::Duration::from_secs(3), gate.reached.notified()).await.unwrap();
        let old = runtime.snapshot();
        runtime.stop_session(&VoiceSessionControlRequest {
            instance_id: old.instance_id,
            session_id: old.session_id,
            turn_id: old.turn_id,
        }).await.unwrap();
        runtime.handle_wake().await.unwrap();
        let new = runtime.snapshot();
        resume_tx.send(()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let after = runtime.snapshot();
        assert_eq!(after.session_id, new.session_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Listening);
        assert!(after.error.is_none(), "late old-turn error leaked into the new session: {:?}", after.error);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_history_failure_cannot_end_a_new_session_after_interrupt() {
        use nomifun_voice_terminal::{contract::VoiceDecision, driver::{RouteError, VoiceRouter}, playback::TestAudioSink};

        struct Gate {
            reached: Notify,
            resume: Notify,
        }
        struct RecordFailureRouter(Arc<Gate>);
        #[async_trait::async_trait]
        impl VoiceRouter for RecordFailureRouter {
            fn is_configured(&self) -> bool { true }
            async fn route_with_status(
                &self,
                _: &str,
                _: Option<&str>,
            ) -> Result<VoiceDecision, RouteError> {
                self.0.reached.notify_one();
                self.0.resume.notified().await;
                Err(RouteError::Record {
                    code: "binding_invalid".into(),
                    content: "old turn record failure".into(),
                })
            }
        }

        let root = tempdir().unwrap();
        let (capture, _producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let gate = Arc::new(Gate { reached: Notify::new(), resume: Notify::new() });
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("旧会话问题")),
            Arc::new(RecordFailureRouter(gate.clone())),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(AtomicUsize::new(0)))),
        );
        runtime.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":false})).unwrap()).unwrap();
        runtime.handle_wake().await.unwrap();
        assert_eq!(runtime.driver.lock().await.handle_event(AcousticEvent::SpeechEnd), Ok(AcousticState::Deciding));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::Transcribing);
        let old = runtime.snapshot();
        let generation = runtime.playback.generation_id();
        VoiceCompanionRuntime::start_utterance(
            vec![0.1; 160],
            runtime.driver.clone(),
            runtime.router.clone(),
            runtime.transcriber.clone(),
            runtime.playback.clone(),
            runtime.event_tx.clone(),
            runtime.tts.clone(),
            runtime.task_client.clone(),
            &runtime.active_turn,
            runtime.input_generation.clone(),
            generation,
            runtime.pending_utterances.clone(),
            old.session_id.clone(),
            runtime.engine_check_native_busy.clone(),
            runtime.native_degraded.clone(),
            runtime.native_job_epoch.clone(),
            runtime.native_admission.clone(),
            None,
            None,
        );
        tokio::time::timeout(std::time::Duration::from_secs(2), gate.reached.notified()).await.unwrap();

        let mut new_driver = runtime.driver.lock().await;
        gate.resume.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while runtime.playback.generation_id() == generation {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        new_driver.reset();
        assert_eq!(new_driver.handle_event(AcousticEvent::WakeHit), Ok(AcousticState::Listening));
        let _ = runtime.event_tx.send(VoiceCompanionEvent::WakeHit {
            keyword: "Hey Flowy".into(),
            request_id: None,
            locale: None,
        });
        let new = runtime.snapshot();
        assert_ne!(new.session_id, old.session_id);
        drop(new_driver);
        let turn = runtime.active_turn.lock().unwrap().take().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), turn).await.unwrap().unwrap();

        let after = runtime.snapshot();
        assert_eq!(after.session_id, new.session_id);
        assert_eq!(after.phase, nomifun_api_types::voice::VoiceSessionPhase::Listening);
        assert!(after.ended_reason.is_none(), "late old-turn history failure ended the new session");
        assert_eq!(runtime.driver.lock().await.current_state(), AcousticState::Listening);
    }

    struct RecordingSpeech(Arc<std::sync::Mutex<Vec<(String, String)>>>);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for RecordingSpeech {
        async fn synthesize_stream(
            &self,
            text: &str,
            voice: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            self.0.lock().unwrap().push((text.to_owned(), voice.to_owned()));
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    #[derive(Clone, Copy)]
    enum InvalidSpeech {
        Silent,
        NonFinite,
        TooShort,
        Empty,
    }

    struct InvalidSpeechProvider(InvalidSpeech);
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for InvalidSpeechProvider {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            let pcm = match self.0 {
                InvalidSpeech::Silent => vec![0.0; 2400],
                InvalidSpeech::NonFinite => vec![f32::NAN; 2400],
                InvalidSpeech::TooShort => vec![0.05; 100],
                InvalidSpeech::Empty => Vec::new(),
            };
            tx.try_send(nomifun_audio::pcm_to_wav(&pcm, 24000)).unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn worker_engine_check_cancel_and_deadline_fail_before_inference() {
        let root = tempdir().unwrap();
        let kws = KwsEngine::new(nomifun_audio::kws::KwsEngineConfig {
            model_dir: Some(root.path().to_path_buf()),
            ..Default::default()
        })
        .unwrap();
        let vad = EnergyVad::new(VadConfig::default());
        let busy = Arc::new(AtomicBool::new(false));
        let job_epoch = Arc::new(AtomicU64::new(0));
        let degraded = Arc::new(NativeHealth::default());
        let runner = NativeCallRunner::new(busy.clone(), job_epoch.clone());
        let kws = NativeKws::new(Box::new(kws), runner.clone());
        let vad = NativeVad::new(Box::new(vad), runner);
        let fixtures = SelfCheckAudio {
            wake: vec![0.1; 3_200],
            command: vec![0.1; 3_200],
            non_wake: vec![0.1; 3_200],
            source: "test".into(),
            online: false,
            generated_at_ms: None,
        };
        let cancelled = AtomicBool::new(true);
        let notify = Notify::new();
        let result = VoiceCompanionRuntime::worker_engine_check(
            &kws,
            &vad,
            &None,
            &fixtures,
            &cancelled,
            &notify,
            &busy,
            &degraded,
            &job_epoch,
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        )
        .await;
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
        let result = VoiceCompanionRuntime::worker_engine_check(
            &kws,
            &vad,
            &None,
            &fixtures,
            &AtomicBool::new(false),
            &notify,
            &busy,
            &degraded,
            &job_epoch,
            std::time::Instant::now() - std::time::Duration::from_millis(1),
        )
        .await;
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
    }

    #[tokio::test]
    async fn cancel_engine_check_only_matches_the_request_id() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        *runtime.engine_check_control.lock().unwrap() = Some(EngineCheckControl {
            id: "current".into(),
            epoch: 1,
            cancel: cancel.clone(),
            cancel_notify: Arc::new(Notify::new()),
        });
        assert!(!runtime.cancel_engine_check("other").await);
        assert!(!cancel.load(Ordering::Acquire));
        assert!(runtime.cancel_engine_check("current").await);
        assert!(cancel.load(Ordering::Acquire));
    }

    #[test]
    fn wake_and_call_admission_are_separate_scopes() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.set_admission("wake", true, nomifun_common::now_ms() + 60_000);
        runtime.set_admission("call", false, 0);
        assert!(runtime.admission_valid("wake"));
        assert!(!runtime.admission_valid("call"));
        runtime.set_admission("wake", false, 0);
        runtime.set_admission("call", true, nomifun_common::now_ms() + 60_000);
        assert!(!runtime.admission_valid("wake"));
        assert!(runtime.admission_valid("call"));
    }
    #[test]
    fn host_pushed_wake_admission_expires_without_background_checks() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        assert!(runtime.admission_valid("wake"), "test replay starts explicitly admitted");
        runtime.set_wake_admission(false, 0);
        assert!(!runtime.admission_valid("wake"));
        runtime.set_wake_admission(true, nomifun_common::now_ms() + 60_000);
        assert!(runtime.admission_valid("wake"));
        runtime.set_wake_admission(true, nomifun_common::now_ms() - 1);
        assert!(!runtime.admission_valid("wake"), "an expired push must deny wake");
    }
    struct GatedStt {
        blocking: Arc<AtomicBool>,
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    struct BlockingNativeStt(BlockingGate);

    #[async_trait::async_trait]
    impl SttCallback for BlockingNativeStt {
        async fn transcribe(&self, _: AudioChannel, _: Vec<f32>, _: u32) -> Option<String> {
            let gate = self.0.clone();
            tokio::task::spawn_blocking(move || gate.block()).await.unwrap();
            Some("recognized".into())
        }
    }

    #[tokio::test]
    async fn aborting_asr_keeps_the_native_lease_until_blocking_work_returns() {
        let gate = BlockingGate::default();
        let _cleanup = ReleaseBlockingGateOnDrop(gate.clone());
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let health = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        let task = {
            let (busy, epoch, health, gate) = (busy.clone(), epoch.clone(), health.clone(), gate.clone());
            tokio::spawn(async move {
                VoiceCompanionRuntime::transcribe_cancelable(
                    Arc::new(BlockingNativeStt(gate)), vec![0.1; 160], &AtomicBool::new(false), &Notify::new(),
                    &busy, &health, &epoch, std::time::Instant::now() + std::time::Duration::from_secs(2),
                    true, Some(reservation), None,
                    #[cfg(feature = "native-fault-injection")]
                    None,
                ).await
            })
        };
        wait_for_gate_entry(&gate).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!admission.has_pending());
        assert!(busy.load(Ordering::Acquire), "blocking work still owns the native lease");
        let runner = NativeCallRunner::with_admission(busy.clone(), epoch, health, admission, false);
        let result = runner.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), false).await;
        assert_eq!(result.err().as_deref(), Some("engine_check_native_busy"));
        gate.release();
        wait_for_native_release(&busy).await;
    }

    #[async_trait::async_trait]
    impl SttCallback for GatedStt {
        async fn transcribe(
            &self,
            _channel: AudioChannel,
            _pcm: Vec<f32>,
            _sample_rate: u32,
        ) -> Option<String> {
            self.entered.notify_one();
            if self.blocking.load(Ordering::Acquire) {
                self.release.notified().await;
            }
            Some("今天天气 hello friend".into())
        }
    }

    fn engine_command(
        request_id: &str,
        deadline: std::time::Instant,
        cancel: Arc<AtomicBool>,
        native_busy: Arc<AtomicBool>,
        native_job_epoch: Arc<AtomicU64>,
    ) -> (WorkerEngineCheck, oneshot::Receiver<Result<WorkerEngineCheckResult, String>>) {
        let (reply, rx) = oneshot::channel();
        (
            WorkerEngineCheck {
                request_id: request_id.into(),
                deadline,
                cancel,
                cancel_notify: Arc::new(Notify::new()),
                native_busy,
                native_job_epoch,
                fixtures: SelfCheckAudio {
                    wake: vec![0.1; 3_200],
                    command: vec![0.1; 3_200],
                    non_wake: vec![0.1; 3_200],
                    source: "test".into(),
                    online: false,
                    generated_at_ms: None,
                },
                reply,
            },
            rx,
        )
    }

    #[tokio::test]
    async fn non_priority_runner_cannot_acquire_lease_while_reservation_is_active() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        assert!(admission.has_pending());

        let runner = NativeCallRunner::with_admission(
            busy.clone(),
            epoch.clone(),
            degraded,
            admission.clone(),
            false,
        );

        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        // Fast-fail call immediately fails because reservation is pending
        let result = runner
            .acquire(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
                &cancel,
                &notify,
                false,
            )
            .await;
        assert_eq!(result.err().as_deref(), Some("engine_check_native_busy"));
        assert!(!busy.load(Ordering::Acquire));

        // Waiting call yields and does not acquire lease before reservation is dropped
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_clone = acquired.clone();
        let runner_clone = runner.clone();
        let handle = tokio::spawn(async move {
            let cancel = AtomicBool::new(false);
            let notify = Notify::new();
            if runner_clone
                .acquire(
                    std::time::Instant::now() + std::time::Duration::from_millis(300),
                    &cancel,
                    &notify,
                    true,
                )
                .await
                .is_ok()
            {
                acquired_clone.store(true, Ordering::Release);
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!acquired.load(Ordering::Acquire));
        assert!(!busy.load(Ordering::Acquire));

        drop(reservation);
        handle.await.unwrap();
        assert!(acquired.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn reservation_and_background_claim_have_one_linearization_point() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let health = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let (resume, receiver) = std::sync::mpsc::channel();
        let gate = Arc::new(AdmissionClaimTestGate { reached: Notify::new(), resume: std::sync::Mutex::new(receiver) });
        *admission.before_background_claim.lock().unwrap() = Some(gate.clone());
        let runner = NativeCallRunner::with_admission(busy.clone(), epoch.clone(), health.clone(), admission.clone(), false);
        let handle = tokio::runtime::Handle::current();
        let background = { let runner = runner.clone(); tokio::task::spawn_blocking(move || handle.block_on(async move {
            runner.acquire(std::time::Instant::now() + std::time::Duration::from_secs(2), &AtomicBool::new(false), &Notify::new(), false).await.unwrap()
        })) };
        tokio::time::timeout(std::time::Duration::from_secs(1), gate.reached.notified()).await.unwrap();
        let attempted = Arc::new(Notify::new());
        let reserving = { let admission = admission.clone(); let attempted = attempted.clone(); tokio::task::spawn_blocking(move || {
            attempted.notify_one(); admission.reserve()
        }) };
        attempted.notified().await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(!reserving.is_finished(), "reservation cannot complete between the protected check and claim");
        resume.send(()).unwrap();
        let owner = background.await.unwrap();
        let reservation = reserving.await.unwrap();
        drop(owner);
        assert!(admission.has_pending());
        let denied = runner.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), false).await;
        assert_eq!(denied.err().as_deref(), Some("engine_check_native_busy"));
        let priority = NativeCallRunner::with_admission(busy.clone(), epoch, health, admission.clone(), true);
        let owner = priority.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), true).await.unwrap();
        drop(reservation);
        drop(owner);
        assert!(!busy.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn non_priority_runner_proceeds_after_reservation_is_dropped() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let mut reservation = Some(admission.reserve());

        let runner = NativeCallRunner::with_admission(
            busy.clone(),
            epoch.clone(),
            degraded,
            admission.clone(),
            false,
        );

        let handle = tokio::spawn(async move {
            let cancel = AtomicBool::new(false);
            let notify = Notify::new();
            runner
                .acquire(
                    std::time::Instant::now() + std::time::Duration::from_secs(1),
                    &cancel,
                    &notify,
                    true,
                )
                .await
        });

        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert!(admission.has_pending());

        drop(reservation.take());
        assert!(!admission.has_pending());

        let guard = handle
            .await
            .unwrap()
            .expect("runner should acquire after reservation drop");
        assert!(busy.load(Ordering::Acquire));
        drop(guard);
        assert!(!busy.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn ordinary_asr_waits_for_a_transient_native_lease() {
        let busy = Arc::new(AtomicBool::new(true));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        let released = {
            let busy = busy.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                busy.store(false, Ordering::Release);
            })
        };
        let started = std::time::Instant::now();
        let asr: Arc<dyn SttCallback> =
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("hello"));
        let result = VoiceCompanionRuntime::transcribe_cancelable(
            asr,
            vec![0.1; 160],
            &cancel,
            &notify,
            &busy,
            &degraded,
            &epoch,
            started + std::time::Duration::from_secs(1),
            true,
            Some(reservation),
            None,
            #[cfg(feature = "native-fault-injection")]
            None,
        )
        .await;
        released.await.unwrap();
        assert_eq!(result.unwrap().as_deref(), Some("hello"));
        assert!(started.elapsed() >= std::time::Duration::from_millis(40));
        assert!(!admission.has_pending());
    }

    #[tokio::test]
    async fn asr_task_cancellation_resets_reservation_and_unblocks_native_calls() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let runner = NativeCallRunner::with_admission(
            busy.clone(),
            epoch.clone(),
            degraded.clone(),
            admission.clone(),
            false,
        );
        let owner = runner.acquire(std::time::Instant::now() + std::time::Duration::from_secs(1), &AtomicBool::new(false), &Notify::new(), false).await.unwrap();
        let reservation = admission.reserve();
        let entered = Arc::new(Notify::new());
        let task = {
            let entered = entered.clone();
            let busy = busy.clone();
            let epoch = epoch.clone();
            let degraded = degraded.clone();
            tokio::spawn(async move {
                entered.notify_one();
                VoiceCompanionRuntime::transcribe_cancelable(
                    Arc::new(nomifun_audio::stt::FakeSttCallback::always("hello")), vec![0.1; 160],
                    &AtomicBool::new(false), &Notify::new(), &busy, &degraded, &epoch,
                    std::time::Instant::now() + std::time::Duration::from_secs(2), true,
                    Some(reservation), None,
                    #[cfg(feature = "native-fault-injection")]
                    None,
                ).await
            })
        };
        entered.notified().await;
        assert!(admission.has_pending());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!admission.has_pending());
        assert!(busy.load(Ordering::Acquire), "canceling a waiter must not release another native owner");
        drop(owner);
        let guard = runner.acquire(std::time::Instant::now() + std::time::Duration::from_millis(100), &AtomicBool::new(false), &Notify::new(), false).await;
        assert!(guard.is_ok());
    }

    #[tokio::test]
    async fn non_priority_asr_yields_when_reservation_is_active() {
        let busy = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(1));
        let degraded = Arc::new(NativeHealth::default());
        let admission = Arc::new(NativeAdmission::default());
        let reservation = admission.reserve();
        assert!(admission.has_pending());

        let cancel = AtomicBool::new(false);
        let notify = Notify::new();
        let asr: Arc<dyn SttCallback> =
            Arc::new(nomifun_audio::stt::FakeSttCallback::always("background"));

        let result = VoiceCompanionRuntime::transcribe_cancelable(
            asr.clone(),
            vec![0.1; 160],
            &cancel,
            &notify,
            &busy,
            &degraded,
            &epoch,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
            true,
            None,
            Some(&admission),
            #[cfg(feature = "native-fault-injection")]
            None,
        )
        .await;
        assert_eq!(result.err().as_deref(), Some("engine_check_native_busy"));
        assert!(!busy.load(Ordering::Acquire));

        drop(reservation);
        let result2 = VoiceCompanionRuntime::transcribe_cancelable(
            asr,
            vec![0.1; 160],
            &cancel,
            &notify,
            &busy,
            &degraded,
            &epoch,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
            true,
            None,
            Some(&admission),
            #[cfg(feature = "native-fault-injection")]
            None,
        )
        .await;
        assert_eq!(result2.unwrap().as_deref(), Some("background"));
    }

    #[tokio::test]
    async fn a_second_request_is_rejected_while_native_work_is_still_owned() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.engine_check_native_busy.store(true, Ordering::Release);
        let response = runtime.self_check_engines().await;
        assert_eq!(response.reason.as_deref(), Some("engine_check_native_busy"));
        assert!(!response.kws_ok && !response.vad_ok && !response.asr_ok);
        assert!(!runtime.engine_check_active.load(Ordering::Acquire));
        runtime.engine_check_native_busy.store(false, Ordering::Release);
    }

    #[tokio::test]
    async fn cancel_during_worker_asr_returns_bounded_and_keeps_native_busy_until_release() {
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let blocking = Arc::new(AtomicBool::new(true));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let stt = GatedStt {
            blocking: blocking.clone(),
            entered: entered.clone(),
            release: release.clone(),
        };
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(stt),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime);
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let cancel = Arc::new(AtomicBool::new(false));
        let busy = runtime.engine_check_native_busy.clone();
        let (command, reply) = engine_command(
            "cancel-during-asr",
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            cancel.clone(),
            busy.clone(),
            runtime.native_job_epoch.clone(),
        );
        let notify = command.cancel_notify.clone();
        runtime
            .engine_check_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(command)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .expect("worker ASR started");
        cancel.store(true, Ordering::Release);
        notify.notify_waiters();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), reply)
            .await
            .expect("cancel must return promptly")
            .unwrap();
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
        assert!(!runtime.native_degraded.is_degraded());
        assert!(busy.load(Ordering::Acquire), "native decode must still be owned");
        blocking.store(false, Ordering::Release);
        release.notify_waiters();
        for _ in 0..50 {
            if !busy.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!busy.load(Ordering::Acquire), "native ownership must clear only on completion");

        let (command, reply) = engine_command(
            "recovered",
            std::time::Instant::now() + std::time::Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            busy.clone(),
            runtime.native_job_epoch.clone(),
        );
        runtime
            .engine_check_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(command)
            .await
            .unwrap();
        let recovered = tokio::time::timeout(std::time::Duration::from_secs(2), reply)
            .await
            .expect("next check must recover")
            .unwrap()
            .expect("next check must run");
        assert!(recovered.asr_ok);
        worker.abort();
        let _ = worker.await;
        producer.close();
    }

    #[tokio::test]
    async fn timeout_during_worker_asr_does_not_grant_and_keeps_ownership() {
        let root = tempdir().unwrap();
        let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
        let blocking = Arc::new(AtomicBool::new(true));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(GatedStt {
                blocking: blocking.clone(),
                entered: entered.clone(),
                release: release.clone(),
            }),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime);
        let worker = runtime.start_worker();
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let busy = runtime.engine_check_native_busy.clone();
        let (command, reply) = engine_command(
            "timeout-during-asr",
            std::time::Instant::now() + std::time::Duration::from_millis(750),
            Arc::new(AtomicBool::new(false)),
            busy.clone(),
            runtime.native_job_epoch.clone(),
        );
        runtime
            .engine_check_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(command)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
            .await
            .expect("worker ASR started");
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), reply)
            .await
            .expect("deadline must return promptly")
            .unwrap();
        assert_eq!(result.unwrap_err(), "engine_check_timeout");
        assert!(runtime.native_degraded.is_degraded());
        assert!(busy.load(Ordering::Acquire));
        blocking.store(false, Ordering::Release);
        release.notify_waiters();
        for _ in 0..50 {
            if !busy.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(!busy.load(Ordering::Acquire));
        worker.abort();
        let _ = worker.await;
        producer.close();
    }

    #[tokio::test]
    async fn tts_self_check_is_excluded_by_an_engine_check() {
        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        runtime.engine_check_active.store(true, Ordering::Release);
        assert_eq!(
            runtime.self_check_tts().await.unwrap_err(),
            "engine_check_in_progress"
        );
        runtime.engine_check_active.store(false, Ordering::Release);
    }

    #[tokio::test]
    async fn wake_is_rejected_while_a_fixed_self_check_owns_the_runtime() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(Arc::new(std::sync::atomic::AtomicUsize::new(0)))),
        );
        runtime.self_check_active.store(true, Ordering::Release);
        let result = runtime.handle_wake_request("Hey Flowy", None).await;
        assert!(matches!(result, Err(VoiceWakeError::Busy)));
        runtime.self_check_active.store(false, Ordering::Release);
    }

    struct SlowSpeech;
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for SlowSpeech {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&vec![0.05; 2400], 24000)).unwrap();
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn daemon_cancel_stops_the_matching_self_check() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            Arc::new(ModelManager::new(root.path())),
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(SlowSpeech),
        ));
        let check = runtime.clone();
        let task = tokio::spawn(async move { check.self_check_tts_with_id("check-cancel").await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert!(runtime.cancel_self_check("check-cancel").await);
        let result = task.await.unwrap();
        assert!(matches!(result, Err(reason) if reason == "self_check_cancelled"));
        assert!(!runtime.self_check_active.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn tts_self_check_rejects_silent_nonfinite_short_and_empty_audio() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        for case in [
            InvalidSpeech::Silent,
            InvalidSpeech::NonFinite,
            InvalidSpeech::TooShort,
            InvalidSpeech::Empty,
        ] {
            let root = tempdir().unwrap();
            let models = Arc::new(ModelManager::new(root.path()));
            let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
            let (runtime, _) = VoiceCompanionRuntime::from_capture(
                models,
                Arc::new(OobeManager::new_bypassed()),
                Arc::new(Mutex::new(capture)),
                Arc::new(nomifun_audio::stt::NullSttCallback),
                Arc::new(QwenVoiceRouter::new()),
            );
            let runtime = runtime.with_test_playback(
                Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
                Arc::new(InvalidSpeechProvider(case)),
            );
            let result = runtime.self_check_tts().await.unwrap();
            assert!(!result.ok, "invalid audio must fail the check");
            assert!(!result.playback_ok, "invalid audio must never be played");
            assert!(result.languages.iter().all(|language| !language.ok));
        }
    }
    #[tokio::test]
    async fn tts_self_check_reports_bilingual_decode_and_sink_consumption() {
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        );

        let result = runtime.self_check_tts().await.unwrap();
        assert!(result.ok, "both fixed languages must decode");
        assert!(result.playback_ok, "the fixed sample must reach the sink");
        assert_eq!(result.languages.len(), 2);
        assert!(result.languages.iter().all(|language| language.ok));
        assert!(result.languages.iter().all(|language| language.samples > 0));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn tts_self_check_startup_broadcast_and_bilingual_stages() {
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(RecordingSpeech(recorded.clone())),
        );

        // 1. Default stage (formerly "语音连接测试。") with zh-CN
        recorded.lock().unwrap().clear();
        let res_zh = runtime.self_check_tts_with_options(None, Some("zh-CN"), None).await.unwrap();
        assert!(res_zh.ok);
        let calls_zh = recorded.lock().unwrap().clone();
        assert_eq!(calls_zh[0], ("正在启动语音服务。".to_string(), "zh-CN-XiaoxiaoNeural".to_string()));

        // 2. Default stage with en-US
        recorded.lock().unwrap().clear();
        let res_en = runtime.self_check_tts_with_options(None, Some("en-US"), None).await.unwrap();
        assert!(res_en.ok);
        let calls_en = recorded.lock().unwrap().clone();
        assert_eq!(calls_en[1], ("Starting voice service.".to_string(), "en-US-JennyNeural".to_string()));

        // 3. Preparing stage with zh-CN
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("zh-CN"), Some("preparing")).await.unwrap();
        let calls_prep_zh = recorded.lock().unwrap().clone();
        assert_eq!(calls_prep_zh[0], ("正在准备语音通话。".to_string(), "zh-CN-XiaoxiaoNeural".to_string()));

        // 4. Preparing stage with en-US
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("en-US"), Some("preparing")).await.unwrap();
        let calls_prep_en = recorded.lock().unwrap().clone();
        assert_eq!(calls_prep_en[1], ("Preparing your voice call.".to_string(), "en-US-JennyNeural".to_string()));

        // 5. Task creation with zh-CN
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("zh-CN"), Some("create_task")).await.unwrap();
        let calls_task_zh = recorded.lock().unwrap().clone();
        assert_eq!(calls_task_zh[0], ("创建语音任务中".to_string(), "zh-CN-XiaoxiaoNeural".to_string()));

        // 6. Task creation with en-US
        recorded.lock().unwrap().clear();
        runtime.self_check_tts_with_options(None, Some("en-US"), Some("create_task")).await.unwrap();
        let calls_task_en = recorded.lock().unwrap().clone();
        assert_eq!(calls_task_en[1], ("Creating voice task...".to_string(), "en-US-JennyNeural".to_string()));
    }
    #[tokio::test]
    async fn test_progress_must_not_block_final_result_of_same_task() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceSessionPhase};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let worker = runtime.start_worker();
        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "task-001".into(),
                notification_id: Some(format!("{kind}:task-001")),
                attempt_id: Some(format!("{kind}:task-001:1")),
                goal: None,
                text: text.into(),
            }
        };

        // 1. Progress announcement accepted
        runtime.announce_task_result(notice(&runtime, "progress", "正在调用搜索工具。")).await.unwrap();

        // Wait until speech finished and follow-up or idle entered
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while runtime.snapshot().phase != VoiceSessionPhase::Idle && runtime.snapshot().phase != VoiceSessionPhase::FollowUp {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        // 2. Final result announcement for same command_id must succeed and not be blocked as conflict
        let result = runtime.announce_task_result(notice(&runtime, "result", "这是最终搜索结果。")).await;
        worker.abort();

        assert_eq!(result, Ok(()), "different notification_id for the same command_id must succeed");
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        // 3. Status query by attempt_id, notification_id, command_id
        assert!(runtime.notification_status("result:task-001:1").await.is_some());
        assert!(runtime.notification_status("result:task-001").await.is_some());
        assert!(runtime.notification_status("task-001").await.is_some());
    }

    #[tokio::test]
    async fn test_explicit_replay_must_create_a_receipt_and_play_again() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceSessionPhase};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let worker = runtime.start_worker();
        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "same-task".into(),
                notification_id: Some(format!("{kind}:same-task")),
                attempt_id: Some(format!("{kind}:same-task:1")),
                goal: None,
                text: text.into(),
            }
        };

        let first = notice(&runtime, "result", "这是最终结果。");
        runtime.announce_task_result(first).await.unwrap();

        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            while runtime.snapshot().phase != VoiceSessionPhase::Idle && runtime.snapshot().phase != VoiceSessionPhase::FollowUp {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();

        let mut replay = notice(&runtime, "result", "这是最终结果。");
        replay.attempt_id = Some("result:same-task:2".into());
        runtime.announce_task_result(replay).await.unwrap();
        worker.abort();

        assert!(
            runtime.notification_status("result:same-task:2").await.is_some(),
            "accepted explicit replay must have its own attempt receipt"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_changed_payload_of_same_logical_notification_must_conflict() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest};
        use nomifun_voice_terminal::playback::TestAudioSink;
        use std::sync::atomic::AtomicUsize;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls)),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "off",
                    "volume": 0,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let notice = |rt: &VoiceCompanionRuntime, kind: &str, text: &str| {
            let snapshot = rt.snapshot();
            VoiceTaskResultAnnouncement {
                scope: VoiceSessionControlRequest {
                    instance_id: snapshot.instance_id,
                    session_id: snapshot.session_id,
                    turn_id: snapshot.turn_id,
                },
                command_id: "same-task".into(),
                notification_id: Some(format!("{kind}:same-task")),
                attempt_id: Some(format!("{kind}:same-task:1")),
                goal: None,
                text: text.into(),
            }
        };

        runtime.announce_task_result(notice(&runtime, "result", "原始结果。")).await.unwrap();
        let mut changed = notice(&runtime, "result", "不同的新结果。");
        changed.attempt_id = Some("changed-attempt".into());
        assert_eq!(runtime.announce_task_result(changed).await, Err("task_result_conflict"));
    }

    struct FailingTts;
    #[async_trait::async_trait]
    impl nomifun_voice_terminal::tts::TtsProvider for FailingTts {
        async fn synthesize_stream(
            &self,
            _: &str,
            _: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            Err(nomifun_voice_terminal::tts::TtsError::Synthesis("mock_network_down".into()))
        }
    }

    #[tokio::test]
    async fn test_task_result_plays_chime_and_opens_four_second_follow_up_window() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceCueKind};
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(MockSpeech(calls.clone())),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "light",
                    "volume": 80,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let mut rx = runtime.event_sender().subscribe();
        let snapshot = runtime.snapshot();
        let notice = VoiceTaskResultAnnouncement {
            scope: VoiceSessionControlRequest {
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                turn_id: snapshot.turn_id,
            },
            command_id: "cmd-step5".into(),
            notification_id: Some("notif-step5".into()),
            attempt_id: Some("attempt-step5:1".into()),
            goal: Some("查询北京天气".into()),
            text: "北京今天晴，气温 15 到 25 度。".into(),
        };

        let result = runtime.announce_task_result(notice).await;
        assert_eq!(result, Ok(()));

        let mut saw_chime = false;
        let mut saw_followup_window = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                VoiceCompanionEvent::Feedback { diagnostic } if diagnostic.kind == VoiceCueKind::Recognized => {
                    saw_chime = true;
                }
                VoiceCompanionEvent::FollowUpWindow { timeout_secs } => {
                    assert_eq!(timeout_secs, 180, "follow-up window must strictly be 180 seconds");
                    saw_followup_window = true;
                }
                _ => {}
            }
        }
        assert!(saw_chime, "task completion chime (Recognized) must play before speaking result");
        assert!(saw_followup_window, "follow-up window of 180 seconds must be announced");
    }

    #[tokio::test]
    async fn test_task_result_tts_failure_plays_error_cue_fallback() {
        use nomifun_api_types::voice::{VoiceTaskResultAnnouncement, VoiceSessionControlRequest, VoiceCueKind};
        use nomifun_voice_terminal::playback::TestAudioSink;

        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let runtime = Arc::new(runtime.with_test_playback(
            Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new()))),
            Arc::new(FailingTts),
        ));
        runtime
            .update_feedback(
                serde_json::from_value(serde_json::json!({
                    "mode": "light",
                    "volume": 80,
                    "speak_replies": true,
                }))
                .unwrap(),
            )
            .unwrap();

        let mut rx = runtime.event_sender().subscribe();
        let snapshot = runtime.snapshot();
        let notice = VoiceTaskResultAnnouncement {
            scope: VoiceSessionControlRequest {
                instance_id: snapshot.instance_id,
                session_id: snapshot.session_id,
                turn_id: snapshot.turn_id,
            },
            command_id: "cmd-tts-fail".into(),
            notification_id: Some("notif-tts-fail".into()),
            attempt_id: Some("attempt-tts-fail:1".into()),
            goal: Some("测试TTS失败".into()),
            text: "这是一条无法合成的语音消息。".into(),
        };

        let result = runtime.announce_task_result(notice).await;
        assert_eq!(result, Ok(()));

        let mut saw_error_cue = false;
        let mut saw_error_event = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                VoiceCompanionEvent::Feedback { diagnostic } if diagnostic.kind == VoiceCueKind::Error => {
                    saw_error_cue = true;
                }
                VoiceCompanionEvent::Error { stage, code } => {
                    assert_eq!(stage, "playback");
                    assert_eq!(code, "tts_failed");
                    saw_error_event = true;
                }
                _ => {}
            }
        }
        assert!(saw_error_event, "error event must be emitted on TTS synthesis failure");
        assert!(saw_error_cue, "hardcoded local error cue must play on TTS failure");
    }

    #[tokio::test]
    async fn reproduction_task_cancellation_and_interrupt_closure() {
        use nomifun_voice_terminal::playback::TestAudioSink;
        let root = tempdir().unwrap();
        let models = Arc::new(ModelManager::new(root.path()));
        let (capture, _) = VoiceCapturePipeline::new(PipelineConfig::default());
        let (runtime, _) = VoiceCompanionRuntime::from_capture(
            models,
            Arc::new(OobeManager::new_bypassed()),
            Arc::new(Mutex::new(capture)),
            Arc::new(nomifun_audio::stt::NullSttCallback),
            Arc::new(QwenVoiceRouter::new()),
        );
        let playback = Arc::new(PlaybackPipeline::new(Arc::new(TestAudioSink::new())));
        let runtime = Arc::new(runtime.with_test_playback(playback.clone(), Arc::new(FailingTts)));

        // Setup a simulated task dispatching state
        {
            let mut d = runtime.driver.lock().await;
            let _ = d.handle_event(AcousticEvent::WakeHit);
            let _ = d.handle_event(AcousticEvent::SpeechEnd);
            let _ = d.handle_event(AcousticEvent::IntentAgentTask);
            assert_eq!(d.current_state(), AcousticState::DispatchingTask);
        }

        // Simulate interruption/generation bump while in DispatchingTask
        playback.interrupt().await.unwrap();

        // Check update_task_status_versioned cleans up DispatchingTask
        runtime.update_task_status_versioned(
            Some(nomifun_api_types::voice::VoiceTaskSnapshot {
                command_id: "cmd-test".into(),
                session_id: "s-1".into(),
                acoustic_turn_id: "t-1".into(),
                status: nomifun_api_types::voice::VoiceTaskStatus::Failed,
                error_code: Some("cancelled".into()),
                conversation_id: None,
                message_id: None,
                result_text: None,
                runtime_turn_id: None,
                result_ok: None,
            }),
            None,
            Some(1),
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let state = runtime.driver.lock().await.current_state();
        assert_ne!(
            state,
            AcousticState::DispatchingTask,
            "driver must not be trapped in DispatchingTask when task is interrupted or cancelled"
        );
        assert!(
            matches!(state, AcousticState::Listening | AcousticState::Idle),
            "driver must recover to Listening or Idle so future voice turns can proceed"
        );
    }

    struct RecordingTaskClient {
        current: std::sync::Mutex<Option<nomifun_api_types::voice::VoiceTaskSnapshot>>,
        controls: std::sync::Mutex<Vec<nomifun_api_types::voice::VoiceTaskControlCommand>>,
    }

    #[async_trait::async_trait]
    impl nomifun_voice_terminal::task_client::VoiceTaskClient for RecordingTaskClient {
        async fn submit(&self, _: nomifun_api_types::voice::VoiceTaskSubmitRequest)
            -> Result<nomifun_api_types::voice::VoiceTaskDelivery, String> {
            Err("unused".into())
        }
        async fn current(&self) -> Result<Option<nomifun_api_types::voice::VoiceTaskSnapshot>, String> {
            Ok(self.current.lock().unwrap().clone())
        }
        async fn control(&self, request: nomifun_api_types::voice::VoiceTaskControlCommand)
            -> Result<nomifun_api_types::voice::VoiceTaskSnapshot, String> {
            self.controls.lock().unwrap().push(request);
            self.current.lock().unwrap().clone().ok_or_else(|| "task_not_found".to_owned())
        }
    }

    async fn execute_local_task_control(
        input: &str,
        client: &RecordingTaskClient,
        scope: &nomifun_api_types::voice::VoiceSessionSnapshot,
    ) -> Result<Option<nomifun_api_types::voice::VoiceTaskSnapshot>, String> {
        let Some(action) = local_task_control_action(input, client).await else {
            return Ok(None);
        };
        let Some(task) = client.current().await? else {
            return Err("task_not_found".into());
        };
        client.control(nomifun_api_types::voice::VoiceTaskControlCommand {
            scope: nomifun_api_types::voice::VoiceSessionControlRequest {
                instance_id: scope.instance_id.clone(),
                session_id: scope.session_id.clone(),
                turn_id: scope.turn_id.clone(),
            },
            task: nomifun_api_types::voice::VoiceTaskReference::from(&task),
            action,
            raw_user_text: Some(input.to_owned()),
            locale: scope.locale.clone(),
        }).await.map(Some)
    }

    #[tokio::test]
    async fn local_task_control_never_cancels_on_negation_and_carries_target_identity() {
        use nomifun_api_types::voice::{
            VoiceBindingSnapshot, VoiceSessionSnapshot, VoiceTaskControlAction, VoiceTaskSnapshot,
            VoiceTaskStatus,
        };
        let task = VoiceTaskSnapshot {
            command_id: "command-a".into(),
            session_id: "session-a".into(),
            acoustic_turn_id: "origin-a".into(),
            conversation_id: Some("conversation-a".into()),
            message_id: Some("message-a".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        let client = RecordingTaskClient {
            current: std::sync::Mutex::new(Some(task)),
            controls: std::sync::Mutex::new(Vec::new()),
        };
        let scope = VoiceSessionSnapshot {
            instance_id: "instance-a".into(),
            session_id: Some("session-a".into()),
            turn_id: Some("turn-a".into()),
            locale: Some("en-US".into()),
            binding: Some(VoiceBindingSnapshot {
                session_id: "session-a".into(),
                binding_id: "binding-a".into(),
                conversation_id: "conversation-a".into(),
                workspace: None,
                memory_owner_companion_id: None,
            }),
            ..Default::default()
        };
        for input in [
            "never cancel task",
            "do not ever cancel task",
            "don't you cancel task",
            "We should cancel the task tomorrow",
            "Explain how to cancel the task",
            "If it fails, cancel the task",
            "stop",
            "请明天取消任务",
            "如果出错，请取消任务",
            "不要取消任务",
            "千万别取消任务",
            "取消任务，但是不要停止",
            "任务取消了",
            "任务已经结束了",
            "今天天气怎么样",
            "今天天气情况怎么样",
            "最近身体情况怎么样",
            "停止任务播放",
        ] {
            let result = execute_local_task_control(input, &client, &scope).await;
            assert!(result.is_ok(), "negated/ambiguous input must not control: {input:?}");
            assert_eq!(client.controls.lock().unwrap().len(), 0, "no control call for {input:?}");
        }

        let query = execute_local_task_control("任务进度怎么样", &client, &scope).await.unwrap().unwrap();
        assert_eq!(query.command_id, "command-a");
        {
            let controls = client.controls.lock().unwrap();
            assert_eq!(controls.len(), 1);
            assert_eq!(controls[0].action, VoiceTaskControlAction::QueryStatus);
            assert_eq!(controls[0].task.command_id, "command-a");
            assert_eq!(controls[0].task.conversation_id.as_deref(), Some("conversation-a"));
            assert_eq!(controls[0].scope.session_id.as_deref(), Some("session-a"));
        }

        let cancelled = execute_local_task_control("cancel the task", &client, &scope).await.unwrap().unwrap();
        assert_eq!(cancelled.command_id, "command-a");
        {
            let controls = client.controls.lock().unwrap();
            assert_eq!(controls.len(), 2);
            assert_eq!(controls[1].action, VoiceTaskControlAction::CancelTask);
            assert_eq!(controls[1].task.command_id, "command-a");
            assert_eq!(controls[1].scope.turn_id.as_deref(), Some("turn-a"));
        }

        let other_scope = VoiceSessionSnapshot {
            session_id: Some("session-b".into()),
            turn_id: Some("turn-b".into()),
            binding: Some(VoiceBindingSnapshot {
                session_id: "session-b".into(),
                binding_id: "binding-b".into(),
                conversation_id: "conversation-b".into(),
                workspace: None,
                memory_owner_companion_id: None,
            }),
            ..scope.clone()
        };
        let cross = execute_local_task_control("cancel the task", &client, &other_scope).await.unwrap().unwrap();
        assert_eq!(cross.command_id, "command-a", "the daemon must not retarget another task");
        {
            let controls = client.controls.lock().unwrap();
            assert_eq!(controls.len(), 3);
            assert_eq!(controls[2].scope.session_id.as_deref(), Some("session-b"));
            assert_eq!(controls[2].task.command_id, "command-a");
            assert_eq!(controls[2].task.conversation_id.as_deref(), Some("conversation-a"));
        }
    }

    #[test]
    fn task_status_prompt_is_limited_to_the_current_acoustic_session() {
        use nomifun_api_types::voice::{VoiceSessionSnapshot, VoiceTaskSnapshot, VoiceTaskStatus};
        let task = VoiceTaskSnapshot {
            command_id: "command-a".into(),
            session_id: "session-a".into(),
            acoustic_turn_id: "origin-a".into(),
            conversation_id: Some("conversation-a".into()),
            message_id: Some("message-a".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        let other = VoiceSessionSnapshot {
            session_id: Some("session-b".into()),
            ..Default::default()
        };
        assert!(task_status_for_prompt(Some(&task), &other).is_none());
        let same = VoiceSessionSnapshot {
            session_id: Some("session-a".into()),
            ..Default::default()
        };
        let status = task_status_for_prompt(Some(&task), &same).expect("same session must include status");
        assert!(status.contains("command-a"));
        assert!(status.contains("accepted"));
        assert!(task_status_for_prompt(None, &same).is_none());
    }

    #[test]
    fn task_control_requires_an_affirmative_explicit_cancel() {
        use nomifun_api_types::voice::VoiceTaskControlAction::{CancelTask, QueryStatus};
        let cases = [
            ("cancel task", Some(CancelTask)),
            ("cancel the task", Some(CancelTask)),
            ("please cancel the current task", Some(CancelTask)),
            ("CANCEL THE TASK", Some(CancelTask)),
            ("stop the task", Some(CancelTask)),
            ("stop the current job", Some(CancelTask)),
            ("取消任务", Some(CancelTask)),
            ("请帮我取消当前任务", Some(CancelTask)),
            ("停止任务", Some(CancelTask)),
            ("结束任务", Some(CancelTask)),
            ("取消", None),
            ("stop", None),
            ("never cancel task", None),
            ("do not ever cancel task", None),
            ("don't you cancel task", None),
            ("please don't cancel the task", None),
            ("do not cancel task", None),
            ("DON'T CANCEL TASK", None),
            ("never ever cancel task", None),
            ("I would never want you to cancel the task", None),
            ("don't stop the task", None),
            ("stop the task from failing", None),
            ("不要取消任务", None),
            ("千万别取消任务", None),
            ("不要现在取消任务", None),
            ("不要取消", None),
            ("do not stop", None),
            ("停止播放", None),
            ("please stop", None),
            ("取消任务，但是不要停止", None),
            ("不要取消任务，取消吧", None),
            ("取消任务并查询进度", Some(QueryStatus)),
            ("don't cancel the task, just check progress", Some(QueryStatus)),
            ("never cancel task but show progress", Some(QueryStatus)),
            ("任务进度怎么样", Some(QueryStatus)),
            ("what's the task status", Some(QueryStatus)),
            ("今天天气怎么样", None),
            ("今天天气情况怎么样", None),
            ("最近身体情况怎么样", None),
            ("任务情况怎么样", Some(QueryStatus)),
            ("任务取消了", None),
            ("任务已经结束了", None),
            ("任务停止了", None),
            ("任务取消", None),
            ("任务结束", None),
            ("停止任务播放", None),
            ("请取消任务", Some(CancelTask)),
            ("把任务取消", Some(CancelTask)),
            ("立即停止任务", Some(CancelTask)),
            ("你好", None),
            ("创建任务", None),
            ("结束了吗", None),
            ("取消任务吧", Some(CancelTask)),
            ("把任务取消吧", Some(CancelTask)),
            ("请把任务取消", Some(CancelTask)),
            ("取消任务，谢谢", Some(CancelTask)),
            ("你取消任务", Some(CancelTask)),
            ("取消任务。", Some(CancelTask)),
            ("我把任务取消了", None),
            ("把任务停止了", None),
            ("把任务取消了", None),
            ("取消任务很重要", None),
            ("取消任务，可以吗", None),
            ("取消任务可以吗", None),
            ("取消任务好吗", None),
            ("取消任务，好吗", None),
            ("取消任务，多久", None),
            ("取消任务？", None),
            ("取消？", None),
            ("stop?", None),
            ("stop the task?", None),
            ("cancel?", None),
            ("cancel the task?", None),
            ("我取消任务", None),
            ("他取消任务", None),
            ("取消任务了", None),
        ];
        for (input, expected) in cases {
            assert_eq!(detect_task_control(input), expected, "input={input:?}");
        }
    }
}

#[cfg(any(test, feature = "voice-skeleton"))]
pub static PARENT_DISPATCH_QUEUE_GATE: std::sync::OnceLock<std::sync::Arc<SpeechEndTestGate>> = std::sync::OnceLock::new();
