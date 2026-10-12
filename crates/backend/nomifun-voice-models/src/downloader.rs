//! Dual-source smart probing downloader with HTTP Range resumption and CDN failover.
//!
//! Baseline:
//! - `model-management.zh.md` §3
//! - `wave-1-acoustic.zh.md` §3 (VT-1-05, VT-1-06)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// Mirror source endpoint configuration.
#[derive(Debug, Clone)]
pub struct MirrorConfig {
    pub id: String,
    pub base_url: String,
    pub probe_url: Option<String>,
    pub is_modelscope: bool,
    pub is_fallback_cdn: bool,
}

impl MirrorConfig {
    /// Returns default production mirrors in priority order.
    pub fn default_mirrors() -> Vec<Self> {
        let mut mirrors = Vec::new();

        // Support custom mirror override via environment variable
        if let Ok(custom_url) = std::env::var("AI_MATE_MODEL_MIRROR_URL") {
            let custom_url = custom_url.trim().to_string();
            if !custom_url.is_empty() {
                mirrors.push(Self {
                    id: "custom_mirror".to_string(),
                    base_url: custom_url.clone(),
                    probe_url: Some(format!("{}/probe", custom_url.trim_end_matches('/'))),
                    is_modelscope: false,
                    is_fallback_cdn: false,
                });
            }
        }

        mirrors.push(Self {
            id: "modelscope".to_string(),
            base_url: "https://modelscope.cn".to_string(),
            probe_url: Some(
                "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=tokens.txt"
                    .to_string(),
            ),
            is_modelscope: true,
            is_fallback_cdn: false,
        });

        mirrors
    }

    /// Resolves concrete download URL for a relative model asset path.
    pub fn resolve_url(&self, rel_path: &str) -> String {
        let clean_path = rel_path.replace('\\', "/");
        if self.is_modelscope && self.base_url.contains("modelscope.cn") {
            match clean_path.as_str() {
                "kws/encoder.int8.onnx" => {
                    "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx".to_string()
                }
                "kws/decoder.int8.onnx" => {
                    "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx".to_string()
                }
                "kws/joiner.int8.onnx" => {
                    "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx".to_string()
                }
                "kws/tokens.txt" => {
                    "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=tokens.txt".to_string()
                }
                "kws/keywords.txt" => {
                    "https://modelscope.cn/api/v1/models/pkufool/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/repo?Revision=master&FilePath=keywords.txt".to_string()
                }
                "vad/silero_vad.onnx" => {
                    "https://modelscope.cn/api/v1/models/manyeyes/silero-vad-onnx/repo?Revision=master&FilePath=silero_vad.onnx".to_string()
                }
                "asr/model.int8.onnx" => {
                    "https://modelscope.cn/api/v1/models/pengzhendong/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/repo?Revision=master&FilePath=model.int8.onnx".to_string()
                }
                "asr/tokens.txt" => {
                    "https://modelscope.cn/api/v1/models/pengzhendong/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/repo?Revision=master&FilePath=tokens.txt".to_string()
                }
                other => {
                    format!("{}/{}", self.base_url.trim_end_matches('/'), other)
                }
            }
        } else if self.id == "hf_mirror"
            || self.base_url.contains("hf-mirror.com")
            || self.base_url.contains("huggingface.co")
        {
            let base = self.base_url.trim_end_matches('/');
            match clean_path.as_str() {
                "kws/encoder.int8.onnx" => {
                    format!("{base}/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx")
                }
                "kws/decoder.int8.onnx" => {
                    format!("{base}/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx")
                }
                "kws/joiner.int8.onnx" => {
                    format!("{base}/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx")
                }
                "kws/tokens.txt" => {
                    format!("{base}/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/tokens.txt")
                }
                "kws/keywords.txt" => {
                    format!("{base}/csukuangfj/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/resolve/main/keywords.txt")
                }
                "vad/silero_vad.onnx" => {
                    format!("{base}/csukuangfj/silero-vad-onnx/resolve/main/silero_vad.onnx")
                }
                "asr/model.int8.onnx" => {
                    format!("{base}/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/resolve/main/model.int8.onnx")
                }
                "asr/tokens.txt" => {
                    format!("{base}/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue/resolve/main/tokens.txt")
                }
                other => {
                    format!("{base}/{other}")
                }
            }
        } else {
            format!("{}/{}", self.base_url.trim_end_matches('/'), clean_path)
        }
    }
}

