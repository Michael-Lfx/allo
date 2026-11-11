//! Local TTS latency benchmark.
//!
//! Measures the real Matcha/Vocos pipeline through `SherpaOfflineTtsProvider`,
//! which is the same path the daemon plays. Emits one JSON object so the numbers
//! can be pasted into PROGRESS.md / the final report.
//!
//! Run:
//!   cargo run -p nomifun-voice-terminal --example tts_bench --features local-tts -- \
//!     --model-dir "$LOCALAPPDATA/Flowy/models/tts" --threads 2,4,6 --iterations 5

use std::path::PathBuf;

#[cfg(feature = "local-tts")]
use std::time::Instant;

#[cfg(feature = "local-tts")]
use nomifun_voice_terminal::tts::{
    SherpaOfflineTtsConfig, SherpaOfflineTtsProvider, TtsProvider,
};

/// Distinct 15-20 character sentences: warm samples must not hit the
/// fixed-phrase cache, otherwise the benchmark would flatter the number.
const WARM_TEXTS: [&str; 20] = [
    "你好，我是你的语音伴侣。",
    "今天天气不错，我们出去走走吧。",
    "我正在为你整理刚才的会议记录。",
    "请稍等，我马上帮你处理这件事。",
    "这个方案我已经仔细看过了。",
    "会议改到明天下午三点开始。",
    "你先休息一下，我马上就好。",
    "文件已经保存到你的桌面上。",
    "我记得你昨天问过这个问题。",
    "好的，我这就去安排一下。",
    "明天上午我们先把接口对一下。",
    "这段代码我建议拆成两个函数。",
    "你刚才说的那个问题我记下了。",
    "语音链路的延迟已经降下来了。",
    "我先把今天的待办事项列出来。",
    "这个改动需要补一个回归测试。",
    "请你确认一下这份会议纪要。",
    "我马上把结果发到你的邮箱里。",
    "外面下雨了，出门记得带伞。",
    "大概还需要五分钟就能完成。",
];
const COLD_TEXT: &str = "冷启动测试，请稍等一下。";
const SAMPLE_RATE: u64 = 22_050;

#[cfg(feature = "local-tts")]
#[tokio::main]
async fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("tts_bench: {error}");
            std::process::exit(2);
        }
    };
    if let Err(missing) = check_model_files(&options.model_dir) {
        eprintln!("tts_bench: model files missing under {:?}:", options.model_dir);
        for path in missing {
            eprintln!("  {}", path.display());
        }
        std::process::exit(2);
    }

    let mut sweeps = Vec::new();
    for threads in &options.threads {
        let mut config = SherpaOfflineTtsConfig::from_dir(&options.model_dir);
        config.num_threads = *threads;
        let provider = SherpaOfflineTtsProvider::new(config);
        if !provider.is_available() {
            eprintln!("tts_bench: provider reports unavailable for threads={threads}");
            std::process::exit(2);
        }

        // Production prewarms the engine (daemon start / call start), so the
        // meaningful cold number is the first utterance after that prewarm.
        let load_started = Instant::now();
        if options.warm {
            provider.warm_up().await;
        }
        let engine_load_ms = load_started.elapsed().as_secs_f64() * 1_000.0;
        let cold = synthesize(&provider, &options.text).await;
        // Cache-hit replay must match the streamed WAV's PCM sample count:
        // this catches a dropped tail or a duplicated gap frame.
        let replay = synthesize(&provider, &options.text).await;
        let first_pcm_diff = cold
            .pcm
            .iter()
            .zip(replay.pcm.iter())
            .position(|(left, right)| left != right);
        let pcm_consistent = replay.pcm_samples == cold.pcm_samples
            && replay.pcm_hash == cold.pcm_hash;
        let mut warm_full_ms = Vec::new();
        let mut warm_first_ms = Vec::new();
        let mut audio_ms = 0.0_f64;
        let mut wav_bytes = 0_usize;
        for index in 0..options.iterations {
            let text = warm_text(index);
            let sample = synthesize(&provider, &text).await;
            warm_full_ms.push(sample.full_ms);
            warm_first_ms.push(sample.first_chunk_ms);
            audio_ms = sample.audio_ms;
            wav_bytes = sample.bytes;
        }
        let median_full = median(&mut warm_full_ms);
        let median_first = median(&mut warm_first_ms);

        sweeps.push(serde_json::json!({
            "num_threads": threads,
            "cold_load_ms": round2(engine_load_ms),
            "cold_first_utterance_ms": round2(cold.full_ms),
            "cold_first_chunk_ms": round2(cold.first_chunk_ms),
            "cold_full_ms": round2(cold.full_ms),
            "warm_median_ms": round2(median_full),
            "warm_first_chunk_median_ms": round2(median_first),
            "warm_min_ms": round2(warm_full_ms.first().copied().unwrap_or(0.0)),
            "warm_max_ms": round2(warm_full_ms.last().copied().unwrap_or(0.0)),
            "warm_samples": warm_full_ms.iter().map(|value| round2(*value)).collect::<Vec<_>>(),
            "first_chunk_ms": round2(median_first),
            "wav_bytes": wav_bytes,
            "audio_ms": round2(audio_ms),
            "cold_pcm_samples": cold.pcm_samples,
            "replay_pcm_samples": replay.pcm_samples,
            "cold_pcm_hash": cold.pcm_hash,
            "replay_pcm_hash": replay.pcm_hash,
            "pcm_first_diff_byte": first_pcm_diff,
            "pcm_consistent": pcm_consistent,
        }));
    }

    let primary = sweeps.first().cloned().unwrap_or_else(|| serde_json::json!({}));
    let report = serde_json::json!({
        "model_dir": options.model_dir.to_string_lossy(),
        "text": options.text,
        "text_chars": options.text.chars().count(),
        "iterations": options.iterations,
        "cold_load_ms": primary["cold_load_ms"].clone(),
        "warm_median_ms": primary["warm_median_ms"].clone(),
        "first_chunk_ms": primary["first_chunk_ms"].clone(),
        "sweeps": sweeps,
    });
    println!("{}", serde_json::to_string_pretty(&report).expect("serialize report"));
}

