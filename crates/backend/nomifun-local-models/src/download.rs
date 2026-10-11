//! Streaming download + checksum + zip extraction helpers.
//!
//! The transport is deliberately defensive: a dedicated client (connect + read
//! timeouts, explicit User-Agent, still proxy-aware), a resumable `.part`
//! sidecar, per-mirror backoff/retry, and — for large files served with
//! `Accept-Ranges: bytes` — parallel range requests into the same file. The
//! SHA-256 is only computed once, by the caller, after the file is assembled,
//! because resumed/segmented bytes cannot be hashed while they stream.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use nomifun_api_types::LocalModelDownloadErrorCode;
use reqwest::header::{ACCEPT_RANGES, CONTENT_RANGE, RANGE};
use reqwest::{Response, StatusCode};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::{LocalModelError, LocalModelResult};
use crate::progress::DownloadProgress;

/// Connect budget for a download request.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Abort a read that produces no new bytes for this long, then retry.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for a response header before switching mirrors. A mirror
/// that is merely slow to answer still gets a generous window; only a truly
/// unresponsive one is abandoned.
const HEADER_TIMEOUT: Duration = Duration::from_secs(60);
/// How long to wait for a free connection slot before retrying.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(60);
/// Identify the downloader to model hosts.
const USER_AGENT: &str = "Flowy-LocalModels/1.0 (+https://nomifun.com)";

/// Retries against one mirror before switching to another.
const MAX_ATTEMPTS_PER_URL: usize = 3;
/// Whole-file attempt budget (probe + segmented/single) across all mirrors.
const MAX_FILE_ATTEMPTS: usize = 6;
/// Per-segment attempt budget across all mirrors.
const MAX_SEGMENT_ATTEMPTS: usize = 8;
/// Files at least this large are candidates for parallel range requests.
const SEGMENT_MIN_SIZE: u64 = 64 * 1024 * 1024;
/// Never split into a lane smaller than this.
const MIN_SEGMENT_BYTES: u64 = 16 * 1024 * 1024;
/// Upper bound on connections a single file may open.
const MAX_LANES_PER_FILE: usize = 4;

/// Build the dedicated download client (proxy-aware, with timeouts).
pub(crate) fn download_client() -> reqwest::Client {
    let builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .user_agent(USER_AGENT);
    nomifun_net::proxy::apply_detected_proxy(builder)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Hash a file on disk with SHA-256, returning lowercase hex.
pub(crate) async fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Hash a file on disk with SHA-256 while honoring a cancellation flag.
///
/// The background verification pipeline shares one flag across the whole
/// install; when the user cancels (or another file fails its checksum) the read
/// stops promptly and reports [`LocalModelError::Cancelled`] rather than a
/// spurious checksum failure.
pub(crate) async fn sha256_file_cancellable(
    path: &Path,
    cancel: &AtomicBool,
) -> LocalModelResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(LocalModelError::Cancelled);
        }
        let read = tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// True when the file exists and its length matches the expected size (when
/// known). Used to decide whether an existing file is about to be re-hashed.
pub(crate) async fn file_size_matches(path: &Path, size: u64) -> bool {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => size == 0 || metadata.len() == size,
        Err(_) => false,
    }
}

/// True when the file exists, matches the expected size (when known) and the
/// expected SHA-256 (when known).
pub(crate) async fn file_matches(path: &Path, size: u64, sha256: Option<&str>) -> bool {
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return false;
    };
    if size > 0 && metadata.len() != size {
        return false;
    }
    match sha256 {
        Some(expected) => match sha256_file(path).await {
            Ok(actual) => actual.eq_ignore_ascii_case(expected),
            Err(_) => false,
        },
        None => true,
    }
}

/// Everything one model file download needs.
pub(crate) struct FileDownload<'a> {
    pub client: &'a reqwest::Client,
    /// Mirrors, tried in order then round-robin.
    pub urls: &'a [&'a str],
    pub dest: &'a Path,
    pub file_size: u64,
    pub cancel: &'a AtomicBool,
    pub progress: &'a DownloadProgress,
    /// Bytes accounted for by files already completed in this install.
    pub completed_before: u64,
    /// Global connection budget shared across all in-flight models.
    pub semaphore: Arc<Semaphore>,
}

