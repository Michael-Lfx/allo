//! SHA256 integrity verification and corruption detection.
//!
//! Baseline:
//! - `model-management.zh.md` §2
//! - `wave-1-acoustic.zh.md` §3 (VT-1-07, VT-1-08)

use std::collections::HashMap;
use std::path::Path;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use nomifun_api_types::voice::ModelInstallState;
use crate::layout::inspect_model_files;

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub fallback_urls: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KwsGroup {
    pub group_id: String,
    pub files: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceManifest {
    pub schema_version: u32,
    pub suite_version: String,
    pub default_kws_group: String,
    pub kws_groups: Vec<KwsGroup>,
    pub files: Vec<ManifestFile>,
}

pub fn embedded_manifest() -> &'static VoiceManifest {
    static MANIFEST: std::sync::OnceLock<VoiceManifest> = std::sync::OnceLock::new();
    MANIFEST.get_or_init(|| serde_json::from_slice(include_bytes!("../resources/voice-suite-manifest.json"))
        .expect("embedded voice manifest must be valid JSON"))
}

pub fn pinned_source_url(relative: &str) -> Option<&'static str> {
    embedded_manifest().files.iter().find(|f| f.path == relative).and_then(|f| f.url.as_deref())
}

pub fn matching_kws_group(actual: &HashMap<String, String>) -> Option<&'static KwsGroup> {
    embedded_manifest().kws_groups.iter().find(|group| group.files == *actual)
}

pub fn default_kws_group() -> &'static KwsGroup {
    let manifest = embedded_manifest();
    manifest
        .kws_groups
        .iter()
        .find(|group| group.group_id == manifest.default_kws_group)
        .expect("the embedded manifest must contain its declared default KWS group")
}

/// Selects one supported KWS group from the complete files that are present.
///
/// The manager's own baseline counts as the supported default identity so an
/// isolated/custom suite can still be repaired without inventing a second trust
/// root. A marker or persisted rebuild intent only selects a target; evidence
/// must still be internally consistent.
pub fn infer_kws_group(
    actual: &HashMap<String, String>,
    baseline: &HashMap<String, String>,
    marker: &str,
) -> Result<&'static KwsGroup, String> {
    let manifest = embedded_manifest();
    let default = default_kws_group();
    if let Some(group) = matching_kws_group(actual) {
        return Ok(group);
    }
    let matches_baseline_default = !actual.is_empty()
        && actual.iter().all(|(path, hash)| baseline.get(path) == Some(hash));
    if actual.len() == default.files.len() && matches_baseline_default {
        return Ok(default);
    }
    if actual.is_empty() {
        return Ok(default);
    }
    let marker = marker.trim();
    if !marker.is_empty() {
        let Some(group) = manifest.kws_groups.iter().find(|group| group.group_id == marker) else {
            return Err(format!(
                "kws_group_ambiguous: recorded KWS group {marker:?} is unsupported; preserve existing files or explicitly confirm rebuilding the supported default group"
            ));
        };
        let identified = manifest.kws_groups.iter().find(|candidate| {
            actual
                .iter()
                .all(|(path, hash)| candidate.files.get(path) == Some(hash))
        });
        let contradicts_identified = identified.is_some_and(|candidate| candidate.group_id != group.group_id);
        let contradicts_baseline = matches_baseline_default && group.group_id != default.group_id;
        return if contradicts_identified || contradicts_baseline {
            Err(format!(
                "kws_group_ambiguous: recorded KWS group {marker:?} contradicts the existing files; preserve them or explicitly confirm rebuilding the supported default group"
            ))
        } else {
            Ok(group)
        };
    }
    let mut candidates = manifest.kws_groups.iter().filter(|group| {
        actual
            .iter()
            .all(|(path, hash)| group.files.get(path) == Some(hash))
    });
    match (candidates.next(), candidates.next()) {
        (Some(group), None) => Ok(group),
        _ if matches_baseline_default => Ok(default),
        _ => Err(
            "kws_group_ambiguous: existing KWS files do not identify one supported group; preserve them or explicitly confirm rebuilding the supported default group"
                .into(),
        ),
    }
}

