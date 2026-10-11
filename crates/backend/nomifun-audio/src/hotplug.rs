//! USB microphone hotplug self-healing watchdog.
//!
//! Authoritative baseline:
//! - docs/architecture/voice-agent/interaction.zh.md (§5 TC-AM-011, Gate 4)
//! - docs/architecture/voice-agent/execution/wave-4-hardening.zh.md (T4-05, VT-4-02)
//!
//! Features:
//! - Monitors underlying audio capture sources for `CaptureReadResult::DeviceError`
//!   or hardware disconnection.
//! - Enters non-panicking, graceful self-healing state (`HotplugState::Degraded`).
//! - Periodically probes system default audio input device (or mock source factory)
//!   and rebinds default source within <= 2.0 seconds after physical replug.
//! - Automatically restores lock-free SPSC audio sampling and wake word listening.
//! - Exposes lock-free atomic queries for health and hotplug statistics.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::capture::{AudioCaptureSource, CaptureReadResult};
use crate::mic::MicSource;

/// Operational status of the audio capture source under hotplug supervision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugState {
    /// Audio capture device is active and operating normally.
    Healthy,
    /// Audio device disconnected or experienced hardware error; actively probing for reconnection.
    Degraded,
    /// Audio device has been successfully rebound after a hardware error.
    Recovered,
}

/// Statistics on hotplug events and self-healing recovery times.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotplugStats {
    /// Total number of hardware disconnects / `DeviceError` occurrences observed.
    pub disconnect_count: u64,
    /// Total number of successful re-enumerations and rebinds.
    pub reconnect_success_count: u64,
    /// Latency of the most recent recovery in milliseconds (from disconnect/probe to rebind).
    pub last_recovery_time_ms: Option<u64>,
}

/// Factory function producing a freshly opened `AudioCaptureSource`.
pub type AudioSourceFactory =
    Arc<dyn Fn() -> Result<Box<dyn AudioCaptureSource>, String> + Send + Sync>;

/// Configuration parameters for `UsbHotplugWatchdog`.
#[derive(Debug, Clone)]
pub struct UsbHotplugWatchdogConfig {
    /// Target audio sample rate in Hz (e.g. 16_000).
    pub sample_rate: u32,
    /// Interval between reconnection probe attempts when in Degraded state (default: 50ms).
    pub probe_interval: Duration,
    /// Maximum allowed recovery budget (default: 2.0s per Gate 4 SLA).
    pub max_recovery_budget: Duration,
}

impl Default for UsbHotplugWatchdogConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            probe_interval: Duration::from_millis(50),
            max_recovery_budget: Duration::from_secs(2),
        }
    }
}

struct StatsAtomics {
    disconnect_count: AtomicU64,
    reconnect_success_count: AtomicU64,
    last_recovery_time_ms: AtomicU64,
}

struct WatchdogInner {
    source: Option<Box<dyn AudioCaptureSource>>,
    last_disconnect_time: Option<Instant>,
    last_probe_time: Option<Instant>,
    state: HotplugState,
    disconnect_count: u64,
    reconnect_success_count: u64,
    last_recovery_time_ms: Option<u64>,
}

/// USB Microphone Hotplug Self-Healing Watchdog.
///
/// Wraps an `AudioCaptureSource` and implements `AudioCaptureSource` itself,
/// transparently absorbing hardware disconnects and rebinding system default
/// input upon replug within <= 2 seconds.
pub struct UsbHotplugWatchdog {
    config: UsbHotplugWatchdogConfig,
    factory: AudioSourceFactory,
    inner: Arc<Mutex<WatchdogInner>>,
    state_atomic: Arc<AtomicU8>,
    stats_atomics: Arc<StatsAtomics>,
}

impl UsbHotplugWatchdog {
    /// Creates a watchdog wrapping the system default microphone.
    ///
    /// If default mic is currently available, starts in `Healthy`; if not,
    /// starts in `Degraded` and begins probing for connection.
    pub fn new(target_sample_rate: u32) -> Self {
        let sample_rate = if target_sample_rate == 0 {
            16_000
        } else {
            target_sample_rate
        };
        let config = UsbHotplugWatchdogConfig {
            sample_rate,
            probe_interval: Duration::from_millis(50),
            max_recovery_budget: Duration::from_secs(2),
        };

        let factory: AudioSourceFactory = Arc::new(move || {
            MicSource::new(sample_rate).map(|src| Box::new(src) as Box<dyn AudioCaptureSource>)
        });

        Self::with_factory(config, factory)
    }