/// Download `dest` from the first working mirror, resuming from a `.part` when
/// possible and falling back to a single connection when ranges are missing.
pub(crate) async fn download_file(req: FileDownload<'_>) -> LocalModelResult<()> {
    if req.urls.is_empty() {
        return Err(LocalModelError::download(
            LocalModelDownloadErrorCode::Network,
            "no download source".to_owned(),
        ));
    }
    let part = part_path(req.dest);
    if let Some(parent) = req.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut resume_from = existing_len(&part, req.file_size).await;
    req.progress
        .reset(req.completed_before + resume_from, Instant::now());

    let mut url_index = 0usize;
    let mut url_attempts = 0usize;
    let mut total_attempts = 0usize;
    loop {
        if req.cancel.load(Ordering::Relaxed) {
            return Err(LocalModelError::Cancelled);
        }
        let url = req.urls[url_index % req.urls.len()];
        tracing::info!(
            host = %url_host(url),
            attempt = total_attempts + 1,
            "requesting model file from mirror"
        );
        match attempt_from_url(&req, url, &part, &mut resume_from).await {
            Ok(()) => {
                if req.cancel.load(Ordering::Relaxed) {
                    let _ = tokio::fs::remove_file(&part).await;
                    return Err(LocalModelError::Cancelled);
                }
                finalize(&part, req.dest).await?;
                return Ok(());
            }
            Err(LocalModelError::Cancelled) => return Err(LocalModelError::Cancelled),
            Err(error) => {
                total_attempts += 1;
                tracing::info!(
                    host = %url_host(url),
                    attempt = total_attempts,
                    error = %error,
                    "model file attempt failed"
                );
                if total_attempts >= MAX_FILE_ATTEMPTS {
                    return Err(error);
                }
                if is_retryable(&error) && url_attempts + 1 < MAX_ATTEMPTS_PER_URL {
                    url_attempts += 1;
                } else {
                    url_attempts = 0;
                    url_index = url_index.wrapping_add(1);
                }
                sleep_backoff(url_attempts).await;
                resume_from = existing_len(&part, req.file_size).await;
                req.progress
                    .reset(req.completed_before + resume_from, Instant::now());
            }
        }
    }
}

/// One pass against a single mirror: probe range support, then either stream
/// the (possibly resumed) remainder or fan out to parallel segments.
async fn attempt_from_url(
    req: &FileDownload<'_>,
    url: &str,
    part: &Path,
    resume_from: &mut u64,
) -> LocalModelResult<()> {
    let start = *resume_from;
    let (response, permit) = send_range(req, url, start, None).await?;
    let status = response.status();

    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        if req.file_size > 0 && start >= req.file_size {
            *resume_from = req.file_size;
            return Ok(());
        }
        reset_part(part).await;
        *resume_from = 0;
        req.progress.reset(req.completed_before, Instant::now());
        return Err(LocalModelError::download(
            LocalModelDownloadErrorCode::Network,
            format!("{url}: range not satisfiable, restarting"),
        ));
    }
    if !status.is_success() {
        return Err(http_error(status));
    }

    let partial = status == StatusCode::PARTIAL_CONTENT;
    let ranges_supported = partial || accepts_ranges(&response);
    let (offset, server_total) = if partial {
        match content_range(&response) {
            Some((range_start, _end, total)) => (range_start, total),
            None => (start, 0),
        }
    } else {
        (0, response.content_length().unwrap_or(0))
    };

    if req.file_size > 0 && server_total > 0 && server_total != req.file_size {
        reset_part(part).await;
        *resume_from = 0;
        req.progress.reset(req.completed_before, Instant::now());
        return Err(LocalModelError::download(
            LocalModelDownloadErrorCode::Network,
            format!("{url}: size {server_total} != manifest {}", req.file_size),
        ));
    }

    if !partial && start > 0 {
        // The server ignored our Range; the body starts at zero, so drop the
        // partial prefix rather than splicing two different files together.
        reset_part(part).await;
    }
    *resume_from = offset;
    req.progress
        .reset(req.completed_before + offset, Instant::now());
    // A catalog entry may not know the byte size (Houmo's list does not). The
    // GET response does. Adopt it so the card can leave 0%.
    adopt_unknown_file_size(req.progress, req.file_size, req.completed_before, server_total);
    let effective_size = if req.file_size > 0 {
        req.file_size
    } else {
        server_total
    };

    let remaining = effective_size.saturating_sub(offset);
    let lanes = plan_lanes(&req.semaphore, effective_size, remaining);
    if ranges_supported && effective_size >= SEGMENT_MIN_SIZE && lanes > 1 {
        // The range we just opened becomes the first lane: its body is written
        // and counted immediately, so the card leaves "connecting" as soon as
        // the first bytes land instead of waiting for a probe response to be
        // dropped and every segment re-opened. The remaining lanes open in
        // parallel while this one keeps reading.
        let segments = plan_segments(offset, effective_size, lanes);
        if let Err(error) = download_segments(req, part, segments, Some((response, permit))).await {
            // Segments write out of order, so a partial file may contain holes;
            // discard it rather than resume an ambiguous offset.
            reset_part(part).await;
            req.progress.reset(req.completed_before, Instant::now());
            return Err(error);
        }
        *resume_from = effective_size;
        return Ok(());
    }

    let cap = if effective_size > 0 {
        effective_size.saturating_sub(offset)
    } else {
        u64::MAX
    };
    let written = stream_response(response, part, offset, cap, req.cancel, req.progress, permit).await?;
    *resume_from = offset + written;
    Ok(())
}

