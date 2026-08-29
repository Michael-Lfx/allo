//! Local ffmpeg helpers — concat, last-frame, and scene-cut extraction.
//!
//! ViMax used PySceneDetect ContentDetector to split transition videos and take
//! the first frame of scene 2. We approximate that with ffmpeg's built-in
//! `scene` filter; if no second scene is found we fall back to the last frame.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use tokio::process::Command;

use crate::error::{VimaxError, VimaxResult};

/// Spawn ffmpeg/ffprobe without flashing a console on Windows GUI hosts.
fn ffmpeg_command(bin: impl AsRef<Path>) -> Command {
    let mut cmd = Command::new(bin.as_ref());
    // CREATE_NO_WINDOW (0x08000000): Allo is a GUI app; bare `Command::new(ffmpeg)`
    // otherwise pops a CMD window on every last-frame extract / concat.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

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

/// Longest side for multimodal payloads — full film stills (10–25MB) hang or OOM vision calls.
pub const VISION_THUMB_MAX_SIDE: u32 = 768;
/// Skip re-encode when the source is already a small JPEG/WebP/PNG.
const VISION_SKIP_REENCODE_BYTES: usize = 400 * 1024;

/// Decode + JPEG-thumbnail bytes for vision chat (keeps payloads small and fast).
pub fn jpeg_thumb_bytes_for_vision(bytes: &[u8]) -> VimaxResult<Vec<u8>> {
    if bytes.len() <= VISION_SKIP_REENCODE_BYTES && image_magic_kind(bytes) == Some("jpeg") {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| {
        VimaxError::Media(format!("decode image for vision thumb: {e}"))
    })?;
    let thumb = img.thumbnail(VISION_THUMB_MAX_SIDE, VISION_THUMB_MAX_SIDE);
    let mut out = std::io::Cursor::new(Vec::new());
    thumb
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| VimaxError::Media(format!("encode vision jpeg thumb: {e}")))?;
    Ok(out.into_inner())
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

/// Sidecar path used for atomic image writes (`photo.png` → `.photo.png.part-123.png`).
///
/// Keep a real `.png` suffix so decoders/savers never infer format from `.part` alone,
/// and include the pid to avoid collisions when two cameos overwrite the same dest.
pub fn image_part_path(out_path: &Path) -> PathBuf {
    let parent = out_path.parent().unwrap_or_else(|| Path::new("."));
    let name = out_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("image.png");
    parent.join(format!(".{name}.part-{}.png", std::process::id()))
}

/// Decode image bytes, normalize to PNG via a `.part` sidecar, then rename into place.
pub fn write_image_bytes_atomic(bytes: &[u8], out_path: &Path) -> VimaxResult<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VimaxError::Media(e.to_string()))?;
    }
    let part = image_part_path(out_path);
    write_image_bytes_as_png(bytes, &part)?;
    replace_file_atomic(&part, out_path)
}

/// Copy an on-disk image into place without re-encoding (cameo photos are already PNG).
pub fn copy_image_file_atomic(src: &Path, out_path: &Path) -> VimaxResult<()> {
    if !src.is_file() {
        return Err(VimaxError::Media(format!(
            "copy source missing: {}",
            src.display()
        )));
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VimaxError::Media(e.to_string()))?;
    }
    let part = image_part_path(out_path);
    std::fs::copy(src, &part).map_err(|e| {
        VimaxError::Media(format!(
            "copy {} → {}: {e}",
            src.display(),
            part.display()
        ))
    })?;
    replace_file_atomic(&part, out_path)
}

