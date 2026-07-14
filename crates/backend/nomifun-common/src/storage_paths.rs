//! Shared local storage path resolution for Flowy hosts and agent tooling.

use std::io;
use std::path::{Path, PathBuf};

/// Vendor segment under the per-user application-data directory.
pub const DATA_VENDOR_DIR: &str = "Flowy";

/// Default data-dir leaf for the stable channel.
pub const NOMI_LEAF_STABLE: &str = "Nomi";

/// Extreme fallback parent when `dirs::data_local_dir()` is unavailable.
pub const LEGACY_TEMP_DATA_PARENT: &str = "nomifun-data";

/// New extreme fallback parent (preferred once legacy temp installs are gone).
pub const FLOWY_TEMP_DATA_PARENT: &str = "flowy-data";

/// Temp-dir sandbox for uploads, attachments, and companion figure staging.
pub const TEMP_UPLOAD_SANDBOX: &str = "flowy";

/// Temp-dir root for browser engine profile/data when no explicit data dir is set.
pub const TEMP_BROWSER_DATA: &str = "flowy-browser-data";

/// Primary SQLite database file name.
pub const DATABASE_FILE: &str = "flowy-backend.db";

/// Legacy SQLite database file name (pre-Flowy rebrand).
pub const LEGACY_DATABASE_FILE: &str = "nomifun-backend.db";

/// SQLite WAL sidecar for [`DATABASE_FILE`].
pub const DATABASE_WAL_FILE: &str = "flowy-backend.db-wal";

/// SQLite SHM sidecar for [`DATABASE_FILE`].
pub const DATABASE_SHM_FILE: &str = "flowy-backend.db-shm";

/// Cross-process migration advisory lock for [`DATABASE_FILE`].
pub const DATABASE_MIGRATE_LOCK_FILE: &str = "flowy-backend.db.migrate.lock";

/// Transient quiescence-probe residue (desktop relocation).
pub const DATABASE_PROBE_FILE: &str = "flowy-backend.db.probe";

/// Legacy SQLite sidecars and lock (wiped/migrated alongside the main db).
pub const LEGACY_DATABASE_WAL_FILE: &str = "nomifun-backend.db-wal";
pub const LEGACY_DATABASE_SHM_FILE: &str = "nomifun-backend.db-shm";
pub const LEGACY_DATABASE_MIGRATE_LOCK_FILE: &str = "nomifun-backend.db.migrate.lock";
pub const LEGACY_DATABASE_PROBE_FILE: &str = "nomifun-backend.db.probe";

/// Primary database family members in wipe/rename order (sidecars first, main db last).
pub const DATABASE_FAMILY: [&str; 4] = [
    DATABASE_WAL_FILE,
    DATABASE_SHM_FILE,
    DATABASE_MIGRATE_LOCK_FILE,
    DATABASE_FILE,
];

/// Legacy database family members in wipe/rename order.
pub const LEGACY_DATABASE_FAMILY: [&str; 4] = [
    LEGACY_DATABASE_WAL_FILE,
    LEGACY_DATABASE_SHM_FILE,
    LEGACY_DATABASE_MIGRATE_LOCK_FILE,
    LEGACY_DATABASE_FILE,
];

/// All database-family filenames for factory reset (Flowy + legacy).
pub const DATABASE_FAMILY_FILES: &[&str] = &[
    DATABASE_WAL_FILE,
    DATABASE_SHM_FILE,
    DATABASE_MIGRATE_LOCK_FILE,
    DATABASE_PROBE_FILE,
    DATABASE_FILE,
    LEGACY_DATABASE_WAL_FILE,
    LEGACY_DATABASE_SHM_FILE,
    LEGACY_DATABASE_MIGRATE_LOCK_FILE,
    LEGACY_DATABASE_PROBE_FILE,
    LEGACY_DATABASE_FILE,
];

/// Primary database path under `data_dir`.
pub fn database_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DATABASE_FILE)
}

/// Legacy database path under `data_dir`.
pub fn legacy_database_path(data_dir: &Path) -> PathBuf {
    data_dir.join(LEGACY_DATABASE_FILE)
}

/// True when either the Flowy or legacy main database file exists in `data_dir`.
pub fn database_exists(data_dir: &Path) -> bool {
    database_path(data_dir).exists() || legacy_database_path(data_dir).exists()
}

/// Prefer the Flowy database when present; otherwise the legacy file (pre-rename).
pub fn existing_database_file(data_dir: &Path) -> Option<PathBuf> {
    let flowy = database_path(data_dir);
    if flowy.exists() {
        return Some(flowy);
    }
    let legacy = legacy_database_path(data_dir);
    if legacy.exists() {
        return Some(legacy);
    }
    None
}

