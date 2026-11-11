//! Regenerate the six built-in vocal WAV assets with Edge TTS.
//!
//! Run:
//!   cargo run -p nomifun-voice-terminal --example generate_vocal_assets_edge -- \
//!     --out-dir build.noindex/generated-vocal-edge
//!
//! Writes 16 kHz / mono / 16-bit WAV with 30 ms leading/trailing silence so the
//! checked-in assets keep one fixed format. Review the output first; replacing
//! `crates/backend/nomifun-audio/assets/vocal/` is a separate manual step.

use nomifun_audio::pcm_util::resample_mono;
use nomifun_voice_terminal::tts::{EdgeTtsProvider, TtsProvider};
use sha2::{Digest, Sha256};

#[tokio::main]
async fn main() {
    let mut out_dir = None;
    let mut speed = 1.1_f32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => out_dir = args.next(),
            "--speed" => {
                speed = args
                    .next()
                    .expect("--speed value")
                    .parse()
                    .expect("valid speed")
            }
            other => panic!("unknown argument: {other}"),
        }
    }
    let out_dir = out_dir.expect("--out-dir is required");
    if !speed.is_finite() || !(0.5..=2.0).contains(&speed) {
        panic!("--speed must be a finite value within 0.5..=2.0");
    }
    std::fs::create_dir_all(&out_dir).expect("create output directory");

    let provider = EdgeTtsProvider::new();
    let phrases = [
        ("wake_zh.wav", "我在", "zh-CN-XiaoxiaoNeural"),
        ("wake_en.wav", "I'm here", "en-US-JennyNeural"),
        ("barge_zh.wav", "你说", "zh-CN-XiaoxiaoNeural"),
        ("barge_en.wav", "Go ahead", "en-US-JennyNeural"),
        ("dismiss_zh.wav", "好的，再见", "zh-CN-XiaoxiaoNeural"),
        ("dismiss_en.wav", "Goodbye", "en-US-JennyNeural"),
    ];

    for (filename, text, voice) in phrases {
        let spec = format!("{voice};speed={speed:.2}");
        let mut stream = provider
            .synthesize_stream(text, &spec)
            .await
            .unwrap_or_else(|error| panic!("{filename}: edge synthesis failed: {error}"));
        let mut samples = Vec::new();
        let mut source_rate = 0u32;
        while let Some(chunk) = stream.recv().await {
            if chunk.is_empty() {
                continue;
            }
            let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&chunk);
            if source_rate == 0 {
                source_rate = rate;
            } else {
                assert_eq!(source_rate, rate, "{filename}: chunk sample rate changed");
            }
            samples.extend(pcm);
        }
        assert!(!samples.is_empty(), "{filename}: no PCM produced");

        let pcm_16k = resample_mono(&samples, source_rate, 16_000);
        let first = pcm_16k
            .iter()
            .position(|sample| sample.abs() >= 0.005)
            .unwrap_or(0);
        let last = pcm_16k
            .iter()
            .rposition(|sample| sample.abs() >= 0.005)
            .unwrap_or(pcm_16k.len().saturating_sub(1));
        let trimmed = &pcm_16k[first..=last];
        let mut final_pcm = vec![0.0f32; 480];
        final_pcm.extend_from_slice(trimmed);
        final_pcm.extend(vec![0.0f32; 480]);
        let peak = final_pcm
            .iter()
            .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
        assert!(peak <= 0.999, "{filename}: clipping peak {peak}");

        let wav = nomifun_audio::pcm_to_wav(&final_pcm, 16_000);
        let path = std::path::Path::new(&out_dir).join(filename);
        std::fs::write(&path, &wav).expect("write wav");
        let hash = format!("{:x}", Sha256::digest(&wav));
        let duration_ms = final_pcm.len() as u64 * 1000 / 16_000;
        println!(
            "{} text={text:?} voice={voice} source_rate={source_rate} duration_ms={duration_ms} peak={peak:.4} sha256={hash}",
            path.display()
        );
    }
}
