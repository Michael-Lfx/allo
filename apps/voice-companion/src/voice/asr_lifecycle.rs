//! ASR Lifecycle Owner and Supervision Domain.
//!
//! Provides single-owner lifecycle management for local/offline ASR instances:
//! - State machine: Unloaded -> Loading -> Ready -> Transcribing -> TimingIdle -> Unloading -> Draining -> Fault
//! - Decoupled admission/supervision from KWS/VAD (separate busy/epoch/watchdog)
//! - Cold-state single utterance preservation with deadline tracking and rejection of 2nd utterance
//! - Generation tracking: cancelled/superseded generations are discarded, never returned for dispatch
//! - Active lease tracking with atomic check-and-acquire to prevent race with reaper
//! - Weak instance caching: re-wakes seamlessly reuse living instances without double-allocation
//! - Native completion ownership: aborted leases retain in-flight draining protection until resolved
//! - Persistent background loading and unloading: caller abort does not strand state or leave busy=true
//! - Serialized unload-to-load transition: wake requests during unloading wait for complete destruction

#![allow(dead_code)]

use std::sync::{Arc, Weak};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrState {
    Unloaded,
    Loading,
    Ready,
    Transcribing,
    TimingIdle,
    Unloading,
    Draining,
    Fault,
}

#[derive(Debug, Clone)]
pub struct ColdWaitingUtterance {
    pub session_id: String,
    pub turn_id: u64,
    pub generation_id: u64,
    pub pcm: Vec<f32>,
    pub completed_at: Instant,
    pub deadline: Instant,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AsrLifecycleError {
    #[error("cold_state_busy: already buffering a pending cold utterance")]
    ColdSecondUtteranceRejected,
    #[error("asr_loading_timeout: model preparation exceeded deadline")]
    LoadingTimeout,
    #[error("asr_turn_deadline_expired: turn exceeded absolute 30s deadline")]
    TurnDeadlineExpired,
    #[error("asr_degraded: recognizer in fault state")]
    Degraded,
    #[error("asr_cancelled: turn cancelled before recognition")]
    Cancelled,
    #[error("input_truncated_or_gap: sequence gap or overflow detected")]
    InputTruncated,
}

pub struct AsrSupervisionDomain {
    pub asr_busy: AtomicBool,
    pub asr_epoch: AtomicU64,
    pub asr_degraded: AtomicBool,
    pub last_heartbeat: Mutex<Instant>,
}

impl AsrSupervisionDomain {
    pub fn new() -> Self {
        Self {
            asr_busy: AtomicBool::new(false),
            asr_epoch: AtomicU64::new(0),
            asr_degraded: AtomicBool::new(false),
            last_heartbeat: Mutex::new(Instant::now()),
        }
    }

    pub fn bump_epoch(&self) -> u64 {
        self.asr_epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn mark_busy(&self, busy: bool) {
        self.asr_busy.store(busy, Ordering::SeqCst);
    }

    pub fn mark_degraded(&self, degraded: bool) {
        self.asr_degraded.store(degraded, Ordering::SeqCst);
    }
}

/// Guard representing an active lease on the loaded ASR instance.
/// When dropped, if uncompleted, marks in-flight draining to prevent premature reap.
pub struct AsrLease<T: Send + Sync + 'static> {
    pub instance: Arc<T>,
    active_counter: Arc<AtomicUsize>,
    draining_flag: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}

impl<T: Send + Sync + 'static> std::ops::Deref for AsrLease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.instance
    }
}

impl<T: Send + Sync + 'static> AsrLease<T> {
    pub fn mark_completed(&self) {
        self.completed.store(true, Ordering::SeqCst);
    }
}

impl<T: Send + Sync + 'static> Drop for AsrLease<T> {
    fn drop(&mut self) {
        self.active_counter.fetch_sub(1, Ordering::SeqCst);
        if !self.completed.load(Ordering::SeqCst) {
            // Aborted or cancelled: mark draining to protect in-flight native work
            self.draining_flag.store(true, Ordering::SeqCst);
        }
    }
}

pub struct AsrLifecycleOwner<T: Send + Sync + 'static> {
    pub state: Arc<Mutex<AsrState>>,
    pub instance: Arc<Mutex<Option<Arc<T>>>>,
    weak_instance: Arc<Mutex<Option<Weak<T>>>>,
    active_leases: Arc<AtomicUsize>,
    in_flight_draining: Arc<AtomicBool>,
    supervision: Arc<AsrSupervisionDomain>,
    cold_waiting: Mutex<Option<ColdWaitingUtterance>>,
    idle_timeout: Duration,
    last_active: Arc<Mutex<Instant>>,
    loader: Arc<dyn Fn() -> Result<Arc<T>, String> + Send + Sync>,
    unloader: Option<Arc<dyn Fn(Arc<T>) + Send + Sync>>,
    state_changed: Arc<Notify>,
    unload_finished: Arc<Notify>,
    active_generation: AtomicU64,
    load_channel: Arc<Mutex<Option<watch::Receiver<Option<Result<(), AsrLifecycleError>>>>>>,
}

