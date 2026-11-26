//! App data root and runtime temp directory. Cloud session persistence lives
//! in `flowy_auth`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_SESSION_BACKUPS: usize = 5;
const BACKUPS_DIR: &str = "backups";
const BACKUP_PREFIX: &str = "sessions-";
const FINGERPRINT_FILE: &str = ".fingerprint";
const SESSION_FILES: [&str; 2] = ["history.json", "index.json"];

pub use flowy_auth::{AuthError, Session, SessionStore, valid_email};

#[must_use]
pub fn default_root() -> PathBuf {
    directories::ProjectDirs::from(
        crate::brand::qualifier_tld(),
        crate::brand::qualifier_org(),
        crate::brand::qualifier_app(),
    )
    .map(|dirs| dirs.data_dir().to_path_buf())
    .or_else(|| std::env::current_dir().ok())
    .unwrap_or_else(|| PathBuf::from("."))
}

/// Directory storing rolling pre-boot snapshots of non-empty local sessions.
#[must_use]
pub fn backups_dir(root: &Path) -> PathBuf {
    root.join(BACKUPS_DIR)
}

fn json_has_non_empty_sessions(name: &str, bytes: &[u8]) -> bool {
    let trimmed = bytes.trim_ascii();
    if trimmed.is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(trimmed) else {
        // Keep unreadable/corrupt non-empty session stores before Engine
        // quarantines or rewrites them on startup.
        return true;
    };
    if let Some(arr) =
        value.get("sessions").and_then(serde_json::Value::as_array)
    {
        return !arr.is_empty();
    }
    if name == "index.json"
        && let Some(arr) = value.as_array()
    {
        return !arr.is_empty();
    }
    false
}

fn fnv1a_fingerprint(files: &[(&str, Vec<u8>)]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, bytes) in files {
        for byte in name
            .as_bytes()
            .iter()
            .chain(std::iter::once(&0xff_u8))
            .chain(bytes.iter())
            .chain(std::iter::once(&0xfe_u8))
        {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

fn existing_session_backups(backups: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(backups) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|ft| ft.is_dir())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(BACKUP_PREFIX))
        })
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    Ok(dirs)
}

