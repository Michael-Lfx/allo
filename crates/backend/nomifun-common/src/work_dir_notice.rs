//! Persisted notice that the boot layer recreated a missing work root.
//!
//! The boot layer writes this file into the data dir when it had to recreate a
//! deleted workspace root, so the settings UI can tell the user that the
//! previous workspace contents are gone instead of silently replacing them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File under the data dir holding the most recent recreation notice.
pub const WORK_DIR_RECREATED_FILE: &str = "work-dir-recreated.json";

/// The notice is a two-field JSON document; anything larger is not ours.
const MAX_NOTICE_BYTES: u64 = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkDirRecreatedNotice {
    pub work_dir: String,
    pub recreated_at: i64,
}

fn notice_path(data_dir: &Path) -> PathBuf {
    data_dir.join(WORK_DIR_RECREATED_FILE)
}

/// Best-effort record of a recreated work root. Boot must not fail because the
/// notice could not be written; callers log the failure instead. Atomic replace
/// keeps a crash from leaving a torn document behind.
pub fn record(data_dir: &Path, work_dir: &Path) -> std::io::Result<()> {
    let notice = WorkDirRecreatedNotice {
        work_dir: work_dir.to_string_lossy().into_owned(),
        recreated_at: crate::now_ms(),
    };
    let json = serde_json::to_vec_pretty(&notice)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    crate::dir_config::write_atomic_replace(&notice_path(data_dir), &json)
}

/// Read the notice, tolerating a missing or malformed file. Uses the shared
/// bounded, no-reparse reader so a replaced or symlinked marker cannot feed
/// arbitrary bytes into the API.
pub fn read(data_dir: &Path) -> Option<WorkDirRecreatedNotice> {
    let bytes =
        crate::dir_config::read_bounded_regular_file(&notice_path(data_dir), MAX_NOTICE_BYTES)
            .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Remove the notice, e.g. after a successful work-dir relocation makes it
/// stale. A missing file is fine.
pub fn clear(data_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(notice_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_data_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nomifun-wdnotice-{tag}-{}", crate::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn record_then_read_roundtrips() {
        let data_dir = temp_data_dir("roundtrip");
        let work_dir = data_dir.join("workspace");
        record(&data_dir, &work_dir).unwrap();
        let notice = read(&data_dir).unwrap();
        assert_eq!(notice.work_dir, work_dir.to_string_lossy());
        assert!(notice.recreated_at > 0);
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn read_tolerates_missing_and_malformed_files() {
        let data_dir = temp_data_dir("tolerant");
        assert!(read(&data_dir).is_none());
        std::fs::write(notice_path(&data_dir), b"not json").unwrap();
        assert!(read(&data_dir).is_none());
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn read_rejects_oversized_files() {
        let data_dir = temp_data_dir("oversized");
        let oversized = vec![b' '; (MAX_NOTICE_BYTES + 1) as usize];
        std::fs::write(notice_path(&data_dir), oversized).unwrap();
        assert!(read(&data_dir).is_none());
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn clear_removes_the_notice_and_tolerates_missing_files() {
        let data_dir = temp_data_dir("clear");
        clear(&data_dir).unwrap();
        record(&data_dir, &data_dir.join("ws")).unwrap();
        assert!(read(&data_dir).is_some());
        clear(&data_dir).unwrap();
        assert!(read(&data_dir).is_none());
        let _ = std::fs::remove_dir_all(&data_dir);
    }
}
