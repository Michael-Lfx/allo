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
use tracing::{info, warn};

/// Mirror source endpoint configuration.
#[derive(Debug, Clone)]
pub struct MirrorConfig {
    pub id: String,
    pub base_url: String,
    pub probe_url: Option<String>,
    pub is_modelscope: bool,
    pub is_fallback_cdn: bool,
}

/// Heuristic detection of China network environment.
///
/// Priority:
/// 1. `FLOWY_PREFER_CHINA_MIRROR=1` or `FLOWY_MIRROR=cn|china` -> true
/// 2. `FLOWY_PREFER_GLOBAL_MIRROR=1` or `FLOWY_MIRROR=global|en` -> false
/// 3. Cargo configuration using domestic mirror (rsproxy / tuna / ustc / aliyun)
/// 4. Locale environment variables (`zh_CN`, `zh-cn`)
/// 5. Timezone environment variables (`Asia/Shanghai`, `PRC`, `CST-8`)
/// 6. System local timezone offset == UTC+8 (via `chrono::Local`)
pub fn is_china_network_environment() -> bool {
    if let Ok(val) = std::env::var("FLOWY_PREFER_CHINA_MIRROR")
        .or_else(|_| std::env::var("FLOWY_MIRROR"))
        .or_else(|_| std::env::var("VOICE_MODEL_MIRROR_PREFER"))
    {
        let v = val.trim().to_lowercase();
        if v == "1" || v == "true" || v == "cn" || v == "china" || v == "domestic" {
            return true;
        }
        if v == "0" || v == "false" || v == "global" || v == "en" || v == "overseas" {
            return false;
        }
    }

    if let Ok(val) = std::env::var("FLOWY_PREFER_GLOBAL_MIRROR") {
        let v = val.trim().to_lowercase();
        if v == "1" || v == "true" {
            return false;
        }
    }

    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        if let Some(root_path) = Path::new(&manifest_dir).ancestors().nth(3) {
            let cargo_config = root_path.join(".cargo").join("config.toml");
            if let Ok(content) = std::fs::read_to_string(cargo_config) {
                if content.contains("rsproxy")
                    || content.contains("tuna.tsinghua")
                    || content.contains("ustc")
                {
                    return true;
                }
            }
        }
    }

    for var in ["LC_ALL", "LANG", "LC_MESSAGES", "USERLOCALE"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_lowercase();
            if lower.contains("zh_cn") || lower.contains("zh-cn") || lower.contains("chinese") {
                return true;
            }
        }
    }

    if let Ok(tz) = std::env::var("TZ") {
        let lower = tz.to_lowercase();
        if lower.contains("shanghai")
            || lower.contains("beijing")
            || lower.contains("prc")
            || lower.contains("cst-8")
        {
            return true;
        }
    }

    let offset_seconds = chrono::Local::now().offset().local_minus_utc();
    if offset_seconds == 8 * 3600 {
        return true;
    }

    false
}

impl MirrorConfig {
    /// Returns default production mirrors in priority order based on network environment.
    pub fn default_mirrors() -> Vec<Self> {
        Self::mirrors_for_environment(is_china_network_environment())
    }

    /// Resolves default production mirrors for explicitly specified network environment.
    pub fn mirrors_for_environment(is_cn: bool) -> Vec<Self> {
        let mut mirrors = vec![
            Self { id: "pinned".into(), base_url: String::new(), probe_url: None, is_modelscope: false, is_fallback_cdn: false },
            Self { id: "pinned_fallback".into(), base_url: String::new(), probe_url: None, is_modelscope: false, is_fallback_cdn: true },
        ];
        if !is_cn { mirrors.reverse(); }
        mirrors
    }

    /// Resolves concrete download URL for a relative model asset path.
    pub fn resolve_url(&self, rel_path: &str) -> String {
        if self.id == "pinned" {
            return crate::verifier::pinned_source_url(rel_path).unwrap_or("").to_string();
        }
        if self.id == "pinned_fallback" {
            return crate::verifier::embedded_manifest().files.iter().find(|f| f.path == rel_path)
                .and_then(|f| f.fallback_urls.first()).cloned().unwrap_or_default();
        }
        format!("{}/{}", self.base_url.trim_end_matches('/'), rel_path)
    }