/// Move `part` onto `out_path`, with a Windows-friendly copy fallback.
fn replace_file_atomic(part: &Path, out_path: &Path) -> VimaxResult<()> {
    if !part.is_file() {
        return Err(VimaxError::Media(format!(
            "failed to finalize image {}: part sidecar missing at {}",
            out_path.display(),
            part.display()
        )));
    }
    if out_path.exists() {
        // Best-effort; Windows may still block replace if a preview has the file open.
        let _ = std::fs::remove_file(out_path);
    }
    match std::fs::rename(part, out_path) {
        Ok(()) => Ok(()),
        Err(rename_err) => match std::fs::copy(part, out_path) {
            Ok(_) => {
                let _ = std::fs::remove_file(part);
                Ok(())
            }
            Err(copy_err) => {
                let _ = std::fs::remove_file(part);
                Err(VimaxError::Media(format!(
                    "failed to finalize image {}: rename={rename_err}; copy={copy_err}",
                    out_path.display()
                )))
            }
        },
    }
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

fn ffmpeg_missing_error(detail: Option<String>) -> VimaxError {
    match detail {
        Some(d) => VimaxError::Media(format!(
            "ffmpeg not found and auto-install failed: {d}. \
             Install ffmpeg on PATH or retry when network mirrors are reachable."
        )),
        None => VimaxError::Media(
            "ffmpeg not found — enable NOMIFUN_AUTO_ENSURE_DEPS (default: on) \
             or install ffmpeg on PATH"
                .into(),
        ),
    }
}

/// Resolve ffmpeg, downloading into Allo's managed `bin/` when missing.
///
/// Unlike a bare PATH check, this actually awaits [`nomi_config::ensure_ffmpeg`]
/// so ViMax scene render / last-frame extract do not fail with a misleading
/// "retry shortly" while nothing is installing.
async fn ensure_ffmpeg_ready() -> VimaxResult<PathBuf> {
    if let Some(path) = nomi_config::resolve_ffmpeg_executable() {
        return Ok(path);
    }

    if !nomi_config::auto_ensure_enabled() {
        return Err(ffmpeg_missing_error(None));
    }

    tracing::info!("ffmpeg missing; Allo is installing it into the managed bin directory");
    match nomi_config::ensure_ffmpeg(false).await {
        Ok(path) => Ok(path),
        Err(e) => Err(ffmpeg_missing_error(Some(e.to_string()))),
    }
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

/// Probe a local video's duration in seconds (ffprobe, then ffmpeg `-i` fallback).
pub async fn probe_media_duration_secs(path: &Path) -> Option<f64> {
    let ffmpeg = ensure_ffmpeg_ready().await.ok()?;
    probe_duration_secs(&ffmpeg, path).await
}

/// Probe container duration in seconds (ffprobe preferred; ffmpeg `-i` fallback).
async fn probe_duration_secs(ffmpeg: &Path, input: &Path) -> Option<f64> {
    let ffprobe = ffprobe_executable(ffmpeg);
    if let Ok(output) = ffmpeg_command(&ffprobe)
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
    let output = ffmpeg_command(ffmpeg)
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
    let ffmpeg = ensure_ffmpeg_ready().await?;
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let norm_dir = out_path.with_extension("concat_norm");
    let _ = tokio::fs::remove_dir_all(&norm_dir).await;
    tokio::fs::create_dir_all(&norm_dir).await?;

    // One film canvas: session aspect, sized so every clip downscales (never
    // upscales). Same-ratio shots just shrink; a rogue portrait/landscape shot
    // letterboxes instead of hijacking the whole film's orientation.
    let (canvas_w, canvas_h) = concat_target_size(&ffmpeg, clip_paths, out_path).await;
    tracing::info!(
        canvas_w,
        canvas_h,
        clips = clip_paths.len(),
        "vimax concat canvas"
    );

    let mut normalized: Vec<PathBuf> = Vec::with_capacity(clip_paths.len());
    {
        // Finite parallelism per encoder (NVENC consumer GPUs cap sessions at
        // ~3-5) — near-linear speedup for N clips without starving the encode.
        let plan = nomi_config::ffmpeg_hw::select_video_encode_plan(&ffmpeg).await;
        let parallelism = nomi_config::ffmpeg_hw::recommended_parallelism(&plan);
        let sem = Arc::new(tokio::sync::Semaphore::new(parallelism));
        let mut set = tokio::task::JoinSet::new();
        for (i, clip) in clip_paths.iter().enumerate() {
            let ffmpeg = ffmpeg.clone();
            let input = (*clip).to_path_buf();
            let dest = norm_dir.join(format!("{i:03}.mp4"));
            let permit = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = permit.acquire_owned().await.map_err(|_| {
                    VimaxError::Media("normalize semaphore closed".into())
                })?;
                normalize_clip_for_concat(&ffmpeg, &input, &dest, canvas_w, canvas_h)
                    .await
                    .map(|_| i)
            });
        }
        let mut slots: Vec<Option<PathBuf>> = (0..clip_paths.len()).map(|_| None).collect();
        while let Some(joined) = set.join_next().await {
            let i = joined
                .map_err(|e| VimaxError::Media(format!("normalize join: {e}")))??;
            slots[i] = Some(norm_dir.join(format!("{i:03}.mp4")));
        }
        for slot in slots {
            normalized.push(slot.expect("every spawned normalize joins exactly once"));
        }
    }

    let out = out_path.to_str().unwrap_or("");
    let n = normalized.len();

    // Fast path: normalized clips already share one video codec/params and each
    // has audio padded to its own video length (normalize does this), so the
    // concat DEMUXER + stream copy is safe and turns the final join into a
    // seconds-level remux. Falls back to the filter re-encode below on any
    // mismatch or validation failure.
    let copy_ok = try_remux_concat(&ffmpeg, &normalized, out_path).await?;
    if copy_ok {
        let _ = tokio::fs::remove_dir_all(&norm_dir).await;
        tracing::info!(out = %out_path.display(), "vimax concat via stream copy");
        return Ok(());
    }
    let _ = tokio::fs::remove_file(out_path).await;

    // Same hardware encode plan as per-clip normalize (cached, one recipe). A
    // hardware plan that fails at runtime retries once with libx264.
    let plan = nomi_config::ffmpeg_hw::select_video_encode_plan(&ffmpeg).await;
    let fallback = nomi_config::ffmpeg_hw::software_fallback_plan(&plan);

    // filter_complex concat keeps each shot's A/V pair locked together.
    // Re-assert fps/SAR/timebase here (size already unified in normalize).
    // Parallel HW normalize can still leave mixed timebases which concat rejects.
    let build_filter_args = |p: &nomi_config::ffmpeg_hw::VideoEncodePlan| -> Vec<String> {
        let mut filter = String::new();
        for i in 0..n {
            filter.push_str(&format!(
                "[{i}:v:0]fps=24,setsar=1,format=yuv420p,setpts=PTS-STARTPTS[v{i}];[{i}:a:0]aresample=44100:async=1,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS[a{i}];"
            ));
        }
        for i in 0..n {
            filter.push_str(&format!("[v{i}][a{i}]"));
        }
        filter.push_str(&format!("concat=n={n}:v=1:a=1[v][a]"));
        let mut map_v = "[v]";
        if let Some(vf) = p.hwupload_vf {
            filter.push_str(&format!(";[v]{vf}[vh]"));
            map_v = "[vh]";
        }

        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
        ];
        args.extend(p.input_args.iter().map(|s| (*s).to_string()));
        for norm in &normalized {
            args.push("-i".into());
            args.push(norm.to_string_lossy().into_owned());
        }
        args.extend([
            "-filter_complex".into(),
            filter,
            "-map".into(),
            map_v.into(),
            "-map".into(),
            "[a]".into(),
        ]);
        args.extend(p.encode_args().iter().map(|s| (*s).to_string()));
        args.extend([
            "-r".into(),
            "24".into(),
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
        args
    };

    let mut last_hint: Option<String> = None;
    for (label, p) in [("hw", &plan), ("sw", &fallback)] {
        let args = build_filter_args(p);
        let (ok, err) = run_ffmpeg_owned_capture(&ffmpeg, &args).await?;
        if ok {
            let _ = tokio::fs::remove_dir_all(&norm_dir).await;
            tracing::info!(out = %out_path.display(), encoder = p.codec, "vimax concat filter re-encode ({label})");
            return Ok(());
        }
        last_hint = ffmpeg_stderr_hint(&err).or_else(|| {
            let trimmed = err.trim();
            (!trimmed.is_empty()).then(|| trimmed.chars().take(240).collect())
        });
        tracing::warn!(
            label,
            hint = last_hint.as_deref().unwrap_or(""),
            "ffmpeg concat filter failed; retrying with software encoder"
        );
        let _ = tokio::fs::remove_file(out_path).await;
        if !p.uses_hw {
            break;
        }
    }

    let _ = tokio::fs::remove_dir_all(&norm_dir).await;
    Err(VimaxError::Media(format!(
        "ffmpeg concat filter failed for {}{}",
        out_path.display(),
        last_hint
            .map(|d| format!(" — ffmpeg: {d}"))
            .unwrap_or_default()
    )))
}

fn even_dim(n: u32) -> u32 {
    (n & !1).max(2)
}

/// Largest even WxH of aspect `aw:ah` that fits inside `src` (never upscales).
fn max_aspect_rect_inside(src_w: u32, src_h: u32, aw: u32, ah: u32) -> (u32, u32) {
    if src_w < 2 || src_h < 2 || aw == 0 || ah == 0 {
        return (2, 2);
    }
    let (w, h) = if (src_w as u64).saturating_mul(ah as u64)
        <= (src_h as u64).saturating_mul(aw as u64)
    {
        let h = ((src_w as u64).saturating_mul(ah as u64) / aw as u64) as u32;
        (src_w, h)
    } else {
        let w = ((src_h as u64).saturating_mul(aw as u64) / ah as u64) as u32;
        (w, src_h)
    };
    (even_dim(w), even_dim(h))
}

/// Film canvas: session aspect, limited by the smallest clip after fitting
/// that aspect inside it. Same-ratio 1080p+720p → 720p; a 9:16 stray in a
/// 16:9 film letterboxes instead of turning the concat vertical.
fn concat_canvas_for_aspect(sizes: &[(u32, u32)], aspect: &str) -> (u32, u32) {
    let (aw, ah) = crate::aspect::aspect_parts(aspect);
    sizes
        .iter()
        .copied()
        .filter(|(w, h)| *w >= 2 && *h >= 2)
        .map(|(w, h)| max_aspect_rect_inside(w, h, aw, ah))
        .min_by_key(|(w, h)| w.saturating_mul(*h))
        .unwrap_or_else(|| {
            let (w, h) = crate::aspect::aspect_to_upload_dims(aspect);
            (even_dim(w), even_dim(h))
        })
}

async fn concat_target_size(ffmpeg: &Path, clips: &[&Path], out_path: &Path) -> (u32, u32) {
    let aspect = match out_path.parent() {
        Some(dir) => crate::aspect::load_aspect_from_dir(dir).await,
        None => crate::aspect::DEFAULT_ASPECT_RATIO.to_string(),
    };
    let mut sizes = Vec::new();
    for clip in clips {
        let Some(sig) = nomi_config::ffmpeg_hw::probe_stream_signature(ffmpeg, clip).await else {
            continue;
        };
        sizes.push((sig.width, sig.height));
    }
    concat_canvas_for_aspect(&sizes, &aspect)
}

fn concat_normalize_vf(p: &nomi_config::ffmpeg_hw::VideoEncodePlan, w: u32, h: u32) -> String {
    let geometry = format!(
        "fps=24,scale={w}:{h}:force_original_aspect_ratio=decrease:flags=bicubic,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1"
    );
    if p.hwupload_vf.is_some() {
        format!("{geometry},format=nv12,hwupload,setpts=PTS-STARTPTS")
    } else {
        format!("{geometry},format=yuv420p,setpts=PTS-STARTPTS")
    }
}

/// Concat demuxer list entry. Forward slashes avoid Windows `file 'C:\foo'`
/// treating `\f` / `\U` as escapes.
fn ffmpeg_concat_list_line(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "/")
        .replace('\'', r"'\''");
    format!("file '{escaped}'\n")
}

