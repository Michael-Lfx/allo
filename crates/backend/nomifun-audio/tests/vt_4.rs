//! VT-4: Wave 4 宿主打包与硬件可靠性加固纵向测试集 (Host Packaging & Reliability Hardening)
//!
//! Authoritative baselines:
//! - docs/architecture/voice-agent/interaction.zh.md (§5 TC-AM-011, Gate 4)
//! - docs/architecture/voice-agent/execution/wave-4-hardening.zh.md (T4-05, VT-4-02)
//! - docs/architecture/voice-agent/execution/test-strategy.zh.md

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use nomifun_audio::capture::{AudioCaptureSource, CaptureReadResult};
use nomifun_audio::hotplug::{HotplugState, UsbHotplugWatchdog, UsbHotplugWatchdogConfig};
use nomifun_audio::kws::KwsEngine;
use tokio::sync::Mutex;

/// Mock audio capture source for simulating hardware disconnects and replugs
struct MockFailableSource {
    label: String,
    sample_rate: u32,
    should_fail: Arc<AtomicBool>,
    failure_message: String,
    chunks: Arc<Mutex<VecDeque<Vec<f32>>>>,
}

impl MockFailableSource {
    fn new(
        label: impl Into<String>,
        sample_rate: u32,
        should_fail: Arc<AtomicBool>,
        failure_message: impl Into<String>,
        chunks: Vec<Vec<f32>>,
    ) -> Self {
        Self {
            label: label.into(),
            sample_rate,
            should_fail,
            failure_message: failure_message.into(),
            chunks: Arc::new(Mutex::new(chunks.into())),
        }
    }
}