/// Download every `(start, end)` segment of the current file concurrently.
///
/// `first_response` optionally carries the range request already opened for the
/// first segment; keeping it avoids dropping a live response and re-requesting
/// the same bytes. Any remaining segment opens its own connection.
async fn download_segments(
    req: &FileDownload<'_>,
    part: &Path,
    segments: Vec<(u64, u64)>,
    first_response: Option<(Response, OwnedSemaphorePermit)>,
) -> LocalModelResult<()> {
    let mut first_response = first_response;
    let mut futures: Vec<Pin<Box<dyn Future<Output = LocalModelResult<()>> + Send + '_>>> =
        Vec::with_capacity(segments.len());
    for (index, (start, end)) in segments.into_iter().enumerate() {
        if index == 0 {
            if let Some((response, permit)) = first_response.take() {
                futures.push(Box::pin(async move {
                    let expected = end.saturating_sub(start);
                    let written =
                        stream_response(response, part, start, expected, req.cancel, req.progress, permit)
                            .await?;
                    if written < expected {
                        return Err(LocalModelError::download(
                            LocalModelDownloadErrorCode::Network,
                            "first segment ended early".to_owned(),
                        ));
                    }
                    Ok(())
                }));
                continue;
            }
        }
        futures.push(Box::pin(download_segment(req, part, start, end)));
    }
    for result in futures_util::future::join_all(futures).await {
        result?;
    }
    Ok(())
}

/// Download a single `[seg_start, seg_end)` window, retrying and switching
/// mirrors without re-fetching the parts already written.
async fn download_segment(
    req: &FileDownload<'_>,
    part: &Path,
    seg_start: u64,
    seg_end: u64,
) -> LocalModelResult<()> {
    let mut written: u64 = 0;
    let mut url_index = 0usize;
    let mut url_attempts = 0usize;
    let mut total_attempts = 0usize;
    loop {
        if req.cancel.load(Ordering::Relaxed) {
            return Err(LocalModelError::Cancelled);
        }
        let from = seg_start + written;
        if from >= seg_end {
            return Ok(());
        }
        let url = req.urls[url_index % req.urls.len()];
        tracing::info!(
            host = %url_host(url),
            attempt = total_attempts + 1,
            from,
            to = seg_end,
            "requesting segment from mirror"
        );
        let result = download_segment_once(req, url, part, from, seg_end, &mut written).await;
        match result {
            Ok(()) if seg_start + written >= seg_end => return Ok(()),
            Ok(()) => {
                // Short read: treat as a transient stall and retry the same url.
                total_attempts += 1;
                tracing::info!(
                    host = %url_host(url),
                    attempt = total_attempts,
                    "segment ended early, retrying"
                );
                if total_attempts >= MAX_SEGMENT_ATTEMPTS {
                    return Err(LocalModelError::download(
                        LocalModelDownloadErrorCode::Network,
                        format!("{url}: segment ended early"),
                    ));
                }
                sleep_backoff(url_attempts).await;
            }
            Err(LocalModelError::Cancelled) => return Err(LocalModelError::Cancelled),
            Err(error) => {
                total_attempts += 1;
                tracing::info!(
                    host = %url_host(url),
                    attempt = total_attempts,
                    error = %error,
                    "segment attempt failed"
                );
                if total_attempts >= MAX_SEGMENT_ATTEMPTS {
                    return Err(error);
                }
                if is_retryable(&error) && url_attempts + 1 < MAX_ATTEMPTS_PER_URL {
                    url_attempts += 1;
                } else {
                    url_attempts = 0;
                    url_index = url_index.wrapping_add(1);
                }
                sleep_backoff(url_attempts).await;
            }
        }
    }
}

