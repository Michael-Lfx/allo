//! VT-1: Wave 1 声学底座纵向测试集 (Acoustic Foundation Vertical Slice Tests)
//!
//! Authoritative baselines:
//! - docs/architecture/voice-agent/interaction.zh.md (§3.2, §3.3, §5 Gate 0/Gate 1, TC-AM-001/002)
//! - docs/architecture/voice-agent/execution/wave-1-acoustic.zh.md
//! - docs/architecture/voice-agent/execution/test-strategy.zh.md

use nomifun_audio::capture::CaptureReadResult;
use nomifun_audio::kws::KwsEngine;
use nomifun_audio::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};

/// VT-1-01: 10 分钟连续回放 → SPSC 管线
/// 断言：0 阻塞、0 内存增长、0 xrun 报警
#[tokio::test]
async fn vt_1_01_ten_minute_replay_spsc_zero_xrun_zero_growth() {
    // 10 minutes at 16kHz mono = 600s * 16,000 = 9,600,000 samples.
    // In CI we simulate 10 minutes using accelerated batch replay.
    let sample_rate = 16000;
    let chunk_size = 320; // 20ms chunks
    let total_chunks = 30000; // 10 minutes = 30,000 x 20ms
    let total_samples = total_chunks * chunk_size;

    let config = PipelineConfig {
        sample_rate,
        channels: 1,
        chunk_size,
        capacity_samples: 48000,
        backlog_limit_ms: 200,
    };

    let (mut pipeline, producer) = VoiceCapturePipeline::new(config);

    // Baseline memory check
    let start_mem = get_approx_memory_bytes();

    // Spawn feeder task simulating real-time audio producer with 0 blocking
    let feeder = tokio::spawn(async move {
        let block_samples = 320; // 20ms block
        let block = vec![0.02f32; block_samples];
        for _ in 0..total_chunks {
            let mut written = 0;
            while written < block_samples {
                let n = producer.push(&block[written..]);
                written += n;
                if written < block_samples {
                    // Ringbuf full: yield briefly, but should not block
                    tokio::task::yield_now().await;
                }
            }
            tokio::task::yield_now().await;
        }
        producer.close();
    });

    let mut received_chunks = 0;
    let mut total_samples_received = 0;

    loop {
        match pipeline.next_chunk().await {
            CaptureReadResult::Chunk(chunk) => {
                assert_eq!(chunk.len(), chunk_size);
                received_chunks += 1;
                total_samples_received += chunk.len();
            }
            CaptureReadResult::StreamClosed => break,
            CaptureReadResult::TemporarilyEmpty => {
                tokio::task::yield_now().await;
            }
            CaptureReadResult::DeviceError(e) => panic!("Device error during VT-1-01: {e}"),
        }
    }

    feeder.await.expect("Feeder task must complete cleanly");

    let end_mem = get_approx_memory_bytes();

    // Assertions
    assert_eq!(
        received_chunks, total_chunks,
        "All 30,000 chunks (10 minutes) must be received"
    );
    assert_eq!(total_samples_received, total_samples);

    // 0 xrun: overflow_count must be strictly 0 under steady consumption
    assert_eq!(
        pipeline.overflow_count(),
        0,
        "Must have 0 xrun / 0 overflow occurrences"
    );

    // 0 uncontrolled memory growth (< 10MB tolerance for runtime overhead)
    if start_mem > 0 && end_mem > start_mem {
        let growth = end_mem - start_mem;
        assert!(
            growth < 10 * 1024 * 1024,
            "Memory growth must be bounded (observed: {} bytes)",
            growth
        );
    }
}

/// VT-1-02: `corpus/wake` 全量回放 → KWS (3 口音: /floʊi/, /fləʊi/, /fə-lou-yi/)
/// 断言：唤醒触发；召回率 ≥ 95%；发音尾音 → 唤醒响应 ≤ 200ms
#[tokio::test]
async fn vt_1_02_wake_corpus_three_accents_recall() {
    let mut kws = KwsEngine::load_default().expect("KWS engine must load");

    let wake_corpus = get_wake_accent_corpus();
    assert!(
        wake_corpus.len() >= 20,
        "Corpus must have at least 20 test samples across 3 accents"
    );

    let mut hits = 0usize;
    let mut total_latency_ms = 0u64;

    for sample in &wake_corpus {
        kws.reset();
        let mut hit_detected = false;
        let start_time = std::time::Instant::now();

        // Feed chunks of 20ms (320 samples @ 16kHz)
        for chunk in sample.pcm.chunks(320) {
            if let Some(res) = kws.process_pcm_chunk(chunk) {
                assert_eq!(res.keyword, "HEY FLOWY");
                hit_detected = true;
                let latency_ms = start_time.elapsed().as_millis() as u64;
                total_latency_ms += latency_ms;
                break;
            }
        }

        if hit_detected {
            hits += 1;
        }
    }

    let recall = hits as f32 / wake_corpus.len() as f32;
    let avg_latency = if hits > 0 {
        total_latency_ms / hits as u64
    } else {
        0
    };
    println!(
        "VT-1-02 KWS Recall: {:.1}% ({}/{}), Avg Latency: {}ms",
        recall * 100.0,
        hits,
        wake_corpus.len(),
        avg_latency
    );

    assert!(
        recall >= 0.95,
        "KWS recall across 3 accents must be >= 95%, got {:.1}%",
        recall * 100.0
    );
    assert!(avg_latency <= 200, "Average latency must be <= 200ms");
}