    pub fn supports(&self, rel_path: &str) -> bool {
        if self.id.starts_with("pinned") { return !self.resolve_url(rel_path).is_empty(); }
        let clean_path = rel_path.replace('\\', "/");
        if self.is_modelscope && self.base_url.contains("modelscope.cn") {
            // ModelScope upstream hosts ASR, KWS and VAD models directly.
            // Offline TTS models (Matcha + Vocos) are hosted on HuggingFace / hf-mirror.
            return !clean_path.starts_with("tts/");
        }
        if self.id == "hf_mirror"
            || self.id == "hf_official"
            || self.base_url.contains("hf-mirror.com")
            || self.base_url.contains("huggingface.co")
        {
            return clean_path.starts_with("asr/")
                || clean_path.starts_with("vad/")
                || clean_path.starts_with("tts/");
        }
        true
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

    #[error("Protocol violation: {0}")]
    ProtocolViolation(String),

    #[error("Server returned unexpected status: {0}")]
    UnexpectedStatus(StatusCode),

    #[error("Download speed {actual_bps} B/s dropped below threshold {threshold_bps} B/s on mirror {mirror_id}")]
    SpeedTooLow {
        actual_bps: u64,
        threshold_bps: u64,
        mirror_id: String,
    },
}

impl DownloaderError {
    /// Determines whether the error is transient and safe to retry with Range resumption on the same mirror.
    pub fn is_transient_retryable(&self) -> bool {
        match self {
            DownloaderError::Http(err) => {
                err.is_timeout() || err.is_connect() || err.is_body()
            }
            DownloaderError::Interrupted(_) => true,
            DownloaderError::UnexpectedStatus(status) => {
                status.is_server_error()
                    || *status == StatusCode::REQUEST_TIMEOUT
                    || *status == StatusCode::TOO_MANY_REQUESTS
            }
            // Deterministic protocol errors, 404, 401, 403, 416, local IO error, SpeedTooLow, AllMirrorsFailed should NEVER be blindly retried
            DownloaderError::ProtocolViolation(_)
            | DownloaderError::Io(_)
            | DownloaderError::SpeedTooLow { .. }
            | DownloaderError::AllMirrorsFailed => false,
        }
    }
}

/// Parses `Content-Range: bytes <start>-<end>/<total|*>`.
fn parse_content_range(value: &str) -> Option<(u64, u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let end = end.trim().parse::<u64>().ok()?;
    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse::<u64>().ok()?),
    };
    Some((start, end, total))
}

async fn remove_if_exists(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => warn!(path = %path.display(), %error, "Failed to discard partial download"),
    }
}

/// Downloader coordinating mirror probing, HTTP Range resumption, and failover.
#[derive(Debug, Clone)]
pub struct ModelDownloader {
    mirrors: Vec<MirrorConfig>,
    client: reqwest::Client,
    probe_timeout: Duration,
    min_speed_bps: u64,
    speed_gate_initial_delay: Duration,
    speed_gate_max_delay: Duration,
}

#[cfg(test)]
mod replacement_tests {
    #[tokio::test]
    async fn failed_final_verification_restores_old_target_and_preserves_rejected_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("model");
        let staged = dir.path().join("model.verified");
        let backup = dir.path().join("model.backup");
        std::fs::write(&target, b"post-promotion corruption").unwrap();
        std::fs::write(&backup, b"previous valid model").unwrap();
        super::rollback_replacement(&target, &staged, &backup).await.unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"previous valid model");
        assert_eq!(std::fs::read(&staged).unwrap(), b"post-promotion corruption");
        assert!(!backup.exists());
    }
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new(MirrorConfig::default_mirrors())
    }
}

/// Owned transaction artifacts; names bind a partial to both expected hash and size.
pub fn transaction_paths(target: &Path, hash: &str, size: u64) -> [PathBuf; 3] {
    let prefix = format!("{}.{hash}.{size}", target.display());
    [PathBuf::from(format!("{prefix}.verified")), PathBuf::from(format!("{prefix}.verified.part")), PathBuf::from(format!("{prefix}.backup"))]
}

async fn rollback_replacement(target: &Path, staged: &Path, backup: &Path) -> std::io::Result<()> {
    // Keep the rejected new bytes for diagnosis; never unlink the old backup.
    tokio::fs::rename(target, staged).await?;
    if backup.exists() { tokio::fs::rename(backup, target).await?; }
    Ok(())
}

