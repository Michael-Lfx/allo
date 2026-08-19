//! Pack recent application logs into a ZIP for customer-support diagnostics.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use nomifun_api_types::SupportLogsPackResponse;
use nomifun_common::AppError;
use serde::Deserialize;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// Maximum total raw log bytes included before truncation (newest-first).
pub const MAX_RAW_BYTES: u64 = 40 * 1024 * 1024;
/// Only include log files modified within this many days.
pub const MAX_AGE_DAYS: u64 = 3;
const FAILED_PROVIDER_SSE_DIRECTORY: &str = "diagnostics/failed-provider-sse";
const MAX_FAILED_SSE_FILES: usize = 4;
const MAX_FAILED_SSE_BYTES: u64 = 1024 * 1024;
const OBSERVATION_DIRECTORY: &str = "diagnostics/observation";
const MAX_OBSERVATION_FILES: usize = 20;
const MAX_OBSERVATION_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
struct CandidateLog {
    path: PathBuf,
    name: String,
    size: u64,
    modified: SystemTime,
}

#[derive(Debug, Clone)]
struct FailedSseCapture {
    sse_path: PathBuf,
    metadata_path: PathBuf,
    sse_file_name: String,
    metadata_file_name: String,
    size: u64,
    modified: SystemTime,
}

#[derive(Debug, Deserialize)]
struct FailedSseMetadata {
    turn_id: Option<String>,
    sse_file: String,
}

fn is_support_log_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("nomicore.log") || lower.contains("nomi.log")
}

fn list_candidates(
    log_dir: &Path,
    now: SystemTime,
    max_age_days: u64,
) -> Result<Vec<CandidateLog>, AppError> {
    if !log_dir.exists() {
        return Ok(Vec::new());
    }
    let min_mtime = now
        .checked_sub(Duration::from_secs(max_age_days.saturating_mul(24 * 60 * 60)))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut out = Vec::new();
    let entries = std::fs::read_dir(log_dir).map_err(|e| {
        AppError::Internal(format!(
            "cannot read log dir '{}': {e}",
            log_dir.display()
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| AppError::Internal(format!("log dir entry: {e}")))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_support_log_filename(&name) {
            continue;
        }
        let meta = entry
            .metadata()
            .map_err(|e| AppError::Internal(format!("stat '{}': {e}", path.display())))?;
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified < min_mtime {
            continue;
        }
        out.push(CandidateLog {
            path,
            name,
            size: meta.len(),
            modified,
        });
    }

    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.size.cmp(&a.size)));
    Ok(out)
}

fn is_valid_turn_id(turn_id: &str) -> bool {
    !turn_id.is_empty()
        && turn_id.len() <= 64
        && turn_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn is_simple_sse_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("sse"))
}

fn list_failed_sse_captures(data_dir: &Path, turn_id: &str) -> Result<Vec<FailedSseCapture>, AppError> {
    if !is_valid_turn_id(turn_id) {
        return Err(AppError::BadRequest("turnId must contain only letters, digits, '-' or '_'".into()));
    }

    let directory = data_dir.join(FAILED_PROVIDER_SSE_DIRECTORY);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(&directory).map_err(|error| {
        AppError::Internal(format!(
            "cannot read failed provider SSE directory '{}': {error}",
            directory.display()
        ))
    })?;

    let mut captures = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| AppError::Internal(format!("failed SSE directory entry: {error}")))?;
        let metadata_path = entry.path();
        if !metadata_path.is_file()
            || !metadata_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        let metadata_file_name = entry.file_name().to_string_lossy().into_owned();
        let Ok(metadata_bytes) = std::fs::read(&metadata_path) else {
            continue;
        };
        let Ok(metadata) = serde_json::from_slice::<FailedSseMetadata>(&metadata_bytes) else {
            continue;
        };
        if metadata.turn_id.as_deref() != Some(turn_id) || !is_simple_sse_file_name(&metadata.sse_file) {
            continue;
        }

        let sse_path = directory.join(&metadata.sse_file);
        let Ok(sse_metadata) = sse_path.metadata() else {
            continue;
        };
        if !sse_metadata.is_file() || sse_metadata.len() > MAX_FAILED_SSE_BYTES {
            continue;
        }
        captures.push(FailedSseCapture {
            sse_path,
            metadata_path,
            sse_file_name: metadata.sse_file,
            metadata_file_name,
            size: sse_metadata.len(),
            modified: sse_metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }

    captures.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.size.cmp(&a.size)));
    captures.truncate(MAX_FAILED_SSE_FILES);
    Ok(captures)
}