/// One range request for a segment, appending into `written` on success.
async fn download_segment_once(
    req: &FileDownload<'_>,
    url: &str,
    part: &Path,
    from: u64,
    seg_end: u64,
    written: &mut u64,
) -> LocalModelResult<()> {
    let (response, permit) = send_range(req, url, from, Some(seg_end)).await?;
    let status = response.status();
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        if from >= seg_end {
            return Ok(());
        }
        return Err(http_error(status));
    }
    if status == StatusCode::OK && from > 0 {
        // The mirror ignored the Range and is streaming the whole file; writing
        // it at an offset would corrupt the assembly.
        return Err(LocalModelError::download(
            LocalModelDownloadErrorCode::Network,
            format!("{url}: range ignored mid-file"),
        ));
    }
    if !status.is_success() {
        return Err(http_error(status));
    }
    let max = seg_end - from;
    let bytes = stream_response(response, part, from, max, req.cancel, req.progress, permit).await?;
    *written += bytes;
    Ok(())
}

/// Stream a response body into `part` at `offset`, adding progress per chunk.
#[allow(clippy::too_many_arguments)]
async fn stream_response(
    response: Response,
    part: &Path,
    offset: u64,
    max_bytes: u64,
    cancel: &AtomicBool,
    progress: &DownloadProgress,
    permit: OwnedSemaphorePermit,
) -> LocalModelResult<u64> {
    let _permit = permit;
    let mut file = open_for_range(part, offset).await?;
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();
    while written < max_bytes {
        let Some(chunk) = stream.next().await else {
            break;
        };
        if cancel.load(Ordering::Relaxed) {
            return Err(LocalModelError::Cancelled);
        }
        let chunk = chunk.map_err(map_body_error)?;
        let remaining = max_bytes - written;
        let take = (chunk.len() as u64).min(remaining) as usize;
        file.write_all(&chunk[..take]).await.map_err(map_disk_error)?;
        written += take as u64;
        progress.add(take as u64, Instant::now());
        if (take as u64) < chunk.len() as u64 {
            break;
        }
    }
    file.flush().await.map_err(map_disk_error)?;
    Ok(written)
}

/// Send a `Range` request, holding a global connection permit for its lifetime.
///
/// Both the wait for a free connection slot and the wait for the response
/// header are bounded. A mirror that never answers the header within the budget
/// is abandoned so the caller can switch mirrors; the body read keeps its own
/// stall timeout on the client.
async fn send_range(
    req: &FileDownload<'_>,
    url: &str,
    start: u64,
    end: Option<u64>,
) -> LocalModelResult<(Response, OwnedSemaphorePermit)> {
    let permit = tokio::time::timeout(ACQUIRE_TIMEOUT, Arc::clone(&req.semaphore).acquire_owned())
        .await
        .map_err(|_| {
            LocalModelError::download(
                LocalModelDownloadErrorCode::Timeout,
                "timed out waiting for a download connection slot".to_owned(),
            )
        })?
        .map_err(|_| {
            LocalModelError::download(
                LocalModelDownloadErrorCode::Unknown,
                "download semaphore closed".to_owned(),
            )
        })?;
    let range = match end {
        Some(end) => format!("bytes={start}-{}", end.saturating_sub(1)),
        None => format!("bytes={start}-"),
    };
    let response = tokio::time::timeout(
        HEADER_TIMEOUT,
        req.client.get(url).header(RANGE, range).send(),
    )
    .await
    .map_err(|_| LocalModelError::HeaderTimeout(url_host(url)))?
    .map_err(map_send_error)?;
    Ok((response, permit))
}

/// The host portion of a URL, for attempt logging without leaking the path.
fn url_host(url: &str) -> String {
    url.split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(url)
        .to_owned()
}

/// Split `[start, end)` into at most `lanes` contiguous, near-equal ranges.
pub(crate) fn plan_segments(start: u64, end: u64, lanes: usize) -> Vec<(u64, u64)> {
    let len = end.saturating_sub(start);
    if len == 0 {
        return Vec::new();
    }
    let lanes = (lanes.max(1) as u64).min(len);
    let base = len / lanes;
    let extra = len % lanes;
    let mut segments = Vec::with_capacity(lanes as usize);
    let mut cursor = start;
    for index in 0..lanes {
        let size = base + u64::from(index < extra);
        segments.push((cursor, cursor + size));
        cursor += size;
    }
    segments
}