/// Take a rolling backup of non-empty local sessions before opening the engine.
///
/// Empty or unchanged stores never evict older backups, and at most
/// [`MAX_SESSION_BACKUPS`] snapshots are retained.
pub fn snapshot_sessions_before_boot(root: &Path) -> io::Result<bool> {
    let sessions_dir = root.join("sessions");
    let mut captured: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut has_meaningful_data = false;
    for name in SESSION_FILES {
        let path = sessions_dir.join(name);
        match fs::read(&path) {
            Ok(bytes) => {
                if json_has_non_empty_sessions(name, &bytes) {
                    has_meaningful_data = true;
                }
                captured.push((name, bytes));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if !has_meaningful_data {
        return Ok(false);
    }
    let fingerprint = fnv1a_fingerprint(&captured);
    let backups = backups_dir(root);
    let existing = existing_session_backups(&backups)?;
    if let Some(latest) = existing.last()
        && let Ok(saved_fp) = fs::read_to_string(latest.join(FINGERPRINT_FILE))
        && saved_fp.trim() == fingerprint
    {
        return Ok(false);
    }

    fs::create_dir_all(&backups)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let staging = backups.join(format!(".tmp-{}-{millis}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let write_res = (|| -> io::Result<()> {
        for (name, bytes) in &captured {
            fs::write(staging.join(name), bytes)?;
        }
        fs::write(staging.join(FINGERPRINT_FILE), fingerprint.as_bytes())
    })();
    if let Err(error) = write_res {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    let mut seq = 0u32;
    let mut final_dir = backups.join(format!("{BACKUP_PREFIX}{millis:016}"));
    while final_dir.exists() {
        seq += 1;
        final_dir = backups.join(format!("{BACKUP_PREFIX}{millis:016}-{seq}"));
    }
    if let Err(error) = fs::rename(&staging, &final_dir) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    let updated = existing_session_backups(&backups)?;
    if let Some(excess) = updated.len().checked_sub(MAX_SESSION_BACKUPS) {
        for old in updated.into_iter().take(excess) {
            let _ = fs::remove_dir_all(old);
        }
    }
    Ok(true)
}

/// Per-user runtime temp directory, next to sessions and logs.
#[must_use]
pub fn runtime_temp_dir(root: &Path) -> PathBuf {
    root.join("runtime-tmp")
}

/// Create the runtime temp directory and prove it is writable.
///
/// Tools stage scripts and capture output here, so an unwritable temp root must
/// fail before the first tool call instead of surfacing as a permission error
/// mid-turn.
pub fn ensure_runtime_temp(root: &Path) -> std::io::Result<PathBuf> {
    let dir = runtime_temp_dir(root);
    std::fs::create_dir_all(&dir)?;
    let probe = dir.join(format!(".probe-{}", std::process::id()));
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_SESSION_BACKUPS, backups_dir, ensure_runtime_temp,
        existing_session_backups, runtime_temp_dir,
        snapshot_sessions_before_boot,
    };
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(tag: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!(
            "flowy-session-{tag}-{}-{stamp}-{id}",
            std::process::id()
        ))
    }

    #[test]
    fn runtime_temp_dir_is_created_and_writable() {
        let root = test_dir("tmp");
        let dir = ensure_runtime_temp(&root).unwrap();
        assert_eq!(dir, runtime_temp_dir(&root));
        assert!(dir.is_dir());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn snapshot_ignores_empty_stores_and_preserves_prior_backups() {
        let root = test_dir("empty");
        let _ = fs::remove_dir_all(&root);
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).unwrap();

        assert!(!snapshot_sessions_before_boot(&root).unwrap());
        fs::write(
            sessions.join("history.json"),
            br#"{"active":null,"sessions":[{"id":"s1","messages":[]}]}"#,
        )
        .unwrap();
        assert!(snapshot_sessions_before_boot(&root).unwrap());
        assert_eq!(
            existing_session_backups(&backups_dir(&root)).unwrap().len(),
            1
        );

        // Wiping live stores to empty must never replace or prune valid backups.
        fs::write(
            sessions.join("history.json"),
            br#"{"active":null,"sessions":[]}"#,
        )
        .unwrap();
        fs::write(sessions.join("index.json"), b"[]").unwrap();
        assert!(!snapshot_sessions_before_boot(&root).unwrap());
        let preserved = existing_session_backups(&backups_dir(&root)).unwrap();
        assert_eq!(preserved.len(), 1);
        let saved =
            fs::read_to_string(preserved[0].join("history.json")).unwrap();
        assert!(saved.contains("\"s1\""));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn snapshot_dedups_identical_data_and_keeps_latest_five() {
        let root = test_dir("rotate");
        let _ = fs::remove_dir_all(&root);
        let sessions = root.join("sessions");
        fs::create_dir_all(&sessions).unwrap();

        for round in 1..=7 {
            fs::write(
                sessions.join("history.json"),
                format!(
                    r#"{{"active":"s{round}","sessions":[{{"id":"s{round}","messages":[]}}]}}"#
                ),
            )
            .unwrap();
            assert!(snapshot_sessions_before_boot(&root).unwrap());
            // Re-running with unchanged contents skips duplicate snapshot creation.
            assert!(!snapshot_sessions_before_boot(&root).unwrap());
        }

        let kept = existing_session_backups(&backups_dir(&root)).unwrap();
        assert_eq!(kept.len(), MAX_SESSION_BACKUPS);
        let newest =
            fs::read_to_string(kept.last().unwrap().join("history.json"))
                .unwrap();
        assert!(newest.contains("\"s7\""));
        let oldest_kept =
            fs::read_to_string(kept[0].join("history.json")).unwrap();
        assert!(oldest_kept.contains("\"s3\""));

        fs::remove_dir_all(&root).unwrap();
    }
}