/// Errors occurring during model download operations.
#[derive(Debug, thiserror::Error)]
pub enum DownloaderError {
    #[error("All candidate mirrors failed or timed out")]
    AllMirrorsFailed,

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Download interrupted: {0}")]
    Interrupted(String),

    #[error("Server returned unexpected status: {0}")]
    UnexpectedStatus(StatusCode),
}

/// Downloader coordinating mirror probing, HTTP Range resumption, and failover.
#[derive(Debug, Clone)]
pub struct ModelDownloader {
    mirrors: Vec<MirrorConfig>,
    client: reqwest::Client,
    probe_timeout: Duration,
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new(MirrorConfig::default_mirrors())
    }
}

impl ModelDownloader {
    /// Initializes a downloader with the provided mirror candidate list.
    pub fn new(mirrors: Vec<MirrorConfig>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Flowy/1.6.1")
            .connect_timeout(Duration::from_millis(3000))
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            mirrors,
            client,
            probe_timeout: Duration::from_millis(3000),
        }
    }

    /// Accesses the configured mirrors.
    pub fn mirrors(&self) -> &[MirrorConfig] {
        &self.mirrors
    }

    /// Concurrent lightweight HEAD probing across candidate mirrors.
    ///
    /// Rules:
    /// 1. Prioritize ModelScope: if connected and RTT < 800ms, lock domestic fast source.
    /// 2. If timeout or abroad (>800ms / unreachable), fallback to fastest global mirror.
    /// 3. If primary mirrors fail, fallback to CDN mirror.
    pub async fn probe_best_source(&self) -> Result<MirrorConfig, DownloaderError> {
        if self.mirrors.is_empty() {
            return Err(DownloaderError::AllMirrorsFailed);
        }
        if self.mirrors.len() == 1 {
            return Ok(self.mirrors[0].clone());
        }

        let mut probe_handles = Vec::new();

        for mirror in &self.mirrors {
            let mirror_clone = mirror.clone();
            let client = self.client.clone();
            let timeout = self.probe_timeout;

            let probe_url = mirror
                .probe_url
                .clone()
                .unwrap_or_else(|| format!("{}/probe", mirror.base_url.trim_end_matches('/')));

            let handle = tokio::spawn(async move {
                let start = Instant::now();
                let resp = client
                    .head(&probe_url)
                    .timeout(timeout)
                    .send()
                    .await;

                let rtt = start.elapsed();
                match resp {
                    Ok(r) if r.status().is_success() || r.status().is_redirection() => {
                        Some((mirror_clone, rtt))
                    }
                    _ => None,
                }
            });
            probe_handles.push(handle);
        }

        let mut successful = Vec::new();
        for handle in probe_handles {
            if let Ok(Some(result)) = handle.await {
                successful.push(result);
            }
        }

        if successful.is_empty() {
            // If probe failed (e.g. strict firewall on HEAD), fall back to first mirror candidate
            if let Some(cdn) = self.mirrors.iter().find(|m| m.is_fallback_cdn) {
                return Ok(cdn.clone());
            }
            if let Some(first) = self.mirrors.first() {
                return Ok(first.clone());
            }
            return Err(DownloaderError::AllMirrorsFailed);
        }

        // Rule 1: Check ModelScope (< 800ms)
        if let Some((ms_mirror, rtt)) = successful.iter().find(|(m, _)| m.is_modelscope) {
            if *rtt < Duration::from_millis(800) {
                debug!("Selected ModelScope source (RTT: {:?})", rtt);
                return Ok(ms_mirror.clone());
            }
        }

        // Rule 2: Pick fastest non-fallback mirror
        successful.sort_by_key(|(_, rtt)| *rtt);
        if let Some((best_primary, _)) = successful.iter().find(|(m, _)| !m.is_fallback_cdn) {
            return Ok(best_primary.clone());
        }

        // Rule 3: Pick fastest fallback mirror
        Ok(successful[0].0.clone())
    }

    /// Downloads a single file with HTTP Range resumption support.
    ///
    /// Saves incoming bytes to `<target_path>.part` and renames atomically upon completion.
    pub async fn download_file_with_resume(
        &self,
        mirror: &MirrorConfig,
        rel_path: &str,
        target_path: &Path,
    ) -> Result<u64, DownloaderError> {
        let url = mirror.resolve_url(rel_path);
        let part_path = PathBuf::from(format!("{}.part", target_path.display()));

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut existing_bytes: u64 = 0;
        if part_path.exists() {
            if let Ok(meta) = tokio::fs::metadata(&part_path).await {
                existing_bytes = meta.len();
            }
        }

        let mut req = self.client.get(&url);
        if existing_bytes > 0 {
            req = req.header(RANGE, format!("bytes={existing_bytes}-"));
        }

        let resp = req.send().await?;
        let status = resp.status();

        let (mut file, start_offset, expected_total) = match status {
            StatusCode::PARTIAL_CONTENT => {
                let total = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.split('/').last())
                    .and_then(|s| s.parse::<u64>().ok());

                let file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .append(true)
                    .open(&part_path)
                    .await?;
                (file, existing_bytes, total)
            }
            StatusCode::OK => {
                let total = resp
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                let file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&part_path)
                    .await?;
                (file, 0, total)
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                // Already downloaded or out of range
                if target_path.exists() {
                    let len = tokio::fs::metadata(target_path).await?.len();
                    return Ok(len);
                }
                // Try from scratch
                let file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&part_path)
                    .await?;
                (file, 0, None)
            }
            other => return Err(DownloaderError::UnexpectedStatus(other)),
        };

        let mut stream = resp.bytes_stream();
        let mut downloaded_bytes = start_offset;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            file.write_all(&chunk).await?;
            downloaded_bytes = downloaded_bytes.saturating_add(chunk.len() as u64);
        }
        file.flush().await?;
        drop(file);

        if let Some(expected) = expected_total {
            if downloaded_bytes < expected {
                return Err(DownloaderError::Interrupted(format!(
                    "Stream truncated: got {downloaded_bytes} bytes, expected {expected}"
                )));
            }
        }

        tokio::fs::rename(&part_path, target_path).await?;
        Ok(downloaded_bytes)
    }

    /// Downloads a file, automatically failing over to the official CDN mirror
    /// using Range resumption if the primary mirror fails or drops connection.
    pub async fn download_with_failover(
        &self,
        rel_path: &str,
        target_path: &Path,
        _expected_total_bytes: u64,
    ) -> Result<(), DownloaderError> {
        let mut last_error = None;
        let mut tried_ids = std::collections::HashSet::new();

        // Attempt 1: Probe and download from best primary mirror
        if let Ok(mirror) = self.probe_best_source().await {
            tried_ids.insert(mirror.id.clone());
            match self.download_file_with_resume(&mirror, rel_path, target_path).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    warn!("Primary mirror {} failed ({}), attempting failover", mirror.id, e);
                    last_error = Some(e);
                }
            }
        }

        // Attempt 2: Fallback to remaining mirrors (including fallback CDN)
        for mirror in &self.mirrors {
            if tried_ids.contains(&mirror.id) {
                continue;
            }
            info!("Failing over to candidate mirror {} for {}", mirror.id, rel_path);
            match self.download_file_with_resume(mirror, rel_path, target_path).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    warn!("Failover mirror {} failed: {e}", mirror.id);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(DownloaderError::AllMirrorsFailed))
    }
}