    /// Creates a watchdog using a custom config and audio source factory.
    pub fn with_factory(
        config: UsbHotplugWatchdogConfig,
        factory: AudioSourceFactory,
    ) -> Self {
        let initial_source = factory().ok();
        let state = if initial_source.is_some() {
            HotplugState::Healthy
        } else {
            HotplugState::Degraded
        };

        let state_val = match state {
            HotplugState::Healthy => 0,
            HotplugState::Degraded => 1,
            HotplugState::Recovered => 2,
        };

        let inner = WatchdogInner {
            source: initial_source,
            last_disconnect_time: if state == HotplugState::Degraded {
                Some(Instant::now())
            } else {
                None
            },
            last_probe_time: None,
            state,
            disconnect_count: 0,
            reconnect_success_count: 0,
            last_recovery_time_ms: None,
        };

        Self {
            config,
            factory,
            inner: Arc::new(Mutex::new(inner)),
            state_atomic: Arc::new(AtomicU8::new(state_val)),
            stats_atomics: Arc::new(StatsAtomics {
                disconnect_count: AtomicU64::new(0),
                reconnect_success_count: AtomicU64::new(0),
                last_recovery_time_ms: AtomicU64::new(u64::MAX),
            }),
        }
    }

    /// Creates a watchdog with an explicit initial source and factory for rebind.
    pub fn with_initial_source(
        config: UsbHotplugWatchdogConfig,
        source: Box<dyn AudioCaptureSource>,
        factory: AudioSourceFactory,
    ) -> Self {
        let inner = WatchdogInner {
            source: Some(source),
            last_disconnect_time: None,
            last_probe_time: None,
            state: HotplugState::Healthy,
            disconnect_count: 0,
            reconnect_success_count: 0,
            last_recovery_time_ms: None,
        };

        Self {
            config,
            factory,
            inner: Arc::new(Mutex::new(inner)),
            state_atomic: Arc::new(AtomicU8::new(0)),
            stats_atomics: Arc::new(StatsAtomics {
                disconnect_count: AtomicU64::new(0),
                reconnect_success_count: AtomicU64::new(0),
                last_recovery_time_ms: AtomicU64::new(u64::MAX),
            }),
        }
    }

    /// Current operational status (lock-free query).
    pub fn state(&self) -> HotplugState {
        match self.state_atomic.load(Ordering::Acquire) {
            0 => HotplugState::Healthy,
            1 => HotplugState::Degraded,
            2 => HotplugState::Recovered,
            _ => HotplugState::Degraded,
        }
    }

    /// Current hotplug and self-healing statistics (lock-free query).
    pub fn stats(&self) -> HotplugStats {
        let last_rec = self.stats_atomics.last_recovery_time_ms.load(Ordering::Acquire);
        HotplugStats {
            disconnect_count: self.stats_atomics.disconnect_count.load(Ordering::Acquire),
            reconnect_success_count: self
                .stats_atomics
                .reconnect_success_count
                .load(Ordering::Acquire),
            last_recovery_time_ms: if last_rec == u64::MAX {
                None
            } else {
                Some(last_rec)
            },
        }
    }

    /// Checks if device is in healthy streaming mode.
    pub fn is_healthy(&self) -> bool {
        self.state() == HotplugState::Healthy
    }

    /// Checks if device is currently degraded / searching for reconnection.
    pub fn is_degraded(&self) -> bool {
        self.state() == HotplugState::Degraded
    }

    fn sync_atomics(&self, inner: &WatchdogInner) {
        let s_val = match inner.state {
            HotplugState::Healthy => 0,
            HotplugState::Degraded => 1,
            HotplugState::Recovered => 2,
        };
        self.state_atomic.store(s_val, Ordering::Release);
        self.stats_atomics
            .disconnect_count
            .store(inner.disconnect_count, Ordering::Release);
        self.stats_atomics
            .reconnect_success_count
            .store(inner.reconnect_success_count, Ordering::Release);
        self.stats_atomics.last_recovery_time_ms.store(
            inner.last_recovery_time_ms.unwrap_or(u64::MAX),
            Ordering::Release,
        );
    }
}

