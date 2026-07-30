//! Local ffmpeg helpers — concat, last-frame, and scene-cut extraction.
//!
//! ViMax used PySceneDetect ContentDetector to split transition videos and take
//! the first frame of scene 2. We approximate that with ffmpeg's built-in
//! `scene` filter; if no second scene is found we fall back to the last frame.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use tokio::process::Command;

use crate::error::{VimaxError, VimaxResult};

/// PNG / JPEG / WEBP magic — used to reject HTML error bodies saved as `.png`.
pub fn image_magic_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
        Some("png")
    } else if bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff {
        Some("jpeg")
    } else if bytes.len() >= 12
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
    {
        Some("webp")
    } else {
        None
    }
}

/// True when path exists and decodes as a real raster image (not HTML/JSON mislabeled as PNG).
pub fn is_usable_image_file(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if bytes.len() < 24 || image_magic_kind(&bytes).is_none() {
        return false;
    }
    image::load_from_memory(&bytes).is_ok()
}

/// Decode arbitrary image bytes (JPEG/PNG/WEBP) and write a real PNG to `out_path`.
/// Seedream often returns JPEG URLs while callers always use `.png` destinations.
pub fn write_image_bytes_as_png(bytes: &[u8], out_path: &Path) -> VimaxResult<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VimaxError::Media(e.to_string()))?;
    }
    if image_magic_kind(bytes).is_none() {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(80)]);
        return Err(VimaxError::Media(format!(
            "downloaded image is not PNG/JPEG/WEBP (head={head:?})"
        )));
    }
    let img = image::load_from_memory(bytes).map_err(|e| {
        VimaxError::Media(format!("decode image for {}: {e}", out_path.display()))
    })?;
    img.save_with_format(out_path, image::ImageFormat::Png)
        .map_err(|e| VimaxError::Media(format!("save png {}: {e}", out_path.display())))?;
    Ok(())
}

/// Sidecar path used for atomic image writes (`photo.png` → `photo.png.part`).
pub fn image_part_path(out_path: &Path) -> PathBuf {
    let mut s = out_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Decode image bytes, normalize to PNG via a `.part` sidecar, then rename into place.
pub fn write_image_bytes_atomic(bytes: &[u8], out_path: &Path) -> VimaxResult<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VimaxError::Media(e.to_string()))?;
    }
    let part = image_part_path(out_path);
    write_image_bytes_as_png(bytes, &part)?;
    if out_path.exists() {
        let _ = std::fs::remove_file(out_path);
    }
    std::fs::rename(&part, out_path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        VimaxError::Media(format!(
            "failed to finalize image {}: {e}",
            out_path.display()
        ))
    })?;
    Ok(())
}

/// Remove incomplete / unusable image artifacts so resume will regenerate them.
pub fn scrub_unusable_image(path: &Path) -> VimaxResult<()> {
    let part = image_part_path(path);
    if part.exists() {
        let _ = std::fs::remove_file(&part);
    }
    if path.exists() && !is_usable_image_file(path) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Tile reference images into one horizontal strip (fallback for single-slot img2img APIs).
/// Panel order should be: character bible(s) → empty set plate → prop/continuity.
pub fn compose_reference_strip(paths: &[&Path], out_path: &Path) -> VimaxResult<()> {
    if paths.is_empty() {
        return Err(VimaxError::Media("compose_reference_strip: no images".into()));
    }
    if paths.len() == 1 {
        // Normalize JPEG-as-.png (and similar) into a real PNG for downstream APIs.
        let bytes = std::fs::read(paths[0]).map_err(|e| VimaxError::Media(e.to_string()))?;
        if image_magic_kind(&bytes) == Some("png") {
            std::fs::write(out_path, &bytes).map_err(|e| VimaxError::Media(e.to_string()))?;
        } else {
            write_image_bytes_as_png(&bytes, out_path)?;
        }
        return Ok(());
    }

    const PANEL_H: u32 = 512;
    const GAP: u32 = 8;
    const MAX_PANELS: usize = 8;
    let mut panels: Vec<RgbaImage> = Vec::new();
    for p in paths.iter().take(MAX_PANELS) {
        let bytes = std::fs::read(p)
            .map_err(|e| VimaxError::Media(format!("read ref {}: {e}", p.display())))?;
        let img = image::load_from_memory(&bytes)
            .map_err(|e| {
                VimaxError::Media(format!(
                    "open ref {} ({} bytes, magic={:?}): {e}",
                    p.display(),
                    bytes.len(),
                    image_magic_kind(&bytes)
                ))
            })?
            .into_rgba8();
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            continue;
        }
        let new_w = ((w as f32) * (PANEL_H as f32) / (h as f32)).round().max(1.0) as u32;
        let resized = image::imageops::resize(&img, new_w, PANEL_H, FilterType::Triangle);
        panels.push(resized);
    }
    if panels.is_empty() {
        return Err(VimaxError::Media("compose_reference_strip: all panels empty".into()));
    }

    let total_w: u32 = panels.iter().map(|p| p.width()).sum::<u32>()
        + GAP * (panels.len().saturating_sub(1) as u32);
    let mut canvas = RgbaImage::from_pixel(total_w, PANEL_H, Rgba([24, 24, 28, 255]));
    let mut x = 0u32;
    for panel in &panels {
        image::imageops::overlay(&mut canvas, panel, x as i64, 0);
        x += panel.width() + GAP;
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VimaxError::Media(e.to_string()))?;
    }
    DynamicImage::ImageRgba8(canvas)
        .save(out_path)
        .map_err(|e| VimaxError::Media(format!("save strip {}: {e}", out_path.display())))?;
    Ok(())
}