pub fn pinned_size(relative: &str) -> Option<u64> {
    embedded_manifest().files.iter().find(|f| f.path == relative).map(|f| f.size)
}

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

/// The downloaded distribution model asset files for local offline TTS.
pub const DOWNLOAD_TTS_FILES: &[&str] = &[
    "tts/matcha-icefall-zh-baker/model-steps-3.onnx",
    "tts/matcha-icefall-zh-baker/tokens.txt",
    "tts/matcha-icefall-zh-baker/lexicon.txt",
    "tts/matcha-icefall-zh-baker/date.fst",
    "tts/matcha-icefall-zh-baker/number.fst",
    "tts/matcha-icefall-zh-baker/phone.fst",
    "tts/matcha-icefall-en_US-ljspeech/model-steps-3.onnx",
    "tts/matcha-icefall-en_US-ljspeech/tokens.txt",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/en_dict",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/phontab",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/phonindex",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/phondata",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/intonations",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en",
    "tts/matcha-icefall-en_US-ljspeech/espeak-ng-data/lang/gmw/en-US",
    "tts/vocos-22khz-univ.onnx",
];

/// Default canonical checksums embedded in the binary.
/// In production, these are the authoritative baseline hashes for the pinned models.
pub fn default_canonical_manifest() -> HashMap<String, String> {
    embedded_manifest().files.iter().filter(|f| !f.path.starts_with("tts/")).map(|f| (f.path.clone(), f.sha256.clone())).collect()
}

/// Default canonical checksums for local offline TTS models.
pub fn default_tts_manifest() -> HashMap<String, String> {
    embedded_manifest().files.iter().filter(|f| f.path.starts_with("tts/")).map(|f| (f.path.clone(), f.sha256.clone())).collect()
}

use tokio::io::AsyncReadExt;

/// Computes the SHA256 hex digest of a byte slice.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Computes the SHA256 hex digest of a file on disk using a bounded heap buffer.
///
/// The read buffer must live on the heap: an inline 64 KiB array inside this
/// async state machine made the host daemon's 1 MB main-thread stack overflow
/// during provisioning (rev2 diagnosis `provision-stack-1mb.txt`).
pub async fn compute_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];

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

