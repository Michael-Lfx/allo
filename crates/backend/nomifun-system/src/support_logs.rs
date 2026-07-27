//! Pack recent application logs into a ZIP for customer-support diagnostics.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use nomifun_api_types::SupportLogsPackResponse;
use nomifun_common::AppError;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

/// Maximum total raw log bytes included before truncation (newest-first).
pub const MAX_RAW_BYTES: u64 = 40 * 1024 * 1024;
/// Only include log files modified within this many days.
pub const MAX_AGE_DAYS: u64 = 3;

#[derive(Debug, Clone)]
struct CandidateLog {
    path: PathBuf,
    name: String,
    size: u64,
    modified: SystemTime,
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

/// Pack recent logs under `log_dir` into a temp ZIP.
pub fn pack_support_logs(log_dir: &Path) -> Result<SupportLogsPackResponse, AppError> {
    pack_support_logs_with_limits(log_dir, SystemTime::now(), MAX_RAW_BYTES, MAX_AGE_DAYS)
}

pub fn pack_support_logs_with_limits(
    log_dir: &Path,
    now: SystemTime,
    max_raw_bytes: u64,
    max_age_days: u64,
) -> Result<SupportLogsPackResponse, AppError> {
    let candidates = list_candidates(log_dir, now, max_age_days)?;
    let (selected, truncated) = select_files(candidates, max_raw_bytes);

    if selected.is_empty() {
        return Err(AppError::BadRequest(
            "no recent log files found to upload".into(),
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
    use std::io::Write;
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
}