fn require_ffmpeg() -> VimaxResult<PathBuf> {
    nomi_config::resolve_ffmpeg_executable().ok_or_else(|| {
        VimaxError::Media(
            "ffmpeg not found — Allo will auto-install on first use; retry shortly".into(),
        )
    })
}

fn ffprobe_executable(ffmpeg: &Path) -> PathBuf {
    ffmpeg
        .parent()
        .map(|dir| {
            #[cfg(windows)]
            {
                dir.join("ffprobe.exe")
            }
            #[cfg(not(windows))]
            {
                dir.join("ffprobe")
            }
        })
        .unwrap_or_else(|| PathBuf::from("ffprobe"))
}

/// Probe container duration in seconds (ffprobe preferred; ffmpeg `-i` fallback).
async fn probe_duration_secs(ffmpeg: &Path, input: &Path) -> Option<f64> {
    let ffprobe = ffprobe_executable(ffmpeg);
    if let Ok(output) = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
    {
        if output.status.success() {
            if let Some(d) = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|d| *d > 0.0 && d.is_finite())
            {
                return Some(d);
            }
        }
    }

    // essentials installs sometimes only expose ffmpeg.exe on PATH — parse banner.
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(input)
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;
    let err = String::from_utf8_lossy(&output.stderr);
    parse_ffmpeg_duration_banner(&err)
}

fn parse_ffmpeg_duration_banner(stderr: &str) -> Option<f64> {
    // Duration: 00:00:12.48, start: 0.000000, bitrate: ...
    let marker = "Duration: ";
    let idx = stderr.find(marker)?;
    let rest = &stderr[idx + marker.len()..];
    let token = rest.split(',').next()?.trim();
    let mut parts = token.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    let total = h * 3600.0 + m * 60.0 + s;
    (total > 0.0 && total.is_finite()).then_some(total)
}

/// Concatenate ordered video clips → `out_path`.
///
/// Per-clip normalize forces identical CFR video + stereo AAC of **equal**
/// duration (audio padded/trimmed to the video length). Final join uses the
/// `concat` **filter** (not the concat demuxer): demuxer stitches A/V as
/// independent streams, so short audio on early shots makes the last shot's
/// picture play over silence even when that shot's own file has sound.
pub async fn concat_videos(clip_paths: &[&Path], out_path: &Path) -> VimaxResult<()> {
    if clip_paths.is_empty() {
        return Err(VimaxError::Media("no clips to concatenate".into()));
    }
    let ffmpeg = require_ffmpeg()?;
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let norm_dir = out_path.with_extension("concat_norm");
    let _ = tokio::fs::remove_dir_all(&norm_dir).await;
    tokio::fs::create_dir_all(&norm_dir).await?;

    let mut normalized: Vec<PathBuf> = Vec::with_capacity(clip_paths.len());
    for (i, clip) in clip_paths.iter().enumerate() {
        let dest = norm_dir.join(format!("{i:03}.mp4"));
        normalize_clip_for_concat(&ffmpeg, clip, &dest).await?;
        normalized.push(dest);
    }

    let out = out_path.to_str().unwrap_or("");
    let n = normalized.len();

    // filter_complex concat keeps each shot's A/V pair locked together.
    let mut filter = String::new();
    for i in 0..n {
        filter.push_str(&format!("[{i}:v:0][{i}:a:0]"));
    }
    filter.push_str(&format!("concat=n={n}:v=1:a=1[v][a]"));

    let mut args: Vec<String> = vec!["-y".into()];
    for p in &normalized {
        args.push("-i".into());
        args.push(p.to_string_lossy().into_owned());
    }
    args.extend([
        "-filter_complex".into(),
        filter,
        "-map".into(),
        "[v]".into(),
        "-map".into(),
        "[a]".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "medium".into(),
        "-crf".into(),
        "18".into(),
        "-c:a".into(),
        "aac".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "2".into(),
        "-movflags".into(),
        "+faststart".into(),
        out.into(),
    ]);

    let status = run_ffmpeg_owned(&ffmpeg, &args).await?;
    let _ = tokio::fs::remove_dir_all(&norm_dir).await;

    if !status.success() {
        return Err(VimaxError::Media(format!(
            "ffmpeg concat filter failed (exit {:?})",
            status.code()
        )));
    }
    Ok(())
}

