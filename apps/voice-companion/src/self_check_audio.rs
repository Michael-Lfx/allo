//! Installed self-check audio resolution and bounded fixture generation.
//!
//! Production may ship licensed fixed WAV files next to the daemon or in the
//! user data directory. When no fixed set is present, the existing TTS provider
//! synthesizes the same public test sentences once into a checksummed cache.
//! Those files are test inputs only; they are never bundled or fed into a real
//! user session. Offline generation without an installed offline voice fails
//! closed instead of claiming engine readiness.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nomifun_voice_terminal::tts::TtsProvider;
use tokio::sync::Notify;

pub const WAKE_FILE: &str = "wake-jenny-16k.wav";
pub const COMMAND_FILE: &str = "command-16k.wav";
pub const NON_WAKE_FILE: &str = "non-wake-16k.wav";
const MANIFEST_FILE: &str = "manifest.json";
const SCHEMA: u32 = 1;
const MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const MAX_SAMPLES: usize = 16_000 * 20;

#[derive(Clone, Debug)]
pub struct SelfCheckAudio {
    pub wake: Vec<f32>,
    pub command: Vec<f32>,
    pub non_wake: Vec<f32>,
    pub source: String,
    pub online: bool,
    pub generated_at_ms: Option<i64>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn user_audio_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("NOMIFUN_VOICE_SELF_CHECK_CACHE_DIR") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local)
                .join("Flowy")
                .join("Voice-Companion")
                .join("audio-fixtures");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Flowy")
                .join("Voice-Companion")
                .join("audio-fixtures");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data)
                .join("flowy")
                .join("voice-companion")
                .join("audio-fixtures");
        }
    }
    std::env::temp_dir()
        .join("Flowy")
        .join("Voice-Companion")
        .join("audio-fixtures")
}

fn executable_audio_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("audio-fixtures");
    dir.is_dir().then_some(dir)
}

fn resample_16k(input: &[f32], from_sr: u32) -> Vec<f32> {
    if from_sr == 16_000 {
        return input.to_vec();
    }
    if input.is_empty() || from_sr == 0 {
        return Vec::new();
    }
    let ratio = 16_000f64 / from_sr as f64;
    let len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(len);
    for index in 0..len {
        let source = index as f64 / ratio;
        let first = source.floor() as usize;
        let fraction = (source - first as f64) as f32;
        let a = input.get(first).copied().unwrap_or(0.0);
        let b = input.get(first + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * fraction);
    }
    out
}

fn read_pcm(path: &Path, name: &str) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read_fixed_audio:{name}:{error}"))?;
    let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&bytes);
    if pcm.is_empty() {
        return Err(format!("empty_fixed_audio:{name}"));
    }
    let pcm = resample_16k(&pcm, rate);
    if pcm.is_empty() || pcm.len() > MAX_SAMPLES {
        return Err(format!("invalid_fixed_audio:{name}:samples={}", pcm.len()));
    }
    let duration_ms = pcm.len() as u64 * 1_000 / 16_000;
    if !(200..=20_000).contains(&duration_ms) {
        return Err(format!("invalid_fixed_audio:{name}:duration_ms={duration_ms}"));
    }
    let peak = pcm.iter().map(|sample| sample.abs()).fold(0.0f32, f32::max);
    if !peak.is_finite() || peak < 0.01 {
        return Err(format!("silent_fixed_audio:{name}"));
    }
    Ok(pcm)
}

fn load_set(dir: &Path, source_prefix: &str) -> Result<SelfCheckAudio, String> {
    let wake = read_pcm(&dir.join(WAKE_FILE), WAKE_FILE)?;
    let command = read_pcm(&dir.join(COMMAND_FILE), COMMAND_FILE)?;
    let non_wake = read_pcm(&dir.join(NON_WAKE_FILE), NON_WAKE_FILE)?;
    let modified = std::fs::metadata(dir.join(WAKE_FILE))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64);
    Ok(SelfCheckAudio {
        wake,
        command,
        non_wake,
        source: format!("{source_prefix}_fixed_audio"),
        online: false,
        generated_at_ms: modified,
    })
}