/// Parse a `Content-Range` header value (`bytes 100-499/1234`).
pub(crate) fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let total = if total.trim() == "*" {
        0
    } else {
        total.trim().parse().ok()?
    };
    let (start, end) = range.trim().split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?, total))
}

fn plan_lanes(semaphore: &Semaphore, file_size: u64, remaining: u64) -> usize {
    if file_size < SEGMENT_MIN_SIZE {
        return 1;
    }
    let available = semaphore.available_permits().max(1);
    let by_permit = MAX_LANES_PER_FILE.min(available);
    let by_size = (remaining / MIN_SEGMENT_BYTES).max(1) as usize;
    by_permit.min(by_size).max(1)
}

fn accepts_ranges(response: &Response) -> bool {
    response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("bytes"))
}

fn content_range(response: &Response) -> Option<(u64, u64, u64)> {
    response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_range)
}

fn is_retryable(error: &LocalModelError) -> bool {
    match error {
        LocalModelError::Download(download) => match download.code {
            LocalModelDownloadErrorCode::Timeout | LocalModelDownloadErrorCode::Network => true,
            LocalModelDownloadErrorCode::Http => download
                .detail
                .as_deref()
                .and_then(|detail| detail.parse::<u16>().ok())
                .is_some_and(|status| status == 429 || (500..600).contains(&status)),
            _ => false,
        },
        _ => false,
    }
}

fn http_error(status: StatusCode) -> LocalModelError {
    LocalModelError::download(
        LocalModelDownloadErrorCode::Http,
        status.as_u16().to_string(),
    )
}

fn map_send_error(error: reqwest::Error) -> LocalModelError {
    let code = if error.is_timeout() {
        LocalModelDownloadErrorCode::Timeout
    } else {
        LocalModelDownloadErrorCode::Network
    };
    LocalModelError::download(code, error.to_string())
}

fn map_body_error(error: reqwest::Error) -> LocalModelError {
    let code = if error.is_timeout() {
        LocalModelDownloadErrorCode::Timeout
    } else {
        LocalModelDownloadErrorCode::Network
    };
    LocalModelError::download(code, error.to_string())
}

fn map_disk_error(error: std::io::Error) -> LocalModelError {
    LocalModelError::download(LocalModelDownloadErrorCode::Disk, error.to_string())
}

async fn sleep_backoff(attempts: usize) {
    let shift = attempts.saturating_sub(1).min(4) as u32;
    let millis = 400u64 << shift;
    tokio::time::sleep(Duration::from_millis(millis)).await;
}

async fn open_for_range(path: &Path, offset: u64) -> std::io::Result<tokio::fs::File> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(path)
        .await?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset)).await?;
    }
    Ok(file)
}

async fn existing_len(part: &Path, file_size: u64) -> u64 {
    match tokio::fs::metadata(part).await {
        Ok(metadata) if file_size == 0 || metadata.len() <= file_size => metadata.len(),
        Ok(_) => {
            let _ = tokio::fs::remove_file(part).await;
            0
        }
        Err(_) => 0,
    }
}

async fn reset_part(part: &Path) {
    let _ = tokio::fs::remove_file(part).await;
}

async fn finalize(part: &Path, dest: &Path) -> LocalModelResult<()> {
    if dest.exists() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    tokio::fs::rename(part, dest).await.map_err(map_disk_error)
}

/// Raise the install total when the catalog did not declare a file size and
/// the response just told us one. A known manifest size is left alone.
pub(crate) fn adopt_unknown_file_size(
    progress: &DownloadProgress,
    file_size: u64,
    completed_before: u64,
    server_total: u64,
) {
    if file_size == 0 && server_total > 0 {
        let floor = completed_before.saturating_add(server_total);
        if floor > progress.total() {
            progress.set_total(floor);
        }
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

/// Extract a zip archive into `dest_dir`, overwriting existing files.
pub(crate) fn extract_zip(zip_path: &Path, dest_dir: &Path) -> LocalModelResult<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| LocalModelError::Internal(error.to_string()))?;
    std::fs::create_dir_all(dest_dir)?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LocalModelError::Internal(error.to_string()))?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out_path = dest_dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