/// Re-encode one shot to CFR 24fps video + stereo AAC matched to video length.
async fn normalize_clip_for_concat(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
) -> VimaxResult<()> {
    let input_s = input.to_str().unwrap_or("");
    let output_s = output.to_str().unwrap_or("");
    let dur = probe_duration_secs(ffmpeg, input).await.unwrap_or(0.0);
    let dur_arg = if dur > 0.05 {
        format!("{dur:.3}")
    } else {
        String::new()
    };
    // whole_dur keeps AAC from ending early (dialogue often shorter than picture).
    let af = if dur > 0.05 {
        format!(
            "aresample=44100:async=1,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS,apad=whole_dur={dur:.3}"
        )
    } else {
        "aresample=44100:async=1,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS,apad"
            .to_string()
    };

    // Path A: clip already has an audio stream.
    let mut args: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        input_s.into(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a:0".into(),
        "-vf".into(),
        "fps=24,format=yuv420p,setpts=PTS-STARTPTS".into(),
        "-af".into(),
        af,
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "18".into(),
        "-c:a".into(),
        "aac".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "2".into(),
        "-movflags".into(),
        "+faststart".into(),
    ];
    if !dur_arg.is_empty() {
        args.push("-t".into());
        args.push(dur_arg.clone());
    } else {
        args.push("-shortest".into());
    }
    args.push(output_s.into());

    let status = run_ffmpeg_owned(ffmpeg, &args).await?;
    if status.success() && is_usable_video_file(output) {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(output).await;

    // Path B: missing/broken audio — synthesize stereo silence for the video duration.
    tracing::warn!(
        clip = %input.display(),
        "normalize_clip: no usable audio; padding with silence"
    );
    let mut args2: Vec<String> = vec![
        "-y".into(),
        "-i".into(),
        input_s.into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "anullsrc=channel_layout=stereo:sample_rate=44100".into(),
        "-vf".into(),
        "fps=24,format=yuv420p,setpts=PTS-STARTPTS".into(),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "18".into(),
        "-c:a".into(),
        "aac".into(),
        "-ar".into(),
        "44100".into(),
        "-ac".into(),
        "2".into(),
        "-movflags".into(),
        "+faststart".into(),
    ];
    if !dur_arg.is_empty() {
        args2.push("-t".into());
        args2.push(dur_arg);
    } else {
        args2.push("-shortest".into());
    }
    args2.push(output_s.into());

    let status2 = run_ffmpeg_owned(ffmpeg, &args2).await?;
    if !status2.success() || !is_usable_video_file(output) {
        return Err(VimaxError::Media(format!(
            "ffmpeg normalize failed for {} (exit {:?})",
            input.display(),
            status2.code()
        )));
    }
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

async fn run_ffmpeg_owned(
    ffmpeg: &Path,
    args: &[String],
) -> VimaxResult<std::process::ExitStatus> {
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

    #[test]
    fn compose_strip_writes_png() {
        use image::{Rgb, RgbImage};
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        RgbImage::from_pixel(40, 30, Rgb([255, 0, 0]))
            .save(&a)
            .unwrap();
        RgbImage::from_pixel(50, 20, Rgb([0, 255, 0]))
            .save(&b)
            .unwrap();
        let out = dir.path().join("strip.png");
        compose_reference_strip(&[a.as_path(), b.as_path()], &out).unwrap();
        assert!(out.exists());
        let img = image::open(&out).unwrap();
        assert_eq!(img.height(), 512);
        assert!(img.width() > 40);
    }

    #[test]
    fn jpeg_bytes_saved_as_png_extension_still_compose() {
        use image::{ImageFormat, Rgb, RgbImage};
        let dir = tempfile::tempdir().unwrap();
        let jpeg_as_png = dir.path().join("three_view.png");
        let mut jpeg_bytes = Vec::new();
        RgbImage::from_pixel(32, 24, Rgb([10, 20, 30]))
            .write_to(&mut std::io::Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(image_magic_kind(&jpeg_bytes), Some("jpeg"));
        std::fs::write(&jpeg_as_png, &jpeg_bytes).unwrap();
        assert!(is_usable_image_file(&jpeg_as_png));

        let out = dir.path().join("normalized.png");
        write_image_bytes_as_png(&jpeg_bytes, &out).unwrap();
        assert_eq!(
            image_magic_kind(&std::fs::read(&out).unwrap()),
            Some("png")
        );

        let strip = dir.path().join("strip.png");
        compose_reference_strip(&[jpeg_as_png.as_path(), out.as_path()], &strip).unwrap();
        assert!(strip.exists());
    }

    #[test]
    fn rejects_html_as_image() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fake.png");
        std::fs::write(&p, b"<html>error</html>").unwrap();
        assert!(!is_usable_image_file(&p));
        assert!(image_magic_kind(b"<html>error</html>").is_none());
    }

    #[test]
    fn atomic_image_write_and_scrub() {
        use image::{ImageFormat, Rgb, RgbImage};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        let mut jpeg = Vec::new();
        RgbImage::from_pixel(8, 8, Rgb([9, 8, 7]))
            .write_to(&mut std::io::Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        write_image_bytes_atomic(&jpeg, &path).unwrap();
        assert!(is_usable_image_file(&path));
        assert!(!image_part_path(&path).exists());
        assert_eq!(
            image_magic_kind(&std::fs::read(&path).unwrap()),
            Some("png")
        );
        std::fs::write(&path, b"<html>").unwrap();
        scrub_unusable_image(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn parses_ffmpeg_duration_banner() {
        let banner = "Input #0, mov, from 'x.mp4':\n  Duration: 00:00:12.48, start: 0.000000, bitrate: 1200 kb/s\n";
        assert!((parse_ffmpeg_duration_banner(banner).unwrap() - 12.48).abs() < 0.01);
    }

    /// Early clips with audio shorter than video used to exhaust the audio
    /// timeline under concat-demuxer, leaving the last shot silent. Reproduce
    /// that shape of inputs and assert the final segment still has energy.
    #[tokio::test]
    async fn concat_keeps_audio_on_last_shot_when_early_audio_is_short() {
        let Ok(ffmpeg) = require_ffmpeg() else {
            eprintln!("skip: ffmpeg not available");
            return;
        };
        let dir = tempfile::tempdir().unwrap();

        // Clip 0: 3s video, only 1.2s of tone (audio shorter than picture).
        // Do NOT use -shortest — that would truncate the whole file to 1.2s.
        let c0 = dir.path().join("0.mp4");
        let st0 = run_ffmpeg(
            &ffmpeg,
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=320x240:d=3:r=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1.2",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                c0.to_str().unwrap(),
            ],
        )
        .await
        .unwrap();
        assert!(st0.success());

        // Clip 1 (last): 3s video + full 3s tone — individually has sound.
        let c1 = dir.path().join("1.mp4");
        let st1 = run_ffmpeg(
            &ffmpeg,
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=320x240:d=3:r=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:duration=3",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                c1.to_str().unwrap(),
            ],
        )
        .await
        .unwrap();
        assert!(st1.success());

        let out = dir.path().join("final.mp4");
        concat_videos(&[c0.as_path(), c1.as_path()], &out)
            .await
            .expect("concat");
        assert!(is_usable_video_file(&out));

        // Sample the last ~2s of the film; mean_volume must not be -inf.
        let detect = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-ss",
                "4",
                "-t",
                "2",
                "-i",
            ])
            .arg(&out)
            .args(["-af", "volumedetect", "-f", "null", "-"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .unwrap();
        let log = String::from_utf8_lossy(&detect.stderr);
        assert!(
            !log.contains("mean_volume: -inf"),
            "last-shot audio silent after concat:\n{log}"
        );
        let mean = log
            .lines()
            .find(|l| l.contains("mean_volume:"))
            .expect("volumedetect mean_volume");
        // Tone should be well above digital silence.
        assert!(
            !mean.contains("-91.") && !mean.contains("-inf"),
            "unexpected mean_volume line: {mean}"
        );
    }
}

