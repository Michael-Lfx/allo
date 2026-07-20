//! Local ffmpeg helpers — concat, last-frame, and scene-cut extraction.
//!
//! ViMax used PySceneDetect ContentDetector to split transition videos and take
//! the first frame of scene 2. We approximate that with ffmpeg's built-in
//! `scene` filter; if no second scene is found we fall back to the last frame.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

use crate::error::{VimaxError, VimaxResult};

fn require_ffmpeg() -> VimaxResult<PathBuf> {
    nomi_config::resolve_ffmpeg_executable().ok_or_else(|| {
        VimaxError::Media(
            "ffmpeg not found — Allo will auto-install on first use; retry shortly".into(),
        )
    })
}

/// Concatenate ordered video clips with the ffmpeg concat demuxer → `out_path`.
pub async fn concat_videos(clip_paths: &[&Path], out_path: &Path) -> VimaxResult<()> {
    if clip_paths.is_empty() {
        return Err(VimaxError::Media("no clips to concatenate".into()));
    }
    let ffmpeg = require_ffmpeg()?;
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let list_path = out_path.with_extension("concat.txt");
    write_concat_list(&list_path, clip_paths).await?;

    let status = run_ffmpeg(
        &ffmpeg,
        &[
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            list_path.to_str().unwrap_or(""),
            "-c",
            "copy",
            out_path.to_str().unwrap_or(""),
        ],
    )
    .await?;

    let _ = tokio::fs::remove_file(&list_path).await;

    if status.success() {
        return Ok(());
    }

    let list_path2 = out_path.with_extension("concat2.txt");
    write_concat_list(&list_path2, clip_paths).await?;
    let status2 = run_ffmpeg(
        &ffmpeg,
        &[
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            list_path2.to_str().unwrap_or(""),
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-c:a",
            "aac",
            "-movflags",
            "+faststart",
            out_path.to_str().unwrap_or(""),
        ],
    )
    .await?;
    let _ = tokio::fs::remove_file(&list_path2).await;
    if !status2.success() {
        return Err(VimaxError::Media(format!(
            "ffmpeg concat failed (exit {:?})",
            status2.code()
        )));
    }
    Ok(())
}

async fn write_concat_list(list_path: &Path, clip_paths: &[&Path]) -> VimaxResult<()> {
    let mut list = String::new();
    for p in clip_paths {
        let abs = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = abs.to_string_lossy().replace('\'', "'\\''");
        list.push_str(&format!("file '{s}'\n"));
    }
    tokio::fs::write(list_path, list).await?;
    Ok(())
}

async fn run_ffmpeg(ffmpeg: &Path, args: &[&str]) -> VimaxResult<std::process::ExitStatus> {
    Command::new(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| VimaxError::Media(format!("ffmpeg spawn: {e}")))
}

/// Extract the last frame of `video_path` to PNG at `out_path`.
pub async fn extract_last_frame(video_path: &Path, out_path: &Path) -> VimaxResult<()> {
    let ffmpeg = require_ffmpeg()?;
    if !video_path.is_file() {
        return Err(VimaxError::Media(format!(
            "video missing: {}",
            video_path.display()
        )));
    }
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let out = out_path.to_str().unwrap_or("");
    let vin = video_path.to_str().unwrap_or("");

    let status = run_ffmpeg(
        &ffmpeg,
        &[
            "-y", "-sseof", "-0.1", "-i", vin, "-frames:v", "1", "-q:v", "2", out,
        ],
    )
    .await?;
    if status.success() && out_path.is_file() {
        return Ok(());
    }

    let status2 = run_ffmpeg(
        &ffmpeg,
        &["-y", "-i", vin, "-vf", "reverse", "-frames:v", "1", out],
    )
    .await?;
    if status2.success() && out_path.is_file() {
        return Ok(());
    }

    Err(VimaxError::Media(format!(
        "ffmpeg extract last frame failed for {}",
        video_path.display()
    )))
}