/// Try joining `clips` with the concat demuxer + full stream copy. Returns true
/// when the output is a plausible join (duration ≈ sum of clips); the caller
/// falls back to a filter re-encode otherwise. Cleanup of a partial output is
/// the caller's job.
async fn try_remux_concat(ffmpeg: &Path, clips: &[PathBuf], out_path: &Path) -> VimaxResult<bool> {
    // Pre-check: per-clip HW→SW fallback can leave a mixed-codec set (some shots
    // NVENC, one libx264) — the copy join requires one uniform signature.
    if !nomi_config::ffmpeg_hw::streams_uniform(ffmpeg, clips).await {
        tracing::info!("normalized clips are not stream-uniform; skipping copy concat");
        return Ok(false);
    }
    let list_path = out_path.with_extension("concat_remux_list.txt");
    let mut list_body = String::new();
    for p in clips {
        list_body.push_str(&ffmpeg_concat_list_line(p));
    }
    let _ = tokio::fs::write(&list_path, &list_body).await;

    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list_path.to_string_lossy().into_owned(),
        "-c".into(),
        "copy".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-y".into(),
        out_path.to_string_lossy().into_owned(),
    ];
    let status = run_ffmpeg_owned(ffmpeg, &args).await?;
    let _ = tokio::fs::remove_file(&list_path).await;
    if !status.success() {
        return Ok(false);
    }
    if !is_usable_video_file(out_path) {
        return Ok(false);
    }
    // Duration ≈ sum of clip durations guards against silent mis-joins.
    let mut expected = 0.0f64;
    let mut probed = 0usize;
    let mut set = tokio::task::JoinSet::new();
    for clip in clips {
        let ffmpeg = ffmpeg.to_path_buf();
        let clip = clip.clone();
        set.spawn(async move { probe_duration_secs(&ffmpeg, &clip).await });
    }
    while let Some(joined) = set.join_next().await {
        if let Ok(Some(d)) = joined {
            expected += d;
            probed += 1;
        }
    }
    let Some(out_dur) = probe_duration_secs(ffmpeg, out_path).await else {
        return Ok(false);
    };
    if probed != clips.len() || expected <= 0.0 {
        return Ok(false);
    }
    Ok((out_dur - expected).abs() <= (expected * 0.01).max(0.5))
}