fn load_cache(dir: &Path) -> Result<Option<SelfCheckAudio>, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&manifest_path).map_err(|error| error.to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "fixed_audio_manifest_corrupt".to_string())?;
    if manifest.get("schema").and_then(|value| value.as_u64()) != Some(SCHEMA as u64) {
        return Err("fixed_audio_manifest_schema_mismatch".into());
    }
    let source = manifest
        .get("source")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "fixed_audio_manifest_missing_source".to_string())?;
    let online = manifest
        .get("online")
        .and_then(|value| value.as_bool())
        .ok_or_else(|| "fixed_audio_manifest_missing_online".to_string())?;
    let generated = manifest
        .get("generated_at_ms")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| "fixed_audio_manifest_missing_time".to_string())?;
    if now_ms().saturating_sub(generated) > MAX_AGE_MS {
        return Err("fixed_audio_expired".into());
    }
    for name in [WAKE_FILE, COMMAND_FILE, NON_WAKE_FILE] {
        let expected = manifest
            .get("files")
            .and_then(|value| value.get(name))
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("fixed_audio_manifest_missing_hash:{name}"))?;
        let bytes = std::fs::read(dir.join(name))
            .map_err(|error| format!("fixed_audio_corrupt:{name}:{error}"))?;
        let actual = nomifun_voice_models::verifier::compute_sha256(&bytes);
        if actual != expected {
            return Err(format!("fixed_audio_corrupt:{name}:sha256"));
        }
    }
    let mut set = load_set(dir, "cached")?;
    set.source = format!("{source}_cached");
    set.online = online;
    set.generated_at_ms = Some(generated);
    Ok(Some(set))
}

fn stop_reason(cancel: &AtomicBool, deadline: std::time::Instant) -> Option<&'static str> {
    if cancel.load(Ordering::Acquire) {
        Some("engine_check_cancelled")
    } else if std::time::Instant::now() >= deadline {
        Some("engine_check_timeout")
    } else {
        None
    }
}

async fn synthesize(
    tts: &dyn TtsProvider,
    text: &str,
    voice: &str,
    cancel: &AtomicBool,
    notify: &Notify,
    deadline: std::time::Instant,
) -> Result<Vec<f32>, String> {
    if let Some(reason) = stop_reason(cancel, deadline) {
        return Err(reason.into());
    }
    let mut stream = tokio::select! {
        _ = notify.notified() => return Err("engine_check_cancelled".into()),
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
            return Err("engine_check_timeout".into());
        }
        result = tts.synthesize_stream(text, voice) => {
            result.map_err(|error| format!("fixture_synthesis_failed:{error}"))?
        }
    };
    let wav = tokio::select! {
        _ = notify.notified() => return Err("engine_check_cancelled".into()),
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
            return Err("engine_check_timeout".into());
        }
        result = tokio::time::timeout(Duration::from_secs(25), stream.recv()) => {
            result
                .map_err(|_| "fixture_synthesis_timeout".to_string())?
                .ok_or_else(|| "fixture_synthesis_no_audio".to_string())?
        }
    };
    let (pcm, rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
    let pcm = resample_16k(&pcm, rate);
    if pcm.is_empty() || pcm.len() > MAX_SAMPLES {
        return Err("fixture_synthesis_invalid_pcm".into());
    }
    let peak = pcm.iter().map(|sample| sample.abs()).fold(0.0f32, f32::max);
    if !peak.is_finite() || peak < 0.01 {
        return Err("fixture_synthesis_silent".into());
    }
    Ok(pcm)
}

