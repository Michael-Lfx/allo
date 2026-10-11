//! DTOs for the local model runtime surface (`/api/local-models/*`).
//!
//! The local model store ships three herd-blessed catalog entries backed by a
//! single `llama-server.exe` runtime variant (`standard`). These types mirror
//! the manifest metadata plus the live install/run state the store renders.

use serde::{Deserialize, Serialize};

/// Bilingual label used across the read-only catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalModelText {
    pub zh: String,
    pub en: String,
}

/// Install/run lifecycle state of a single local model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelRunStatus {
    /// No model files on disk.
    Uninstalled,
    /// Model files are being fetched.
    Downloading,
    /// Files verified; the model is not running.
    Installed,
    /// A `llama-server` process is booting and has not passed its health check.
    Starting,
    /// `llama-server` is healthy and published as an OpenAI provider.
    Running,
}

impl LocalModelRunStatus {
    pub const fn is_busy(self) -> bool {
        matches!(self, Self::Downloading | Self::Starting)
    }
}

/// What the install pipeline is doing right now, while a model is downloading.
///
/// Hashing a finished file runs in the background, so the store card can be
/// downloading the next file and verifying the previous one at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelDownloadPhase {
    /// A connection is being opened; no bytes have landed yet.
    Connecting,
    /// Bytes are moving and nothing is being hashed.
    Downloading,
    /// A finished file is being hashed and no next file is downloading.
    Verifying,
    /// A finished file is being hashed while the next file downloads.
    VerifyingDownloading,
}

/// A single downloadable file that belongs to a model or the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalModelFile {
    pub file_name: String,
    pub file_size: u64,
    pub sha256: Option<String>,
}

/// Stable machine-readable code for a failed local model download.
///
/// The renderer maps the code onto a localized sentence, while `detail`
/// carries the variable part (HTTP status, file name, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelDownloadErrorCode {
    /// The connection or an in-flight read timed out.
    Timeout,
    /// The server answered with a non-success HTTP status.
    Http,
    /// The transport failed: connection reset, DNS, TLS, early EOF.
    Network,
    /// The assembled file did not match the manifest SHA-256.
    Checksum,
    /// Writing the file to disk failed.
    Disk,
    /// Anything not covered above.
    Unknown,
}

impl LocalModelDownloadErrorCode {
    /// Lowercase wire/display token, e.g. `timeout`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Http => "http",
            Self::Network => "network",
            Self::Checksum => "checksum",
            Self::Disk => "disk",
            Self::Unknown => "unknown",
        }
    }
}

/// The reason the most recent download attempt failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalModelDownloadError {
    /// Stable machine code the renderer localizes.
    pub code: LocalModelDownloadErrorCode,
    /// Optional variable detail: HTTP status, file name, transport message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl LocalModelDownloadError {
    /// Build an error with a structured detail.
    pub fn new(code: LocalModelDownloadErrorCode, detail: impl Into<Option<String>>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    /// Build an error that carries only a code.
    pub const fn coded(code: LocalModelDownloadErrorCode) -> Self {
        Self { code, detail: None }
    }
}

impl std::fmt::Display for LocalModelDownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.detail.as_deref() {
            Some(detail) if !detail.is_empty() => write!(formatter, "{}: {detail}", self.code.as_str()),
            _ => formatter.write_str(self.code.as_str()),
        }
    }
}

/// Full launch options accepted by `POST /api/local-models/:id/start`.
///
/// Every field is optional: the server fills in manifest defaults, then the
/// model's last successful launch options override them. This mirrors the
/// herdsman chat start dialog so the store can round-trip a saved launch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LocalModelLaunchOptions {
    pub context_size: Option<u64>,
    pub cache_type_k: Option<String>,
    pub cache_type_v: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub top_k: Option<u32>,
    pub repeat_penalty: Option<f64>,
    pub min_p: Option<f64>,
    pub parallel: Option<u32>,
    pub batch_size: Option<u32>,
    pub ubatch_size: Option<u32>,
    pub gpu_layers: Option<u32>,
    pub gpu_layers_auto: Option<bool>,
    pub threads: Option<u32>,
    pub threads_batch: Option<u32>,
    pub cache_ram: Option<i64>,
    pub no_mmap: Option<bool>,
    pub cont_batching: Option<bool>,
    pub no_kv_offload: Option<bool>,
    pub enable_dflash: Option<bool>,
    pub dflash_gpu_layers: Option<u32>,
    pub dflash_draft_min: Option<u32>,
    pub dflash_draft_max: Option<u32>,
    pub mtp_speculative_decoding: Option<bool>,
    pub mtp_draft_max: Option<u32>,
    pub ngram_speculative_decoding: Option<bool>,
    pub spec_type: Option<String>,
    pub spec_ngram_size_n: Option<u32>,
    pub draft_min: Option<u32>,
    pub draft_max: Option<u32>,
}

/// Which catalog a model belongs to.
///
/// `builtin` is the baked-in llama.cpp store. `houmo` is the Houmo NPU
/// catalog queried from the same remote list `model-cli query` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalModelSource {
    #[default]
    Builtin,
    Houmo,
}

/// One catalog entry plus its live install/run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelStatus {
    pub id: String,
    /// Catalog partition. Absent on older payloads, which are builtin models.
    #[serde(default)]
    pub source: LocalModelSource,
    pub display_name: LocalModelText,
    pub description: LocalModelText,
    #[serde(rename = "type")]
    pub model_type: String,
    pub capabilities: Vec<String>,
    pub tags: Vec<String>,
    pub scenarios: Vec<String>,
    pub parameter_count: f64,
    pub size_bytes: u64,
    pub engines: Vec<String>,
    pub npu: bool,
    pub realtime: bool,
    pub author: String,
    pub license: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub status: LocalModelRunStatus,
    /// Download progress in `[0, 100]`.
    pub download_progress: f64,
    /// Download speed in bytes/second while downloading.
    pub download_speed: u64,
    /// The pipeline stage while downloading; absent in every other state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_phase: Option<LocalModelDownloadPhase>,
    /// Why the last download attempt failed; cleared when a new download starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_error: Option<LocalModelDownloadError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_path: Option<String>,
    pub main_gguf: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mmproj: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dflash: Option<String>,
    pub supports_mtp: bool,
    pub supports_dflash: bool,
    pub context_size: u64,
    pub max_tokens: u64,
    /// The most recent successful launch options, echoed back into the dialog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_launch_options: Option<LocalModelLaunchOptions>,
}

/// Response for `GET /api/local-models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelsResponse {
    pub models: Vec<LocalModelStatus>,
    /// Set when the Houmo catalog could not be refreshed and nothing is cached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub houmo_error: Option<String>,
}

/// Response for a single-model action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelResponse {
    pub model: LocalModelStatus,
}