#[async_trait]
impl AudioCaptureSource for MockFailableSource {
    async fn read_chunk(&self) -> CaptureReadResult {
        if self.should_fail.load(Ordering::Acquire) {
            return CaptureReadResult::DeviceError(self.failure_message.clone());
        }

        let mut lock = self.chunks.lock().await;
        if let Some(chunk) = lock.pop_front() {
            CaptureReadResult::Chunk(chunk)
        } else {
            CaptureReadResult::TemporarilyEmpty
        }
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn channels(&self) -> u16 {
        1
    }

    fn label(&self) -> &str {
        &self.label
    }
}

/// Helper: generate synthetic wake word waveform (/floʊi/ accent, ~350Hz, 1 second @ 16kHz)
fn generate_wake_audio_chunks(chunk_size: usize) -> Vec<Vec<f32>> {
    let total_samples = 16000;
    let mut pcm = vec![0.0f32; total_samples];
    for (i, sample) in pcm.iter_mut().enumerate() {
        let t = i as f32 / 16000.0;
        *sample = (2.0 * std::f32::consts::PI * 350.0 * t).sin() * 0.6;
    }
    pcm.chunks(chunk_size).map(|c| c.to_vec()).collect()
}

/// VT-4-02: 模拟 USB 麦克风拔出再插回
///
/// 断言：
/// 1. 麦克风硬件断开 (`CaptureReadResult::DeviceError`) 触发看门狗捕获
/// 2. 日志与运行状态无 panic，平稳进入 `Degraded` 状态
/// 3. 硬件插回后，在 ≤ 2 秒内重新枚举并绑定系统默认输入源
/// 4. 恢复无锁 SPSC 音频采样与唤醒监听，KWS 引擎成功检测到唤醒词 "HEY FLOWY"
/// 5. 实测自愈恢复延迟严格满足 ≤ 2.0 秒 SLA
#[tokio::test]
async fn vt_4_02_usb_mic_hotplug_watchdog_recovers_within_two_seconds() {
    let sample_rate = 16000;
    let chunk_size = 320; // 20ms chunks

    // Shared mock hardware state
    let hardware_plugged = Arc::new(AtomicBool::new(true));

    let initial_fail = Arc::new(AtomicBool::new(false));
    let initial_fail_clone = Arc::clone(&initial_fail);

    // Initial source streams silence chunks
    let initial_chunks = vec![vec![0.0f32; chunk_size]; 50];
    let initial_source = Box::new(MockFailableSource::new(
        "mock-usb-mic-initial",
        sample_rate,
        initial_fail_clone,
        "USB microphone disconnected: I/O communication failed (0x8007045D)",
        initial_chunks,
    ));

    // Factory simulates OS device enumeration and default input binding
    let factory_plugged = Arc::clone(&hardware_plugged);
    let replug_source_factory = Arc::new(move || -> Result<Box<dyn AudioCaptureSource>, String> {
        if !factory_plugged.load(Ordering::Acquire) {
            return Err("OS default input device not found: no audio endpoints".to_string());
        }

        // When plugged back in, provide source loaded with wake word audio
        let wake_chunks = generate_wake_audio_chunks(chunk_size);
        let healthy = Arc::new(AtomicBool::new(false));
        Ok(Box::new(MockFailableSource::new(
            "mock-usb-mic-rebound",
            sample_rate,
            healthy,
            "",
            wake_chunks,
        )))
    });

    let config = UsbHotplugWatchdogConfig {
        sample_rate,
        probe_interval: Duration::from_millis(50),
        max_recovery_budget: Duration::from_secs(2),
    };

    let watchdog = UsbHotplugWatchdog::with_initial_source(
        config,
        initial_source,
        replug_source_factory,
    );

    assert_eq!(watchdog.state(), HotplugState::Healthy);
    assert_eq!(watchdog.sample_rate(), 16000);
    assert_eq!(watchdog.channels(), 1);

    // 1. Initial healthy chunk consumption
    match watchdog.read_chunk().await {
        CaptureReadResult::Chunk(c) => assert_eq!(c.len(), chunk_size),
        other => panic!("Expected healthy audio chunk, got {other:?}"),
    }

    // 2. Simulate user abruptly pulling out the USB microphone
    println!("[VT-4-02] Simulating USB microphone disconnect...");
    hardware_plugged.store(false, Ordering::Release);
    initial_fail.store(true, Ordering::Release);

    // 3. Next read detects hardware disconnect
    let read_res = watchdog.read_chunk().await;
    // Watchdog captures DeviceError, transitions to Degraded, and yields TemporarilyEmpty while probing
    assert_eq!(watchdog.state(), HotplugState::Degraded);
    assert_eq!(watchdog.stats().disconnect_count, 1);
    match read_res {
        CaptureReadResult::TemporarilyEmpty | CaptureReadResult::DeviceError(_) => {
            // Self-healing in progress
        }
        other => panic!("Unexpected result during unplug: {other:?}"),
    }

    // 4. While unplugged, watchdog stays in Degraded and non-blocking polling returns TemporarilyEmpty
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let res = watchdog.read_chunk().await;
        assert_eq!(watchdog.state(), HotplugState::Degraded);
        assert_eq!(res, CaptureReadResult::TemporarilyEmpty);
    }

    // 5. Simulate user plugging the USB microphone back in
    println!("[VT-4-02] Simulating USB microphone replug...");
    let replug_time = Instant::now();
    hardware_plugged.store(true, Ordering::Release);

    // Initialize KWS engine to verify wake word listening resumes
    let mut kws = KwsEngine::load_default().expect("KWS engine must load");
    let mut wake_detected = false;
    let mut chunks_received_after_replug = 0;
    let mut first_recovered_chunk_latency: Option<Duration> = None;

    // 6. Audio loop continuously consumes from watchdog, asserting recovery within 2 seconds
    let timeout = Duration::from_secs(2);
    while replug_time.elapsed() < timeout {
        match watchdog.read_chunk().await {
            CaptureReadResult::Chunk(chunk) => {
                if first_recovered_chunk_latency.is_none() {
                    first_recovered_chunk_latency = Some(replug_time.elapsed());
                }
                chunks_received_after_replug += 1;
                if let Some(res) = kws.process_pcm_chunk(&chunk) {
                    if res.keyword == "HEY FLOWY" {
                        wake_detected = true;
                        break;
                    }
                }
            }
            CaptureReadResult::TemporarilyEmpty => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            CaptureReadResult::StreamClosed => {
                break;
            }
            CaptureReadResult::DeviceError(e) => {
                panic!("Unexpected DeviceError after replug: {e}");
            }
        }
    }

