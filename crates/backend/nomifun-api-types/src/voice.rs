//! Voice Companion & Local Voice Model DTO contracts.
//!
//! Authoritative references:
//! - `docs/architecture/voice-agent/interaction.zh.md` §3.4.4
//! - `docs/architecture/voice-agent/model-management.zh.md` §4

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// 7-state lifecycle machine for local voice model assets.
///
/// Baseline: `interaction.zh.md` §3.4.4 and `model-management.zh.md` §4.
/// Wire format: tagged with `"status"`, variant struct payloads in `"detail"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "detail")]
pub enum ModelInstallState {
    /// 未安装：本地模型文件缺失或不完整
    NotInstalled,
    /// 探针选源中：正在并发探测 ModelScope / GitHub 等镜像源
    ProbingSource,
    /// 下载中：包含正在下载文件、字节数进度、百分比、实时下载速度
    Downloading {
        current_file: String,
        file_index: usize,
        total_files: usize,
        downloaded_bytes: u64,
        total_bytes: u64,
        progress_percent: f32,
        speed_bytes_per_sec: u64,
    },
    /// 密码学哈希自检中：7 个下载分发文件逐个 SHA256 校验
    VerifyingSha256 {
        current_file: String,
        verified_files: usize,
        total_files: usize,
    },
    /// 完全就绪：校验 100% 匹配，可供声学引擎 mmap 安全加载
    Ready {
        models_dir: PathBuf,
        total_size_bytes: u64,
    },
    /// 安装或校验失败：包含错误原因与可重试标志
    Failed {
        reason: String,
        recoverable: bool,
    },
    /// 正在卸载释放磁盘空间 (桌面端按需触发)
    Removing,
}

impl Default for ModelInstallState {
    fn default() -> Self {
        Self::NotInstalled
    }
}

impl ModelInstallState {
    /// Returns the wire status name of this variant.
    pub fn status_name(&self) -> &'static str {
        match self {
            Self::NotInstalled => "NotInstalled",
            Self::ProbingSource => "ProbingSource",
            Self::Downloading { .. } => "Downloading",
            Self::VerifyingSha256 { .. } => "VerifyingSha256",
            Self::Ready { .. } => "Ready",
            Self::Failed { .. } => "Failed",
            Self::Removing => "Removing",
        }
    }

    /// Whether the models are fully ready for acoustic inference.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// Whether the model manager is currently executing an asynchronous transition.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::ProbingSource
                | Self::Downloading { .. }
                | Self::VerifyingSha256 { .. }
                | Self::Removing
        )
    }

    /// Whether the model manager is in a failed state.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Detailed status for an individual model asset file on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelFileDetail {
    /// Relative subpath in the model topology (e.g. "kws/encoder.int8.onnx")
    pub path: String,
    /// File size in bytes (0 if missing)
    pub size_bytes: u64,
    /// Expected SHA256 checksum from the authoritative manifest
    pub expected_sha256: Option<String>,
    /// Actual SHA256 checksum if computed
    pub actual_sha256: Option<String>,
    /// Whether the file is present on disk and non-empty
    pub exists: bool,
}

/// Response DTO for `GET /api/voice/models/status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelStatusResponse {
    /// Current lifecycle state of the model manager
    pub state: ModelInstallState,
    /// Root path to models directory
    pub models_dir: PathBuf,
    /// Total disk space consumed by installed models in bytes
    pub total_size_bytes: u64,
    /// Per-file breakdown of model assets
    pub files: Vec<VoiceModelFileDetail>,
}

impl VoiceModelStatusResponse {
    /// Creates a status response indicating models are not installed.
    pub fn not_installed(models_dir: PathBuf) -> Self {
        Self {
            state: ModelInstallState::NotInstalled,
            models_dir,
            total_size_bytes: 0,
            files: Vec::new(),
        }
    }
}

/// Request DTO for `POST /api/voice/models/install`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelInstallRequest {
    /// Preferred mirror source ("auto", "modelscope", "github")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_mirror: Option<String>,
    /// Optional custom models directory override
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dir: Option<PathBuf>,
}

/// Response DTO for `POST /api/voice/models/install`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelInstallResponse {
    /// Whether the installation task was accepted/started
    pub accepted: bool,
    /// Current or newly transitioned lifecycle state
    pub state: ModelInstallState,
    /// Optional status or informational message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Server-Sent Event wrapper for `GET /api/voice/models/progress`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelProgressEvent {
    /// Event name/type (e.g. "progress")
    pub event: String,
    /// The current model installation state payload
    pub data: ModelInstallState,
}

impl VoiceModelProgressEvent {
    /// Create a new progress event.
    pub fn new(data: ModelInstallState) -> Self {
        Self {
            event: "progress".to_string(),
            data,
        }
    }
}

/// Request DTO for `DELETE /api/voice/models`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelDeleteRequest {
    /// Optional force flag to remove even if in unexpected state
    #[serde(default)]
    pub force: bool,
}

/// Response DTO for `DELETE /api/voice/models`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelDeleteResponse {
    /// Whether the deletion completed successfully
    pub success: bool,
    /// Number of bytes freed from disk
    pub freed_bytes: u64,
    /// State after deletion (resets to `NotInstalled`)
    pub state: ModelInstallState,
    /// Optional status message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Response DTO for `GET /api/voice/diagnostics/device`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceDeviceStatusResponse {
    /// Whether a default system microphone is detected and accessible
    pub mic_available: bool,
    /// Device name if available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// Default hardware sample rate in Hz
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// Channel count (e.g. 1 mono, 2 stereo)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    /// Error message if probe failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn default_asr_sample_rate() -> u32 {
    16_000
}

/// Request DTO for `POST /api/voice/diagnostics/asr-test`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceAsrTestRequest {
    /// Optional base64-encoded audio (WAV container or raw 16-bit PCM)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_base64: Option<String>,
    /// Sample rate in Hz (default 16000)
    #[serde(default = "default_asr_sample_rate")]
    pub sample_rate: u32,
}

/// Response DTO for `POST /api/voice/diagnostics/asr-test`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceAsrTestResponse {
    /// Whether recognition completed successfully
    pub success: bool,
    /// Transcribed text from SenseVoice model
    pub text: String,
    /// Duration of audio in milliseconds
    pub duration_ms: u64,
    /// Error message if ASR failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