fn select_files(candidates: Vec<CandidateLog>, max_raw_bytes: u64) -> (Vec<CandidateLog>, bool) {
    let mut selected = Vec::new();
    let mut total = 0u64;
    let mut truncated = false;

    for item in candidates {
        if total >= max_raw_bytes {
            truncated = true;
            break;
        }
        if item.size == 0 {
            selected.push(item);
            continue;
        }
        if total + item.size > max_raw_bytes {
            truncated = true;
            if selected.is_empty() {
                selected.push(item);
            }
            break;
        }
        total = total.saturating_add(item.size);
        selected.push(item);
    }

    (selected, truncated)
}

fn write_redacted_log_entry(
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    entry_name: &str,
    source: &Path,
    max_bytes: Option<u64>,
) -> Result<(), AppError> {
    zip.start_file(entry_name, options).map_err(|e| {
        AppError::Internal(format!("ZIP: start entry '{entry_name}': {e}"))
    })?;

    let file = File::open(source).map_err(|e| {
        AppError::Internal(format!("cannot open log '{}': {e}", source.display()))
    })?;
    let mut reader = BufReader::new(file);
    let mut written = 0u64;
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| {
            AppError::Internal(format!("read log '{}': {e}", source.display()))
        })?;
        if n == 0 {
            break;
        }
        if let Some(limit) = max_bytes
            && written >= limit
        {
            break;
        }
        let redacted = nomi_redact::redact_secrets_owned(std::mem::take(&mut line));
        if let Some(limit) = max_bytes {
            let remaining = limit.saturating_sub(written) as usize;
            let bytes = redacted.as_bytes();
            let chunk = if bytes.len() > remaining {
                &bytes[..remaining]
            } else {
                bytes
            };
            zip.write_all(chunk).map_err(|e| {
                AppError::Internal(format!("ZIP: write entry '{entry_name}': {e}"))
            })?;
            written = written.saturating_add(chunk.len() as u64);
        } else {
            zip.write_all(redacted.as_bytes()).map_err(|e| {
                AppError::Internal(format!("ZIP: write entry '{entry_name}': {e}"))
            })?;
            written = written.saturating_add(redacted.len() as u64);
        }
    }

    Ok(())
}

fn write_redacted_file_entry(
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
    entry_name: &str,
    source: &Path,
) -> Result<(), AppError> {
    zip.start_file(entry_name, options).map_err(|error| {
        AppError::Internal(format!("ZIP: start entry '{entry_name}': {error}"))
    })?;
    let bytes = std::fs::read(source)
        .map_err(|error| AppError::Internal(format!("cannot read '{}': {error}", source.display())))?;
    let redacted = nomi_redact::redact_secrets_owned(String::from_utf8_lossy(&bytes).into_owned());
    zip.write_all(redacted.as_bytes())
        .map_err(|error| AppError::Internal(format!("ZIP: write entry '{entry_name}': {error}")))
}

/// Pack recent logs under `log_dir` into a temp ZIP.
pub fn pack_support_logs(log_dir: &Path) -> Result<SupportLogsPackResponse, AppError> {
    pack_support_logs_with_limits(log_dir, SystemTime::now(), MAX_RAW_BYTES, MAX_AGE_DAYS)
}