/// Validate local wake phrases without rewriting user configuration.
pub fn validate_keywords(tokens: &str, keywords: &str) -> Result<(), String> {
    let vocabulary: std::collections::HashSet<&str> = tokens.lines().filter_map(|line| line.split_whitespace().next()).collect();
    let mut phrases = 0;
    for line in keywords.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let mut count = 0;
        for token in line.split_whitespace().take_while(|token| !token.starts_with([':', '#', '@'])) {
            if !vocabulary.contains(token) { return Err(format!("configuration: kws/keywords.txt token {token:?} is absent from the selected KWS vocabulary; edit the local wake phrase")); }
            count += 1;
        }
        if count > 0 { phrases += 1; }
    }
    if phrases == 0 { return Err("configuration: kws/keywords.txt has no valid wake phrase".into()); }
    Ok(())
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

    /// Cryptographically verifies all downloaded offline TTS model assets against
    /// the manager's baseline TTS manifest. A reduced/custom suite without TTS
    /// entries is treated as not pinning TTS; production managers always carry
    /// the authoritative TTS hashes.
    pub async fn verify_tts(&self, models_dir: &Path) -> bool {
        if !self
            .baseline_manifest
            .keys()
            .any(|path| path.starts_with("tts/"))
        {
            return true;
        }
        for &rel_path in DOWNLOAD_TTS_FILES {
            let full_path = models_dir.join(rel_path);
            if !full_path.exists() {
                return false;
            }
            if let Some(expected) = self.baseline_manifest.get(rel_path) {
                match compute_file_sha256(&full_path).await {
                    Ok(actual) => {
                        if !actual.eq_ignore_ascii_case(expected) {
                            return false;
                        }
                    }
                    Err(_) => return false,
                }
            }
        }
        true
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
        let _on_disk_manifest = {
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

        // Compute the complete KWS identity once; group acceptance is atomic.
        let mut actual_kws = HashMap::new();
        for path in ["kws/encoder.int8.onnx", "kws/decoder.int8.onnx", "kws/joiner.int8.onnx", "kws/tokens.txt"] {
            if let Ok(hash) = compute_file_sha256(&models_dir.join(path)).await {
                actual_kws.insert(path, hash);
            }
        }
        let kws_group_valid = embedded_manifest().kws_groups.iter().any(|group| {
            group.files.iter().all(|(path, expected)| actual_kws.get(path.as_str()).is_some_and(|actual| actual.eq_ignore_ascii_case(expected)))
        });

        // Level 2: Verify each downloaded file against authoritative baseline
        for rel_path in DOWNLOAD_FILES {
            let expected_hash = self.baseline_manifest.get(*rel_path);
            let full_path = models_dir.join(rel_path);

            match compute_file_sha256(&full_path).await {
                Ok(actual_hash) => {
                    let is_placeholder = expected_hash.map(|h| h.chars().all(|c| c == '0')).unwrap_or(true);

                    if !is_placeholder {
                        // A KWS install is valid only when all four files belong to one
                        // supported group; never accept a cross-group mixture.
                        if let Some(expected) = expected_hash {
                            let is_match = if rel_path.starts_with("kws/") {
                                (kws_group_valid || actual_kws.iter().all(|(path, hash)| self.baseline_manifest.get(*path) == Some(hash))) && (actual_hash.eq_ignore_ascii_case(expected) || embedded_manifest().kws_groups.iter().any(|group| group.files.get(*rel_path).is_some_and(|hash| hash.eq_ignore_ascii_case(&actual_hash))))
                            } else {
                                actual_hash.eq_ignore_ascii_case(expected)
                            };
                            if !is_match {
                                return ModelInstallState::Failed {
                                    reason: format!(
                                        "Checksum mismatch for {rel_path}: expected {expected}, got {actual_hash}"
                                    ),
                                    recoverable: true,
                                };
                            }
                        }
                    } else {
                        // Fail-closed security gate: unpinned placeholder hashes are forbidden in production
                        #[cfg(not(any(test, feature = "voice-skeleton")))]
                        {
                            return ModelInstallState::Failed {
                                reason: format!(
                                    "Security failure: unpinned placeholder baseline checksum for {rel_path}. Strict verification required in production."
                                ),
                                recoverable: false,
                            };
                        }

                        #[cfg(any(test, feature = "voice-skeleton"))]
                        if let Some(ref disk_manifest) = _on_disk_manifest {
                            if let Some(expected_disk) = disk_manifest.get(*rel_path) {
                                if !actual_hash.eq_ignore_ascii_case(expected_disk) {
                                    return ModelInstallState::Failed {
                                        reason: format!(
                                            "Checksum mismatch for {rel_path}: expected {expected_disk}, got {actual_hash}"
                                        ),
                                        recoverable: true,
                                    };
                                }
                            } else {
                                return ModelInstallState::Failed {
                                    reason: format!("File {rel_path} missing in on-disk checksums.sha256"),
                                    recoverable: true,
                                };
                            }
                        } else {
                            return ModelInstallState::Failed {
                                reason: format!("No baseline or disk checksums available for {rel_path}"),
                                recoverable: true,
                            };
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

        if kws_group_valid {
            let tokens = tokio::fs::read_to_string(models_dir.join("kws/tokens.txt")).await.unwrap_or_default();
            let keywords = tokio::fs::read_to_string(models_dir.join("kws/keywords.txt")).await.unwrap_or_default();
            if let Err(reason) = validate_keywords(&tokens, &keywords) {
                return ModelInstallState::Failed { reason, recoverable: true };
            }
        }
        // All matched successfully
        ModelInstallState::Ready {
            models_dir: models_dir.to_path_buf(),
            total_size_bytes,
        }
    }
}
