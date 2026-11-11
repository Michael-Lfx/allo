//! P1 Wake Comparison Probe: Evaluates available authorized audio fixtures
//! under Cascade Wake vs. Pure KWS mode.
//!
//! Run with: `cargo run -p voice-companion --example wake_comparison_probe`

use std::path::PathBuf;
use std::time::Instant;
use nomifun_audio::{KwsEngine, KwsEngineConfig};
use nomifun_audio::vad::{VadConfig, create_vad};
use nomifun_audio::recorder::SttCallback;
use nomifun_audio::stt::try_load_sherpa;
use nomifun_audio::frame::AudioChannel;
use nomifun_audio::voice_pipeline::{PipelineConfig, VoiceCapturePipeline};
use nomifun_audio::capture::CaptureReadResult;

struct AudioSample {
    name: &'static str,
    path: &'static str,
    expected_wake: bool,
    category: &'static str,
}

const SAMPLES: &[AudioSample] = &[
    AudioSample { name: "wake-guy-16k", path: "docs/architecture/voice-agent/execution/audio-fixtures/wake-guy-16k.wav", expected_wake: true, category: "wake_standard_male" },
    AudioSample { name: "wake-guy-24k", path: "docs/architecture/voice-agent/execution/audio-fixtures/wake-guy.wav", expected_wake: true, category: "wake_standard_male" },
    AudioSample { name: "wake-jenny-16k", path: "docs/architecture/voice-agent/execution/audio-fixtures/wake-jenny-16k.wav", expected_wake: true, category: "wake_standard_female" },
    AudioSample { name: "wake-jenny-24k", path: "docs/architecture/voice-agent/execution/audio-fixtures/wake-jenny.wav", expected_wake: true, category: "wake_standard_female" },
    AudioSample { name: "wake-and-command-16k", path: "docs/architecture/voice-agent/execution/audio-fixtures/wake-and-command-16k.wav", expected_wake: true, category: "wake_plus_command" },
    AudioSample { name: "command-16k", path: "docs/architecture/voice-agent/execution/audio-fixtures/command-16k.wav", expected_wake: false, category: "command_only" },
    AudioSample { name: "command-24k", path: "docs/architecture/voice-agent/execution/audio-fixtures/command.wav", expected_wake: false, category: "command_only" },
    AudioSample { name: "continuous-command-16k", path: "docs/architecture/voice-agent/execution/audio-fixtures/continuous-command-16k.wav", expected_wake: false, category: "multi_turn_command" },
    AudioSample { name: "continuous-command-24k", path: "docs/architecture/voice-agent/execution/audio-fixtures/continuous-command.wav", expected_wake: false, category: "multi_turn_command" },
    AudioSample { name: "non-wake-16k", path: "docs/architecture/voice-agent/execution/audio-fixtures/non-wake-16k.wav", expected_wake: false, category: "negative_speech" },
    AudioSample { name: "non-wake-24k", path: "docs/architecture/voice-agent/execution/audio-fixtures/non-wake.wav", expected_wake: false, category: "negative_speech" },
];

async fn normalize_to_16k_mono(raw_pcm: &[f32], rate: u32) -> Vec<f32> {
    let (mut pipeline, producer) = VoiceCapturePipeline::new(PipelineConfig {
        sample_rate: rate,
        channels: 1,
        capacity_samples: raw_pcm.len().max(16000),
        backlog_limit_ms: 60000,
        ..Default::default()
    });
    producer.push(raw_pcm);
    producer.close();
    let mut normalized = Vec::new();
    loop {
        match pipeline.next_chunk().await {
            CaptureReadResult::Chunk(frame) => normalized.extend(frame),
            CaptureReadResult::StreamClosed => break,
            _ => {}
        }
    }
    normalized
}