/// Extract a gzip-compressed tar archive into `dest_dir`, overwriting existing
/// files. Entries that are not regular files or directories are skipped, any
/// entry whose path escapes `dest_dir` is rejected, and the `llama-server`
/// binary is kept executable.
pub(crate) fn extract_tar_gz(archive_path: &Path, dest_dir: &Path) -> LocalModelResult<()> {
    let file = std::fs::File::open(archive_path)?;
    let decompressor = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decompressor);
    std::fs::create_dir_all(dest_dir)?;
    let entries = archive
        .entries()
        .map_err(|error| LocalModelError::Internal(error.to_string()))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| LocalModelError::Internal(error.to_string()))?;
        let path = entry
            .path()
            .map_err(|error| LocalModelError::Internal(error.to_string()))?;
        let relative = relative_archive_path(&path)?;
        let out_path = dest_dir.join(&relative);
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if !entry_type.is_file() {
            // Symlinks, hardlinks, and device nodes have no place in a runtime.
            continue;
        }
        let mode = entry.header().mode().ok();
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        set_runtime_mode(&out_path, mode, &relative);
    }
    Ok(())
}

/// Reject absolute paths and any `..` component, returning the safe relative
/// path to join under the extraction root.
fn relative_archive_path(path: &Path) -> LocalModelResult<PathBuf> {
    if path.is_absolute() {
        return Err(LocalModelError::Internal(format!(
            "archive entry has an absolute path: {}",
            path.display()
        )));
    }
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => safe.push(part),
            std::path::Component::CurDir => {}
            _ => {
                return Err(LocalModelError::Internal(format!(
                    "archive entry escapes the target directory: {}",
                    path.display()
                )));
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err(LocalModelError::Internal(
            "archive entry has an empty path".to_owned(),
        ));
    }
    Ok(safe)
}

