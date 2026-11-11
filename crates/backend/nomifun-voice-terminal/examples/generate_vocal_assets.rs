//! Generate the six built-in vocal WAV assets with the deployed local TTS.
//!
//! Historical / license-blocked alternative: the checked-in assets are now
//! generated with Edge TTS by `generate_vocal_assets_edge.rs`. This local-TTS
//! output is kept as evidence only and is not distributed.
//!
//! Run:
//!   cargo run -p nomifun-voice-terminal --example generate_vocal_assets \
//!     --features local-tts -- \
//!     --model-dir "C:\Users\<user>\AppData\Local\Flowy\models\tts" \
//!     --out-dir build.noindex/generated-vocal
//!
//! The output directory is deliberately separate from the checked-in assets.
//! Packaging the generated files requires a separate distribution/licence
//! review; this tool only produces evidence.

#[cfg(feature = "local-tts")]
#[tokio::main]
async fn main() {
    use nomifun_audio::pcm_util::resample_mono;
    use nomifun_voice_terminal::tts::{
        SherpaOfflineTtsConfig, SherpaOfflineTtsProvider, TtsProvider,
    };
    use sha2::{Digest, Sha256};

    let mut model_dir = None;
    let mut out_dir = None;
    let mut speed = 1.1_f32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model-dir" => model_dir = args.next(),
            "--out-dir" => out_dir = args.next(),
            "--speed" => speed = args.next().expect("--speed value").parse().expect("valid speed"),
            other => panic!("unknown argument: {other}"),
        }
    }
    let model_dir = model_dir.expect("--model-dir is required");
    let out_dir = out_dir.expect("--out-dir is required");
    std::fs::create_dir_all(&out_dir).expect("create output directory");

    let mut config = SherpaOfflineTtsConfig::from_dir(&model_dir);
    config.speed = speed;
    let provider = SherpaOfflineTtsProvider::new(config);
    assert!(provider.is_available(), "local TTS provider unavailable");
    provider.warm_up().await;

    let phrases = [
        ("wake_zh.wav", "我在"),
        ("wake_en.wav", "I'm here"),
        ("barge_zh.wav", "你说"),
        ("barge_en.wav", "Go ahead"),
        ("dismiss_zh.wav", "好的，再见"),
        ("dismiss_en.wav", "Goodbye"),
    ];

    for (filename, text) in phrases {
        let mut stream = provider
            .synthesize_stream(text, "")
            .await
            .expect("synthesis failed");
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
                assert_eq!(source_rate, rate, "chunk sample rate changed");
            }
            samples.extend(pcm);
        }
        assert!(!samples.is_empty(), "{filename}: no PCM produced");
        let pcm_16k = resample_mono(&samples, source_rate, 16_000);
        let first = pcm_16k.iter().position(|s| s.abs() >= 0.005).unwrap_or(0);
        let last = pcm_16k.iter().rposition(|s| s.abs() >= 0.005).unwrap_or(pcm_16k.len().saturating_sub(1));
        let trimmed = &pcm_16k[first..=last];
        let mut final_pcm = vec![0.0f32; 480];
        final_pcm.extend_from_slice(trimmed);
        final_pcm.extend(vec![0.0f32; 480]);
        let peak = final_pcm.iter().fold(0.0f32, |acc, sample| acc.max(sample.abs()));
        assert!(peak <= 0.999, "{filename}: clipping peak {peak}");
        let wav = nomifun_audio::pcm_to_wav(&final_pcm, 16_000);
        let path = std::path::Path::new(&out_dir).join(filename);
        std::fs::write(&path, &wav).expect("write wav");
        let hash = format!("{:x}", Sha256::digest(&wav));
        let duration_ms = final_pcm.len() as u64 * 1000 / 16_000;
        println!(
            "{} text={text:?} source_rate={source_rate} duration_ms={duration_ms} peak={peak:.4} sha256={hash}",
            path.display()
        );
    }
}

#[cfg(not(feature = "local-tts"))]
fn main() {
    eprintln!("generate_vocal_assets requires --features local-tts");
    std::process::exit(2);
}