#[tokio::main]
async fn main() {
    println!("=== P1 WAKE COMPARISON PROBE ===");
    let models_dir = PathBuf::from("D:/tmp/flowy-dev/models");

    // Initialize ASR for cascade mode
    let asr = try_load_sherpa(Some(&models_dir))
        .expect("Sherpa model loading failed")
        .expect("Failed to initialize SherpaSttCallback");

    let mut csv_lines = vec![
        "sample_id,category,expected_wake,cascade_hit,cascade_source,cascade_latency_ms,cascade_asr_calls,pure_kws_hit,pure_kws_source,pure_kws_latency_ms,pure_kws_asr_calls,match_consistent".to_string()
    ];

    println!("\nTesting {} authorized samples...", SAMPLES.len());

    for sample in SAMPLES {
        let wav_bytes = std::fs::read(sample.path).expect("Read sample wav");
        let (raw_pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav_bytes);
        let pcm = normalize_to_16k_mono(&raw_pcm, rate).await;

        // -------------------------------------------------------------
        // Run Mode 1: Cascade Wake (KWS + VAD/ASR fallback)
        // -------------------------------------------------------------
        let mut kws_cascade = KwsEngine::new(KwsEngineConfig {
            model_dir: Some(models_dir.clone()),
            ..Default::default()
        }).expect("Create KwsEngine");

        let mut vad = create_vad(VadConfig::default());
        let cascade_start = Instant::now();
        let mut cascade_hit = false;
        let mut cascade_source = "none";
        let mut cascade_asr_calls = 0;
        let mut utterance_pcm = Vec::new();

        for chunk in pcm.chunks(320) {
            let hit = kws_cascade.process_pcm_chunk(chunk).map(|r| r.keyword);
            if let Some(_kw) = hit {
                cascade_hit = true;
                cascade_source = "kws_streaming";
                break;
            }

            // Stage 2: VAD accumulation
            let is_speech = vad.process_frame(chunk);
            if is_speech {
                utterance_pcm.extend_from_slice(chunk);
                if utterance_pcm.len() > 16000 * 4 {
                    utterance_pcm.drain(0..16000);
                }
            } else if !utterance_pcm.is_empty() {
                if utterance_pcm.len() >= 4000 {
                    let snippet = std::mem::take(&mut utterance_pcm);
                    cascade_asr_calls += 1;
                    if let Some(text) = asr.transcribe(AudioChannel::Mic, snippet, 16000).await {
                        if let Some(_matched) = nomifun_audio::match_wake_phrase(&text) {
                            cascade_hit = true;
                            cascade_source = "asr_cascade";
                            break;
                        }
                    }
                } else {
                    utterance_pcm.clear();
                }
            }
        }
        let cascade_latency_ms = cascade_start.elapsed().as_secs_f64() * 1000.0;

        // -------------------------------------------------------------
        // Run Mode 2: Pure KWS (No ASR fallback)
        // -------------------------------------------------------------
        let mut kws_pure = KwsEngine::new(KwsEngineConfig {
            model_dir: Some(models_dir.clone()),
            ..Default::default()
        }).expect("Create KwsEngine");

        let pure_start = Instant::now();
        let mut pure_hit = false;
        let pure_asr_calls = 0;

        for chunk in pcm.chunks(320) {
            let hit = kws_pure.process_pcm_chunk(chunk).map(|r| r.keyword);
            if let Some(_kw) = hit {
                pure_hit = true;
                break;
            }
        }
        let pure_latency_ms = pure_start.elapsed().as_secs_f64() * 1000.0;
        let pure_source = if pure_hit { "kws_streaming" } else { "none" };

        let consistent = cascade_hit == pure_hit;

        println!("  Sample {:<24} | Expected: {:<5} | Cascade: {:<5} ({:<13}, ASR={}) | Pure KWS: {:<5} (ASR={}) | Match: {}",
            sample.name, sample.expected_wake, cascade_hit, cascade_source, cascade_asr_calls, pure_hit, pure_asr_calls, consistent
        );

        csv_lines.push(format!(
            "{},{},{},{},{},{:.2},{},{},{},{:.2},{},{}",
            sample.name,
            sample.category,
            sample.expected_wake,
            cascade_hit,
            cascade_source,
            cascade_latency_ms,
            cascade_asr_calls,
            pure_hit,
            pure_source,
            pure_latency_ms,
            pure_asr_calls,
            consistent
        ));
    }

    let csv_content = csv_lines.join("\n");
    let output_path = "build.noindex/voice-asr-lifecycle-execution-20261029/wake-results.csv";
    std::fs::write(output_path, csv_content).expect("Write wake-results.csv");
    println!("\nP1 Wake comparison results written to {}", output_path);
}
