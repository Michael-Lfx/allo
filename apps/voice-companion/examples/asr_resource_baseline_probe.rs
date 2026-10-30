//! P0 Baseline Probe: Measures real SenseVoiceSmall ASR loading, inference,
//! drop memory reclamation, reload latency, and unloaded audio pipeline timing.
//!
//! Run with: `cargo run -p voice-companion --example asr_resource_baseline_probe`

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::frame::AudioChannel;
use nomifun_audio::recorder::SttCallback;
use nomifun_audio::stt::try_load_sherpa;
use nomifun_audio::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySample {
    pub working_set_bytes: u64,
    pub private_bytes: u64,
    pub virtual_bytes: u64,
}

#[cfg(windows)]
fn query_windows_memory() -> MemorySample {
    use std::mem::MaybeUninit;
    #[repr(C)]
    #[allow(non_snake_case)]
    struct PROCESS_MEMORY_COUNTERS_EX {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
        PrivateUsage: usize,
    }

    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessMemoryInfo(
            process: isize,
            counters: *mut PROCESS_MEMORY_COUNTERS_EX,
            cb: u32,
        ) -> i32;
    }

    unsafe {
        let mut pmc = MaybeUninit::<PROCESS_MEMORY_COUNTERS_EX>::zeroed();
        let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ret = GetProcessMemoryInfo(GetCurrentProcess(), pmc.as_mut_ptr(), cb);
        if ret != 0 {
            let counters = pmc.assume_init();
            MemorySample {
                working_set_bytes: counters.WorkingSetSize as u64,
                private_bytes: counters.PrivateUsage as u64,
                virtual_bytes: counters.PagefileUsage as u64,
            }
        } else {
            MemorySample {
                working_set_bytes: 0,
                private_bytes: 0,
                virtual_bytes: 0,
            }
        }
    }
}

#[cfg(not(windows))]
fn query_windows_memory() -> MemorySample {
    MemorySample {
        working_set_bytes: 0,
        private_bytes: 0,
        virtual_bytes: 0,
    }
}