    let recovery_latency = replug_time.elapsed();
    println!(
        "[VT-4-02] Watchdog recovery completed in {:?}, first chunk: {:?}, chunks: {}, wake_detected: {}",
        recovery_latency, first_recovered_chunk_latency, chunks_received_after_replug, wake_detected
    );

    // 7. Verify Gate 4 & TC-AM-011 criteria
    assert!(
        wake_detected,
        "Wake word 'HEY FLOWY' must be detected after USB replug"
    );
    assert!(
        recovery_latency <= Duration::from_secs(2),
        "Recovery latency must be <= 2.0s SLA (Gate 4), got {:?}",
        recovery_latency
    );
    assert!(
        first_recovered_chunk_latency.is_some()
            && first_recovered_chunk_latency.unwrap() <= Duration::from_secs(2),
        "First audio chunk after replug must arrive within 2 seconds"
    );

    let stats = watchdog.stats();
    assert_eq!(stats.disconnect_count, 1, "Disconnect count must be 1");
    assert_eq!(
        stats.reconnect_success_count, 1,
        "Reconnect success count must be 1"
    );
    assert!(
        watchdog.state() == HotplugState::Healthy
            || watchdog.state() == HotplugState::Recovered
    );
}

/// Test watchdog starting in Degraded state when initial device is absent, then healing on replug
#[tokio::test]
async fn vt_4_watchdog_starts_degraded_when_no_initial_device_and_heals() {
    let sample_rate = 16000;
    let plugged = Arc::new(AtomicBool::new(false));
    let plugged_factory = Arc::clone(&plugged);

    let factory = Arc::new(move || -> Result<Box<dyn AudioCaptureSource>, String> {
        if plugged_factory.load(Ordering::Acquire) {
            let chunk_size = 320;
            let chunks = vec![vec![0.1f32; chunk_size]; 10];
            let healthy = Arc::new(AtomicBool::new(false));
            Ok(Box::new(MockFailableSource::new(
                "mock-newly-plugged",
                sample_rate,
                healthy,
                "",
                chunks,
            )))
        } else {
            Err("No device found".into())
        }
    });

    let config = UsbHotplugWatchdogConfig {
        sample_rate,
        probe_interval: Duration::from_millis(50),
        max_recovery_budget: Duration::from_secs(2),
    };

    let watchdog = UsbHotplugWatchdog::with_factory(config, factory);
    assert_eq!(watchdog.state(), HotplugState::Degraded);

    let res = watchdog.read_chunk().await;
    assert_eq!(res, CaptureReadResult::TemporarilyEmpty);

    // Now plug device in
    plugged.store(true, Ordering::Release);
    tokio::time::sleep(Duration::from_millis(60)).await;

    let res2 = watchdog.read_chunk().await;
    match res2 {
        CaptureReadResult::Chunk(c) => assert_eq!(c.len(), 320),
        other => panic!("Expected chunk after plug-in, got {other:?}"),
    }

    assert!(watchdog.is_healthy() || watchdog.state() == HotplugState::Recovered);
    assert_eq!(watchdog.stats().reconnect_success_count, 1);
}