impl<T: Send + Sync + 'static> AsrLifecycleOwner<T> {
    pub fn new(
        idle_timeout: Duration,
        loader: Arc<dyn Fn() -> Result<Arc<T>, String> + Send + Sync>,
        unloader: Option<Arc<dyn Fn(Arc<T>) + Send + Sync>>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(AsrState::Unloaded)),
            instance: Arc::new(Mutex::new(None)),
            weak_instance: Arc::new(Mutex::new(None)),
            active_leases: Arc::new(AtomicUsize::new(0)),
            in_flight_draining: Arc::new(AtomicBool::new(false)),
            supervision: Arc::new(AsrSupervisionDomain::new()),
            cold_waiting: Mutex::new(None),
            idle_timeout,
            last_active: Arc::new(Mutex::new(Instant::now())),
            loader,
            unloader,
            state_changed: Arc::new(Notify::new()),
            unload_finished: Arc::new(Notify::new()),
            active_generation: AtomicU64::new(1),
            load_channel: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn current_state(&self) -> AsrState {
        *self.state.lock().await
    }

    pub fn supervision(&self) -> Arc<AsrSupervisionDomain> {
        Arc::clone(&self.supervision)
    }

    pub fn next_generation(&self) -> u64 {
        self.active_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn current_generation(&self) -> u64 {
        self.active_generation.load(Ordering::SeqCst)
    }

    /// Trigger asynchronous model preparation upon WakeHit or explicit call.
    /// Reuses in-flight loading if already loading; reuses warm instance if in TimingIdle/Ready/Draining.
    /// Serializes cleanly behind any ongoing unload operation.
    pub async fn request_wake(&self) -> Result<(), AsrLifecycleError> {
        loop {
            let mut st = self.state.lock().await;
            match *st {
                AsrState::Ready | AsrState::Transcribing => {
                    *self.last_active.lock().await = Instant::now();
                    return Ok(());
                }
                AsrState::TimingIdle | AsrState::Draining => {
                    *st = AsrState::Ready;
                    self.in_flight_draining.store(false, Ordering::SeqCst);
                    *self.last_active.lock().await = Instant::now();
                    self.state_changed.notify_waiters();
                    return Ok(());
                }
                AsrState::Loading => {
                    let rx_opt = self.load_channel.lock().await.clone();
                    drop(st);
                    if let Some(mut rx) = rx_opt {
                        loop {
                            if let Some(res) = rx.borrow().clone() {
                                return res;
                            }
                            if rx.changed().await.is_err() {
                                break;
                            }
                        }
                    }
                    continue;
                }
                AsrState::Unloading => {
                    let unload_notifier = Arc::clone(&self.unload_finished);
                    drop(st);
                    unload_notifier.notified().await;
                    continue;
                }
                AsrState::Fault => {
                    if self.supervision.asr_degraded.load(Ordering::SeqCst) {
                        return Err(AsrLifecycleError::Degraded);
                    }
                }
                AsrState::Unloaded => {}
            }

            // Check if existing instance can be upgraded from weak reference
            let weak_guard = self.weak_instance.lock().await;
            if let Some(weak) = weak_guard.as_ref() {
                if let Some(reused) = weak.upgrade() {
                    let mut inst_lock = self.instance.lock().await;
                    *inst_lock = Some(reused);
                    *st = AsrState::Ready;
                    self.in_flight_draining.store(false, Ordering::SeqCst);
                    *self.last_active.lock().await = Instant::now();
                    self.state_changed.notify_waiters();
                    return Ok(());
                }
            }
            drop(weak_guard);

            // Double check existing instance
            let inst_lock = self.instance.lock().await;
            if inst_lock.is_some() {
                *st = AsrState::Ready;
                self.in_flight_draining.store(false, Ordering::SeqCst);
                *self.last_active.lock().await = Instant::now();
                return Ok(());
            }
            drop(inst_lock);

            *st = AsrState::Loading;
            drop(st);

            let (tx, rx) = watch::channel(None);
            *self.load_channel.lock().await = Some(rx);

            let loader = Arc::clone(&self.loader);
            let supervision = Arc::clone(&self.supervision);
            let state_arc = Arc::clone(&self.state);
            let instance_arc = Arc::clone(&self.instance);
            let weak_instance_arc = Arc::clone(&self.weak_instance);
            let last_active_arc = Arc::clone(&self.last_active);
            let state_changed = Arc::clone(&self.state_changed);
            let load_channel_arc = Arc::clone(&self.load_channel);

            supervision.mark_busy(true);
            let _epoch = supervision.bump_epoch();

            tokio::spawn(async move {
                let load_res = tokio::task::spawn_blocking(move || (loader)()).await;
                supervision.mark_busy(false);

                let mut st = state_arc.lock().await;
                let outcome = match load_res {
                    Ok(Ok(loaded)) => {
                        *weak_instance_arc.lock().await = Some(Arc::downgrade(&loaded));
                        *instance_arc.lock().await = Some(loaded);
                        *st = AsrState::Ready;
                        *last_active_arc.lock().await = Instant::now();
                        state_changed.notify_waiters();
                        Ok(())
                    }
                    Ok(Err(_err)) => {
                        supervision.mark_degraded(true);
                        *st = AsrState::Fault;
                        state_changed.notify_waiters();
                        Err(AsrLifecycleError::Degraded)
                    }
                    Err(_) => {
                        supervision.mark_degraded(true);
                        *st = AsrState::Fault;
                        state_changed.notify_waiters();
                        Err(AsrLifecycleError::Degraded)
                    }
                };

                let _ = tx.send(Some(outcome));
                *load_channel_arc.lock().await = None;
            });

            let rx_opt = self.load_channel.lock().await.clone();
            if let Some(mut rx) = rx_opt {
                loop {
                    if let Some(res) = rx.borrow().clone() {
                        return res;
                    }
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            }
            return Ok(());
        }
    }

    pub async fn buffer_cold_utterance(
        &self,
        session_id: String,
        turn_id: u64,
        generation_id: u64,
        pcm: Vec<f32>,
        deadline: Instant,
    ) -> Result<(), AsrLifecycleError> {
        let mut waiting = self.cold_waiting.lock().await;
        if waiting.is_some() {
            return Err(AsrLifecycleError::ColdSecondUtteranceRejected);
        }

        *waiting = Some(ColdWaitingUtterance {
            session_id,
            turn_id,
            generation_id,
            pcm,
            completed_at: Instant::now(),
            deadline,
        });
        Ok(())
    }

    pub async fn take_cold_utterance(&self) -> Result<Option<ColdWaitingUtterance>, AsrLifecycleError> {
        let mut waiting = self.cold_waiting.lock().await;
        if let Some(item) = waiting.take() {
            if item.generation_id != self.active_generation.load(Ordering::SeqCst) {
                return Ok(None);
            }
            if Instant::now() > item.deadline {
                return Err(AsrLifecycleError::TurnDeadlineExpired);
            }
            Ok(Some(item))
        } else {
            Ok(None)
        }
    }

    pub async fn clear_cold_utterance(&self) {
        let mut waiting = self.cold_waiting.lock().await;
        *waiting = None;
    }

    /// Execute a transcription lease on the loaded ASR instance.
    pub async fn acquire_transcriber(&self) -> Result<AsrLease<T>, AsrLifecycleError> {
        let inst = self.instance.lock().await;
        let mut st = self.state.lock().await;

        if *st == AsrState::Fault || *st == AsrState::Unloaded {
            return Err(AsrLifecycleError::Degraded);
        }

        if let Some(arc) = inst.as_ref() {
            if *st == AsrState::TimingIdle || *st == AsrState::Draining {
                *st = AsrState::Ready;
            }
            *self.last_active.lock().await = Instant::now();
            self.active_leases.fetch_add(1, Ordering::SeqCst);
            Ok(AsrLease {
                instance: Arc::clone(arc),
                active_counter: Arc::clone(&self.active_leases),
                draining_flag: Arc::clone(&self.in_flight_draining),
                completed: Arc::new(AtomicBool::new(false)),
            })
        } else {
            Err(AsrLifecycleError::Degraded)
        }
    }

    /// Background idle reaper step.
    /// Draining and active leases strictly block reaping!
    pub async fn try_reap_idle(&self) -> bool {
        let mut st = self.state.lock().await;
        if *st == AsrState::Ready {
            *st = AsrState::TimingIdle;
        }

        if *st != AsrState::TimingIdle {
            return false;
        }

        if self.active_leases.load(Ordering::SeqCst) > 0 {
            return false;
        }

        // Native in-flight draining strictly blocks reaping!
        if self.in_flight_draining.load(Ordering::SeqCst) {
            return false;
        }

        let elapsed = self.last_active.lock().await.elapsed();
        if elapsed < self.idle_timeout {
            return false;
        }

        // Advance state to Unloading first under state lock
        *st = AsrState::Unloading;
        drop(st);

        // Then acquire instance lock and extract instance
        let mut inst = self.instance.lock().await;
        let arc_opt = inst.take();
        drop(inst);

        let unloader = self.unloader.clone();
        let state_arc = Arc::clone(&self.state);
        let unload_finished = Arc::clone(&self.unload_finished);
        let state_changed = Arc::clone(&self.state_changed);

        // Spawn detached blocking task: survives reaper future abort!
        let join_handle = tokio::task::spawn(async move {
            tokio::task::spawn_blocking(move || {
                if let Some(arc) = arc_opt {
                    if let Some(unloader_fn) = unloader {
                        (unloader_fn)(arc);
                    } else {
                        drop(arc);
                    }
                }
            }).await.unwrap();

            let mut st = state_arc.lock().await;
            *st = AsrState::Unloaded;
            unload_finished.notify_waiters();
            state_changed.notify_waiters();
        });

        let _ = join_handle.await;

        true
    }
}
