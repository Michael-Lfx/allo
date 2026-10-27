//! Explicit real-model replay. No microphone, physical output or cloud dialogue.
#![cfg(feature = "voice-skeleton")]
use voice_companion::{oobe::OobeManager, voice::VoiceCompanionRuntime};
use nomifun_audio::{
    KwsEngine, KwsEngineConfig,
    capture::CaptureReadResult,
    frame::AudioChannel,
    recorder::SttCallback,
    voice_pipeline::{PipelineConfig, VoiceCapturePipeline},
};
use nomifun_voice_terminal::driver::StubVoiceRouter;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;

fn models() -> PathBuf {
    PathBuf::from(
        std::env::var_os("VOICE_REPLAY_MODEL_DIR")
            .expect("set VOICE_REPLAY_MODEL_DIR to an installed real model suite"),
    )
}
fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/architecture/voice-agent/execution/audio-fixtures")
}

fn require_fixed_audio(path: &std::path::Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    Err(format!(
        "missing_fixed_audio_asset: {}; prepare the fixture offline instead of synthesizing it through Edge",
        path.display()
    ))
}

async fn standard_pcm(name: &str) -> Vec<f32> {
    let path = fixtures().join(format!("{name}.wav"));
    require_fixed_audio(&path).unwrap_or_else(|error| panic!("{error}"));
    let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&std::fs::read(path).unwrap());
    assert!(pcm.iter().any(|sample| sample.abs() > 0.001));
    // Use the same normalization pipeline as live capture, with an offline
    // backlog bound sized for the fixture instead of dropping all but 100 ms.
    let (mut pipeline, producer) = VoiceCapturePipeline::new(PipelineConfig {
        sample_rate: rate,
        capacity_samples: pcm.len().max(16000),
        backlog_limit_ms: 60000,
        ..Default::default()
    });
    assert_eq!(producer.push(&pcm), pcm.len());
    producer.close();
    let mut normalized = Vec::new();
    loop {
        match pipeline.next_chunk().await {
            CaptureReadResult::Chunk(frame) => normalized.extend(frame),
            CaptureReadResult::StreamClosed => break,
            other => panic!("unexpected capture {other:?}"),
        }
    }
    std::fs::write(
        fixtures().join(format!("{name}-16k.wav")),
        nomifun_audio::pcm_to_wav(&normalized, 16000),
    )
    .unwrap();
    normalized
}

