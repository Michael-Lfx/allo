//! Screen capture via xcap, downscaled and PNG/base64-encoded for the LLM.
//!
//! Coordinate systems: `Monitor::width()/height()` report the size enigo's
//! absolute mouse coordinates use (logical points on macOS, device pixels on
//! Windows/Linux), while `capture_image()` returns physical pixels (2x on
//! Retina). `CaptureGeometry` records both so clicks can be mapped back.

use base64::Engine as _;
use xcap::Monitor;

use nomi_types::tool::ToolImage;

use crate::permissions;
use crate::scale::fit_within;

/// Geometry of the most recent capture, used to map LLM (screenshot-pixel)
/// coordinates to absolute screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureGeometry {
    /// Dimensions of the image the LLM sees (after downscaling).
    pub img_w: u32,
    pub img_h: u32,
    /// Monitor size in the coordinate system input synthesis uses.
    pub logical_w: u32,
    pub logical_h: u32,
    /// Monitor origin in the global (multi-display) coordinate space.
    pub origin_x: i32,
    pub origin_y: i32,
}

/// A completed screen capture. `image` is the downscaled RGBA frame (not yet
/// encoded) so callers can draw a Set-of-Marks overlay before encoding; use
/// `encode_png` to produce the `ToolImage`.
#[derive(Debug)]
pub struct CapturedScreen {
    pub image: image::RgbaImage,
    pub geometry: CaptureGeometry,
    /// Raw captured frame size in physical pixels (before downscaling).
    pub physical_w: u32,
    pub physical_h: u32,
    /// Index of the captured monitor within `Monitor::all()`.
    pub display_index: usize,
}

/// A screen-space rectangle in the same logical coordinate system xcap reports
/// for windows and monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

const MAX_SCREENSHOT_PNG_BYTES: usize = 5 * 1024 * 1024;
const MIN_SCREENSHOT_LONG_EDGE: u32 = 320;
const MAX_ENCODE_ATTEMPTS: usize = 12;

/// Encoded screenshot and the exact pixel geometry sent to the model.
#[derive(Debug)]
pub struct EncodedPng {
    pub image: ToolImage,
    pub width: u32,
    pub height: u32,
}

fn png_bytes(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode screenshot as PNG: {e}"))?;
    Ok(png)
}

/// Encode a (possibly overlay-annotated) RGBA frame as a byte-bounded PNG.
/// High-entropy or unusually large frames are downscaled deterministically so
/// native Computer screenshots can never exceed the shared 5 MiB image limit.
pub fn encode_png(img: &image::RgbaImage) -> Result<EncodedPng, String> {
    if img.width() == 0 || img.height() == 0 {
        return Err("Cannot encode an empty screenshot".to_owned());
    }

    let mut working = img.clone();
    for _ in 0..MAX_ENCODE_ATTEMPTS {
        let png = png_bytes(&working)?;
        if png.len() <= MAX_SCREENSHOT_PNG_BYTES {
            return Ok(EncodedPng {
                width: working.width(),
                height: working.height(),
                image: ToolImage {
                    media_type: "image/png".to_string(),
                    data: base64::engine::general_purpose::STANDARD.encode(&png),
                },
            });
        }

        let old_w = working.width();
        let old_h = working.height();
        if old_w.max(old_h) <= MIN_SCREENSHOT_LONG_EDGE {
            break;
        }
        let ratio = ((MAX_SCREENSHOT_PNG_BYTES as f64 / png.len() as f64).sqrt() * 0.92)
            .clamp(0.1, 0.9);
        let min_ratio = MIN_SCREENSHOT_LONG_EDGE as f64 / old_w.max(old_h) as f64;
        let ratio = ratio.max(min_ratio.min(0.9));
        let new_w = ((old_w as f64 * ratio).floor() as u32).max(1);
        let new_h = ((old_h as f64 * ratio).floor() as u32).max(1);
        if (new_w, new_h) == (old_w, old_h) {
            break;
        }
        working = image::imageops::resize(
            &working,
            new_w,
            new_h,
            image::imageops::FilterType::Triangle,
        );
    }

    Err(format!(
        "Screenshot could not be reduced below the {} MiB provider image limit",
        MAX_SCREENSHOT_PNG_BYTES / (1024 * 1024)
    ))
}

