//! Disk cache for the models.dev registry.
//!
//! Atomicity is achieved by writing to a sibling temp file then renaming —
//! the standard POSIX `rename(2)` is atomic on the same filesystem, and the
//! `tempfile` crate's `NamedTempFile::persist` performs the rename.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tracing::debug;

/// Default disk cache path: `<cache_root>/models_dev/registry.json`.
///
/// Resolution order for the cache root:
/// 1. `NOMIFUN_CACHE_DIR` environment variable
/// 2. `dirs::cache_dir()/nomifun`
/// 3. system temp directory `/nomifun`
pub fn default_cache_path() -> PathBuf {
    let root = std::env::var_os("NOMIFUN_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("nomifun")))
        .unwrap_or_else(|| std::env::temp_dir().join("nomifun"));
    root.join("models_dev").join("registry.json")
}

/// Load a registry snapshot from disk; returns `None` on any failure.
///
/// "Failure" is broad on purpose: missing file, permission errors, malformed
/// JSON, I/O errors are all treated as "no cache available".
pub fn load(path: &Path) -> Option<Value> {
    if !path.exists() {
        return None;
    }
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v) if v.is_object() => Some(v),
            Ok(_) => {
                debug!(?path, "models.dev disk cache is not an object; ignoring");
                None
            }
            Err(e) => {
                debug!(?path, "Failed to parse models.dev disk cache: {e}");
                None
            }
        },
        Err(e) => {
            debug!(?path, "Failed to read models.dev disk cache: {e}");
            None
        }
    }
}

/// Atomically save a registry snapshot to disk.
///
/// Steps:
/// 1. Create parent directory if missing.
/// 2. Write to a `tempfile::NamedTempFile` in the same directory.
/// 3. `persist()` (rename) over the destination.
///
/// On any error returns `Err`; callers may choose to log and continue.
pub fn save(path: &Path, data: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let bytes =
        serde_json::to_vec(data).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&bytes)?;
    tmp.flush()?;
    tmp.persist(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let data = json!({
            "anthropic": {
                "name": "Anthropic",
                "models": {"claude-sonnet-4-5": {"limit": {"context": 200000}}}
            }
        });
        save(&path, &data).unwrap();
        let loaded = load(&path).expect("cache should load");
        assert_eq!(loaded, data);
    }

    #[test]
    fn load_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_invalid_json_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"not json{{{").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn load_non_object_json_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("array.json");
        std::fs::write(&path, b"[1, 2, 3]").unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sub/dir/cache.json");
        let data = json!({"k": "v"});
        save(&path, &data).unwrap();
        assert!(path.exists());
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, data);
    }

    #[test]
    fn default_cache_path_ends_with_registry_json() {
        let path = default_cache_path();
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some("registry.json")
        );
        assert!(
            path.components()
                .any(|c| c.as_os_str() == "models_dev")
        );
    }

    #[test]
    fn default_cache_path_respects_env() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: test-only env mutation; serialised by the test runner for this crate.
        unsafe {
            std::env::set_var("NOMIFUN_CACHE_DIR", dir.path());
        }
        let path = default_cache_path();
        assert!(path.starts_with(dir.path()));
        assert_eq!(
            path,
            dir.path().join("models_dev").join("registry.json")
        );
        unsafe {
            std::env::remove_var("NOMIFUN_CACHE_DIR");
        }
    }
}