impl ModelDownloader {
    /// Reuse verified targets and promote only content with the expected identity.
    pub async fn download_verified(&self, relative: &str, target: &Path, hash: &str, size: Option<u64>) -> Result<(), DownloaderError> {
        let [staged, part, backup] = transaction_paths(target, hash, size.unwrap_or(0));
        let matches = |path: PathBuf| async move {
            if let Some(size) = size {
                if tokio::fs::metadata(&path).await.map(|m| m.len()).ok() != Some(size) { return false; }
            }
            crate::verifier::compute_file_sha256(&path).await.ok().is_some_and(|h| h == hash)
        };
        if matches(target.to_path_buf()).await {
            if backup.exists() { tokio::fs::remove_file(&backup).await?; }
            return Ok(());
        }
        if backup.exists() {
            if !target.exists() {
                tokio::fs::rename(&backup, target).await?;
                if matches(target.to_path_buf()).await { return Ok(()); }
            } else {
                return Err(DownloaderError::ProtocolViolation(format!("recovery: {relative}: both target and backup require review")));
            }
        }
        if !matches(staged.clone()).await {
            if staged.exists() { tokio::fs::remove_file(&staged).await?; }
            if let Some(size) = size {
                if tokio::fs::metadata(&part).await.map(|m| m.len() >= size).unwrap_or(false) {
                    if matches(part.clone()).await {
                        tokio::fs::rename(&part, &staged).await?;
                    } else {
                        tokio::fs::remove_file(&part).await?;
                    }
                }
            }
            if !staged.exists() {
                let candidates: Vec<_> = self.mirrors.iter().filter(|m| m.supports(relative)).collect();
                let mut last_error = DownloaderError::AllMirrorsFailed;
                for (index, mirror) in candidates.iter().enumerate() {
                    let result = self.download_file_bounded(mirror, relative, &staged, index + 1 == candidates.len(), size).await;
                    match result {
                        Ok(_) if matches(staged.clone()).await => break,
                        Ok(_) => {
                            let actual = crate::verifier::compute_file_sha256(&staged).await?;
                            tokio::fs::remove_file(&staged).await?;
                            last_error = DownloaderError::ProtocolViolation(format!("verify: {relative}: source_id={}: expected {hash}, actual {actual}", mirror.id));
                        }
                        Err(error) => last_error = error,
                    }
                }
                if !staged.exists() { return Err(last_error); }
            }
        }
        if !matches(staged.clone()).await {
            let actual = crate::verifier::compute_file_sha256(&staged).await?;
            tokio::fs::remove_file(&staged).await?;
            return Err(DownloaderError::ProtocolViolation(format!("verify: {relative}: expected {hash}, actual {actual}; retry this file")));
        }
        if target.exists() { tokio::fs::rename(target, &backup).await?; }
        if let Err(error) = tokio::fs::rename(&staged, target).await {
            if backup.exists() {
                tokio::fs::rename(&backup, target).await.map_err(|rollback| DownloaderError::ProtocolViolation(format!("replace: {relative}: {error}; rollback failed: {rollback}")))?;
            }
            return Err(error.into());
        }
        if !matches(target.to_path_buf()).await {
            rollback_replacement(target, &staged, &backup).await.map_err(|error| DownloaderError::ProtocolViolation(format!("replace: {relative}: final verification failed; rollback failed: {error}; artifacts retained")))?;
            return Err(DownloaderError::ProtocolViolation(format!("replace: {relative}: final verification failed; previous target restored")));
        }
        if backup.exists() { tokio::fs::remove_file(&backup).await?; }
        Ok(())
    }