/// Pick the monitor to capture: explicit index, else the primary, else the
/// first one listed.
fn select_monitor(monitors: &[Monitor], display: Option<usize>) -> Result<usize, String> {
    if monitors.is_empty() {
        return Err(format!(
            "No displays found. {}",
            permissions::screen_capture_hint_detailed()
        ));
    }
    match display {
        Some(idx) => {
            if idx < monitors.len() {
                Ok(idx)
            } else {
                Err(format!(
                    "Display {idx} does not exist; {} display(s) available (0-{}).",
                    monitors.len(),
                    monitors.len() - 1
                ))
            }
        }
        None => Ok(monitors
            .iter()
            .position(|m| m.is_primary().unwrap_or(false))
            .unwrap_or(0)),
    }
}

fn monitor_bounds(monitor: &Monitor) -> (i32, i32, u32, u32) {
    (
        monitor.x().unwrap_or(0),
        monitor.y().unwrap_or(0),
        monitor.width().unwrap_or(0),
        monitor.height().unwrap_or(0),
    )
}

fn intersection_area(region: CaptureRegion, monitor: (i32, i32, u32, u32)) -> u64 {
    let (mx, my, mw, mh) = monitor;
    let left = i64::from(region.x).max(i64::from(mx));
    let top = i64::from(region.y).max(i64::from(my));
    let right = (i64::from(region.x) + i64::from(region.width))
        .min(i64::from(mx) + i64::from(mw));
    let bottom = (i64::from(region.y) + i64::from(region.height))
        .min(i64::from(my) + i64::from(mh));
    if right <= left || bottom <= top {
        0
    } else {
        ((right - left) as u64) * ((bottom - top) as u64)
    }
}

fn select_monitor_for_region(
    monitors: &[Monitor],
    display: Option<usize>,
    region: Option<CaptureRegion>,
) -> Result<usize, String> {
    if display.is_some() || region.is_none() {
        return select_monitor(monitors, display);
    }
    let region = region.expect("checked above");
    let best = monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| (index, intersection_area(region, monitor_bounds(monitor))))
        .max_by_key(|(_, area)| *area)
        .filter(|(_, area)| *area > 0)
        .map(|(index, _)| index);
    best.ok_or_else(|| {
        format!(
            "The target window at ({}, {}) size {}x{} is outside all visible displays. {}",
            region.x,
            region.y,
            region.width,
            region.height,
            permissions::screen_capture_hint_detailed()
        )
    })
}

/// Convert a logical screen region into a physical crop inside a monitor frame.
fn physical_crop(
    region: CaptureRegion,
    monitor: (i32, i32, u32, u32),
    physical_size: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let (mx, my, mw, mh) = monitor;
    if mw == 0 || mh == 0 || physical_size.0 == 0 || physical_size.1 == 0 {
        return None;
    }
    let left = i64::from(region.x).max(i64::from(mx));
    let top = i64::from(region.y).max(i64::from(my));
    let right = (i64::from(region.x) + i64::from(region.width))
        .min(i64::from(mx) + i64::from(mw));
    let bottom = (i64::from(region.y) + i64::from(region.height))
        .min(i64::from(my) + i64::from(mh));
    if right <= left || bottom <= top {
        return None;
    }
    let sx = f64::from(physical_size.0) / f64::from(mw);
    let sy = f64::from(physical_size.1) / f64::from(mh);
    let x = ((left - i64::from(mx)) as f64 * sx).floor().max(0.0) as u32;
    let y = ((top - i64::from(my)) as f64 * sy).floor().max(0.0) as u32;
    let x2 = ((right - i64::from(mx)) as f64 * sx).ceil().min(f64::from(physical_size.0)) as u32;
    let y2 = ((bottom - i64::from(my)) as f64 * sy).ceil().min(f64::from(physical_size.1)) as u32;
    Some((x, y, x2.saturating_sub(x).max(1), y2.saturating_sub(y).max(1)))
}