#[tokio::main]
async fn main() {
    println!("=== ASR RESOURCE BASELINE PROBE (PID: {}) ===", std::process::id());

    let models_dir = PathBuf::from("D:/tmp/flowy-dev/models");
    let asr_model_path = models_dir.join("asr/model.int8.onnx");
    if !asr_model_path.is_file() {
        eprintln!("ERROR: Model file not found at {:?}", asr_model_path);
        std::process::exit(1);
    }

    // -----------------------------------------------------------------------
    // Part 1: Unloaded Audio Pipeline Baseline
    // -----------------------------------------------------------------------
    println!("\n[Part 1] Testing Unloaded Audio Pipeline (20ms frames, SPSC, 100ms backlog limit)...");
    let sample_rate = 16000;
    let chunk_size = 320; // 20ms
    let pipeline_config = PipelineConfig {
        sample_rate,
        channels: 1,
        chunk_size,
        capacity_samples: 48000,
        backlog_limit_ms: 100,
    };

    let (mut pipeline, producer) = VoiceCapturePipeline::new(pipeline_config);
    let mut consumer_intervals_ms = Vec::new();
    let mut frame_ages_ms = Vec::new();

    // Spawn producer delivering 100 frames at 20ms cadence
    let producer_handle = tokio::spawn(async move {
        for frame_idx in 0..100 {
            let pcm = vec![0.01f32; 320];
            producer.push(&pcm);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        producer.close();
    });

    let mut last_consumer_tick = Instant::now();
    let consumer_start = Instant::now();
    let mut frames_consumed = 0;

    while let CaptureReadResult::Chunk(_chunk) = pipeline.next_chunk().await {
        let now = Instant::now();
        let interval = now.duration_since(last_consumer_tick).as_secs_f64() * 1000.0;
        last_consumer_tick = now;
        frames_consumed += 1;

        if frames_consumed > 1 {
            consumer_intervals_ms.push(interval);
        }

        // Theoretical frame age based on producer 20ms rate
        let expected_prod_time = consumer_start + Duration::from_millis((frames_consumed - 1) * 20);
        let age_ms = if now >= expected_prod_time {
            now.duration_since(expected_prod_time).as_secs_f64() * 1000.0
        } else {
            0.0
        };
        frame_ages_ms.push(age_ms);
    }
    let _ = producer_handle.await;

    consumer_intervals_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    frame_ages_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let max_interval_ms = consumer_intervals_ms.last().copied().unwrap_or(0.0);
    let p50_interval_ms = consumer_intervals_ms.get(consumer_intervals_ms.len() / 2).copied().unwrap_or(0.0);
    let p95_interval_ms = consumer_intervals_ms.get(consumer_intervals_ms.len() * 95 / 100).copied().unwrap_or(0.0);

    let max_age_ms = frame_ages_ms.last().copied().unwrap_or(0.0);
    let p50_age_ms = frame_ages_ms.get(frame_ages_ms.len() / 2).copied().unwrap_or(0.0);
    let p95_age_ms = frame_ages_ms.get(frame_ages_ms.len() * 95 / 100).copied().unwrap_or(0.0);

    println!("  Frames consumed: {}", frames_consumed);
    println!("  Consumer interval (ms): P50={:.2}, P95={:.2}, Max={:.2} (Gate: <=100ms)", p50_interval_ms, p95_interval_ms, max_interval_ms);
    println!("  Frame age (ms): P50={:.2}, P95={:.2}, Max={:.2} (Gate: <=200ms)", p50_age_ms, p95_age_ms, max_age_ms);

    // -----------------------------------------------------------------------
    // Part 2: ASR Lifecycle Resource & Latency Measurements
    // -----------------------------------------------------------------------
    println!("\n[Part 2] Measuring ASR Lifecycle Resources and Latencies...");

    // Stage 1: Initial Baseline Memory (Unloaded)
    tokio::time::sleep(Duration::from_millis(500)).await;
    let baseline_mem = query_windows_memory();
    println!("  1. Baseline (Unloaded):");
    println!("     Working Set:   {:.2} MB ({} bytes)", baseline_mem.working_set_bytes as f64 / 1_048_576.0, baseline_mem.working_set_bytes);
    println!("     Private Bytes: {:.2} MB ({} bytes)", baseline_mem.private_bytes as f64 / 1_048_576.0, baseline_mem.private_bytes);

    // Stage 2: Initial Load (Cold Load)
    println!("\n  2. Executing Initial Cold Load...");
    let load_start = Instant::now();
    let asr_instance = try_load_sherpa(Some(&models_dir))
        .expect("Sherpa model loading failed (returned None)")
        .expect("Failed to initialize SherpaSttCallback");
    let load_duration_ms = load_start.elapsed().as_secs_f64() * 1000.0;
    let loaded_mem = query_windows_memory();

    let ws_load_delta_mb = (loaded_mem.working_set_bytes as f64 - baseline_mem.working_set_bytes as f64) / 1_048_576.0;
    let pb_load_delta_mb = (loaded_mem.private_bytes as f64 - baseline_mem.private_bytes as f64) / 1_048_576.0;

    println!("     Cold Load Time: {:.2} ms", load_duration_ms);
    println!("     Working Set:   {:.2} MB (Delta: +{:.2} MB)", loaded_mem.working_set_bytes as f64 / 1_048_576.0, ws_load_delta_mb);
    println!("     Private Bytes: {:.2} MB (Delta: +{:.2} MB)", loaded_mem.private_bytes as f64 / 1_048_576.0, pb_load_delta_mb);

    // Stage 3: Warm Inference
    println!("\n  3. Executing Warm Inference (command-16k.wav)...");
    let fixture_path = Path::new("docs/architecture/voice-agent/execution/audio-fixtures/command-16k.wav");
    let fixture_bytes = std::fs::read(fixture_path).expect("Read fixture wav");
    let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&fixture_bytes);

    let infer_start = Instant::now();
    let transcript = asr_instance.transcribe(AudioChannel::Mic, pcm, rate).await;
    let infer_duration_ms = infer_start.elapsed().as_secs_f64() * 1000.0;
    let infer_mem = query_windows_memory();

    println!("     Inference Time: {:.2} ms", infer_duration_ms);
    println!("     Transcript: {:?}", transcript);
    println!("     Working Set:   {:.2} MB", infer_mem.working_set_bytes as f64 / 1_048_576.0);
    println!("     Private Bytes: {:.2} MB", infer_mem.private_bytes as f64 / 1_048_576.0);

    // Stage 4: Drop / Unload
    println!("\n  4. Dropping ASR Instance (Testing Reclamation)...");
    drop(asr_instance);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let dropped_mem = query_windows_memory();

    let ws_dropped_delta_mb = (dropped_mem.working_set_bytes as f64 - loaded_mem.working_set_bytes as f64) / 1_048_576.0;
    let pb_dropped_delta_mb = (dropped_mem.private_bytes as f64 - loaded_mem.private_bytes as f64) / 1_048_576.0;

    println!("     Working Set after Drop:   {:.2} MB (Delta from loaded: {:.2} MB)", dropped_mem.working_set_bytes as f64 / 1_048_576.0, ws_dropped_delta_mb);
    println!("     Private Bytes after Drop: {:.2} MB (Delta from loaded: {:.2} MB)", dropped_mem.private_bytes as f64 / 1_048_576.0, pb_dropped_delta_mb);

    // Stage 5: Reload (OS Page Cache Warmth)
    println!("\n  5. Executing Reload (Warm OS Page Cache)...");
    let reload_start = Instant::now();
    let asr_reload = try_load_sherpa(Some(&models_dir))
        .expect("Sherpa model reload failed (returned None)")
        .expect("Failed to initialize SherpaSttCallback on reload");
    let reload_duration_ms = reload_start.elapsed().as_secs_f64() * 1000.0;
    let reload_mem = query_windows_memory();

    println!("     Reload Time: {:.2} ms", reload_duration_ms);
    println!("     Working Set after Reload:   {:.2} MB", reload_mem.working_set_bytes as f64 / 1_048_576.0);
    println!("     Private Bytes after Reload: {:.2} MB", reload_mem.private_bytes as f64 / 1_048_576.0);

    drop(asr_reload);

    // -----------------------------------------------------------------------
    // Part 3: Output JSON Results
    // -----------------------------------------------------------------------
    let results = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "unloaded_pipeline": {
            "frames_consumed": frames_consumed,
            "max_interval_ms": max_interval_ms,
            "p50_interval_ms": p50_interval_ms,
            "p95_interval_ms": p95_interval_ms,
            "max_age_ms": max_age_ms,
            "p50_age_ms": p50_age_ms,
            "p95_age_ms": p95_age_ms,
            "gate_interval_passed": max_interval_ms <= 100.0,
            "gate_age_passed": max_age_ms <= 200.0
        },
        "asr_lifecycle": {
            "cold_load_duration_ms": load_duration_ms,
            "reload_duration_ms": reload_duration_ms,
            "inference_duration_ms": infer_duration_ms,
            "transcript": transcript,
            "baseline_working_set_bytes": baseline_mem.working_set_bytes,
            "baseline_private_bytes": baseline_mem.private_bytes,
            "loaded_working_set_bytes": loaded_mem.working_set_bytes,
            "loaded_private_bytes": loaded_mem.private_bytes,
            "infer_working_set_bytes": infer_mem.working_set_bytes,
            "infer_private_bytes": infer_mem.private_bytes,
            "dropped_working_set_bytes": dropped_mem.working_set_bytes,
            "dropped_private_bytes": dropped_mem.private_bytes,
            "reloaded_working_set_bytes": reload_mem.working_set_bytes,
            "reloaded_private_bytes": reload_mem.private_bytes,
            "working_set_delta_on_load_bytes": loaded_mem.working_set_bytes.saturating_sub(baseline_mem.working_set_bytes),
            "private_bytes_delta_on_load_bytes": loaded_mem.private_bytes.saturating_sub(baseline_mem.private_bytes),
            "working_set_delta_on_drop_bytes": dropped_mem.working_set_bytes as i64 - loaded_mem.working_set_bytes as i64,
            "private_bytes_delta_on_drop_bytes": dropped_mem.private_bytes as i64 - loaded_mem.private_bytes as i64
        }
    });

    let output_path = "build.noindex/voice-asr-lifecycle-execution-20261029/p0_baseline_results.json";
    std::fs::write(output_path, serde_json::to_string_pretty(&results).unwrap()).expect("Write results JSON");
    println!("\nResults successfully saved to {}", output_path);
}