fn write_cache(dir: &Path, model_revision: &str, wake: &[f32], command: &[f32], non_wake: &[f32], source: &str, online: bool) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("fixture_cache_unwritable:{error}"))?;
    let files = [
        (WAKE_FILE, wake),
        (COMMAND_FILE, command),
        (NON_WAKE_FILE, non_wake),
    ];
    let mut hashes = serde_json::Map::new();
    for (name, pcm) in files {
        let bytes = nomifun_audio::pcm_to_wav(pcm, 16_000);
        let tmp = dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, &bytes).map_err(|error| format!("fixture_cache_write:{name}:{error}"))?;
        let target = dir.join(name);
        let _ = std::fs::remove_file(&target);
        std::fs::rename(&tmp, &target)
            .map_err(|error| format!("fixture_cache_commit:{name}:{error}"))?;
        hashes.insert(
            name.to_string(),
            serde_json::Value::String(nomifun_voice_models::verifier::compute_sha256(&bytes)),
        );
    }
    let manifest = serde_json::json!({
        "schema": SCHEMA,
        "model_revision": model_revision,
        "generated_at_ms": now_ms(),
        "source": source,
        "online": online,
        "license_note": "Runtime-generated TTS test audio; not redistributed. Edge usage remains governed by Microsoft service terms.",
        "files": hashes,
    });
    let tmp = dir.join(format!("{MANIFEST_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?)
        .map_err(|error| format!("fixture_cache_manifest_write:{error}"))?;
    let target = dir.join(MANIFEST_FILE);
    let _ = std::fs::remove_file(&target);
    std::fs::rename(&tmp, &target)
        .map_err(|error| format!("fixture_cache_manifest_commit:{error}"))?;
    Ok(())
}

pub async fn load_or_generate(
    model_revision: &str,
    tts: &dyn TtsProvider,
    cancel: &AtomicBool,
) -> Result<SelfCheckAudio, String> {
    load_or_generate_until(
        model_revision,
        tts,
        cancel,
        &Notify::new(),
        std::time::Instant::now() + Duration::from_secs(45),
    )
    .await
}

pub async fn load_or_generate_until(
    model_revision: &str,
    tts: &dyn TtsProvider,
    cancel: &AtomicBool,
    notify: &Notify,
    deadline: std::time::Instant,
) -> Result<SelfCheckAudio, String> {
    if let Some(reason) = stop_reason(cancel, deadline) {
        return Err(reason.into());
    }
    if let Some(dir) = std::env::var_os("NOMIFUN_VOICE_SELF_CHECK_AUDIO_DIR") {
        return load_set(Path::new(&dir), "override");
    }
    if let Some(dir) = executable_audio_dir() {
        return load_set(&dir, "installed");
    }

    let cache = user_audio_dir();
    match load_cache(&cache) {
        Ok(Some(set)) => return Ok(set),
        Ok(None) => {}
        Err(reason) => {
            tracing::warn!(reason, dir = %cache.display(), "fixed self-check audio unusable; regenerating");
        }
    }

    if let Some(reason) = stop_reason(cancel, deadline) {
        return Err(reason.into());
    }
    let wake = synthesize(tts, "Hey Flowy.", "en-US-JennyNeural", cancel, notify, deadline).await?;
    if let Some(reason) = stop_reason(cancel, deadline) {
        return Err(reason.into());
    }
    let command = synthesize(tts, "今天天气怎么样？", "zh-CN-XiaoxiaoNeural", cancel, notify, deadline).await?;
    if let Some(reason) = stop_reason(cancel, deadline) {
        return Err(reason.into());
    }
    let non_wake = synthesize(tts, "Hello friend.", "en-US-JennyNeural", cancel, notify, deadline).await?;
    let diagnostic = tts.diagnostic("");
    let online = diagnostic.as_ref().is_some_and(|value| value.online);
    let source = if online { "edge_online_generated" } else { "offline_tts_generated" };
    if let Some(reason) = stop_reason(cancel, deadline) {
        return Err(reason.into());
    }
    // Reject a provider that returned the same clip for a wake and a
    // non-wake phrase: that cannot prove a real discrimination check.
    if wake.len() == non_wake.len()
        && wake
            .iter()
            .zip(&non_wake)
            .all(|(left, right)| (left - right).abs() < 1e-6)
    {
        return Err("fixture_synthesis_not_distinct".into());
    }
    write_cache(&cache, model_revision, &wake, &command, &non_wake, source, online)?;
    Ok(SelfCheckAudio {
        wake,
        command,
        non_wake,
        source: source.into(),
        online,
        generated_at_ms: Some(now_ms()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tone(frequency: f32, seconds: f32) -> Vec<f32> {
        let count = (16_000.0 * seconds) as usize;
        (0..count)
            .map(|index| {
                (index as f32 / 16_000.0 * frequency * std::f32::consts::TAU).sin() * 0.2
            })
            .collect()
    }

    fn write_bundle(dir: &Path, wake: &[f32], command: &[f32], non_wake: &[f32], generated: i64, revision: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let mut files = serde_json::Map::new();
        for (name, pcm) in [(WAKE_FILE, wake), (COMMAND_FILE, command), (NON_WAKE_FILE, non_wake)] {
            let bytes = nomifun_audio::pcm_to_wav(pcm, 16_000);
            std::fs::write(dir.join(name), &bytes).unwrap();
            files.insert(name.into(), serde_json::Value::String(nomifun_voice_models::verifier::compute_sha256(&bytes)));
        }
        let manifest = serde_json::json!({
            "schema": SCHEMA,
            "model_revision": revision,
            "generated_at_ms": generated,
            "source": "edge_online_generated",
            "online": true,
            "files": files,
        });
        std::fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec(&manifest).unwrap()).unwrap();
    }

    #[test]
    fn corrupt_cached_audio_is_not_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let wake = tone(220.0, 1.0);
        let command = tone(330.0, 1.0);
        let non_wake = tone(440.0, 1.0);
        write_bundle(dir.path(), &wake, &command, &non_wake, now_ms(), "rev-1");
        std::fs::write(dir.path().join(WAKE_FILE), b"not a wav").unwrap();
        assert!(load_cache(dir.path()).is_err());
    }

    #[test]
    fn expired_cached_audio_is_rejected_and_model_revision_does_not_invalidate() {
        let dir = tempfile::tempdir().unwrap();
        let wake = tone(220.0, 1.0);
        let command = tone(330.0, 1.0);
        let non_wake = tone(440.0, 1.0);
        write_bundle(dir.path(), &wake, &command, &non_wake, now_ms() - MAX_AGE_MS - 1, "rev-1");
        assert_eq!(load_cache(dir.path()).unwrap_err(), "fixed_audio_expired");
        write_bundle(dir.path(), &wake, &command, &non_wake, now_ms(), "rev-1");
        let loaded = load_cache(dir.path()).unwrap().expect("cache must survive model revision changes");
        assert_eq!(loaded.wake.len(), wake.len());
    }

    #[test]
    fn missing_override_fails_closed_instead_of_using_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_set(dir.path(), "override").is_err());
    }

    struct PendingTts;

    #[async_trait::async_trait]
    impl TtsProvider for PendingTts {
        async fn synthesize_stream(
            &self,
            _text: &str,
            _voice: &str,
        ) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, nomifun_voice_terminal::tts::TtsError> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn synthesis_stage_obeys_deadline_and_cancel() {
        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Notify::new();
        let started = std::time::Instant::now();
        let result = synthesize(
            &PendingTts,
            "test",
            "voice",
            cancel.as_ref(),
            &notify,
            std::time::Instant::now() + Duration::from_millis(80),
        )
        .await;
        assert_eq!(result.unwrap_err(), "engine_check_timeout");
        assert!(started.elapsed() < Duration::from_secs(1));

        let cancel = Arc::new(AtomicBool::new(false));
        let notify = Arc::new(Notify::new());
        let cancel_task = {
            let cancel = cancel.clone();
            let notify = notify.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(40)).await;
                cancel.store(true, Ordering::Release);
                notify.notify_waiters();
            })
        };
        let result = synthesize(
            &PendingTts,
            "test",
            "voice",
            cancel.as_ref(),
            notify.as_ref(),
            std::time::Instant::now() + Duration::from_secs(5),
        )
        .await;
        assert_eq!(result.unwrap_err(), "engine_check_cancelled");
        cancel_task.await.unwrap();
    }

    #[test]
    fn valid_cache_round_trips_with_checksums() {
        let dir = tempfile::tempdir().unwrap();
        let wake = tone(220.0, 1.0);
        let command = tone(330.0, 1.0);
        let non_wake = tone(440.0, 1.0);
        write_bundle(dir.path(), &wake, &command, &non_wake, now_ms(), "rev-1");
        let loaded = load_cache(dir.path()).unwrap().expect("valid cache");
        assert!(loaded.online);
        assert_eq!(loaded.wake.len(), wake.len());
    }
}