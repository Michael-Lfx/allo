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

/// Inspects the files in `models_dir` and determines the installation state.
///
/// If all 8 model files + `checksums.sha256` exist and have non-zero size,
/// returns `ModelInstallState::Ready`.
/// Otherwise, returns `ModelInstallState::NotInstalled`.
pub fn inspect_model_files(
    models_dir: &Path,
) -> (ModelInstallState, Vec<VoiceModelFileDetail>, u64) {
    let mut details = Vec::with_capacity(EXPECTED_FILES.len());
    let mut total_size_bytes: u64 = 0;
    let mut all_exist = true;

    for &rel_path in EXPECTED_FILES {
        let full_path = models_dir.join(rel_path);
        let (exists, size_bytes) = match std::fs::metadata(&full_path) {
            Ok(meta) if meta.is_file() => {
                let len = meta.len();
                (len > 0, len)
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
}