/// Capture a monitor, downscale to `max_edge`, and encode as base64 PNG.
/// Blocking: call from `spawn_blocking`. On macOS, xcap enumerates displays
/// through AppKit (`NSScreen`), so the native capture work is dispatched to the
/// main queue before it touches xcap.
pub fn capture_screen(display: Option<usize>, max_edge: u32) -> Result<CapturedScreen, String> {
    capture_screen_region(display, None, max_edge)
}

/// Capture only `region` (typically the target window) while retaining enough
/// monitor geometry to map screenshot pixels back to absolute screen input.
pub fn capture_screen_region(
    display: Option<usize>,
    region: Option<CaptureRegion>,
    max_edge: u32,
) -> Result<CapturedScreen, String> {
    crate::macos_main::run_blocking(move || capture_screen_inner(display, region, max_edge))
}

fn capture_screen_inner(
    display: Option<usize>,
    region: Option<CaptureRegion>,
    max_edge: u32,
) -> Result<CapturedScreen, String> {
    // Proactive + authoritative on macOS: a denied Screen Recording grant lets
    // capture "succeed" with a black frame, so fail fast with a clear message
    // instead of relying solely on the downstream black-frame heuristic.
    if permissions::screen_recording_granted() == Some(false) {
        return Err(format!(
            "Screen Recording permission is not granted, so the screen cannot be captured. {}",
            permissions::screen_capture_hint_detailed()
        ));
    }
    let monitors = Monitor::all().map_err(|e| {
        format!(
            "Failed to enumerate displays: {e}. {}",
            permissions::screen_capture_hint_detailed()
        )
    })?;
    let display_index = select_monitor_for_region(&monitors, display, region)?;
    let monitor = &monitors[display_index];

    let mut frame = monitor.capture_image().map_err(|e| {
        format!(
            "Failed to capture the screen: {e}. {}",
            permissions::screen_capture_hint_detailed()
        )
    })?;
    permissions::screenshot_permission_check(&frame)?;

    let (full_physical_w, full_physical_h) = frame.dimensions();
    if full_physical_w == 0 || full_physical_h == 0 {
        return Err(format!(
            "Capture returned an empty frame. {}",
            permissions::screen_capture_hint_detailed()
        ));
    }

    let monitor = monitor_bounds(monitor);
    let scale_x = f64::from(full_physical_w) / f64::from(monitor.2.max(1));
    let scale_y = f64::from(full_physical_h) / f64::from(monitor.3.max(1));
    let (mut physical_w, mut physical_h) = (full_physical_w, full_physical_h);
    let mut logical_w = monitor.2;
    let mut logical_h = monitor.3;
    let mut origin_x = monitor.0;
    let mut origin_y = monitor.1;

    if let Some(region) = region {
        let (crop_x, crop_y, crop_w, crop_h) =
            physical_crop(region, monitor, (full_physical_w, full_physical_h)).ok_or_else(|| {
                "The target window has no visible area on the selected display.".to_string()
            })?;
        frame = image::imageops::crop_imm(&frame, crop_x, crop_y, crop_w, crop_h).to_image();
        physical_w = frame.width();
        physical_h = frame.height();
        logical_w = (f64::from(crop_w) / scale_x).round().max(1.0) as u32;
        logical_h = (f64::from(crop_h) / scale_y).round().max(1.0) as u32;
        origin_x += (f64::from(crop_x) / scale_x).round() as i32;
        origin_y += (f64::from(crop_y) / scale_y).round() as i32;
    }

    let (img_w, img_h) = fit_within(physical_w, physical_h, max_edge);
    let scaled = if (img_w, img_h) == (physical_w, physical_h) {
        frame
    } else {
        image::imageops::resize(&frame, img_w, img_h, image::imageops::FilterType::Triangle)
    };

    Ok(CapturedScreen {
        image: scaled,
        geometry: CaptureGeometry {
            img_w,
            img_h,
            logical_w,
            logical_h,
            origin_x,
            origin_y,
        },
        physical_w,
        physical_h,
        display_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_crop_clips_window_to_monitor() {
        let crop = physical_crop(
            CaptureRegion { x: 1100, y: 50, width: 500, height: 300 },
            (0, 0, 1440, 900),
            (2880, 1800),
        );
        assert_eq!(crop, Some((2200, 100, 680, 600)));
    }

    #[test]
    fn physical_crop_rejects_offscreen_window() {
        assert_eq!(
            physical_crop(
                CaptureRegion { x: 2000, y: 50, width: 500, height: 300 },
                (0, 0, 1440, 900),
                (2880, 1800),
            ),
            None
        );
    }

    #[test]
    fn region_monitor_selection_uses_intersection() {
        let region = CaptureRegion { x: 1200, y: 100, width: 800, height: 600 };
        assert_eq!(intersection_area(region, (0, 0, 1440, 900)), 240 * 600);
        assert_eq!(intersection_area(region, (1440, 0, 1920, 1080)), 560 * 600);
    }

    // Requires a real display and (on macOS) Screen Recording permission.
    #[test]
    #[ignore]
    fn capture_primary_screen_real() {
        let captured = capture_screen(None, 1568).expect("capture should succeed");
        assert!(captured.image.width() > 0 && captured.image.height() > 0);
        let encoded = encode_png(&captured.image).expect("encode should succeed");
        assert_eq!(encoded.image.media_type, "image/png");
        assert!(!encoded.image.data.is_empty());
        assert!(captured.geometry.img_w.max(captured.geometry.img_h) <= 1568);
        assert!(captured.physical_w >= captured.geometry.img_w);
    }

    #[test]
    #[ignore]
    fn capture_invalid_display_errors_real() {
        let err = capture_screen(Some(99), 1568).unwrap_err();
        assert!(err.contains("99"), "error should name the display: {err}");
    }

    #[test]
    fn encode_png_bounds_high_entropy_screenshot_payload() {
        let img = image::RgbaImage::from_fn(3_000, 2_000, |x, y| {
            let mut value = x
                .wrapping_mul(0x9e37_79b9)
                .wrapping_add(y.wrapping_mul(0x85eb_ca6b));
            value ^= value >> 16;
            value = value.wrapping_mul(0x7feb_352d);
            value ^= value >> 15;
            image::Rgba([
                value as u8,
                (value >> 8) as u8,
                (value >> 16) as u8,
                255,
            ])
        });

        let encoded = encode_png(&img).expect("encode should succeed");

        const MAX_BASE64_BYTES: usize = (5 * 1024 * 1024_usize).div_ceil(3) * 4;
        assert!(encoded.image.data.len() <= MAX_BASE64_BYTES);
        assert!(encoded.width > 0 && encoded.height > 0);
        assert!(encoded.width < img.width() || encoded.height < img.height());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_capture_task_runs_inside_dispatcher() {
        use std::sync::{Arc, Mutex};

        let caller_thread = std::thread::current().id();
        let dispatcher_thread = Arc::new(Mutex::new(None));
        let work_thread = Arc::new(Mutex::new(None));
        let dispatcher_thread_seen = dispatcher_thread.clone();
        let work_thread_seen = work_thread.clone();

        let result = crate::macos_main::run_task_with(
            move |task| {
                *dispatcher_thread_seen.lock().unwrap() = Some(std::thread::current().id());
                let handle = std::thread::spawn(task);
                handle.join().expect("dispatched task should not panic")
            },
            move || {
                *work_thread_seen.lock().unwrap() = Some(std::thread::current().id());
                Ok("ok")
            },
        )
        .expect("task should complete");

        assert_eq!(result, "ok");
        assert_eq!(dispatcher_thread.lock().unwrap().unwrap(), caller_thread);
        assert_ne!(
            work_thread.lock().unwrap().unwrap(),
            caller_thread,
            "work must run inside the dispatcher task, not on the caller thread"
        );
    }
}