/// Soft fade at clip edges so hard cuts do not click / jump in BGM loudness.
/// Long enough to soften Seedance per-shot underscore discontinuities, short
/// enough to avoid chewing the last spoken syllable.
pub const CONCAT_AUDIO_EDGE_FADE_SECS: f64 = 0.5;

/// Build the per-clip audio filter used before concat.
///
/// - Normalize sample rate / layout
/// - Pad audio to the full video duration
/// - Soft fade in/out at edges (when the clip is long enough)
fn normalize_audio_filter(dur: f64) -> String {
    let base = if dur > 0.05 {
        format!(
            "aresample=44100:async=1,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS,apad=whole_dur={dur:.3}"
        )
    } else {
        "aresample=44100:async=1,aformat=sample_fmts=fltp:channel_layouts=stereo,asetpts=PTS-STARTPTS,apad"
            .to_string()
    };
    let fade = CONCAT_AUDIO_EDGE_FADE_SECS;
    if dur > fade * 2.0 + 0.05 {
        let out_st = dur - fade;
        format!(
            "{base},afade=t=in:st=0:d={fade:.3},afade=t=out:st={out_st:.3}:d={fade:.3}"
        )
    } else {
        base
    }
}

/// Re-encode one shot to CFR 24fps video + stereo AAC matched to video length,
/// scaled/padded onto a shared even canvas so concat can join them.
async fn normalize_clip_for_concat(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    canvas_w: u32,
    canvas_h: u32,
) -> VimaxResult<()> {
    let input_s = input.to_str().unwrap_or("");
    let output_s = output.to_str().unwrap_or("");
    if !is_usable_video_file(input) {
        let _ = scrub_unusable_video(input).await;
        return Err(VimaxError::Media(format!(
            "concat input is not a valid video container (removed so resume can regenerate): {}",
            input.display()
        )));
    }
    let dur = probe_duration_secs(ffmpeg, input).await.unwrap_or(0.0);
    let dur_arg = if dur > 0.05 {
        format!("{dur:.3}")
    } else {
        String::new()
    };
    // whole_dur keeps AAC from ending early (dialogue often shorter than picture).
    // Edge afade softens BGM/volume jumps at shot boundaries without changing duration.
    let af = normalize_audio_filter(dur);

    // Same hardware plan as the final join — one probe per process, one recipe.
    // A hardware plan that fails at runtime (full NVENC session, driver hiccup)
    // retries the same input with libx264 before giving up.
    let plan = nomi_config::ffmpeg_hw::select_video_encode_plan(ffmpeg).await;
    let fallback = nomi_config::ffmpeg_hw::software_fallback_plan(&plan);

    // The `-vf` chain depends on the ACTUAL plan: VAAPI needs an hwupload stage
    // (nv12 → GPU), everything else takes yuv420p software frames. Computing it
    // per plan keeps the software fallback from inheriting VAAPI's hwupload.
    let normalize_vf = |p: &nomi_config::ffmpeg_hw::VideoEncodePlan| -> String {
        concat_normalize_vf(p, canvas_w, canvas_h)
    };

    // Path A: clip already has an audio stream.
    let build_a = |p: &nomi_config::ffmpeg_hw::VideoEncodePlan, vf: &str| -> Vec<String> {
        let mut args: Vec<String> = vec!["-y".into()];
        // Input-side options (e.g. `-vaapi_device`) must precede `-i`.
        args.extend(p.input_args.iter().map(|s| (*s).to_string()));
        args.extend([
            "-i".into(),
            input_s.into(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "0:a:0".into(),
            "-vf".into(),
            vf.to_string(),
            "-af".into(),
            af.clone(),
        ]);
        args.extend(p.encode_args().iter().map(|s| (*s).to_string()));
        args.extend([
            "-r".into(),
            "24".into(),
            "-c:a".into(),
            "aac".into(),
            "-ar".into(),
            "44100".into(),
            "-ac".into(),
            "2".into(),
            "-movflags".into(),
            "+faststart".into(),
        ]);
        if !dur_arg.is_empty() {
            args.push("-t".into());
            args.push(dur_arg.clone());
        } else {
            args.push("-shortest".into());
        }
        args.push(output_s.into());
        args
    };

    // Path B: missing/broken audio — synthesize stereo silence for the video duration.
    let build_b = |p: &nomi_config::ffmpeg_hw::VideoEncodePlan, vf: &str| -> Vec<String> {
        let mut args: Vec<String> = vec!["-y".into()];
        args.extend(p.input_args.iter().map(|s| (*s).to_string()));
        args.extend([
            "-i".into(),
            input_s.into(),
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "anullsrc=channel_layout=stereo:sample_rate=44100".into(),
            "-vf".into(),
            vf.to_string(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "1:a:0".into(),
        ]);
        args.extend(p.encode_args().iter().map(|s| (*s).to_string()));
        args.extend([
            "-r".into(),
            "24".into(),
            "-c:a".into(),
            "aac".into(),
            "-ar".into(),
            "44100".into(),
            "-ac".into(),
            "2".into(),
            "-movflags".into(),
            "+faststart".into(),
        ]);
        if !dur_arg.is_empty() {
            args.push("-t".into());
            args.push(dur_arg.clone());
        } else {
            args.push("-shortest".into());
        }
        args.push(output_s.into());
        args
    };

    let mut attempts: Vec<(String, String)> = Vec::new();
    // Path A: clip already has an audio stream.
    for (plan_label, p) in [("plan", &plan), ("sw", &fallback)] {
        let vf = normalize_vf(p);
        let args = build_a(p, &vf);
        let (ok, err) = run_ffmpeg_owned_capture(ffmpeg, &args).await?;
        if ok && is_usable_video_file(output) {
            return Ok(());
        }
        attempts.push((format!("path_a_{plan_label}"), err));
        let _ = tokio::fs::remove_file(output).await;
        // Software is the last resort for this audio path — no third try.
        if !p.uses_hw {
            break;
        }
    }
    tracing::warn!(
        clip = %input.display(),
        "normalize_clip: audio path failed; padding with silence"
    );
    // Path B: missing/broken audio — synthesize stereo silence for the video duration.
    for (plan_label, p) in [("plan", &plan), ("sw", &fallback)] {
        let vf = normalize_vf(p);
        let args = build_b(p, &vf);
        let (ok, err) = run_ffmpeg_owned_capture(ffmpeg, &args).await?;
        if ok && is_usable_video_file(output) {
            return Ok(());
        }
        attempts.push((format!("path_b_{plan_label}"), err));
        let _ = tokio::fs::remove_file(output).await;
        if !p.uses_hw {
            break;
        }
    }

    // Corrupt / truncated downloads often pass the old size-only check and only
    // fail here (ffmpeg AVERROR_INVALIDDATA = -1094995529). Drop the bad file
    // so "resume from checkpoint" regenerates the scene instead of looping.
    let detail = attempts
        .iter()
        .find_map(|(_, err)| ffmpeg_stderr_hint(err))
        .or_else(|| attempts.last().map(|(_, err)| err.as_str().to_string()));
    let looks_corrupt = detail
        .as_deref()
        .is_some_and(|s| {
            let lower = s.to_ascii_lowercase();
            lower.contains("invalid data")
                || lower.contains("moov atom not found")
                || lower.contains("error while decoding")
                || lower.contains("could not find codec")
        });
    if looks_corrupt || !video_file_has_magic(input) {
        let _ = tokio::fs::remove_file(input).await;
        return Err(VimaxError::Media(format!(
            "scene/shot video is corrupt or unreadable (removed so resume can regenerate): {}{}",
            input.display(),
            detail
                .map(|d| format!(" — ffmpeg: {d}"))
                .unwrap_or_default()
        )));
    }
    Err(VimaxError::Media(format!(
        "ffmpeg normalize failed for {}{}",
        input.display(),
        detail
            .map(|d| format!(" — ffmpeg: {d}"))
            .unwrap_or_default()
    )))
}

async fn run_ffmpeg(ffmpeg: &Path, args: &[&str]) -> VimaxResult<std::process::ExitStatus> {
    ffmpeg_command(ffmpeg)
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
    ffmpeg_command(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| VimaxError::Media(format!("ffmpeg spawn: {e}")))
}

/// Run ffmpeg and return `(success, stderr)`.
async fn run_ffmpeg_owned_capture(
    ffmpeg: &Path,
    args: &[String],
) -> VimaxResult<(bool, String)> {
    let output = ffmpeg_command(ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| VimaxError::Media(format!("ffmpeg spawn: {e}")))?;
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((output.status.success(), err))
}

fn ffmpeg_stderr_hint(stderr: &str) -> Option<String> {
    let line = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .rev()
        .find(|l| {
            let lower = l.to_ascii_lowercase();
            lower.contains("error")
                || lower.contains("invalid")
                || lower.contains("failed")
                || lower.contains("moov")
        })
        .or_else(|| {
            stderr
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .last()
        })?;
    let trimmed = if line.chars().count() > 240 {
        format!("{}…", line.chars().take(240).collect::<String>())
    } else {
        line.to_string()
    };
    Some(trimmed)
}

/// Extract the last frame of `video_path` to PNG at `out_path`.
pub async fn extract_last_frame(video_path: &Path, out_path: &Path) -> VimaxResult<()> {
    let ffmpeg = ensure_ffmpeg_ready().await?;
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

    // Hardware decode attempt first (fast path); the software attempts below are
    // the automatic fallback when the hwaccel / driver misbehaves.
    let hwaccel = nomi_config::ffmpeg_hw::select_decode_hwaccel(&ffmpeg).await;
    if let Some(accel) = hwaccel
        && let Some(decode) = nomi_config::ffmpeg_hw::hwaccel_decode_args(accel)
    {
        let mut hw_args: Vec<String> = vec!["-y".into()];
        hw_args.extend(decode.iter().map(|s| (*s).to_string()));
        hw_args.extend([
            "-sseof".into(),
            "-0.1".into(),
            "-i".into(),
            vin.into(),
        ]);
        // videotoolbox copies frames to system memory; GPU-resident accels need
        // hwdownload back before the PNG encoder can consume the frame.
        if accel != "videotoolbox" {
            hw_args.push("-vf".into());
            hw_args.push("hwdownload,format=nv12".into());
        }
        hw_args.extend([
            "-frames:v".into(),
            "1".into(),
            "-q:v".into(),
            "2".into(),
            out.into(),
        ]);
        let hw_status = run_ffmpeg_owned(&ffmpeg, &hw_args).await?;
        if hw_status.success() && out_path.is_file() {
            return Ok(());
        }
        let _ = tokio::fs::remove_file(out_path).await;
    }

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

/// Extract a still at roughly `ratio` through the video (0.0–1.0) → PNG.
/// Falls back to the first frame, then the last frame.
pub async fn extract_frame_at_ratio(
    video_path: &Path,
    out_path: &Path,
    ratio: f64,
) -> VimaxResult<()> {
    let ffmpeg = ensure_ffmpeg_ready().await?;
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
    let ratio = ratio.clamp(0.0, 0.95);

    if let Some(dur) = probe_duration_secs(&ffmpeg, video_path).await {
        let seek = (dur * ratio).max(0.0);
        let seek_s = format!("{seek:.3}");
        let status = run_ffmpeg(
            &ffmpeg,
            &[
                "-y",
                "-ss",
                &seek_s,
                "-i",
                vin,
                "-frames:v",
                "1",
                "-q:v",
                "2",
                out,
            ],
        )
        .await?;
        if status.success() && out_path.is_file() && is_usable_image_file(out_path) {
            return Ok(());
        }
        let _ = tokio::fs::remove_file(out_path).await;
    }

    // First frame fallback.
    let status = run_ffmpeg(
        &ffmpeg,
        &["-y", "-i", vin, "-frames:v", "1", "-q:v", "2", out],
    )
    .await?;
    if status.success() && out_path.is_file() && is_usable_image_file(out_path) {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(out_path).await;

    extract_last_frame(video_path, out_path).await
}

/// Extract the first frame of the *second* scene in a transition video.
///
/// Mirrors ViMax `get_new_camera_image`: ContentDetector → Scene-002 first frame,
/// else last frame of the whole clip.
pub async fn extract_new_camera_frame(video_path: &Path, out_path: &Path) -> VimaxResult<()> {
    let ffmpeg = ensure_ffmpeg_ready().await?;
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

/// MP4/MOV (`ftyp`) / WebM / AVI magic — rejects HTML/JSON error bodies saved as `.mp4`.
pub fn video_magic_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Some("mp4")
    } else if bytes.len() >= 4 && bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        Some("webm")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"AVI " {
        Some("avi")
    } else {
        None
    }
}

fn video_file_has_magic(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 32];
    use std::io::Read;
    let n = f.read(&mut head).unwrap_or(0);
    video_magic_kind(&head[..n]).is_some()
}

/// True when `path` exists, is large enough, and looks like a real video container.
pub fn is_usable_video_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() < MIN_USABLE_VIDEO_BYTES {
        return false;
    }
    video_file_has_magic(path)
}

const MIN_USABLE_AUDIO_BYTES: u64 = 256;

/// True when `path` exists and is a non-trivial audio file (wav/mp3/ogg/m4a).
pub fn is_usable_audio_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() < MIN_USABLE_AUDIO_BYTES {
        return false;
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "wav" | "mp3" | "m4a" | "ogg" | "aac" | "flac") {
        return true;
    }
    let mut head = [0u8; 12];
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(n) = f.read(&mut head) else {
        return false;
    };
    audio_magic_kind(&head[..n]).is_some()
}