/// Verify multiple consecutive disconnect / reconnect cycles with zero panics
#[tokio::test]
async fn vt_4_watchdog_multiple_disconnect_cycles_no_panic() {
    let sample_rate = 16000;
    let cycles = 5;

    let is_connected = Arc::new(AtomicBool::new(true));
    let factory_connected = Arc::clone(&is_connected);

    let active_fail_flag = Arc::new(tokio::sync::Mutex::new(Arc::new(AtomicBool::new(false))));
    let factory_fail_flag = Arc::clone(&active_fail_flag);

    let factory = Arc::new(move || -> Result<Box<dyn AudioCaptureSource>, String> {
        if factory_connected.load(Ordering::Acquire) {
            let chunks = vec![vec![0.05f32; 320]; 100];
            let fail = Arc::new(AtomicBool::new(false));
            if let Ok(mut g) = factory_fail_flag.try_lock() {
                *g = Arc::clone(&fail);
            }
            Ok(Box::new(MockFailableSource::new(
                "mock-cycle-source",
                sample_rate,
                fail,
                "Simulated USB disconnect",
                chunks,
            )))
        } else {
            Err("Device disconnected".into())
        }
    });

    let config = UsbHotplugWatchdogConfig {
        sample_rate,
        probe_interval: Duration::from_millis(20),
        max_recovery_budget: Duration::from_secs(2),
    };

    let watchdog = UsbHotplugWatchdog::with_factory(config, factory);

    for cycle in 1..=cycles {
        // 1. Consume healthy chunk
        let res = watchdog.read_chunk().await;
        match res {
            CaptureReadResult::Chunk(c) => assert_eq!(c.len(), 320),
            other => panic!("Cycle {cycle}: expected Chunk, got {other:?}"),
        }

        // 2. Unplug: mark current source as failing and disconnect factory
        is_connected.store(false, Ordering::Release);
        {
            let lock = active_fail_flag.lock().await;
            lock.store(true, Ordering::Release);
        }

        // Watchdog detects DeviceError, transitions to Degraded
        let read_err = watchdog.read_chunk().await;
        assert_eq!(watchdog.state(), HotplugState::Degraded);
        assert!(matches!(
            read_err,
            CaptureReadResult::TemporarilyEmpty | CaptureReadResult::DeviceError(_)
        ));

        // 3. Replug
        is_connected.store(true, Ordering::Release);
        tokio::time::sleep(Duration::from_millis(30)).await;

        // 4. Assert healed
        let res_recovered = watchdog.read_chunk().await;
        match res_recovered {
            CaptureReadResult::Chunk(c) => assert_eq!(c.len(), 320),
            other => panic!("Cycle {cycle}: expected recovered Chunk, got {other:?}"),
        }
        assert!(watchdog.is_healthy() || watchdog.state() == HotplugState::Recovered);
    }

    let stats = watchdog.stats();
    assert_eq!(stats.disconnect_count, cycles as u64);
    assert_eq!(stats.reconnect_success_count, cycles as u64);
}

/// Verify atomic status inspection is non-blocking and thread-safe
#[tokio::test]
async fn vt_4_watchdog_atomic_queries_thread_safety() {
    let sample_rate = 16000;
    let config = UsbHotplugWatchdogConfig {
        sample_rate,
        probe_interval: Duration::from_millis(50),
        max_recovery_budget: Duration::from_secs(2),
    };

    let factory = Arc::new(move || -> Result<Box<dyn AudioCaptureSource>, String> {
        Ok(Box::new(MockFailableSource::new(
            "mock-source",
            sample_rate,
            Arc::new(AtomicBool::new(false)),
            "",
            vec![vec![0.0f32; 160]],
        )))
    });

    let watchdog = Arc::new(UsbHotplugWatchdog::with_factory(config, factory));
    assert!(watchdog.is_healthy());
    assert!(!watchdog.is_degraded());

    // Spawn multiple reader tasks inspecting state & stats concurrently
    let mut handles = Vec::new();
    for _ in 0..10 {
        let wd = Arc::clone(&watchdog);
        handles.push(tokio::spawn(async move {
            for _ in 0..100 {
                let _s = wd.state();
                let _st = wd.stats();
                let _h = wd.is_healthy();
                let _d = wd.is_degraded();
            }
        }));
    }

    for h in handles {
        h.await.expect("Inspector task should complete cleanly");
    }
}