/// Apply the archived permission bits on unix, forcing `llama-server` to stay
/// executable even if the header omits the bit.
fn set_runtime_mode(path: &Path, mode: Option<u32>, relative: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut bits = mode.unwrap_or(0o644) & 0o777;
        if relative.file_name().and_then(|name| name.to_str()) == Some("llama-server") {
            bits |= 0o755;
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            let mut perms = metadata.permissions();
            perms.set_mode(bits);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode, relative);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_segments_covers_the_whole_range_without_gaps() {
        assert_eq!(plan_segments(0, 10, 3), vec![(0, 4), (4, 7), (7, 10)]);
        assert_eq!(plan_segments(0, 9, 3), vec![(0, 3), (3, 6), (6, 9)]);
        let segments = plan_segments(5, 25, 4);
        assert_eq!(segments, vec![(5, 10), (10, 15), (15, 20), (20, 25)]);
        // Ranges stay contiguous and end exactly at `end`.
        let mut cursor = 5;
        for (start, end) in segments {
            assert_eq!(start, cursor);
            cursor = end;
        }
        assert_eq!(cursor, 25);
    }

    #[test]
    fn plan_segments_never_exceeds_lanes_or_one_byte_per_lane() {
        assert!(plan_segments(0, 0, 4).is_empty());
        // More lanes than bytes → one byte per segment.
        assert_eq!(plan_segments(0, 2, 8), vec![(0, 1), (1, 2)]);
        // A single lane is the whole range.
        assert_eq!(plan_segments(100, 200, 1), vec![(100, 200)]);
    }

    #[test]
    fn parse_content_range_reads_start_end_and_total() {
        assert_eq!(
            parse_content_range("bytes 100-499/1234"),
            Some((100, 499, 1234))
        );
        assert_eq!(parse_content_range("bytes 0-99/100"), Some((0, 99, 100)));
        // Unknown total is reported as zero.
        assert_eq!(parse_content_range("bytes 0-99/*"), Some((0, 99, 0)));
        assert_eq!(parse_content_range("not-a-range"), None);
        assert_eq!(parse_content_range("bytes 0-99"), None);
    }

    #[test]
    fn retryable_codes_cover_transient_failures_only() {
        let timeout = LocalModelError::download(LocalModelDownloadErrorCode::Timeout, None);
        let network = LocalModelError::download(LocalModelDownloadErrorCode::Network, None);
        let http_403 = LocalModelError::download(
            LocalModelDownloadErrorCode::Http,
            "403".to_owned(),
        );
        let http_500 = LocalModelError::download(
            LocalModelDownloadErrorCode::Http,
            "500".to_owned(),
        );
        let http_429 = LocalModelError::download(
            LocalModelDownloadErrorCode::Http,
            "429".to_owned(),
        );
        let checksum = LocalModelError::download(LocalModelDownloadErrorCode::Checksum, None);

        assert!(is_retryable(&timeout));
        assert!(is_retryable(&network));
        assert!(is_retryable(&http_429));
        assert!(is_retryable(&http_500));
        assert!(!is_retryable(&http_403));
        assert!(!is_retryable(&checksum));
    }

    #[test]
    fn an_unknown_file_size_adopts_the_response_length() {
        let progress = DownloadProgress::new(0);
        adopt_unknown_file_size(&progress, 0, 0, 1_000);
        progress.reset(250, Instant::now());
        assert_eq!(progress.percent(), 25.0);

        // A later file extends the total; it must not shrink a known one.
        adopt_unknown_file_size(&progress, 0, 1_000, 500);
        assert_eq!(progress.total(), 1_500);
        adopt_unknown_file_size(&progress, 4_000, 0, 9_000);
        assert_eq!(progress.total(), 1_500);
    }

    #[test]
    fn url_host_extracts_only_the_host() {
        assert_eq!(
            url_host("https://modelscope.cn/models/foo/bar.gguf?rev=main"),
            "modelscope.cn"
        );
        assert_eq!(url_host("http://127.0.0.1:8080/a"), "127.0.0.1:8080");
        assert_eq!(url_host("not-a-url"), "not-a-url");
    }

    #[tokio::test]
    async fn file_size_matches_only_when_the_length_is_known_and_equal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("model.gguf");
        tokio::fs::write(&path, vec![0u8; 8]).await.expect("seed");

        assert!(file_size_matches(&path, 8).await);
        // Unknown size means "present", matching `file_matches`.
        assert!(file_size_matches(&path, 0).await);
        assert!(!file_size_matches(&path, 9).await);
        assert!(!file_size_matches(&dir.path().join("missing.gguf"), 8).await);
    }

    #[test]
    fn http_error_carries_the_status_code_for_the_renderer() {
        let error = http_error(StatusCode::FORBIDDEN);
        match error {
            LocalModelError::Download(download) => {
                assert_eq!(download.code, LocalModelDownloadErrorCode::Http);
                assert_eq!(download.detail.as_deref(), Some("403"));
            }
            other => panic!("expected a structured download error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn existing_len_discards_a_part_larger_than_expected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let part = dir.path().join("model.gguf.part");
        tokio::fs::write(&part, vec![0u8; 10]).await.expect("seed");
        assert_eq!(existing_len(&part, 10).await, 10);
        assert_eq!(existing_len(&part, 4).await, 0);
        assert!(!part.exists());
    }

    /// Build a tar.gz. The name bytes are written directly so the archive can
    /// smuggle a path the `tar` crate's `append_data` would otherwise reject.
    fn write_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
        use flate2::write::GzEncoder;
        let encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (name, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            let name_bytes = name.as_bytes();
            assert!(name_bytes.len() <= 100, "test entry name too long");
            header.as_old_mut().name[..name_bytes.len()].copy_from_slice(name_bytes);
            header.set_cksum();
            builder.append(&header, *data).expect("append entry");
        }
        let bytes = builder
            .into_inner()
            .expect("gz writer")
            .finish()
            .expect("finish gz");
        std::fs::write(path, bytes).expect("write archive");
    }

    #[test]
    fn tar_gz_rejects_path_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("evil.tar.gz");
        write_tar_gz(&archive, &[("../evil.txt", b"pwned")]);
        let out = dir.path().join("out");
        let error = extract_tar_gz(&archive, &out).expect_err("must reject `..`");
        assert!(matches!(error, LocalModelError::Internal(_)));
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn tar_gz_extracts_nested_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("runtime.tar.gz");
        write_tar_gz(
            &archive,
            &[("houmo-HLIELLama-xh2/bin/llama-server", b"binary")],
        );
        let out = dir.path().join("out");
        extract_tar_gz(&archive, &out).expect("extract");
        let file = out.join("houmo-HLIELLama-xh2/bin/llama-server");
        assert_eq!(std::fs::read(&file).expect("read"), b"binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).expect("meta").permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "llama-server must stay executable");
        }
    }
}