#[async_trait]
impl AudioCaptureSource for UsbHotplugWatchdog {
    async fn read_chunk(&self) -> CaptureReadResult {
        let mut inner = self.inner.lock().await;

        // 1. If currently in Degraded state (or missing source), probe factory for replug
        if inner.state == HotplugState::Degraded || inner.source.is_none() {
            let now = Instant::now();
            let should_probe = match inner.last_probe_time {
                Some(t) => now.duration_since(t) >= self.config.probe_interval,
                None => true,
            };

            if should_probe {
                inner.last_probe_time = Some(now);
                match (self.factory)() {
                    Ok(new_source) => {
                        let recovery_time = inner
                            .last_disconnect_time
                            .map(|t| now.duration_since(t))
                            .unwrap_or_default();
                        let recovery_ms = recovery_time.as_millis() as u64;

                        inner.reconnect_success_count += 1;
                        inner.last_recovery_time_ms = Some(recovery_ms);
                        inner.state = HotplugState::Recovered;
                        inner.source = Some(new_source);
                        self.sync_atomics(&inner);

                        info!(
                            "UsbHotplugWatchdog: successfully rebound audio input device in {:?}. SPSC sampling and wake listening restored (SLA <= 2.0s: {})",
                            recovery_time,
                            recovery_time <= self.config.max_recovery_budget
                        );

                        if let Some(src) = inner.source.as_ref() {
                            return src.read_chunk().await;
                        }
                    }
                    Err(err) => {
                        debug!(
                            "UsbHotplugWatchdog: reconnection probe failed ({err}), waiting for device replug..."
                        );
                        return CaptureReadResult::TemporarilyEmpty;
                    }
                }
            } else {
                return CaptureReadResult::TemporarilyEmpty;
            }
        }

        // 2. Read next chunk from active underlying source
        let read_res = if let Some(src) = inner.source.as_ref() {
            src.read_chunk().await
        } else {
            CaptureReadResult::TemporarilyEmpty
        };

        match read_res {
            CaptureReadResult::Chunk(chunk) => {
                if inner.state == HotplugState::Recovered {
                    inner.state = HotplugState::Healthy;
                    self.sync_atomics(&inner);
                }
                CaptureReadResult::Chunk(chunk)
            }
            CaptureReadResult::TemporarilyEmpty => CaptureReadResult::TemporarilyEmpty,
            CaptureReadResult::StreamClosed => CaptureReadResult::StreamClosed,
            CaptureReadResult::DeviceError(err) => {
                warn!(
                    "UsbHotplugWatchdog: captured hardware error: '{err}'. Initiating self-healing watchdog reconnect loop..."
                );
                inner.disconnect_count += 1;
                let disconnect_time = Instant::now();
                inner.last_disconnect_time = Some(disconnect_time);
                inner.last_probe_time = Some(disconnect_time);
                inner.state = HotplugState::Degraded;
                inner.source = None;
                self.sync_atomics(&inner);

                // Immediate probe in case of transient bounce or fast re-enumeration
                match (self.factory)() {
                    Ok(new_source) => {
                        let recovery_time = disconnect_time.elapsed();
                        let recovery_ms = recovery_time.as_millis() as u64;

                        inner.reconnect_success_count += 1;
                        inner.last_recovery_time_ms = Some(recovery_ms);
                        inner.state = HotplugState::Recovered;
                        inner.source = Some(new_source);
                        self.sync_atomics(&inner);

                        info!(
                            "UsbHotplugWatchdog: immediate rebind succeeded in {:?}",
                            recovery_time
                        );

                        if let Some(src) = inner.source.as_ref() {
                            return src.read_chunk().await;
                        }
                        CaptureReadResult::TemporarilyEmpty
                    }
                    Err(_) => {
                        // Device is still physically detached; return TemporarilyEmpty to consumer (no panic)
                        CaptureReadResult::TemporarilyEmpty
                    }
                }
            }
        }
    }

    fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    fn channels(&self) -> u16 {
        1
    }

    fn label(&self) -> &str {
        "usb-hotplug-watchdog"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_config_defaults() {
        let cfg = UsbHotplugWatchdogConfig::default();
        assert_eq!(cfg.sample_rate, 16_000);
        assert_eq!(cfg.probe_interval, Duration::from_millis(50));
        assert_eq!(cfg.max_recovery_budget, Duration::from_secs(2));
    }
}