/// Rename a legacy `nomifun-backend.db*` family to `flowy-backend.db*` when present.
pub fn ensure_database_filename_migrated(data_dir: &Path) -> io::Result<()> {
    if database_path(data_dir).exists() {
        return Ok(());
    }
    if !legacy_database_path(data_dir).exists() {
        return Ok(());
    }

    let pairs = [
        (LEGACY_DATABASE_WAL_FILE, DATABASE_WAL_FILE),
        (LEGACY_DATABASE_SHM_FILE, DATABASE_SHM_FILE),
        (LEGACY_DATABASE_MIGRATE_LOCK_FILE, DATABASE_MIGRATE_LOCK_FILE),
        (LEGACY_DATABASE_PROBE_FILE, DATABASE_PROBE_FILE),
        (LEGACY_DATABASE_FILE, DATABASE_FILE),
    ];
    for (from, to) in pairs {
        let src = data_dir.join(from);
        let dst = data_dir.join(to);
        if src.exists() && !dst.exists() {
            std::fs::rename(&src, &dst)?;
        }
    }
    Ok(())
}

/// Resolve the SQLite database path under `data_dir`, migrating legacy filenames first.
pub fn resolve_database_path(data_dir: &Path) -> PathBuf {
    if let Err(err) = ensure_database_filename_migrated(data_dir) {
        tracing::warn!(
            data_dir = %data_dir.display(),
            error = %err,
            "failed to rename legacy nomifun-backend.db family; using existing database file if present"
        );
    }
    existing_database_file(data_dir).unwrap_or_else(|| database_path(data_dir))
}

/// The data-dir leaf for the active build channel: `Nomi` on stable, `Nomi-dev`
/// (etc.) on non-stable channels.
pub fn nomi_leaf(channel_suffix: &str) -> String {
    format!("Nomi{channel_suffix}")
}

/// Default per-user data directory shared by desktop, web, and `nomicore`.
pub fn default_data_dir(channel_suffix: &str) -> PathBuf {
    dirs::data_local_dir()
        .map(|dir| dir.join(DATA_VENDOR_DIR))
        .unwrap_or_else(|| std::env::temp_dir().join(LEGACY_TEMP_DATA_PARENT))
        .join(nomi_leaf(channel_suffix))
}

/// Resolve agent/backend home from env (`FLOWY_HOME` then `NOMIFUN_HOME`).
pub fn resolve_home_from_env() -> Option<PathBuf> {
    for key in ["FLOWY_HOME", "NOMIFUN_HOME"] {
        if let Ok(home) = std::env::var(key) {
            let trimmed = home.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }
    None
}

/// Resolve `data_dir` from env (`FLOWY_DATA_DIR` then `NOMIFUN_DATA_DIR`).
pub fn resolve_data_dir_from_env() -> Option<PathBuf> {
    for key in ["FLOWY_DATA_DIR", "NOMIFUN_DATA_DIR"] {
        if let Ok(dir) = std::env::var(key) {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomi_leaf_stable_is_plain_nomi() {
        assert_eq!(nomi_leaf(""), "Nomi");
    }

    #[test]
    fn nomi_leaf_non_stable_attaches_suffix() {
        assert_eq!(nomi_leaf("-dev"), "Nomi-dev");
    }

    #[test]
    fn default_data_dir_is_absolute_and_flowy_nomi() {
        let dir = default_data_dir("");
        assert!(dir.is_absolute(), "default data dir must be absolute, got {dir:?}");
        assert!(
            dir.ends_with("Flowy/Nomi") || dir.ends_with("nomifun-data/Nomi"),
            "default data dir should end with Flowy/Nomi (or temp fallback), got {dir:?}"
        );
    }

    #[test]
    fn resolve_database_path_uses_flowy_when_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = resolve_database_path(tmp.path());
        assert_eq!(path, database_path(tmp.path()));
        assert!(!path.exists());
    }

    #[test]
    fn resolve_database_path_keeps_existing_flowy_db() {
        let tmp = tempfile::TempDir::new().unwrap();
        let flowy = database_path(tmp.path());
        std::fs::write(&flowy, b"db").unwrap();
        let path = resolve_database_path(tmp.path());
        assert_eq!(path, flowy);
        assert!(!legacy_database_path(tmp.path()).exists());
    }

    #[test]
    fn resolve_database_path_renames_legacy_family() {
        let tmp = tempfile::TempDir::new().unwrap();
        let legacy = legacy_database_path(tmp.path());
        std::fs::write(&legacy, b"db").unwrap();
        std::fs::write(tmp.path().join(LEGACY_DATABASE_WAL_FILE), b"wal").unwrap();
        let path = resolve_database_path(tmp.path());
        assert_eq!(path, database_path(tmp.path()));
        assert!(path.exists());
        assert!(!legacy.exists());
        assert!(tmp.path().join(DATABASE_WAL_FILE).exists());
    }

    #[test]
    fn resolve_database_path_falls_back_to_legacy_when_rename_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let legacy = legacy_database_path(tmp.path());
        std::fs::write(&legacy, b"db").unwrap();

        // Hold the legacy db exclusively on Windows so the rename in
        // ensure_database_filename_migrated fails while flowy-backend.db is absent.
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let _held = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&legacy)
                .unwrap();
            let path = resolve_database_path(tmp.path());
            assert_eq!(path, legacy);
            assert!(legacy.exists());
            assert!(!database_path(tmp.path()).exists());
        }

        #[cfg(not(windows))]
        {
            eprintln!("skipping rename-failure fallback test on non-Windows platforms");
        }
    }
}