/// Pack application logs and, when the report names a failed turn, its bounded
/// provider SSE capture. Optionally include session observation JSONL under
/// `observation/` in the ZIP. The ZIP uses Deflate compression for all inputs.
pub fn pack_support_logs_with_failed_sse(
    log_dir: &Path,
    data_dir: &Path,
    turn_id: Option<&str>,
    observation_paths: &[PathBuf],
) -> Result<SupportLogsPackResponse, AppError> {
    pack_support_logs_inner(
        log_dir,
        SystemTime::now(),
        MAX_RAW_BYTES,
        MAX_AGE_DAYS,
        turn_id.map(|turn_id| (data_dir, turn_id)),
        observation_paths,
    )
}

pub fn pack_support_logs_with_limits(
    log_dir: &Path,
    now: SystemTime,
    max_raw_bytes: u64,
    max_age_days: u64,
) -> Result<SupportLogsPackResponse, AppError> {
    pack_support_logs_inner(log_dir, now, max_raw_bytes, max_age_days, None, &[])
}

/// List on-disk observation JSONL files for a conversation under
/// `{data_dir}/diagnostics/observation/{sanitize(conversation_id)}/`.
///
/// Newest-first by mtime, capped at [`MAX_OBSERVATION_FILES`] / [`MAX_OBSERVATION_BYTES`].
pub fn list_observation_files_for_conversation(
    data_dir: &Path,
    conversation_id: &str,
) -> Result<Vec<PathBuf>, AppError> {
    let safe = sanitize_path_segment(conversation_id);
    let conv_dir = data_dir.join(OBSERVATION_DIRECTORY).join(safe);
    if !conv_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    let entries = std::fs::read_dir(&conv_dir).map_err(|error| {
        AppError::Internal(format!(
            "cannot read observation dir '{}': {error}",
            conv_dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            AppError::Internal(format!("observation dir entry: {error}"))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let meta = std::fs::metadata(&path).map_err(|error| {
            AppError::Internal(format!(
                "cannot stat observation '{}': {error}",
                path.display()
            ))
        })?;
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        candidates.push((path, meta.len(), modified));
    }

    candidates.sort_by(|a, b| b.2.cmp(&a.2));

    let mut selected = Vec::new();
    let mut total_bytes = 0u64;
    for (path, size, _) in candidates {
        if selected.len() >= MAX_OBSERVATION_FILES {
            break;
        }
        if total_bytes.saturating_add(size) > MAX_OBSERVATION_BYTES && !selected.is_empty() {
            break;
        }
        if size > MAX_OBSERVATION_BYTES && selected.is_empty() {
            // Allow a single oversized file; packing will still redact it.
            selected.push(path);
            break;
        }
        if size > MAX_OBSERVATION_BYTES {
            break;
        }
        total_bytes = total_bytes.saturating_add(size);
        selected.push(path);
    }
    Ok(selected)
}

/// Percent-encode anything outside ASCII alphanumeric / `-` / `_` so distinct
/// conversation ids cannot collapse onto one folder. Empty → `"unknown"`.
/// Mirrors `nomi_agent_trace::sanitize_path_segment` without taking that crate
/// as a dependency of `nomifun-system`.
fn sanitize_path_segment(raw: &str) -> String {
    const MAX: usize = 128;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "unknown".to_owned();
    }
    if trimmed.len() <= MAX
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return trimmed.to_owned();
    }
    let mut encoded = String::with_capacity(trimmed.len());
    for byte in trimmed.as_bytes() {
        if byte.is_ascii_alphanumeric() || *byte == b'-' || *byte == b'_' {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    if encoded.len() <= MAX {
        encoded
    } else {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in trimmed.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("h_{hash:016x}")
    }
}

fn pack_support_logs_inner(
    log_dir: &Path,
    now: SystemTime,
    max_raw_bytes: u64,
    max_age_days: u64,
    failed_sse_request: Option<(&Path, &str)>,
    observation_paths: &[PathBuf],
) -> Result<SupportLogsPackResponse, AppError> {
    let candidates = list_candidates(log_dir, now, max_age_days)?;
    let (selected, truncated) = select_files(candidates, max_raw_bytes);
    let failed_sse_captures = match failed_sse_request {
        Some((data_dir, turn_id)) => list_failed_sse_captures(data_dir, turn_id)?,
        None => Vec::new(),
    };

    if selected.is_empty() && failed_sse_captures.is_empty() && observation_paths.is_empty() {
        return Err(AppError::BadRequest(
            "no recent log files, matching failed provider SSE diagnostics, or session observations found to upload".into(),
        ));
    }

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file_name = format!("flowy-support-logs-{stamp}.zip");
    let zip_path = std::env::temp_dir().join(format!(
        "flowy-support-logs-{}-{}.zip",
        stamp,
        uuid::Uuid::new_v4()
    ));

    let file = File::create(&zip_path).map_err(|e| {
        AppError::Internal(format!(
            "cannot create zip '{}': {e}",
            zip_path.display()
        ))
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut included_files = Vec::new();
    let mut remaining_budget = max_raw_bytes;
    let single_oversized = selected.len() == 1 && selected[0].size > max_raw_bytes;

    for item in &selected {
        let max_bytes = if single_oversized {
            Some(max_raw_bytes)
        } else if item.size > remaining_budget {
            Some(remaining_budget)
        } else {
            None
        };
        write_redacted_log_entry(&mut zip, options, &item.name, &item.path, max_bytes)?;
        let consumed = max_bytes.unwrap_or(item.size).min(item.size);
        remaining_budget = remaining_budget.saturating_sub(consumed);
        included_files.push(item.name.clone());
    }

    for capture in &failed_sse_captures {
        let sse_entry_name = format!("failed-provider-sse/{}", capture.sse_file_name);
        write_redacted_file_entry(&mut zip, options, &sse_entry_name, &capture.sse_path)?;
        included_files.push(sse_entry_name);

        let metadata_entry_name = format!("failed-provider-sse/{}", capture.metadata_file_name);
        write_redacted_file_entry(
            &mut zip,
            options,
            &metadata_entry_name,
            &capture.metadata_path,
        )?;
        included_files.push(metadata_entry_name);
    }

    for path in observation_paths {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("events.jsonl");
        let parent = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("conversation");
        let entry_name = format!("observation/{parent}/{file_name}");
        write_redacted_file_entry(&mut zip, options, &entry_name, path)?;
        included_files.push(entry_name);
    }

    zip.finish().map_err(|e| {
        let _ = std::fs::remove_file(&zip_path);
        AppError::Internal(format!("ZIP finalize failed: {e}"))
    })?;

    let byte_size = std::fs::metadata(&zip_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);

    Ok(SupportLogsPackResponse {
        zip_path: zip_path.to_string_lossy().into_owned(),
        file_name,
        byte_size,
        included_files,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Duration;

    fn write_log(dir: &Path, name: &str, bytes: usize, age: Duration) {
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        let chunk = vec![b'a'; bytes];
        f.write_all(&chunk).unwrap();
        let modified = SystemTime::now() - age;
        let file = File::options().write(true).open(&path).unwrap();
        file.set_modified(modified).unwrap();
    }

    #[test]
    fn packs_recent_logs_and_skips_old() {
        let dir = tempfile::tempdir().unwrap();
        write_log(
            dir.path(),
            "2026-07-27.nomicore.log",
            100,
            Duration::from_secs(60),
        );
        write_log(
            dir.path(),
            "old.nomi.log",
            100,
            Duration::from_secs(10 * 24 * 3600),
        );
        write_log(dir.path(), "notes.txt", 100, Duration::from_secs(60));

        let result = pack_support_logs_with_limits(
            dir.path(),
            SystemTime::now(),
            MAX_RAW_BYTES,
            MAX_AGE_DAYS,
        )
        .unwrap();
        assert_eq!(
            result.included_files,
            vec!["2026-07-27.nomicore.log".to_string()]
        );
        assert!(!result.truncated);
        assert!(Path::new(&result.zip_path).exists());
        let _ = std::fs::remove_file(&result.zip_path);
    }

    #[test]
    fn truncates_when_over_budget() {
        let dir = tempfile::tempdir().unwrap();
        let budget = 1000u64;
        write_log(
            dir.path(),
            "newer.nomicore.log",
            700,
            Duration::from_secs(10),
        );
        write_log(dir.path(), "older.nomi.log", 700, Duration::from_secs(20));

        let result =
            pack_support_logs_with_limits(dir.path(), SystemTime::now(), budget, MAX_AGE_DAYS)
                .unwrap();
        assert_eq!(result.included_files.len(), 1);
        assert_eq!(result.included_files[0], "newer.nomicore.log");
        assert!(result.truncated);
        let _ = std::fs::remove_file(&result.zip_path);
    }

    #[test]
    fn errors_when_no_logs() {
        let dir = tempfile::tempdir().unwrap();
        let err = pack_support_logs_with_limits(
            dir.path(),
            SystemTime::now(),
            MAX_RAW_BYTES,
            MAX_AGE_DAYS,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn redacts_secrets_in_zip_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.nomicore.log");
        std::fs::write(&path, "Authorization: Bearer abcdef0123456789ABCDEF\n").unwrap();

        let result = pack_support_logs_with_limits(
            dir.path(),
            SystemTime::now(),
            MAX_RAW_BYTES,
            MAX_AGE_DAYS,
        )
        .unwrap();
        let file = File::open(&result.zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entry = archive.by_index(0).unwrap();
        let mut body = String::new();
        std::io::Read::read_to_string(&mut entry, &mut body).unwrap();
        assert!(body.contains("[REDACTED_SECRET]"));
        assert!(!body.contains("abcdef0123456789ABCDEF"));
        let _ = std::fs::remove_file(&result.zip_path);
    }

    #[test]
    fn packs_only_the_matching_failed_provider_sse_with_deflate_compression() {
        let log_dir = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let diagnostic_dir = data_dir.path().join(FAILED_PROVIDER_SSE_DIRECTORY);
        std::fs::create_dir_all(&diagnostic_dir).unwrap();
        let matching_sse_name = "capture-feedback.sse";
        let matching_metadata_name = "capture-feedback.json";
        std::fs::write(
            diagnostic_dir.join(matching_sse_name),
            "data: {\"message\":\"Authorization: Bearer abcdef0123456789ABCDEF\"}\n\n",
        )
        .unwrap();
        std::fs::write(
            diagnostic_dir.join(matching_metadata_name),
            serde_json::json!({
                "turn_id": "turn-feedback",
                "sse_file": matching_sse_name,
                "failure_reason": "malformed tool arguments"
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(diagnostic_dir.join("other.sse"), "data: other\n\n").unwrap();
        std::fs::write(
            diagnostic_dir.join("other.json"),
            serde_json::json!({"turn_id": "other-turn", "sse_file": "other.sse"}).to_string(),
        )
        .unwrap();

        let result = pack_support_logs_with_failed_sse(
            log_dir.path(),
            data_dir.path(),
            Some("turn-feedback"),
            &[],
        )
        .unwrap();
        assert_eq!(
            result.included_files,
            vec![
                format!("failed-provider-sse/{matching_sse_name}"),
                format!("failed-provider-sse/{matching_metadata_name}"),
            ]
        );

        let file = File::open(&result.zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entry = archive
            .by_name(&format!("failed-provider-sse/{matching_sse_name}"))
            .unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Deflated);
        let mut body = String::new();
        entry.read_to_string(&mut body).unwrap();
        assert!(body.contains("[REDACTED_SECRET]"));
        assert!(!body.contains("abcdef0123456789ABCDEF"));
        drop(entry);
        assert!(archive.by_name("failed-provider-sse/other.sse").is_err());
        let _ = std::fs::remove_file(&result.zip_path);
    }

    #[test]
    fn observation_folder_names_do_not_collapse_distinct_ids() {
        assert_eq!(sanitize_path_segment("abc-DEF_09"), "abc-DEF_09");
        assert_eq!(sanitize_path_segment("../evil/x"), "%2E%2E%2Fevil%2Fx");
        assert_ne!(
            sanitize_path_segment("foo.bar"),
            sanitize_path_segment("foobar")
        );
        assert_eq!(sanitize_path_segment("   "), "unknown");
    }
}