#[test]
fn missing_fixed_audio_is_reported_without_network_synthesis() {
    let path = std::env::temp_dir().join(format!(
        "flowy-voice-missing-fixture-{}.wav",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let error = require_fixed_audio(&path).unwrap_err();
    assert!(error.contains("missing_fixed_audio_asset"));
    assert!(
        !path.exists(),
        "reporting a missing fixture must not create or download audio"
    );
}

#[tokio::test]
#[ignore = "explicit synthetic audio and installed real ASR; no microphone or speaker"]
async fn standard_command_is_recognized_by_real_asr() {
    let pcm = standard_pcm("command").await;
    let asr = nomifun_audio::stt::try_load_sherpa(Some(&models()))
        .unwrap()
        .unwrap();
    let text = asr.transcribe(AudioChannel::Mic, pcm, 16000).await.unwrap();
    println!("DIRECT ASR: {text}");
    assert!(
        text.contains("天气"),
        "standard Chinese speech must produce the actual command"
    );
}

#[tokio::test]
#[ignore = "explicit standard wake audio with installed neural KWS"]
async fn standard_wake_is_detected_by_real_kws() {
    let mut passed = 0;
    for name in ["wake-jenny", "wake-guy"] {
        let mut pcm = standard_pcm(name).await;
        pcm.extend(vec![0.0; 16000]);
        for gain in [1.0, 0.25] {
            let mut kws = KwsEngine::new(KwsEngineConfig {
                model_dir: Some(models()),
                ..Default::default()
            })
            .unwrap();
            assert!(
                kws.is_initialized(),
                "real neural model required; heuristic is not evidence"
            );
            let mut hits = Vec::new();
            for chunk in pcm.chunks(320) {
                let scaled: Vec<f32> = chunk.iter().map(|s| s * gain).collect();
                if let Some(hit) = kws.process_pcm_chunk(&scaled) {
                    hits.push(hit.keyword);
                }
            }
            println!("KWS {name} gain={gain}: {hits:?}");
            passed += usize::from(!hits.is_empty());
        }
    }
    assert_eq!(
        passed, 4,
        "all four clean standard wake samples must be detected"
    );
}

#[tokio::test]
#[ignore = "explicit English fixture and installed real ASR; no microphone or speaker"]
async fn english_fixture_is_recognized_by_real_asr() {
    let pcm = standard_pcm("non-wake").await;
    let asr = nomifun_audio::stt::try_load_sherpa(Some(&models()))
        .unwrap()
        .unwrap();
    let text = asr
        .transcribe(AudioChannel::Mic, pcm, 16000)
        .await
        .unwrap();
    println!("ENGLISH ASR: {text}");
    let lowered = text.to_lowercase();
    assert!(
        lowered.contains("hello") && lowered.contains("friend"),
        "the same SenseVoice model must recognize the English fixture, got: {text}"
    );
}

#[tokio::test]
#[ignore = "installed real Silero VAD; fixed speech and silence fixtures"]
async fn silence_and_speech_are_separated_by_real_vad() {
    use nomifun_audio::vad::{SherpaVad, VadBackend};
    let mut vad = SherpaVad::load(&models()).expect("real Silero VAD model required");
    let silence = vec![0.0f32; 512];
    for _ in 0..30 {
        assert!(
            !vad.process_frame(&silence),
            "silence must not open a speech segment"
        );
    }
    let pcm = standard_pcm("command").await;
    let mut started = false;
    for frame in pcm.chunks(512) {
        if frame.len() < 512 {
            break;
        }
        if vad.process_frame(frame) {
            started = true;
            break;
        }
    }
    assert!(started, "real Silero VAD must detect the fixed speech sample");
    let mut ended = false;
    for _ in 0..120 {
        if !vad.process_frame(&silence) {
            ended = true;
            break;
        }
    }
    assert!(ended, "real Silero VAD must close the segment after silence");
}

#[tokio::test]
#[ignore = "real KWS negative controls with standard audio"]
async fn standard_non_wake_speech_does_not_trigger_kws() {
    for name in ["command", "non-wake"] {
        let mut pcm = standard_pcm(name).await;
        pcm.extend(vec![0.0; 16000]);
        let mut kws = KwsEngine::new(KwsEngineConfig {
            model_dir: Some(models()),
            ..Default::default()
        })
        .unwrap();
        assert!(kws.is_initialized());
        let hits: Vec<_> = pcm
            .chunks(320)
            .filter_map(|frame| kws.process_pcm_chunk(frame))
            .collect();
        println!("NEGATIVE {name}: {hits:?}");
        assert!(
            hits.is_empty(),
            "ordinary speech must not become a wake hit"
        );
    }
}

#[tokio::test]
#[ignore = "real ASR and neural KWS through production worker, paced audio, mock only dialogue"]
async fn standard_wake_and_command_reach_the_production_worker() {
    let mut pcm = vec![0.0; 8000];
    pcm.extend(standard_pcm("wake-jenny").await);
    pcm.extend(vec![0.0; 1280]);
    pcm.extend(standard_pcm("command").await);
    pcm.extend(vec![0.0; 32000]);
    std::fs::write(
        fixtures().join("wake-and-command-16k.wav"),
        nomifun_audio::pcm_to_wav(&pcm, 16000),
    )
    .unwrap();
    assert_command_reaches_worker(pcm).await;
}

#[tokio::test]
#[ignore = "one uninterrupted real utterance through production KWS and ASR"]
async fn standard_continuous_command_keeps_the_first_word() {
    let mut pcm = vec![0.0; 8000];
    pcm.extend(standard_pcm("continuous-command").await);
    pcm.extend(vec![0.0; 32000]);
    let asr = nomifun_audio::stt::try_load_sherpa(Some(&models()))
        .unwrap()
        .unwrap();
    println!(
        "CONTINUOUS DIRECT ASR: {:?}",
        asr.transcribe(AudioChannel::Mic, pcm.clone(), 16000).await
    );
    assert_command_reaches_worker(pcm).await;
}

#[tokio::test]
#[ignore = "quiet post-wake speech through production capture and real ASR; no microphone or speaker"]
async fn quiet_post_wake_command_reaches_asr() {
    let command = standard_pcm("command").await;
    let quiet: Vec<f32> = command.iter().map(|s| s * 0.03).collect();
    let rms = (quiet.iter().map(|s| s * s).sum::<f32>() / quiet.len() as f32).sqrt();
    println!("quiet command rms={rms}");
    assert!(rms < 0.01, "fixture must exercise the old energy gate");
    let asr = nomifun_audio::stt::try_load_sherpa(Some(&models()))
        .unwrap()
        .unwrap();
    let text = asr
        .transcribe(AudioChannel::Mic, quiet.clone(), 16000)
        .await
        .unwrap();
    assert!(
        text.contains("天气"),
        "real ASR can recognize the quiet input: {text}"
    );
    let mut pcm = vec![0.0; 8000];
    pcm.extend(quiet);
    pcm.extend(vec![0.0; 32000]);
    assert_worker(pcm, true).await;
}

#[tokio::test]
#[ignore = "speech starting within six seconds must continue beyond the wake deadline"]
async fn late_starting_speech_is_not_cut_off_at_the_wake_deadline() {
    let mut pcm = vec![0.0; 16000 * 4 + 8000];
    pcm.extend(standard_pcm("command").await);
    pcm.extend(vec![0.0; 32000]);
    assert_worker(pcm, true).await;
}

#[tokio::test]
#[ignore = "real speech detector silence control through the production worker"]
async fn silent_post_wake_input_explains_why_asr_did_not_start() {
    let asr = Arc::new(
        nomifun_audio::stt::try_load_sherpa(Some(&models()))
            .unwrap()
            .unwrap(),
    );
    let router = Arc::new(StubVoiceRouter::chat("must not reply to silence"));
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(models())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        asr,
        router.clone(),
    );
    let worker = runtime.start_worker();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !runtime
            .snapshot()
            .audio
            .is_some_and(|audio| audio.vad_ready)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("installed engines must be ready before the wake window begins");
    runtime.handle_wake().await.unwrap();
    let started = tokio::time::Instant::now();
    for index in 0..325 {
        producer.push(&[0.0; 320]);
        tokio::time::sleep_until(started + Duration::from_millis((index + 1) * 20)).await;
    }
    let snapshot = runtime.snapshot();
    worker.abort();
    assert_eq!(
        snapshot.phase,
        nomifun_api_types::voice::VoiceSessionPhase::Idle
    );
    assert!(snapshot.error.is_none());
    assert_eq!(
        snapshot.ended_reason,
        Some(nomifun_api_types::voice::VoiceSessionEndReason::InitialSilence)
    );
    assert_eq!(router.call_count(), 0);
    assert!(snapshot.conversation.is_empty());
}

#[tokio::test]
#[ignore = "built-in cue audio through real KWS/VAD/ASR; no microphone or physical speaker"]
async fn built_in_cue_audio_does_not_create_a_voice_request() {
    let asr = Arc::new(
        nomifun_audio::stt::try_load_sherpa(Some(&models()))
            .unwrap()
            .unwrap(),
    );
    let router = Arc::new(StubVoiceRouter::chat("must not answer its own cue"));
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(models())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        asr,
        router.clone(),
    );
    let worker = runtime.start_worker();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if runtime
                .snapshot()
                .audio
                .is_some_and(|audio| audio.kws_ready && audio.vad_ready && audio.asr_ready)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual neural engines must be ready, not just the replay flag");
    let mut pcm = Vec::new();
    for cue in nomifun_audio::EarconKind::ALL {
        let (samples, rate) = nomifun_audio::parse_audio_frame_to_pcm(cue.as_wav_bytes());
        assert_eq!(rate, 16000);
        assert!(samples.iter().any(|sample| sample.abs() > 0.001));
        let quieter: Vec<f32> = samples.iter().map(|sample| sample * 0.5).collect();
        pcm.extend(samples);
        pcm.extend(vec![0.0; 16000]);
        pcm.extend(quieter);
        pcm.extend(vec![0.0; 16000]);
    }
    let generated = std::fs::read(fixtures().join("../R3-processing-cue.wav"))
        .expect("run the full-flow processing test to record the actual generated cue first");
    let (samples, rate) = nomifun_audio::parse_audio_frame_to_pcm(&generated);
    assert_eq!(rate, 16000);
    assert!(samples.iter().any(|sample| sample.abs() > 0.001));
    pcm.extend(samples);
    pcm.extend(vec![0.0; 16000]);
    let start = tokio::time::Instant::now();
    for (index, frame) in pcm.chunks(320).enumerate() {
        assert_eq!(
            producer.push(frame),
            frame.len(),
            "cue replay must not overflow capture"
        );
        tokio::time::sleep_until(start + Duration::from_millis((index as u64 + 1) * 20)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let snapshot = runtime.snapshot();
    worker.abort();
    assert_eq!(
        snapshot.phase,
        nomifun_api_types::voice::VoiceSessionPhase::Idle
    );
    assert!(
        snapshot.session_id.is_none(),
        "built-in cue audio must not be accepted as a wake"
    );
    assert!(
        snapshot.conversation.is_empty(),
        "cue audio must not create an ASR/model turn"
    );
    assert_eq!(router.call_count(), 0);
}

async fn assert_command_reaches_worker(pcm: Vec<f32>) {
    assert_worker(pcm, false).await;
}

async fn assert_worker(pcm: Vec<f32>, manual_wake: bool) {
    let asr = Arc::new(
        nomifun_audio::stt::try_load_sherpa(Some(&models()))
            .unwrap()
            .unwrap(),
    );
    let router = Arc::new(StubVoiceRouter::chat("测试回答"));
    let (capture, producer) = VoiceCapturePipeline::new(PipelineConfig::default());
    let (runtime, _) = VoiceCompanionRuntime::from_capture(
        Arc::new(nomifun_voice_models::ModelManager::new(models())),
        Arc::new(OobeManager::new_bypassed()),
        Arc::new(Mutex::new(capture)),
        asr,
        router.clone(),
    );
    let runtime = runtime.with_test_playback(
        Arc::new(nomifun_voice_terminal::playback::PlaybackPipeline::new(
            Arc::new(nomifun_voice_terminal::playback::TestAudioSink::new()),
        )),
        Arc::new(nomifun_voice_terminal::tts::NullTts::new()),
    );
    let worker = runtime.start_worker();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !runtime
            .snapshot()
            .audio
            .is_some_and(|audio| audio.vad_ready)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("installed engines must be ready before replaying microphone input");
    if manual_wake {
        runtime.handle_wake().await.unwrap();
    }
    let started = tokio::time::Instant::now();
    for (index, frame) in pcm.chunks(320).enumerate() {
        assert_eq!(producer.push(frame), frame.len());
        tokio::time::sleep_until(started + Duration::from_millis((index as u64 + 1) * 20)).await;
    }
    println!(
        "REPLAY wall_ms={} audio_ms={}",
        started.elapsed().as_millis(),
        pcm.len() / 16
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let snapshot = runtime.snapshot();
    worker.abort();
    let overflows = runtime.capture_pipeline().lock().await.overflow_count();
    println!("CAPTURE overflows={overflows}");
    assert_eq!(
        overflows, 0,
        "paced fixture replay must not silently drop input"
    );
    println!("WORKER: {}", serde_json::to_string(&snapshot).unwrap());
    assert!(
        snapshot.conversation.iter().any(|turn| turn
            .user_text
            .as_ref()
            .is_some_and(|text| text.contains("今天天气怎么样"))),
        "wake plus adjacent command must retain the complete command, including the first word"
    );
    assert_eq!(
        router.call_count(),
        1,
        "one command must route exactly once"
    );
}