    /// Initializes a downloader with the provided mirror candidate list.
    pub fn new(mirrors: Vec<MirrorConfig>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Flowy/1.6.1")
            .connect_timeout(Duration::from_millis(5000))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let min_speed_bps = std::env::var("VOICE_MODEL_MIN_SPEED_BPS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(80 * 1024); // default 80 KB/s

        let initial_delay_secs = std::env::var("VOICE_MODEL_SPEED_GATE_INITIAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(12);

        let max_delay_secs = std::env::var("VOICE_MODEL_SPEED_GATE_MAX_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30)
            .max(initial_delay_secs);

        Self {
            mirrors,
            client,
            probe_timeout: Duration::from_millis(3000),
            min_speed_bps,
            speed_gate_initial_delay: Duration::from_secs(initial_delay_secs),
            speed_gate_max_delay: Duration::from_secs(max_delay_secs),
        }
    }

    /// Configures speed gate parameters for testing or specialized environments.
    pub fn with_speed_gate(
        mut self,
        min_speed_bps: u64,
        initial_delay: Duration,
        max_delay: Duration,
    ) -> Self {
        self.min_speed_bps = min_speed_bps;
        self.speed_gate_initial_delay = initial_delay;
        self.speed_gate_max_delay = max_delay.max(initial_delay);
        self
    }

    /// Accesses the configured mirrors.
    pub fn mirrors(&self) -> &[MirrorConfig] {
        &self.mirrors
    }

    /// Concurrent lightweight HEAD probing across candidate mirrors.
    ///
    /// Selects the first successful mirror in the configured priority order;
    /// slow transfers are handled by the speed gate rather than by reordering.
    pub async fn probe_best_source(&self) -> Result<MirrorConfig, DownloaderError> {
        if self.mirrors.is_empty() {
            return Err(DownloaderError::AllMirrorsFailed);
        }
        if self.mirrors.len() == 1 || self.mirrors[0].id.starts_with("pinned") {
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
                let resp = client
                    .head(&probe_url)
                    .timeout(timeout)
                    .send()
                    .await;

                match resp {
                    Ok(r) if r.status().is_success() || r.status().is_redirection() => {
                        Some(mirror_clone)
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

        if let Some(mirror) = successful.into_iter().next() {
            return Ok(mirror);
        }
        if let Some(first) = self.mirrors.first() {
            return Ok(first.clone());
        }
        Err(DownloaderError::AllMirrorsFailed)
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
        self.download_file_with_resume_internal(mirror, rel_path, target_path, true)
            .await
    }

    /// Internal download implementation supporting speed-gate aborting for failover candidates.
    pub async fn download_file_with_resume_internal(
        &self,
        mirror: &MirrorConfig,
        rel_path: &str,
        target_path: &Path,
        is_last_fallback: bool,
    ) -> Result<u64, DownloaderError> {
        self.download_file_bounded(mirror, rel_path, target_path, is_last_fallback, None).await
    }

    async fn download_file_bounded(
        &self,
        mirror: &MirrorConfig,
        rel_path: &str,
        target_path: &Path,
        is_last_fallback: bool,
        trusted_size: Option<u64>,
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

        let mut requested_start = existing_bytes;
        let mut req = self.client.get(&url);
        if existing_bytes > 0 {
            req = req.header(RANGE, format!("bytes={existing_bytes}-"));
        }

        let resp = tokio::time::timeout(Duration::from_secs(20), req.send()).await
            .map_err(|_| DownloaderError::Interrupted("response header timeout".into()))??;
        let resp = if resp.status() == StatusCode::RANGE_NOT_SATISFIABLE {
            // A 416 means the local partial no longer matches the remote object.
            // Never promote it from a size guess: discard it and fetch from zero.
            warn!(
                rel_path,
                local_partial_bytes = existing_bytes,
                "Range not satisfiable; restarting the transfer from zero"
            );
            remove_if_exists(&part_path).await;
            requested_start = 0;
            tokio::time::timeout(Duration::from_secs(20), self.client.get(&url).send()).await
                .map_err(|_| DownloaderError::Interrupted("response header timeout".into()))??
        } else {
            resp
        };

        let status = resp.status();
        let (mut file, start_offset, expected_total, expected_body) = match status {
            StatusCode::PARTIAL_CONTENT => {
                let (range_start, range_end, total) = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_content_range)
                    .ok_or_else(|| {
                        DownloaderError::ProtocolViolation(
                            "206 response is missing a valid Content-Range".into(),
                        )
                    })?;
                if range_start != requested_start {
                    remove_if_exists(&part_path).await;
                    return Err(DownloaderError::ProtocolViolation(format!(
                        "Content-Range start mismatch: requested {requested_start}, server returned {range_start}"
                    )));
                }
                if range_end < range_start {
                    remove_if_exists(&part_path).await;
                    return Err(DownloaderError::ProtocolViolation(format!(
                        "Content-Range is inverted: {range_start}-{range_end}"
                    )));
                }
                let expected_body = match range_end
                    .checked_sub(range_start)
                    .and_then(|length| length.checked_add(1))
                {
                    Some(length) => length,
                    None => {
                        remove_if_exists(&part_path).await;
                        return Err(DownloaderError::ProtocolViolation(
                            "Content-Range length overflows u64".into(),
                        ));
                    }
                };
                if total.is_some_and(|total| {
                    range_end
                        .checked_add(1)
                        .is_none_or(|end| total < end)
                }) {
                    remove_if_exists(&part_path).await;
                    return Err(DownloaderError::ProtocolViolation(
                        "Content-Range total is smaller than its own end offset".into(),
                    ));
                }
                if let Some(declared) = resp
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                {
                    if declared != expected_body {
                        remove_if_exists(&part_path).await;
                        return Err(DownloaderError::ProtocolViolation(format!(
                            "206 Content-Length {declared} does not match Content-Range length {expected_body}"
                        )));
                    }
                }

                if trusted_size.is_some_and(|size| total.is_some_and(|total| total != size) || range_end.checked_add(1) != Some(size)) {
                    return Err(DownloaderError::ProtocolViolation("Content-Range conflicts with trusted size".into()));
                }
                let file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .append(true)
                    .open(&part_path)
                    .await?;
                (file, range_start, total, Some(expected_body))
            }
            StatusCode::OK => {
                if requested_start > 0 {
                    warn!(
                        rel_path,
                        requested_start,
                        "Server ignored Range; restarting the transfer from zero"
                    );
                }
                let total = resp
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                if trusted_size.is_some_and(|size| total.is_some_and(|total| total != size)) {
                    return Err(DownloaderError::ProtocolViolation("Content-Length conflicts with trusted size".into()));
                }
                let file = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&part_path)
                    .await?;
                (file, 0, total, None)
            }
            other => return Err(DownloaderError::UnexpectedStatus(other)),
        };

        let mut stream = resp.bytes_stream();
        let mut downloaded_bytes = start_offset;
        let mut body_bytes: u64 = 0;
        let stream_start = Instant::now();
        let mut stream_bytes_received: u64 = 0;

        // Bounded per-chunk stall protection: abort if no bytes arrive for 20 seconds
        const CHUNK_STALL_TIMEOUT: Duration = Duration::from_secs(20);

        loop {
            let chunk_opt = match tokio::time::timeout(CHUNK_STALL_TIMEOUT, stream.next()).await {
                Ok(Some(item)) => item?,
                Ok(None) => break,
                Err(_) => {
                    return Err(DownloaderError::Interrupted(
                        "Stream stalled: no bytes received within timeout".into(),
                    ));
                }
            };
            let chunk_len = chunk_opt.len() as u64;
            stream_bytes_received = stream_bytes_received.saturating_add(chunk_len);
            body_bytes = body_bytes.checked_add(chunk_len).ok_or_else(|| {
                DownloaderError::ProtocolViolation("Response body length overflows u64".into())
            })?;
            downloaded_bytes = downloaded_bytes.checked_add(chunk_len).ok_or_else(|| {
                DownloaderError::ProtocolViolation("Downloaded length overflows u64".into())
            })?;

            if trusted_size.is_some_and(|size| downloaded_bytes > size)
                || expected_total.is_some_and(|size| downloaded_bytes > size)
                || expected_body.is_some_and(|size| body_bytes > size)
            {
                drop(file);
                remove_if_exists(&part_path).await;
                return Err(DownloaderError::ProtocolViolation("Response body exceeds permitted size".into()));
            }
            file.write_all(&chunk_opt).await?;

            // Speed gate inspection: during observation window (e.g. 12s~30s), if throughput is below threshold
            // and this is not the last fallback mirror, abort to allow faster failover.
            let elapsed = stream_start.elapsed();
            if !is_last_fallback
                && elapsed >= self.speed_gate_initial_delay
                && elapsed <= self.speed_gate_max_delay
            {
                let secs = elapsed.as_secs_f64();
                if secs > 0.0 {
                    let actual_bps = (stream_bytes_received as f64 / secs) as u64;
                    if actual_bps < self.min_speed_bps {
                        file.flush().await?;
                        drop(file);
                        return Err(DownloaderError::SpeedTooLow {
                            actual_bps,
                            threshold_bps: self.min_speed_bps,
                            mirror_id: mirror.id.clone(),
                        });
                    }
                }
            }
        }
        file.flush().await?;
        drop(file);

        // A 206 body must match the declared range even when the total is `*`.
        if let Some(expected) = expected_body {
            if body_bytes != expected {
                if body_bytes > expected {
                    remove_if_exists(&part_path).await;
                    return Err(DownloaderError::ProtocolViolation(format!(
                        "206 body exceeded the declared range: got {body_bytes} bytes, expected {expected}"
                    )));
                }
                return Err(DownloaderError::Interrupted(format!(
                    "206 body truncated: got {body_bytes} bytes, expected {expected}"
                )));
            }
        }

        if let Some(expected) = trusted_size.or(expected_total) {
            if downloaded_bytes != expected {
                if downloaded_bytes > expected {
                    remove_if_exists(&part_path).await;
                    return Err(DownloaderError::ProtocolViolation(format!(
                        "Stream exceeded the declared size: got {downloaded_bytes} bytes, expected {expected}"
                    )));
                }
                return Err(DownloaderError::Interrupted(format!(
                    "Stream truncated: got {downloaded_bytes} bytes, expected {expected}"
                )));
            }
        }

        // Raw transfer targets are staging paths; never unlink an existing target.
        // Production promotion belongs exclusively to download_verified.
        if target_path.exists() {
            return Err(DownloaderError::ProtocolViolation("transfer target already exists; use verified replacement".into()));
        }
        tokio::fs::rename(&part_path, target_path).await?;
        Ok(downloaded_bytes)
    }

    /// Downloads a file, automatically failing over across mirrors
    /// using Range resumption if throughput is too low or connection drops.
    pub async fn download_with_failover(
        &self,
        rel_path: &str,
        target_path: &Path,
        _expected_total_bytes: u64,
    ) -> Result<(), DownloaderError> {
        let mut last_error = None;
        let mut tried_ids = std::collections::HashSet::new();

        let mut candidate_mirrors = Vec::new();
        if let Ok(mirror) = self.probe_best_source().await {
            if mirror.supports(rel_path) {
                tried_ids.insert(mirror.id.clone());
                candidate_mirrors.push(mirror);
            }
        }
        for mirror in &self.mirrors {
            if !tried_ids.contains(&mirror.id) && mirror.supports(rel_path) {
                candidate_mirrors.push(mirror.clone());
            }
        }

        let total_candidates = candidate_mirrors.len();
        for (idx, mirror) in candidate_mirrors.iter().enumerate() {
            let is_last_fallback = idx + 1 == total_candidates;
            info!(
                mirror_id = %mirror.id,
                rel_path,
                is_last_fallback,
                "Attempting model download"
            );
            // For transient network drops, allow 1 retry on the ModelScope primary mirror with Range resumption
            // before failing over to the next candidate. SpeedTooLow aborts immediately to the next mirror.
            let max_attempts = if mirror.is_modelscope { 2 } else { 1 };
            let mut attempts = 0;
            while attempts < max_attempts {
                attempts += 1;
                match self
                    .download_file_with_resume_internal(mirror, rel_path, target_path, is_last_fallback)
                    .await
                {
                    Ok(_) => return Ok(()),
                    Err(DownloaderError::SpeedTooLow {
                        actual_bps,
                        threshold_bps,
                        mirror_id,
                    }) => {
                        warn!(
                            mirror_id = %mirror_id,
                            actual_bps,
                            threshold_bps,
                            "Throughput below speed gate threshold, failing over to next mirror"
                        );
                        last_error = Some(DownloaderError::SpeedTooLow {
                            actual_bps,
                            threshold_bps,
                            mirror_id,
                        });
                        break;
                    }
                    Err(e) if attempts < max_attempts && e.is_transient_retryable() => {
                        warn!(
                            mirror_id = %mirror.id,
                            error = %e,
                            attempt = attempts,
                            "Mirror download encountered transient error, retrying with range resumption"
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    Err(e) => {
                        warn!(mirror_id = %mirror.id, error = %e, "Mirror download failed, failing over");
                        last_error = Some(e);
                        break;
                    }
                }
            }
        }

        Err(last_error.unwrap_or(DownloaderError::AllMirrorsFailed))
    }
}
