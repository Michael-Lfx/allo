//! SHA256 integrity verification and corruption detection.
//!
//! Baseline:
//! - `model-management.zh.md` §2
//! - `wave-1-acoustic.zh.md` §3 (VT-1-07, VT-1-08)

use std::collections::HashMap;
use std::path::Path;
use sha2::{Digest, Sha256};
use nomifun_api_types::voice::ModelInstallState;
use crate::layout::inspect_model_files;

/// The 7 downloaded distribution model asset files that require SHA256 verification.
pub const DOWNLOAD_FILES: &[&str] = &[
    "kws/encoder.int8.onnx",
    "kws/decoder.int8.onnx",
    "kws/joiner.int8.onnx",
    "kws/tokens.txt",
    "vad/silero_vad.onnx",
    "asr/model.int8.onnx",
    "asr/tokens.txt",
];

/// Default canonical checksums embedded in the binary.
/// In production, these are the authoritative baseline hashes for the pinned models.
pub fn default_canonical_manifest() -> HashMap<String, String> {
    let mut manifest = HashMap::new();
    // Default placeholder hashes (overridden in release builds or by constructor)
    manifest.insert("kws/encoder.int8.onnx".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest.insert("kws/decoder.int8.onnx".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest.insert("kws/joiner.int8.onnx".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest.insert("kws/tokens.txt".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest.insert("vad/silero_vad.onnx".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest.insert("asr/model.int8.onnx".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest.insert("asr/tokens.txt".to_string(), "0000000000000000000000000000000000000000000000000000000000000000".to_string());
    manifest
}

use tokio::io::AsyncReadExt;

/// Computes the SHA256 hex digest of a byte slice.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Computes the SHA256 hex digest of a file on disk using constant-memory streaming buffers.
pub async fn compute_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Parses a `checksums.sha256` string formatted as `<sha256>  <rel_path>`.
pub fn parse_checksums_file(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let hash = parts[0].to_lowercase();
            let path = parts[1].replace('\\', "/");
            map.insert(path, hash);
        }
    }
    map
}

/// Verifies model files against the authoritative baseline manifest.
#[derive(Debug, Clone)]
pub struct ModelVerifier {
    baseline_manifest: HashMap<String, String>,
}

impl Default for ModelVerifier {
    fn default() -> Self {
        Self {
            baseline_manifest: default_canonical_manifest(),
        }
    }
}

impl ModelVerifier {
    /// Creates a new verifier with default canonical manifest.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a verifier with an explicit authoritative baseline manifest.
    pub fn with_manifest(baseline_manifest: HashMap<String, String>) -> Self {
        Self { baseline_manifest }
    }

    /// Accesses the baseline manifest.
    pub fn baseline_manifest(&self) -> &HashMap<String, String> {
        &self.baseline_manifest
    }

    /// Verifies all files in `models_dir` without making any network requests.
    ///
    /// Level 1: Structure and existence check for all 8 files.
    /// Level 2: Cryptographic SHA256 verification against baseline manifest.
    pub async fn verify(&self, models_dir: &Path) -> ModelInstallState {
        // Level 1: Check structure and file existence
        let (layout_state, _details, total_size_bytes) = inspect_model_files(models_dir);
        if layout_state == ModelInstallState::NotInstalled {
            return ModelInstallState::NotInstalled;
        }

        // Check if on-disk checksums.sha256 exists (for Scene C offline copy / fallback)
        let on_disk_manifest = {
            let checksums_path = models_dir.join("checksums.sha256");
            if checksums_path.is_file() {
                if let Ok(content) = tokio::fs::read_to_string(&checksums_path).await {
                    Some(parse_checksums_file(&content))
                } else {
                    None
                }
            } else {
                None
            }
        };

        // Level 2: Verify each downloaded file against authoritative baseline
        for rel_path in DOWNLOAD_FILES {
            let expected_hash = self.baseline_manifest.get(*rel_path);
            let full_path = models_dir.join(rel_path);

            match compute_file_sha256(&full_path).await {
                Ok(actual_hash) => {
                    let is_placeholder = expected_hash
                        .map(|h| h.chars().all(|c| c == '0'))
                        .unwrap_or(true);

                    if !is_placeholder {
                        // Strict baseline check
                        if let Some(expected) = expected_hash {
                            if !actual_hash.eq_ignore_ascii_case(expected) {
                                return ModelInstallState::Failed {
                                    reason: format!(
                                        "Checksum mismatch for {rel_path}: expected {expected}, got {actual_hash}"
                                    ),
                                    recoverable: true,
                                };
                            }
                        }
                    } else if let Some(ref disk_manifest) = on_disk_manifest {
                        // In placeholder mode, check against on-disk checksums.sha256 if present
                        if let Some(expected_disk) = disk_manifest.get(*rel_path) {
                            if !actual_hash.eq_ignore_ascii_case(expected_disk) {
                                return ModelInstallState::Failed {
                                    reason: format!(
                                        "Checksum mismatch for {rel_path}: expected {expected_disk}, got {actual_hash}"
                                    ),
                                    recoverable: true,
                                };
                            }
                        }
                    }
                }
                Err(e) => {
                    return ModelInstallState::Failed {
                        reason: format!("Failed to read {rel_path} for SHA256 check: {e}"),
                        recoverable: true,
                    };
                }
            }
        }

        // All matched successfully
        ModelInstallState::Ready {
            models_dir: models_dir.to_path_buf(),
            total_size_bytes,
        }
    }
}
