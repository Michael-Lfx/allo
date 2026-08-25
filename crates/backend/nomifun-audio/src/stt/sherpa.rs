//! Local sherpa-onnx offline ASR (`feature = "local-stt"`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sherpa_onnx::{
    OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig,
};

use crate::frame::AudioChannel;
use crate::recorder::SttCallback;

use super::SHERPA_ASR_MODEL_DIR_ENV;

/// Error loading or creating a local sherpa recognizer.
#[derive(Debug, Clone)]
pub struct SherpaSttError(pub String);

impl std::fmt::Display for SherpaSttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SherpaSttError {}

struct SherpaInner {
    recognizer: Mutex<OfflineRecognizer>,
}

/// Offline SenseVoice (or compatible) STT via vendored sherpa-onnx.
pub struct SherpaSttCallback {
    inner: Arc<SherpaInner>,
}

impl SherpaSttCallback {
    /// Load from an explicit model directory (SenseVoice layout).
    pub fn try_from_dir(dir: &Path) -> Result<Self, SherpaSttError> {
        let config = sense_voice_config(dir)?;
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            SherpaSttError(format!(
                "sherpa Offline recognizer create failed for {}",
                dir.display()
            ))
        })?;
        Ok(Self {
            inner: Arc::new(SherpaInner {
                recognizer: Mutex::new(recognizer),
            }),
        })
    }

    fn recognize_sync(&self, pcm: &[f32], sample_rate: u32) -> Option<String> {
        if pcm.is_empty() {
            return None;
        }
        let recognizer = self.inner.recognizer.lock().ok()?;
        let stream = recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, pcm);
        recognizer.decode(&stream);
        let text = stream.get_result()?.text;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

#[async_trait]
impl SttCallback for SherpaSttCallback {
    async fn transcribe(
        &self,
        _channel: AudioChannel,
        pcm: Vec<f32>,
        sample_rate: u32,
    ) -> Option<String> {
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || {
            let cb = SherpaSttCallback { inner };
            cb.recognize_sync(&pcm, sample_rate)
        })
        .await
        .ok()
        .flatten()
    }
}

/// Resolve model dir and load sherpa, or `Ok(None)` when unset / missing.
///
/// Order: `model_dir` argument → env `NOMI_SHERPA_ASR_MODEL_DIR`.
/// Returns `Err` only when a directory was provided but is invalid / unloadable.
pub fn try_load_sherpa(model_dir: Option<&Path>) -> Result<Option<SherpaSttCallback>, SherpaSttError> {
    let Some(dir) = resolve_model_dir(model_dir) else {
        return Ok(None);
    };
    if !dir.is_dir() {
        return Err(SherpaSttError(format!(
            "sherpa model dir is not a directory: {}",
            dir.display()
        )));
    }
    match SherpaSttCallback::try_from_dir(&dir) {
        Ok(cb) => Ok(Some(cb)),
        Err(err) => Err(err),
    }
}

fn resolve_model_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    std::env::var_os(SHERPA_ASR_MODEL_DIR_ENV).map(PathBuf::from)
}

/// Expected layout (SenseVoice):
/// ```text
/// <dir>/
///   model.int8.onnx   (or model.onnx)
///   tokens.txt
/// ```
fn sense_voice_config(dir: &Path) -> Result<OfflineRecognizerConfig, SherpaSttError> {
    let tokens = dir.join("tokens.txt");
    if !tokens.is_file() {
        return Err(SherpaSttError(format!(
            "missing tokens.txt under {}",
            dir.display()
        )));
    }

    let model = ["model.int8.onnx", "model.onnx"]
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            SherpaSttError(format!(
                "missing model.int8.onnx or model.onnx under {}",
                dir.display()
            ))
        })?;

    let mut config = OfflineRecognizerConfig::default();
    config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
        model: Some(path_string(&model)),
        language: Some("auto".into()),
        use_itn: true,
    };
    config.model_config.tokens = Some(path_string(&tokens));
    config.model_config.num_threads = 2;
    config.model_config.provider = Some("cpu".into());
    Ok(config)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_dir_contents_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let err = match SherpaSttCallback::try_from_dir(dir.path()) {
            Ok(_) => panic!("expected Err for empty model dir"),
            Err(e) => e,
        };
        assert!(err.0.contains("tokens.txt") || err.0.contains("model"));
    }

    #[test]
    fn try_load_none_when_unset() {
        // Ensure we don't accidentally pick up a real env in CI: only test
        // the explicit-None path when env is unset, else skip assert on Ok(None).
        if std::env::var_os(SHERPA_ASR_MODEL_DIR_ENV).is_none() {
            assert!(matches!(try_load_sherpa(None), Ok(None)));
        }
    }

    #[test]
    fn sense_voice_config_detects_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("tokens.txt"), "a\n").unwrap();
        fs::write(dir.path().join("model.int8.onnx"), b"fake").unwrap();
        let cfg = sense_voice_config(dir.path()).unwrap();
        assert!(cfg.model_config.tokens.unwrap().ends_with("tokens.txt"));
        assert!(
            cfg.model_config
                .sense_voice
                .model
                .unwrap()
                .ends_with("model.int8.onnx")
        );
    }
}
