//! Remap absolute asset paths after importing a project from another machine.
//!
//! Registries (`world_assets_registry.json`, `character_portraits_registry.json`,
//! frame selector JSON, …) historically store absolute paths. After a
//! `.nomivimax` import the files live under a new working dir, but the JSON still
//! points at the exporter's disk — which surfaces as Windows `os error 3`
//! ("系统找不到指定的路径") when uploading Seedance reference images.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use walkdir::WalkDir;

use crate::error::{VimaxError, VimaxResult};

/// JSON files that commonly embed absolute filesystem paths.
fn is_path_bearing_json(name: &str) -> bool {
    matches!(
        name,
        "world_assets_registry.json"
            | "character_portraits_registry.json"
            | "first_frame_selector.json"
            | "last_frame_selector.json"
            | "frame_selector.json"
    ) || name.ends_with("_selector.json")
}

/// True when `s` looks like an absolute local filesystem path (not a URL).
pub fn looks_like_absolute_fs_path(s: &str) -> bool {
    let t = s.trim();
    if t.len() < 3 {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
    {
        return false;
    }
    let bytes = t.as_bytes();
    // Windows drive: `C:\…` / `C:/…`
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // UNC
    if t.starts_with("\\\\") {
        return true;
    }
    // Unix-style absolute (also detect when running on Windows — imported archives
    // may carry Linux exporter paths). Require another slash so bare "/v1" is skipped.
    if t.starts_with('/') && t.matches('/').count() >= 2 {
        return true;
    }
    false
}

/// Map an exporter absolute path onto `new_root` by matching the longest existing suffix.
///
/// Example:  
/// `C:\Users\Admin\…\idea2video\environments\3_dock\plate.png`  
/// + new_root `D:\…\7320\idea2video`  
/// → `D:\…\7320\idea2video\environments\3_dock\plate.png`
pub fn remap_abs_path_to_root(stored: &str, new_root: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(stored.trim());
    if path.as_os_str().is_empty() {
        return None;
    }
    if path.starts_with(new_root) && (path.is_file() || path.is_dir()) {
        return None; // already correct
    }
    if path.is_file() || path.is_dir() {
        return None; // still valid on this machine
    }

    let parts: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_os_string()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return None;
    }

    // Longest suffix first; require ≥2 components to avoid matching a lone basename elsewhere.
    for k in (2..=parts.len()).rev() {
        let mut candidate = new_root.to_path_buf();
        for part in &parts[parts.len() - k..] {
            candidate.push(part);
        }
        if candidate.is_file() || candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn rewrite_value(value: &mut Value, roots: &[&Path]) -> usize {
    match value {
        Value::String(s) => {
            if !looks_like_absolute_fs_path(s) {
                return 0;
            }
            for root in roots {
                if let Some(mapped) = remap_abs_path_to_root(s, root) {
                    let next = mapped.to_string_lossy().into_owned();
                    if next != *s {
                        *s = next;
                        return 1;
                    }
                }
            }
            0
        }
        Value::Array(items) => items.iter_mut().map(|v| rewrite_value(v, roots)).sum(),
        Value::Object(map) => map.values_mut().map(|v| rewrite_value(v, roots)).sum(),
        _ => 0,
    }
}

fn rewrite_json_file(path: &Path, roots: &[&Path]) -> VimaxResult<usize> {
    let raw = fs::read_to_string(path)?;
    let mut value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(0), // non-JSON / partial — skip
    };
    let changed = rewrite_value(&mut value, roots);
    if changed == 0 {
        return Ok(0);
    }
    let pretty = serde_json::to_string_pretty(&value)?;
    fs::write(path, pretty)?;
    Ok(changed)
}

/// Resolve a registry-stored path onto `film_root` (and its session parent).
///
/// Order: existing absolute → relative under film_root → suffix remap onto
/// film_root / session root → original (caller should check usability).
pub fn resolve_stored_asset_path(stored: &str, film_root: &Path) -> PathBuf {
    let raw = stored.trim();
    if raw.is_empty() {
        return PathBuf::new();
    }
    let path = PathBuf::from(raw);
    if path.is_file() {
        return path;
    }
    if !path.is_absolute() && !looks_like_absolute_fs_path(raw) {
        let under_film = film_root.join(raw);
        if under_film.is_file() {
            return under_film;
        }
    }
    let mut roots: Vec<&Path> = vec![film_root];
    if let Some(parent) = film_root.parent() {
        roots.push(parent);
    }
    for root in roots {
        if let Some(mapped) = remap_abs_path_to_root(raw, root) {
            if mapped.is_file() {
                return mapped;
            }
        }
    }
    path
}
///
/// `working_root` is the session working directory (`.working_dir/<id>/`), which
/// may contain `idea2video/`, `script2video/`, or `novel2video/` film trees.
///
/// Returns the number of path strings rewritten.
pub fn remap_imported_working_paths(working_root: &Path) -> VimaxResult<usize> {
    if !working_root.is_dir() {
        return Err(VimaxError::msg(format!(
            "working root missing for path remap: {}",
            working_root.display()
        )));
    }

    // Candidate roots: session root + each film workflow subdir that exists.
    let mut root_bufs: Vec<PathBuf> = vec![working_root.to_path_buf()];
    for name in ["idea2video", "script2video", "novel2video", "action2video"] {
        let sub = working_root.join(name);
        if sub.is_dir() {
            root_bufs.push(sub);
        }
    }
    let roots: Vec<&Path> = root_bufs.iter().map(|p| p.as_path()).collect();

    let mut total = 0usize;
    for entry in WalkDir::new(working_root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !is_path_bearing_json(name) {
            continue;
        }
        match rewrite_json_file(path, &roots) {
            Ok(n) => {
                if n > 0 {
                    tracing::info!(
                        file = %path.display(),
                        rewritten = n,
                        "remapped imported absolute asset paths"
                    );
                    total += n;
                }
            }
            Err(e) => {
                tracing::warn!(
                    file = %path.display(),
                    error = %e,
                    "failed to remap paths in JSON; continuing"
                );
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_windows_and_unix_abs_paths() {
        assert!(looks_like_absolute_fs_path(
            r"C:\Users\Admin\vimax\.working_dir\x\idea2video\environments\a\b.png"
        ));
        assert!(looks_like_absolute_fs_path(
            "/home/admin/.local/share/vimax/idea2video/props/x/y.png"
        ));
        assert!(!looks_like_absolute_fs_path("https://cdn.example/x.png"));
        assert!(!looks_like_absolute_fs_path("environments/a/b.png"));
        assert!(!looks_like_absolute_fs_path(""));
    }

    #[test]
    fn remaps_by_longest_existing_suffix() {
        let dir = tempdir().unwrap();
        let film = dir.path().join("idea2video");
        let plate = film
            .join("environments")
            .join("3_dock")
            .join("plate.png");
        fs::create_dir_all(plate.parent().unwrap()).unwrap();
        fs::write(&plate, b"png").unwrap();

        let foreign = r"C:\Users\Administrator\AppData\Local\Flowy\Nomi\vimax\.working_dir\9367fa55-aaaa\idea2video\environments\3_dock\plate.png";
        let mapped = remap_abs_path_to_root(foreign, &film).expect("remap");
        assert_eq!(mapped, plate);

        // Already-correct path → no remap needed
        assert!(remap_abs_path_to_root(&plate.to_string_lossy(), &film).is_none());
    }

    #[test]
    fn remaps_registry_json_under_working_root() {
        let dir = tempdir().unwrap();
        let working = dir.path().join("session");
        let film = working.join("idea2video");
        let plate_dir = film.join("environments").join("3_dock");
        fs::create_dir_all(&plate_dir).unwrap();
        let plate = plate_dir.join("plate.png");
        fs::write(&plate, b"png").unwrap();

        let foreign = r"C:\Users\Administrator\AppData\Local\Flowy\Nomi\vimax\.working_dir\old-id\idea2video\environments\3_dock\plate.png";
        let registry = film.join("world_assets_registry.json");
        fs::write(
            &registry,
            serde_json::to_string_pretty(&serde_json::json!({
                "environments": {
                    "dock": {
                        "path": foreign,
                        "description": "dock plate"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        // Scene mirror with the same stale path
        let scene = film.join("scene_1");
        fs::create_dir_all(&scene).unwrap();
        let scene_reg = scene.join("world_assets_registry.json");
        fs::copy(&registry, &scene_reg).unwrap();

        let n = remap_imported_working_paths(&working).unwrap();
        assert!(n >= 2, "expected both registries rewritten, got {n}");

        let fixed: Value = serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
        let path = fixed["environments"]["dock"]["path"].as_str().unwrap();
        assert_eq!(Path::new(path), plate.as_path());
        assert!(Path::new(path).is_file());
    }
}