#[cfg(not(feature = "local-tts"))]
fn main() {
    eprintln!("tts_bench requires --features local-tts");
    std::process::exit(2);
}

#[cfg(feature = "local-tts")]
struct Sample {
    bytes: usize,
    pcm: Vec<u8>,
    pcm_samples: usize,
    pcm_hash: u64,
    first_chunk_ms: f64,
    full_ms: f64,
    audio_ms: f64,
}

#[cfg(feature = "local-tts")]
async fn synthesize(provider: &SherpaOfflineTtsProvider, text: &str) -> Sample {
    let started = Instant::now();
    let mut stream = provider
        .synthesize_stream(text, "")
        .await
        .expect("synthesis failed");
    let mut first_chunk_ms = None;
    let mut bytes = 0_usize;
    let mut pcm_bits = Vec::new();
    while let Some(chunk) = stream.recv().await {
        if !chunk.is_empty() {
            first_chunk_ms.get_or_insert_with(|| started.elapsed().as_secs_f64() * 1_000.0);
            bytes += chunk.len();
            let (samples, _sample_rate) = nomifun_audio::parse_audio_frame_to_pcm(&chunk);
            for sample in samples {
                pcm_bits.extend_from_slice(&sample.to_bits().to_le_bytes());
            }
        }
    }
    let full_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let first_chunk_ms = first_chunk_ms.unwrap_or(full_ms);
    let pcm_samples = pcm_bits.len() / 4;
    let audio_ms = wav_duration_ms(pcm_samples) as f64;
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pcm_bits.hash(&mut hasher);
    Sample {
        bytes,
        pcm: pcm_bits,
        pcm_samples,
        pcm_hash: hasher.finish(),
        first_chunk_ms,
        full_ms,
        audio_ms,
    }
}

#[cfg(feature = "local-tts")]
fn warm_text(index: usize) -> String {
    let base = WARM_TEXTS[index % WARM_TEXTS.len()];
    if index < WARM_TEXTS.len() {
        base.to_owned()
    } else {
        format!("{base}（第{}次）", index + 1)
    }
}

#[cfg(feature = "local-tts")]
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

#[cfg(feature = "local-tts")]
fn wav_duration_ms(pcm_samples: usize) -> u64 {
    pcm_samples as u64 * 1_000 / SAMPLE_RATE
}

#[cfg(feature = "local-tts")]
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(feature = "local-tts")]
fn check_model_files(dir: &std::path::Path) -> Result<(), Vec<PathBuf>> {
    let mut missing = Vec::new();
    let required = [
        "matcha-icefall-zh-baker/model-steps-3.onnx",
        "matcha-icefall-zh-baker/tokens.txt",
        "matcha-icefall-zh-baker/lexicon.txt",
        "matcha-icefall-en_US-ljspeech/model-steps-3.onnx",
        "matcha-icefall-en_US-ljspeech/tokens.txt",
        "vocos-22khz-univ.onnx",
    ];
    for relative in required {
        let path = dir.join(relative);
        if !path.is_file() {
            missing.push(path);
        }
    }
    if missing.is_empty() { Ok(()) } else { Err(missing) }
}

#[allow(dead_code)]
struct Options {
    model_dir: PathBuf,
    threads: Vec<i32>,
    iterations: usize,
    text: String,
    warm: bool,
}

#[allow(dead_code)]
impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut model_dir = None;
        let mut threads = vec![2, 4, 6];
        let mut iterations = 10_usize;
        let mut text = COLD_TEXT.to_owned();
        let mut warm = true;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--model-dir" => {
                    model_dir = Some(PathBuf::from(args.next().ok_or("--model-dir needs a path")?));
                }
                "--threads" => {
                    let value = args.next().ok_or("--threads needs a list like 2,4,6")?;
                    threads = value
                        .split(',')
                        .map(|item| item.trim().parse::<i32>().map_err(|_| format!("bad thread count: {item}")))
                        .collect::<Result<Vec<_>, _>>()?;
                    if threads.is_empty() || threads.iter().any(|value| *value <= 0) {
                        return Err("--threads must be positive integers".into());
                    }
                }
                "--iterations" => {
                    iterations = args
                        .next()
                        .ok_or("--iterations needs a number")?
                        .parse::<usize>()
                        .map_err(|_| "--iterations must be a number".to_string())?;
                    if iterations == 0 {
                        return Err("--iterations must be greater than zero".into());
                    }
                }
                "--text" => {
                    text = args.next().ok_or("--text needs a value")?;
                }
                "--no-warm" => {
                    warm = false;
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(Self {
            model_dir: model_dir.ok_or("--model-dir is required")?,
            threads,
            iterations,
            text,
            warm,
        })
    }
}