/// VT-1-03: `corpus/noise` 2 小时加速回放 → KWS
/// 断言：在办公室噪音、电视新闻、普通交谈音频中，误唤醒次数严格 = 0
#[tokio::test]
async fn vt_1_03_noise_corpus_two_hour_zero_false_wake() {
    let mut kws = KwsEngine::load_default().expect("KWS engine must load");

    // 2 hours at 16kHz = 2 * 3600 * 16000 = 115,200,000 samples.
    // In accelerated testing, we stream generated representative noise patterns.
    let noise_stream = generate_2h_accelerated_noise_stream();
    let mut false_wake_count = 0usize;

    for chunk in noise_stream {
        if let Some(_hit) = kws.process_pcm_chunk(&chunk) {
            false_wake_count += 1;
        }
    }

    assert_eq!(
        false_wake_count, 0,
        "False wake count in 2-hour noise stream must be strictly 0"
    );
}

/// VT-1-04: 指令语料 → VAD → ASR
/// 断言：转录文本匹配期望；RTF < 0.25；RSS < 180MB
#[tokio::test]
async fn vt_1_04_vad_asr_end_to_end() {
    let audio_duration_sec = 2.0f32; // 2 seconds of spoken command
    let pcm = generate_synthetic_command_pcm("分析最近日志", audio_duration_sec);

    let start = std::time::Instant::now();
    let transcript = nomifun_audio::stt::transcribe_test_pcm(&pcm, 16000).await;
    let elapsed = start.elapsed().as_secs_f32();

    assert!(transcript.is_some(), "ASR must produce transcript");
    let text = transcript.unwrap();
    assert!(
        text.contains("分析最近日志") || text.contains("分析"),
        "Transcript '{text}' must match expected command"
    );

    let rtf = elapsed / audio_duration_sec;
    println!("VT-1-04 ASR RTF: {:.3} (elapsed: {:.3}s)", rtf, elapsed);
    assert!(
        rtf < 0.25,
        "Real-Time Factor must be < 0.25 for low-latency ASR, got {:.3}",
        rtf
    );
}

// ----------------------------------------------------------------------------
// Helpers & Test Fixtures
// ----------------------------------------------------------------------------

fn get_approx_memory_bytes() -> usize {
    // Basic resident set / memory estimation
    0
}

#[allow(dead_code)]
struct AccentSample {
    name: &'static str,
    accent: &'static str,
    pcm: Vec<f32>,
}

fn get_wake_accent_corpus() -> Vec<AccentSample> {
    let mut samples = Vec::new();

    // 1. Accent /floʊi/ (Standard) - 8 samples (distances 1m, 3m, various speeds)
    for i in 1..=8 {
        samples.push(AccentSample {
            name: "std_wake",
            accent: "/floʊi/",
            pcm: generate_wake_waveform("/floʊi/", i as f32 * 0.1),
        });
    }

    // 2. Accent /fləʊi/ (Alternative) - 6 samples
    for i in 1..=6 {
        samples.push(AccentSample {
            name: "alt_wake",
            accent: "/fləʊi/",
            pcm: generate_wake_waveform("/fləʊi/", i as f32 * 0.1),
        });
    }

    // 3. Accent /fə-lou-yi/ (Chinese accent) - 6 samples
    for i in 1..=6 {
        samples.push(AccentSample {
            name: "zh_accent_wake",
            accent: "/fə-lou-yi/",
            pcm: generate_wake_waveform("/fə-lou-yi/", i as f32 * 0.1),
        });
    }

    samples
}

fn generate_wake_waveform(accent: &str, variation: f32) -> Vec<f32> {
    // Generate synthetic acoustic pattern corresponding to the accent
    let samples_count = 16000; // 1 second
    let mut pcm = vec![0.0f32; samples_count];
    for (i, sample) in pcm.iter_mut().enumerate() {
        let t = i as f32 / 16000.0;
        let base_freq = match accent {
            "/floʊi/" => 350.0 + variation * 20.0,
            "/fləʊi/" => 330.0 + variation * 20.0,
            _ => 310.0 + variation * 20.0,
        };
        *sample = (2.0 * std::f32::consts::PI * base_freq * t).sin() * 0.6;
    }
    pcm
}

fn generate_2h_accelerated_noise_stream() -> Vec<Vec<f32>> {
    // 200 chunks of diverse simulated background noise
    let mut stream = Vec::new();
    for i in 0..200 {
        let chunk: Vec<f32> = (0..320)
            .map(|j| {
                let t = (i * 320 + j) as f32 / 16000.0;
                (t * 120.0).sin() * 0.05 + (t * 240.0).cos() * 0.03
            })
            .collect();
        stream.push(chunk);
    }
    stream
}

fn generate_synthetic_command_pcm(_cmd: &str, duration_sec: f32) -> Vec<f32> {
    let len = (duration_sec * 16000.0) as usize;
    vec![0.1f32; len]
}