/// Extract the first frame of the *second* scene in a transition video.
///
/// Mirrors ViMax `get_new_camera_image`: ContentDetector → Scene-002 first frame,
/// else last frame of the whole clip.
pub async fn extract_new_camera_frame(video_path: &Path, out_path: &Path) -> VimaxResult<()> {
    let ffmpeg = require_ffmpeg()?;
    if !video_path.is_file() {
        return Err(VimaxError::Media(format!(
            "video missing: {}",
            video_path.display()
        )));
    }
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let cache = out_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("cache");
    tokio::fs::create_dir_all(&cache).await?;

    // Dump frames at scene cuts (threshold ~0.3 ≈ PySceneDetect ContentDetector default band).
    let pattern = cache.join("scene_%03d.png");
    let vin = video_path.to_str().unwrap_or("");
    let pat = pattern.to_str().unwrap_or("");
    let status = run_ffmpeg(
        &ffmpeg,
        &[
            "-y",
            "-i",
            vin,
            "-vf",
            "select='gt(scene\\,0.3)',showinfo",
            "-vsync",
            "vfr",
            "-q:v",
            "2",
            pat,
        ],
    )
    .await?;

    // Prefer the second scene-cut frame (index 002) if present — first cut is often
    // near t=0 or the start of scene 1; scene 2 starts at the next dump.
    let second = cache.join("scene_002.png");
    let first = cache.join("scene_001.png");
    if status.success() && second.is_file() {
        tokio::fs::copy(&second, out_path).await?;
        return Ok(());
    }
    // If only one cut frame exists past the start, still try scene_001 as weak signal.
    if status.success() && first.is_file() {
        // Probe: if we also have scene_000-less numbering starting at 001 only,
        // using first cut frame is closer to "new camera" than last frame when
        // the cut is mid-clip. Keep last-frame as ultimate fallback.
        if let Ok(meta) = tokio::fs::metadata(&first).await
            && meta.len() > 0
        {
            // Prefer last frame for single-cut ambiguity (matches ViMax else branch).
        }
    }

    extract_last_frame(video_path, out_path).await
}

/// Minimum size for a "usable" video artifact (filters empty / truncated downloads).
pub const MIN_USABLE_VIDEO_BYTES: u64 = 4096;

/// True when `path` exists and looks like a completed video download.
pub fn is_usable_video_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() >= MIN_USABLE_VIDEO_BYTES)
        .unwrap_or(false)
}

/// Sidecar path used for atomic downloads (`video.mp4` → `video.mp4.part`).
pub fn video_part_path(out_path: &Path) -> PathBuf {
    let mut s = out_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Write bytes to a `.part` file then rename into place (crash-safe resume).
pub async fn write_video_bytes_atomic(out_path: &Path, bytes: &[u8]) -> VimaxResult<()> {
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if (bytes.len() as u64) < MIN_USABLE_VIDEO_BYTES {
        return Err(VimaxError::Video(format!(
            "downloaded video too small ({} bytes) for {}",
            bytes.len(),
            out_path.display()
        )));
    }
    let part = video_part_path(out_path);
    tokio::fs::write(&part, bytes).await?;
    if out_path.exists() {
        let _ = tokio::fs::remove_file(out_path).await;
    }
    tokio::fs::rename(&part, out_path).await.map_err(|e| {
        VimaxError::Video(format!(
            "failed to finalize video {}: {e}",
            out_path.display()
        ))
    })?;
    Ok(())
}

/// Remove incomplete / too-small video artifacts so resume will regenerate them.
pub async fn scrub_unusable_video(path: &Path) -> VimaxResult<()> {
    let part = video_part_path(path);
    if part.exists() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    if path.exists() && !is_usable_video_file(path) {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn usable_video_requires_min_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.mp4");
        assert!(!is_usable_video_file(&path));
        std::fs::write(&path, vec![0u8; 100]).unwrap();
        assert!(!is_usable_video_file(&path));
        std::fs::write(&path, vec![0u8; MIN_USABLE_VIDEO_BYTES as usize]).unwrap();
        assert!(is_usable_video_file(&path));
    }

    #[test]
    fn part_path_appends_suffix() {
        let p = PathBuf::from("shots/3/video.mp4");
        assert_eq!(
            video_part_path(&p),
            PathBuf::from("shots/3/video.mp4.part")
        );
    }

    #[tokio::test]
    async fn atomic_write_replaces_safely() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mp4");
        let bytes = vec![1u8; MIN_USABLE_VIDEO_BYTES as usize];
        write_video_bytes_atomic(&path, &bytes).await.unwrap();
        assert!(is_usable_video_file(&path));
        assert!(!video_part_path(&path).exists());
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[9u8; 10]).unwrap();
        drop(f);
        assert!(!is_usable_video_file(&path));
        scrub_unusable_video(&path).await.unwrap();
        assert!(!path.exists());
    }
}

