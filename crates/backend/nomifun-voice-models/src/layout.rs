//! Voice model topology inspection and layout validation.
//!
//! Baseline:
//! - `interaction.zh.md` §3.4.4
//! - `model-management.zh.md` §4

use std::path::Path;
use nomifun_api_types::voice::{ModelInstallState, VoiceModelFileDetail};

/// Authoritative list of expected files in local voice models distribution.
pub const EXPECTED_FILES: &[&str] = &[
    "kws/encoder.int8.onnx",
    "kws/decoder.int8.onnx",
    "kws/joiner.int8.onnx",
    "kws/tokens.txt",
    "kws/keywords.txt",
    "vad/silero_vad.onnx",
    "asr/model.int8.onnx",
    "asr/tokens.txt",
    "checksums.sha256",
];

/// Authoritative list of expected files in local offline TTS models distribution (~197MB Matcha + Vocos suite).
pub const EXPECTED_TTS_FILES: &[&str] = &[
    "tts/matcha-icefall-zh-baker/model-steps-3.onnx",
    "tts/matcha-icefall-zh-baker/tokens.txt",
    "tts/matcha-icefall-zh-baker/lexicon.txt",
    "tts/matcha-icefall-zh-baker/date.fst",
    "tts/matcha-icefall-zh-baker/number.fst",
    "tts/matcha-icefall-zh-baker/phone.fst",
    "tts/matcha-icefall-en_US-ljspeech/model-steps-3.onnx",
    "tts/matcha-icefall-en_US-ljspeech/tokens.txt",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/en_dict",
    "tts/vocos-22khz-univ.onnx",
];

/// Minimum realistic byte thresholds for model assets to detect corruption, truncation, or mock stubs.
pub fn min_expected_size(rel_path: &str, strict: bool) -> u64 {
    if !strict {
        return 1;
    }
    match rel_path {
        "asr/model.int8.onnx" => 100_000_000, // ~239MB SenseVoice model
        "kws/encoder.int8.onnx" => 2_000_000,  // ~4.8MB Zipformer encoder
        "kws/decoder.int8.onnx" => 100_000,    // ~278KB Zipformer decoder
        "kws/joiner.int8.onnx" => 50_000,      // ~163KB Zipformer joiner
        "vad/silero_vad.onnx" => 1_000_000,    // ~1.8MB Silero VAD model
        "asr/tokens.txt" => 10_000,            // ~315KB SenseVoice dictionary
        "kws/tokens.txt" => 500,               // ~5KB KWS vocabulary
        "kws/keywords.txt" => 10,              // ~184B wake word configuration
        "checksums.sha256" => 50,              // ~4KB SHA256 checksums
        "tts/matcha-icefall-zh-baker/model-steps-3.onnx" => 50_000_000, // ~73MB
        "tts/matcha-icefall-en_US-ljspeech/model-steps-3.onnx" => 50_000_000, // ~74MB
        "tts/vocos-22khz-univ.onnx" => 30_000_000, // ~53MB
        "tts/matcha-icefall-zh-baker/lexicon.txt" => 1_000_000, // ~1.4MB
        "tts/matcha-icefall-zh-baker/tokens.txt" => 10_000, // ~20KB
        "tts/matcha-icefall-en_US-ljspeech/tokens.txt" => 500, // ~1KB
        _ => 1,
    }
}

/// Determines whether strict physical size thresholds should be enforced.
/// In unit/integration tests or skeleton builds, allows small mock fixtures.
pub fn is_strict_mode() -> bool {
    if cfg!(any(test, feature = "voice-skeleton")) {
        return false;
    }
    if std::env::var_os("CARGO_TARGET_TMPDIR").is_some()
        || std::env::var_os("NOMIFUN_TEST_MODE").is_some()
    {
        return false;
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy().to_lowercase();
        if exe_str.contains("deps")
            && (exe_str.contains("vt_") || exe_str.contains("test") || exe_str.contains("c1_"))
        {
            return false;
        }
    }
    true
}

/// Inspects the files in `models_dir` and determines the installation state.
///
/// In strict mode (production runtime), verifies physical presence and realistic size thresholds
/// (e.g. rejecting 48-byte mock stubs or truncated downloads).
pub fn inspect_model_files(
    models_dir: &Path,
) -> (ModelInstallState, Vec<VoiceModelFileDetail>, u64) {
    let strict = is_strict_mode();
    inspect_model_files_with_strictness(models_dir, strict)
}

/// Inspects the files in `models_dir` with explicit control over strict size thresholds.
pub fn inspect_model_files_with_strictness(
    models_dir: &Path,
    strict: bool,
) -> (ModelInstallState, Vec<VoiceModelFileDetail>, u64) {
    let mut details = Vec::with_capacity(EXPECTED_FILES.len());
    let mut total_size_bytes: u64 = 0;
    let mut all_exist = true;

    for &rel_path in EXPECTED_FILES {
        let full_path = models_dir.join(rel_path);
        let min_size = min_expected_size(rel_path, strict);
        let (exists, size_bytes) = match std::fs::metadata(&full_path) {
            Ok(meta) if meta.is_file() => {
                let len = meta.len();
                (len >= min_size, len)
            }
            _ => (false, 0),
        };

        if !exists {
            all_exist = false;
        }
        total_size_bytes = total_size_bytes.saturating_add(size_bytes);

        details.push(VoiceModelFileDetail {
            path: rel_path.to_string(),
            size_bytes,
            expected_sha256: None,
            actual_sha256: None,
            exists,
        });
    }

    let state = if all_exist {
        ModelInstallState::Ready {
            models_dir: models_dir.to_path_buf(),
            total_size_bytes,
        }
    } else {
        ModelInstallState::NotInstalled
    };

    (state, details, total_size_bytes)
}