fn audio_magic_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 12 && bytes[0..4] == *b"RIFF" && bytes[8..12] == *b"WAVE" {
        return Some("wav");
    }
    if bytes.len() >= 3 && bytes[0..3] == *b"ID3" {
        return Some("mp3");
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some("mp3");
    }
    if bytes.len() >= 4 && bytes[0..4] == *b"OggS" {
        return Some("ogg");
    }
    None
}

/// Write audio bytes atomically (voice reference clips).
pub async fn write_audio_bytes_atomic(out_path: &Path, bytes: &[u8]) -> VimaxResult<()> {
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if bytes.len() < MIN_USABLE_AUDIO_BYTES as usize {
        return Err(VimaxError::msg(format!(
            "audio too small ({} bytes) for {}",
            bytes.len(),
            out_path.display()
        )));
    }
    let part = {
        let mut s = out_path.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    };
    tokio::fs::write(&part, bytes).await?;
    if out_path.exists() {
        let _ = tokio::fs::remove_file(out_path).await;
    }
    tokio::fs::rename(&part, out_path).await?;
    Ok(())
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
    if video_magic_kind(bytes).is_none() {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(80)]);
        return Err(VimaxError::Video(format!(
            "downloaded video is not a recognizable container for {} (head={head:?})",
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

/// Remove incomplete / too-small / undecodable video artifacts so resume regenerates them.
///
/// Checks size + container magic first; if those pass, probes with ffprobe/ffmpeg so
/// truncated MP4s that still have an `ftyp` header are not treated as finished.
pub async fn scrub_unusable_video(path: &Path) -> VimaxResult<()> {
    let part = video_part_path(path);
    if part.exists() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    if !path.exists() {
        return Ok(());
    }
    if !is_usable_video_file(path) {
        let _ = tokio::fs::remove_file(path).await;
        return Ok(());
    }
    // Magic OK but body may be truncated (common Seedance / network partial write).
    match ensure_ffmpeg_ready().await {
        Ok(ffmpeg) => {
            if probe_duration_secs(&ffmpeg, path).await.is_none() {
                tracing::warn!(
                    video = %path.display(),
                    "scrubbing video that looks like a container but has no probeable duration"
                );
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        Err(e) => {
            // Don't delete a seemingly valid file just because ffmpeg isn't installed yet.
            tracing::debug!(
                video = %path.display(),
                error = %e,
                "skipping duration probe during scrub (ffmpeg unavailable)"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn usable_video_requires_min_size_and_magic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v.mp4");
        assert!(!is_usable_video_file(&path));
        std::fs::write(&path, vec![0u8; 100]).unwrap();
        assert!(!is_usable_video_file(&path));
        // Large but not a container → reject (old size-only check would accept).
        std::fs::write(&path, vec![0u8; MIN_USABLE_VIDEO_BYTES as usize]).unwrap();
        assert!(!is_usable_video_file(&path));
        std::fs::write(&path, fake_mp4_bytes(MIN_USABLE_VIDEO_BYTES as usize)).unwrap();
        assert!(is_usable_video_file(&path));
        assert_eq!(video_magic_kind(&fake_mp4_bytes(32)), Some("mp4"));
        assert!(video_magic_kind(b"<html>error</html>").is_none());
    }

    fn fake_mp4_bytes(len: usize) -> Vec<u8> {
        let mut v = vec![0u8; len.max(12)];
        v[0..4].copy_from_slice(&20u32.to_be_bytes());
        v[4..8].copy_from_slice(b"ftyp");
        v[8..12].copy_from_slice(b"isom");
        v
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
        let bytes = fake_mp4_bytes(MIN_USABLE_VIDEO_BYTES as usize);
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

    #[tokio::test]
    async fn atomic_write_rejects_html_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.mp4");
        let mut bytes = b"<html>gateway timeout</html>".to_vec();
        bytes.resize(MIN_USABLE_VIDEO_BYTES as usize, b' ');
        let err = write_video_bytes_atomic(&path, &bytes).await.unwrap_err();
        assert!(err.to_string().contains("recognizable container"));
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
    fn image_part_path_keeps_png_suffix() {
        let p = PathBuf::from("character_portraits/0_小雅/小雅_cameo.png");
        let part = image_part_path(&p);
        let name = part.file_name().and_then(|s| s.to_str()).unwrap();
        assert!(name.starts_with(".小雅_cameo.png.part-"));
        assert!(name.ends_with(".png"));
    }

    #[test]
    fn copy_image_file_atomic_overwrites() {
        use image::{ImageFormat, Rgb, RgbImage};
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.png");
        let dest = dir.path().join("nested").join("小雅_cameo.png");
        let mut png = Vec::new();
        RgbImage::from_pixel(6, 6, Rgb([1, 2, 3]))
            .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        std::fs::write(&src, &png).unwrap();
        copy_image_file_atomic(&src, &dest).unwrap();
        assert!(is_usable_image_file(&dest));
        // Second copy overwrites without leaving part sidecars.
        copy_image_file_atomic(&src, &dest).unwrap();
        assert!(is_usable_image_file(&dest));
        assert!(!image_part_path(&dest).exists());
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
        let Some(ffmpeg) = nomi_config::resolve_ffmpeg_executable() else {
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
        let detect = ffmpeg_command(&ffmpeg)
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

    #[test]
    fn concat_canvas_uses_session_aspect_without_upscaling() {
        assert_eq!(
            concat_canvas_for_aspect(&[(1920, 1080), (1280, 720)], "16:9"),
            (1280, 720)
        );
        assert_eq!(
            concat_canvas_for_aspect(&[(1280, 720), (854, 480), (1920, 1080)], "16:9"),
            (852, 480)
        );
        // Portrait stray in a 16:9 film: fit 9:16 inside 16:9, don't flip the film.
        let (w, h) = concat_canvas_for_aspect(&[(1920, 1080), (1080, 1920)], "16:9");
        assert_eq!((w, h), (1080, 606));
        assert_eq!(concat_canvas_for_aspect(&[], "16:9"), (1280, 720));
        assert_eq!(concat_canvas_for_aspect(&[(641, 361)], "16:9"), (640, 360));
    }

    #[test]
    fn concat_list_uses_forward_slashes_on_windows_paths() {
        let line = ffmpeg_concat_list_line(Path::new(r"C:\Users\a\script2video\final_video.mp4"));
        assert!(line.starts_with("file '"));
        assert!(!line.contains('\\'), "{line}");
        assert!(line.contains("C:/Users/a/script2video/final_video.mp4"));
    }

    #[tokio::test]
    async fn concat_joins_clips_with_mismatched_resolution() {
        let Some(ffmpeg) = nomi_config::resolve_ffmpeg_executable() else {
            eprintln!("skip: ffmpeg not available");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let c0 = dir.path().join("wide.mp4");
        let c1 = dir.path().join("tall.mp4");
        for (path, size) in [(&c0, "640x360"), (&c1, "320x240")] {
            let st = run_ffmpeg(
                &ffmpeg,
                &[
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("color=c=green:s={size}:d=1:r=24"),
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1",
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
                    path.to_str().unwrap(),
                ],
            )
            .await
            .unwrap();
            assert!(st.success(), "failed to mint {size} clip");
        }
        let out = dir.path().join("joined.mp4");
        concat_videos(&[c0.as_path(), c1.as_path()], &out)
            .await
            .expect("concat mixed resolutions");
        assert!(is_usable_video_file(&out));
        let dur = probe_duration_secs(&ffmpeg, &out).await.expect("probe duration");
        assert!(
            (1.5..=2.6).contains(&dur),
            "joined duration should be ~2s, got {dur}"
        );
    }

    #[test]
    fn normalize_audio_filter_adds_edge_afade_for_long_clips() {
        let af = normalize_audio_filter(10.0);
        assert!(af.contains("apad=whole_dur=10.000"));
        assert!(af.contains("afade=t=in:st=0:d=0.500"));
        assert!(af.contains("afade=t=out:st=9.500:d=0.500"));

        let short = normalize_audio_filter(0.2);
        assert!(!short.contains("afade"), "too-short clips skip edge fade");
    }

    #[test]
    fn vision_thumb_shrinks_large_png() {
        let mut img = image::RgbaImage::new(2000, 1500);
        for px in img.pixels_mut() {
            *px = image::Rgba([10, 20, 30, 255]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut png),
                image::ImageFormat::Png,
            )
            .unwrap();
        assert!(png.len() > 400 * 1024 || png.len() > 10_000);
        let thumb = jpeg_thumb_bytes_for_vision(&png).unwrap();
        assert!(image_magic_kind(&thumb) == Some("jpeg"));
        assert!(thumb.len() < png.len());
        let decoded = image::load_from_memory(&thumb).unwrap();
        assert!(decoded.width() <= VISION_THUMB_MAX_SIDE);
        assert!(decoded.height() <= VISION_THUMB_MAX_SIDE);
    }
}
