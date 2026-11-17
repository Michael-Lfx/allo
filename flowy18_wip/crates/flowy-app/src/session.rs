//! App data root and runtime temp directory. Cloud session persistence lives
//! in `flowy_auth`.

use std::path::{Path, PathBuf};

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
    use super::{ensure_runtime_temp, runtime_temp_dir};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn runtime_temp_dir_is_created_and_writable() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir()
            .join(format!("flowy-runtime-temp-{}-{stamp}", std::process::id()));
        let dir = ensure_runtime_temp(&root).unwrap();
        assert_eq!(dir, runtime_temp_dir(&root));
        assert!(dir.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