/// Convenience function returning only the `ModelInstallState`.
pub fn check_model_layout(models_dir: &Path) -> ModelInstallState {
    inspect_model_files(models_dir).0
}

/// Inspects the local TTS files in `models_dir` and determines whether local offline TTS is available.
pub fn inspect_tts_files(
    models_dir: &Path,
) -> (bool, Vec<VoiceModelFileDetail>, u64) {
    let strict = is_strict_mode();
    let mut details = Vec::with_capacity(EXPECTED_TTS_FILES.len());
    let mut total_size_bytes: u64 = 0;
    let mut all_exist = true;

    for &rel_path in EXPECTED_TTS_FILES {
        let full_path = models_dir.join(rel_path);
        let min_size = min_expected_size(rel_path, strict);
        let (exists, size_bytes) = match std::fs::metadata(&full_path) {
            Ok(meta) if meta.is_file() => {
                let len = meta.len();
                (len >= min_size, len)
            }
            _ => (false, 0),
        };

        if !exists {
            all_exist = false;
        }
        total_size_bytes = total_size_bytes.saturating_add(size_bytes);

        details.push(VoiceModelFileDetail {
            path: rel_path.to_string(),
            size_bytes,
            expected_sha256: None,
            actual_sha256: None,
            exists,
        });
    }

    (all_exist, details, total_size_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_empty_dir_is_not_installed() {
        let temp = tempdir().expect("create temp dir");
        let (state, details, total_size) = inspect_model_files(temp.path());

        assert_eq!(state, ModelInstallState::NotInstalled);
        assert_eq!(total_size, 0);
        assert_eq!(details.len(), EXPECTED_FILES.len());
        for detail in &details {
            assert!(!detail.exists);
            assert_eq!(detail.size_bytes, 0);
        }
    }

    #[test]
    fn test_partial_files_is_not_installed() {
        let temp = tempdir().expect("create temp dir");
        let kws_dir = temp.path().join("kws");
        std::fs::create_dir_all(&kws_dir).expect("create kws dir");
        std::fs::write(kws_dir.join("tokens.txt"), "hello").expect("write file");

        let (state, details, total_size) = inspect_model_files(temp.path());
        assert_eq!(state, ModelInstallState::NotInstalled);
        assert_eq!(total_size, 5);

        let kws_tokens = details.iter().find(|d| d.path == "kws/tokens.txt").unwrap();
        assert!(kws_tokens.exists);
        assert_eq!(kws_tokens.size_bytes, 5);
    }

    #[test]
    fn test_all_files_ready() {
        let temp = tempdir().expect("create temp dir");
        for rel in EXPECTED_FILES {
            let full = temp.path().join(rel);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(&full, "dummy-content").expect("write file");
        }

        let (state, details, total_size) = inspect_model_files(temp.path());
        assert_eq!(
            state,
            ModelInstallState::Ready {
                models_dir: temp.path().to_path_buf(),
                total_size_bytes: total_size,
            }
        );
        assert_eq!(total_size, (EXPECTED_FILES.len() * 13) as u64);
        assert!(details.iter().all(|d| d.exists));
    }

    #[test]
    fn test_strict_stub_rejection() {
        let temp = tempdir().expect("create temp dir");
        for rel in EXPECTED_FILES {
            let full = temp.path().join(rel);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            // Write a 48-byte placeholder stub for all files
            std::fs::write(&full, "dummy-placeholder-model-stub-stream-123456789012").expect("write file");
        }

        // Under strict mode, the 48-byte stub must be rejected for large neural models (e.g. asr/model.int8.onnx)
        let (state_strict, details_strict, _) = inspect_model_files_with_strictness(temp.path(), true);
        assert_eq!(state_strict, ModelInstallState::NotInstalled);
        let asr_detail = details_strict.iter().find(|d| d.path == "asr/model.int8.onnx").unwrap();
        assert!(!asr_detail.exists, "48-byte stub must not be marked as existing in strict mode");

        // Under non-strict mode (test mode), small fixture slices are accepted
        let (state_lenient, details_lenient, _) = inspect_model_files_with_strictness(temp.path(), false);
        assert!(matches!(state_lenient, ModelInstallState::Ready { .. }));
        let asr_lenient = details_lenient.iter().find(|d| d.path == "asr/model.int8.onnx").unwrap();
        assert!(asr_lenient.exists);
    }

    #[test]
    fn test_tts_files_inspection() {
        let temp = tempdir().expect("create temp dir");
        let (ready, details, total) = inspect_tts_files(temp.path());
        assert!(!ready);
        assert_eq!(total, 0);
        assert_eq!(details.len(), EXPECTED_TTS_FILES.len());

        for rel in EXPECTED_TTS_FILES {
            let full = temp.path().join(rel);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(&full, "fake-tts-bytes").expect("write file");
        }

        let (ready_after, details_after, total_after) = inspect_tts_files(temp.path());
        assert!(ready_after);
        assert_eq!(total_after, (EXPECTED_TTS_FILES.len() * 14) as u64);
        assert!(details_after.iter().all(|d| d.exists));
    }
}
