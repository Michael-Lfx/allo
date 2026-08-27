//! Per-artifact sidecar cache: `path.cache.json` with `{"v":1,"sha256":"<hex>"}`.
//!
//! Each text/JSON planning artifact writes a sidecar keyed by its logical inputs
//! (idea, story, style, user_requirement, etc.). On resume, the artifact is only
//! reused when the sidecar fingerprint matches the current inputs — a stale
//! sidecar means the artifact must be regenerated.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::error::VimaxResult;
use crate::session::{read_json_artifact, write_json_artifact, write_text_artifact};

/// Normalize input for fingerprinting: trim, `\r\n` → `\n`.
pub(crate) fn normalize_for_fingerprint(s: &str) -> String {
    s.trim().replace("\r\n", "\n")
}

/// Compute SHA-256 hex fingerprint from normalized inputs joined by `\n`.
pub(crate) fn artifact_fingerprint(inputs: &[&str]) -> String {
    let normalized: Vec<String> = inputs
        .iter()
        .map(|s| normalize_for_fingerprint(s))
        .collect();
    let joined = normalized.join("\n");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Sidecar path: `path.cache.json`.
fn sidecar_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.cache.json", path.display()))
}

/// True when the sidecar exists and its `sha256` matches `fingerprint`.
pub(crate) async fn sidecar_matches(path: &Path, fingerprint: &str) -> bool {
    let sidecar = sidecar_path(path);
    match tokio::fs::read_to_string(&sidecar).await {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(v) => {
                v.get("v").and_then(|v| v.as_i64()) == Some(1)
                    && v.get("sha256").and_then(|v| v.as_str()) == Some(fingerprint)
            }
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Fingerprint for script2video planning artifacts (`characters` / `storyboard` / shots / camera).
pub(crate) async fn script2video_plan_fingerprint(
    working_dir: &Path,
    script: &str,
    user_requirement: &str,
    style: &str,
) -> String {
    let film_root = super::resolve_film_root(working_dir);
    let scene_chars = working_dir.join("characters.json");
    let film_chars = film_root.join("characters.json");
    if film_chars.exists() && film_chars != scene_chars {
        let film_script = read_film_script_text(&film_root).await;
        let film_style = tokio::fs::read_to_string(film_root.join("style.txt"))
            .await
            .unwrap_or_else(|_| style.to_string());
        return artifact_fingerprint(&[&film_script, user_requirement, &film_style]);
    }
    artifact_fingerprint(&[script, user_requirement, style])
}

async fn read_film_script_text(film_root: &Path) -> String {
    if let Ok(s) = tokio::fs::read_to_string(film_root.join("story.txt")).await {
        if !s.trim().is_empty() {
            return s;
        }
    }
    tokio::fs::read_to_string(film_root.join("script.txt"))
        .await
        .unwrap_or_default()
}

/// True when every listed artifact exists and its sidecar matches `fingerprint`.
pub(crate) async fn plan_artifacts_sidecar_complete(
    working_dir: &Path,
    names: &[&str],
    fingerprint: &str,
) -> bool {
    for name in names {
        let path = working_dir.join(name);
        if !path.is_file() || !sidecar_matches(&path, fingerprint).await {
            return false;
        }
    }
    true
}

/// Persist sidecar with `{"v":1,"sha256":"<hex>"}`.
pub(crate) async fn write_sidecar(path: &Path, fingerprint: &str) -> VimaxResult<()> {
    let sidecar = sidecar_path(path);
    let content = serde_json::json!({"v": 1, "sha256": fingerprint});
    let raw = serde_json::to_string(&content)?;
    write_text_artifact(&sidecar, &raw).await?;
    Ok(())
}

/// Load or generate JSON with a cache sidecar.
///
/// When `path` exists **and** the sidecar fingerprint matches `cache_key`,
/// the artifact is reused without calling `generate`. Otherwise `generate`
/// runs and both the artifact and sidecar are written.
pub(crate) async fn load_or_write_json_cached<T, F, Fut>(
    path: &Path,
    cache_key: &str,
    generate: F,
) -> VimaxResult<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = VimaxResult<T>>,
{
    if path.exists() && sidecar_matches(path, cache_key).await {
        return read_json_artifact(path).await;
    }
    let value = generate().await?;
    write_json_artifact(path, &value).await?;
    write_sidecar(path, cache_key).await?;
    Ok(value)
}

/// Load or generate text with a cache sidecar.
pub(crate) async fn load_or_write_text_cached<F, Fut>(
    path: &Path,
    cache_key: &str,
    generate: F,
) -> VimaxResult<String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = VimaxResult<String>>,
{
    if path.exists() && sidecar_matches(path, cache_key).await {
        return Ok(tokio::fs::read_to_string(path).await?);
    }
    let value = generate().await?;
    write_text_artifact(path, &value).await?;
    write_sidecar(path, cache_key).await?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn fingerprint_deterministic() {
        let a = artifact_fingerprint(&["hello", "world"]);
        let b = artifact_fingerprint(&["hello", "world"]);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_differs_on_input_change() {
        let a = artifact_fingerprint(&["hello", "world"]);
        let b = artifact_fingerprint(&["hello", "WORLD"]);
        assert_ne!(a, b);
    }

    #[test]
    fn normalize_strips_crlf_and_trims() {
        assert_eq!(normalize_for_fingerprint("  a\r\nb  "), "a\nb");
        assert_eq!(normalize_for_fingerprint("a\nb"), "a\nb");
    }

    #[test]
    fn fingerprint_ignores_trailing_whitespace() {
        // Same logical content after normalization → same fingerprint.
        let a = artifact_fingerprint(&["hello  ", " world"]);
        let b = artifact_fingerprint(&["hello", "world"]);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_crlf_vs_lf_same() {
        let a = artifact_fingerprint(&["line1\r\nline2"]);
        let b = artifact_fingerprint(&["line1\nline2"]);
        assert_eq!(a, b);
    }

    #[test]
    fn sidecar_path_extension() {
        let p = Path::new("/tmp/idea2video/story.txt");
        let sc = sidecar_path(p);
        assert_eq!(sc, PathBuf::from("/tmp/idea2video/story.txt.cache.json"));
    }

    #[tokio::test]
    async fn cache_hit_skips_generate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("story.txt");
        let calls = Arc::new(AtomicUsize::new(0));
        let fp = artifact_fingerprint(&["idea-a", "req"]);
        let c = Arc::clone(&calls);
        let _ = load_or_write_text_cached(&path, &fp, || {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("once".into())
            }
        })
        .await
        .unwrap();
        let again = load_or_write_text_cached(&path, &fp, || {
            let c = Arc::clone(&calls);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("twice".into())
            }
        })
        .await
        .unwrap();
        assert_eq!(again, "once");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn idea_change_invalidates_story() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("story.txt");
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        let fp1 = artifact_fingerprint(&["idea-a", "req"]);
        let _ = load_or_write_text_cached(&path, &fp1, || {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("v1".into())
            }
        })
        .await
        .unwrap();
        let fp2 = artifact_fingerprint(&["idea-b", "req"]);
        let v2 = load_or_write_text_cached(&path, &fp2, || {
            let c = Arc::clone(&c);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Ok("v2".into())
            }
        })
        .await
        .unwrap();
        assert_eq!(v2, "v2");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn style_change_does_not_invalidate_story() {
        let dir = tempfile::tempdir().unwrap();
        let story = dir.path().join("story.txt");
        let chars = dir.path().join("characters.json");
        let story_fp = artifact_fingerprint(&["idea", "req"]);
        let _ = load_or_write_text_cached(&story, &story_fp, || async { Ok("story-body".into()) })
            .await
            .unwrap();
        let chars_fp_a = artifact_fingerprint(&["story-body", "style-a", ""]);
        let _ = load_or_write_json_cached(&chars, &chars_fp_a, || async {
            Ok(serde_json::json!({"cast": "a"}))
        })
        .await
        .unwrap();
        let story_again = load_or_write_text_cached(&story, &story_fp, || async {
            Ok("should-not-run".into())
        })
        .await
        .unwrap();
        assert_eq!(story_again, "story-body");
        let chars_fp_b = artifact_fingerprint(&["story-body", "style-b", ""]);
        let regen_calls = Arc::new(AtomicUsize::new(0));
        let rc = Arc::clone(&regen_calls);
        let updated: serde_json::Value = load_or_write_json_cached(&chars, &chars_fp_b, || {
            let rc = Arc::clone(&rc);
            async move {
                rc.fetch_add(1, Ordering::SeqCst);
                Ok(serde_json::json!({"cast": "b"}))
            }
        })
        .await
        .unwrap();
        assert_eq!(updated["cast"], "b");
        assert_eq!(regen_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn duration_not_in_story_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("story.txt");
        let fp = artifact_fingerprint(&["idea", "req"]);
        let _ = load_or_write_text_cached(&path, &fp, || async { Ok("stable".into()) })
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("target_duration_secs.txt"), "30")
            .await
            .unwrap();
        let again = load_or_write_text_cached(&path, &fp, || async { Ok("changed".into()) })
            .await
            .unwrap();
        assert_eq!(again, "stable");
    }
}