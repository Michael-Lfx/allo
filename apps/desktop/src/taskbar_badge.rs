//! Native application badge for pending attention items.
//!
//! The badge is an aggregate presentation of work that still needs attention;
//! it is intentionally not a persisted unread counter. Each source supplies a
//! stable attention id so duplicate terminal events are idempotent and opening
//! one target can clear only the corresponding items.

use std::collections::HashSet;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

const MAX_ATTENTION_ID_LENGTH: usize = 512;

#[cfg(windows)]
const ATTENTION_BG: [u8; 4] = [91, 108, 132, 255]; // Steel blue-gray, #5B6C84.

#[cfg(windows)]
const ATTENTION_FG: [u8; 4] = [248, 249, 245, 255]; // Warm white, not pure white.

#[derive(Default)]
struct BadgeState {
    attention_ids: HashSet<String>,
    /// Owned HICON pointer bits (`HICON.0 as isize`). `0` means none.
    /// Stored as `isize` so the managed state stays `Send + Sync`.
    #[cfg(windows)]
    overlay_icon: isize,
}

/// Process-wide pending-attention badge. Managed via `app.manage`.
#[derive(Default)]
pub struct TaskCompletionBadge {
    state: Mutex<BadgeState>,
}

/// Label painted on the native badge. `None` means clear the platform badge.
pub fn badge_text(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99+".to_owned()),
    }

}

fn count_from_len(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

fn validate_attention_id(attention_id: &str) -> Result<(), String> {
    if attention_id.is_empty() || attention_id.len() > MAX_ATTENTION_ID_LENGTH {
        return Err("attention id is empty or too long".to_owned());
    }
    if !attention_id.starts_with("conversation:")
        && !attention_id.starts_with("requirement:")
        && !attention_id.starts_with("support:")
    {
        return Err("attention id has an unsupported source".to_owned());
    }
    Ok(())
}

fn scope_prefix(source: &str, entity_id: Option<&str>) -> Result<String, String> {
    match source {
        "conversation" => {
            let entity_id = entity_id
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "conversation scope requires an entity id".to_owned())?;
            Ok(format!("conversation:{entity_id}:"))
        }
        "support" => Ok("support:".to_owned()),
        _ => Err(format!("unsupported attention source: {source}")),
    }
}

impl TaskCompletionBadge {
    #[cfg(test)]
    fn count(&self) -> u32 {
        let guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        count_from_len(guard.attention_ids.len())
    }

    fn apply_change<F>(&self, app: &AppHandle, change: F) -> bool
    where
        F: FnOnce(&mut HashSet<String>) -> bool,
    {
        // Keep state mutation and platform rendering in one critical section.
        // This prevents an older asynchronous notification from painting an
        // older count after a newer mark/clear operation has completed.
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = change(&mut guard.attention_ids);
        if changed {
            let count = count_from_len(guard.attention_ids.len());
            apply_overlay(app, &mut guard, count);
        }
        changed
    }

    #[cfg(test)]
    fn mark_for_test(&self, attention_id: &str) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.attention_ids.insert(attention_id.to_owned())
    }

    #[cfg(test)]
    fn clear_for_test(&self, attention_id: &str) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.attention_ids.remove(attention_id)
    }

    #[cfg(test)]
    fn clear_scope_for_test(&self, prefix: &str) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = guard.attention_ids.len();
        guard.attention_ids.retain(|id| !id.starts_with(prefix));
        before != guard.attention_ids.len()
    }

    #[cfg(test)]
    fn clear_all_for_test(&self) -> bool {
        let mut guard = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let changed = !guard.attention_ids.is_empty();
        guard.attention_ids.clear();
        changed
    }
}

/// Add one pending-attention item. Returns `true` only for a newly admitted id.
pub fn mark_attention(app: &AppHandle, attention_id: &str) -> Result<bool, String> {
    validate_attention_id(attention_id)?;
    let Some(state) = app.try_state::<TaskCompletionBadge>() else {
        // The command can be reached during a narrow startup window before the
        // managed state is installed. Do not suppress the actual notification.
        return Ok(true);
    };
    Ok(state.inner().apply_change(app, |ids| ids.insert(attention_id.to_owned())))
}

/// Remove one pending-attention item.
pub fn clear_attention(app: &AppHandle, attention_id: &str) -> Result<bool, String> {
    validate_attention_id(attention_id)?;
    let Some(state) = app.try_state::<TaskCompletionBadge>() else {
        return Ok(false);
    };
    Ok(state.inner().apply_change(app, |ids| ids.remove(attention_id)))
}

/// Remove all pending items for a source scope. Conversation scopes are
/// entity-specific; support is process/account scoped because support ids are
/// server sequence numbers and the current account is already isolated by the
/// authenticated renderer session.
pub fn clear_attention_scope(
    app: &AppHandle,
    source: &str,
    entity_id: Option<&str>,
) -> Result<bool, String> {
    let prefix = scope_prefix(source, entity_id)?;
    let Some(state) = app.try_state::<TaskCompletionBadge>() else {
        return Ok(false);
    };
    Ok(state.inner().apply_change(app, |ids| {
        let before = ids.len();
        ids.retain(|id| !id.starts_with(&prefix));
        before != ids.len()
    }))
}

/// Remove all pending items, reserved for account/session reset and shutdown.
pub fn clear_all_attention(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<TaskCompletionBadge>() else {
        return false;
    };
    state.inner().apply_change(app, |ids| {
        let changed = !ids.is_empty();
        ids.clear();
        changed
    })
}

#[tauri::command]
pub fn clear_attention_cmd(app: AppHandle, attention_id: String) -> Result<(), String> {
    clear_attention(&app, &attention_id).map(|_| ())
}

#[tauri::command]
pub fn clear_attention_scope_cmd(
    app: AppHandle,
    source: String,
    entity_id: Option<String>,
) -> Result<(), String> {
    clear_attention_scope(&app, &source, entity_id.as_deref()).map(|_| ())
}

#[tauri::command]
pub fn clear_all_attention_cmd(app: AppHandle) -> Result<(), String> {
    clear_all_attention(&app);
    Ok(())
}

fn apply_overlay(app: &AppHandle, state: &mut BadgeState, count: u32) {
    #[cfg(windows)]
    {
        if let Err(error) = apply_windows_overlay(app, state, count) {
            tracing::warn!(%error, count, "failed to update taskbar attention badge");
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = state;
        if let Some(window) = app.get_webview_window("main") {
            if let Err(error) = window.set_badge_label(badge_text(count)) {
                tracing::warn!(%error, count, "failed to update Dock attention badge");
            }
        }
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let _ = (app, state, count);
    }
}

#[cfg(windows)]
fn apply_windows_overlay(
    app: &AppHandle,
    state: &mut BadgeState,
    count: u32,
) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window missing".to_owned())?;
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;

    let com_owned = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = (|| {
        let taskbar: ITaskbarList3 = unsafe {
            CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("CoCreateInstance TaskbarList: {e}"))?
        };
        unsafe {
            taskbar
                .HrInit()
                .map_err(|e| format!("ITaskbarList3::HrInit: {e}"))?;
        }

        if state.overlay_icon != 0 {
            let old = HICON(state.overlay_icon as *mut std::ffi::c_void);
            let _ = unsafe { DestroyIcon(old) };
            state.overlay_icon = 0;
        }

        match badge_text(count) {
            None => {
                unsafe {
                    taskbar
                        .SetOverlayIcon(HWND(hwnd.0), HICON::default(), PCWSTR::null())
                        .map_err(|e| format!("SetOverlayIcon clear: {e}"))?;
                }
            }
            Some(label) => {
                let icon = create_badge_icon(&label)?;
                let description: Vec<u16> = label
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let set_result = unsafe {
                    taskbar.SetOverlayIcon(
                        HWND(hwnd.0),
                        icon,
                        PCWSTR::from_raw(description.as_ptr()),
                    )
                };
                if let Err(error) = set_result {
                    let _ = unsafe { DestroyIcon(icon) };
                    return Err(format!("SetOverlayIcon: {error}"));
                }
                state.overlay_icon = icon.0 as isize;
            }
        }
        Ok(())
    })();

    if com_owned {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(windows)]
fn create_badge_icon(
    label: &str,
) -> Result<windows::Win32::UI::WindowsAndMessaging::HICON, String> {
    use windows::Win32::Foundation::TRUE;
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};

    // Keep the HICON at the standard 16px overlay size. The badge itself uses
    // nearly the full canvas so Windows can scale it cleanly for the taskbar DPI.
    const SIZE: usize = 16;

    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return Err("CreateCompatibleDC failed".to_owned());
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE as i32,
                biHeight: -(SIZE as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbmp = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| format!("CreateDIBSection: {e}"))?;
        if bits.is_null() {
            let _ = DeleteDC(hdc);
            return Err("CreateDIBSection returned null bits".to_owned());
        }

        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, SIZE * SIZE);
        pixels.copy_from_slice(&render_badge_pixels(label, SIZE));

        // 1-bpp mask: 0 = show color pixel. All zeros lets the DIB alpha
        // channel provide the transparent antialiased edge.
        let mask_bytes = ((SIZE as i32 + 15) / 16) * 2 * SIZE as i32;
        let mask_buf = vec![0u8; mask_bytes as usize];
        let hmask = CreateBitmap(
            SIZE as i32,
            SIZE as i32,
            1,
            1,
            Some(mask_buf.as_ptr() as *const _),
        );
        if hmask.is_invalid() {
            let _ = DeleteObject(hbmp.into());
            let _ = DeleteDC(hdc);
            return Err("CreateBitmap mask failed".to_owned());
        }

        let info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: hmask,
            hbmColor: hbmp,
        };

        let icon = CreateIconIndirect(&info).map_err(|e| format!("CreateIconIndirect: {e}"))?;
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteObject(hmask.into());
        let _ = DeleteDC(hdc);
        Ok(icon)
    }
}

#[cfg(windows)]
fn render_badge_pixels(label: &str, size: usize) -> Vec<u32> {
    const SCALE: usize = 8;
    let high_size = size * SCALE;
    let mut high = vec![[0u8; 4]; high_size * high_size];

    for y in 0..high_size {
        for x in 0..high_size {
            let sample_x = (x as f32 + 0.5) / SCALE as f32;
            let sample_y = (y as f32 + 0.5) / SCALE as f32;
            if badge_shape_contains(sample_x, sample_y, size as f32, label) {
                high[y * high_size + x] = ATTENTION_BG;
            }
        }
    }
    paint_label(&mut high, high_size, size, label, SCALE);

    let mut output = vec![0u32; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut alpha_sum = 0u32;
            let mut red_premultiplied = 0u32;
            let mut green_premultiplied = 0u32;
            let mut blue_premultiplied = 0u32;
            for sy in 0..SCALE {
                for sx in 0..SCALE {
                    let pixel = high[(y * SCALE + sy) * high_size + x * SCALE + sx];
                    let alpha = u32::from(pixel[3]);
                    alpha_sum += alpha;
                    red_premultiplied += u32::from(pixel[0]) * alpha;
                    green_premultiplied += u32::from(pixel[1]) * alpha;
                    blue_premultiplied += u32::from(pixel[2]) * alpha;
                }
            }
            if alpha_sum == 0 {
                continue;
            }
            let samples = (SCALE * SCALE) as u32;
            let alpha = (alpha_sum / samples).min(255);
            let red = (red_premultiplied / alpha_sum).min(255);
            let green = (green_premultiplied / alpha_sum).min(255);
            let blue = (blue_premultiplied / alpha_sum).min(255);
            output[y * size + x] = (alpha << 24) | (red << 16) | (green << 8) | blue;
        }
    }
    output
}

#[cfg(windows)]
fn badge_shape_contains(x: f32, y: f32, size: f32, label: &str) -> bool {
    let center_y = size / 2.0;
    if label.chars().count() <= 1 {
        let center = size / 2.0;
        let radius = size / 2.0 - 0.35;
        let dx = x - center;
        let dy = y - center;
        return dx * dx + dy * dy <= radius * radius;
    }

    // Fit the capsule to the rendered glyphs: one or two digits get the larger
    // glyph treatment, while `99+` keeps the compact layout needed by the
    // fixed 16px overlay canvas.
    let char_count = label.chars().count();
    let glyph_width = char_count * if char_count <= 2 { 5 } else { 4 }
        + char_count.saturating_sub(1);
    let horizontal_padding = if char_count <= 2 { 3 } else { 4 };
    let capsule_width = (glyph_width + horizontal_padding).min(size as usize) as f32;
    let left = (size - capsule_width) / 2.0;
    let right = left + capsule_width;
    let top = 0.35;
    let bottom = size - 0.35;
    let radius = ((bottom - top) / 2.0).min(capsule_width / 2.0);
    if y < top || y > bottom {
        return false;
    }
    if x >= left + radius && x <= right - radius {
        return true;
    }
    let end_center_x = if x < left + radius {
        left + radius
    } else {
        right - radius
    };
    let dx = x - end_center_x;
    let dy = y - center_y;
    dx * dx + dy * dy <= radius * radius
}

#[cfg(windows)]
fn paint_label(
    pixels: &mut [[u8; 4]],
    size: usize,
    logical_size: usize,
    label: &str,
    scale: usize,
) {
    const GLYPH_W: usize = 5;
    const GLYPH_H: usize = 8;
    const COMPACT_GLYPH_W: usize = 4;

    fn glyph(ch: char) -> Option<[u8; GLYPH_H]> {
        Some(match ch {
            '0' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
            '1' => [0b00110, 0b01110, 0b00110, 0b00110, 0b00110, 0b00110, 0b00110, 0b11111],
            '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
            '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b10001, 0b01110],
            '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b10010, 0b11111, 0b00010, 0b00010],
            '5' => [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b10001, 0b10001, 0b01110],
            '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
            '7' => [0b11111, 0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
            '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
            '9' => [0b01110, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
            '+' => [0b00000, 0b00100, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
            _ => return None,
        })
    }

    let chars: Vec<char> = label.chars().filter(|ch| glyph(*ch).is_some()).collect();
    if chars.is_empty() {
        return;
    }
    let layout_glyph_width = if chars.len() > 2 { COMPACT_GLYPH_W } else { GLYPH_W };
    let total_w = chars.len() * layout_glyph_width + chars.len().saturating_sub(1);
    let origin_x = logical_size.saturating_sub(total_w) / 2;
    let origin_y = logical_size.saturating_sub(GLYPH_H) / 2;
    let mut x_offset = origin_x;

    for ch in chars {
        let Some(rows) = glyph(ch) else {
            continue;
        };
        for (row_index, row) in rows.iter().enumerate() {
            for col in 0..GLYPH_W {
                if (row >> (GLYPH_W - 1 - col)) & 1 == 0 {
                    continue;
                }
                let inset = scale as f32 * 0.02;
                let cell_left = x_offset as f32
                    + col as f32 * layout_glyph_width as f32 / GLYPH_W as f32;
                let cell_right = x_offset as f32
                    + (col + 1) as f32 * layout_glyph_width as f32 / GLYPH_W as f32;
                let left = cell_left * scale as f32 + inset;
                let top = (origin_y + row_index) as f32 * scale as f32 + inset;
                let right = cell_right * scale as f32 - inset;
                let bottom = (origin_y + row_index + 1) as f32 * scale as f32 - inset;
                let radius = scale as f32 * 0.10;
                let min_x = left.max(0.0) as usize;
                let max_x = right.ceil().min(size as f32) as usize;
                let min_y = top.max(0.0) as usize;
                let max_y = bottom.ceil().min(size as f32) as usize;
                for y in min_y..max_y {
                    for x in min_x..max_x {
                        let sample_x = x as f32 + 0.5;
                        let sample_y = y as f32 + 0.5;
                        if rounded_rect_contains(sample_x, sample_y, left, top, right, bottom, radius) {
                            pixels[y * size + x] = ATTENTION_FG;
                        }
                    }
                }
            }
        }
        x_offset += layout_glyph_width + 1;
    }
}

#[cfg(windows)]
fn rounded_rect_contains(
    x: f32,
    y: f32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
) -> bool {
    if x < left || x > right || y < top || y > bottom {
        return false;
    }
    let nearest_x = x.clamp(left + radius, right - radius);
    let nearest_y = y.clamp(top + radius, bottom - radius);
    let dx = x - nearest_x;
    let dy = y - nearest_y;
    dx * dx + dy * dy <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONVERSATION_A: &str = "conversation:c1:turn:t1";
    const CONVERSATION_B: &str = "conversation:c1:turn:t2";
    const REQUIREMENT: &str = "requirement:r1";
    const SUPPORT: &str = "support:7";

    #[test]
    fn badge_text_clears_at_zero() {
        assert_eq!(badge_text(0), None);
    }

    #[test]
    fn badge_text_shows_exact_count_up_to_99() {
        assert_eq!(badge_text(1).as_deref(), Some("1"));
        assert_eq!(badge_text(9).as_deref(), Some("9"));
        assert_eq!(badge_text(10).as_deref(), Some("10"));
        assert_eq!(badge_text(99).as_deref(), Some("99"));
    }

    #[test]
    fn badge_text_uses_overflow_label_above_99() {
        assert_eq!(badge_text(100).as_deref(), Some("99+"));
        assert_eq!(badge_text(u32::MAX).as_deref(), Some("99+"));
    }

    #[test]
    fn duplicate_attention_is_idempotent() {
        let badge = TaskCompletionBadge::default();
        assert!(badge.mark_for_test(CONVERSATION_A));
        assert!(!badge.mark_for_test(CONVERSATION_A));
        assert_eq!(badge.count(), 1);
    }

    #[test]
    fn exact_clear_preserves_other_sources() {
        let badge = TaskCompletionBadge::default();
        assert!(badge.mark_for_test(CONVERSATION_A));
        assert!(badge.mark_for_test(REQUIREMENT));
        assert!(badge.mark_for_test(SUPPORT));
        assert!(badge.clear_for_test(CONVERSATION_A));
        assert_eq!(badge.count(), 2);
        assert!(!badge.clear_for_test(CONVERSATION_A));
    }

    #[test]
    fn conversation_scope_clear_preserves_other_conversations() {
        let badge = TaskCompletionBadge::default();
        assert!(badge.mark_for_test(CONVERSATION_A));
        assert!(badge.mark_for_test(CONVERSATION_B));
        assert!(badge.mark_for_test("conversation:c2:turn:t3"));
        assert!(badge.clear_scope_for_test("conversation:c1:"));
        assert_eq!(badge.count(), 1);
    }

    #[test]
    fn clear_all_is_reserved_but_available_for_session_reset() {
        let badge = TaskCompletionBadge::default();
        assert!(badge.mark_for_test(CONVERSATION_A));
        assert!(badge.mark_for_test(REQUIREMENT));
        assert!(badge.clear_all_for_test());
        assert!(!badge.clear_all_for_test());
        assert_eq!(badge.count(), 0);
    }

    #[test]
    fn attention_id_validation_rejects_unknown_sources() {
        assert!(validate_attention_id(CONVERSATION_A).is_ok());
        assert!(validate_attention_id("unknown:1").is_err());
        assert!(validate_attention_id("").is_err());
    }

    #[test]
    fn support_scope_is_process_scoped() {
        assert_eq!(scope_prefix("support", None).unwrap(), "support:");
        assert_eq!(scope_prefix("conversation", Some("c1")).unwrap(), "conversation:c1:");
        assert!(scope_prefix("conversation", None).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn raster_badge_has_antialiased_edges_and_plus_glyph() {
        let pixels = render_badge_pixels("99+", 16);
        assert!(pixels.iter().any(|pixel| {
            let alpha = (pixel >> 24) & 0xff;
            alpha > 0 && alpha < 255
        }));
        assert!(pixels.iter().any(|pixel| {
            let expected = (u32::from(ATTENTION_BG[3]) << 24)
                | (u32::from(ATTENTION_BG[0]) << 16)
                | (u32::from(ATTENTION_BG[1]) << 8)
                | u32::from(ATTENTION_BG[2]);
            *pixel == expected
        }));
        let plus = render_badge_pixels("+", 16);
        assert!(plus.iter().any(|pixel| {
            let alpha = (pixel >> 24) & 0xff;
            let red = (pixel >> 16) & 0xff;
            let green = (pixel >> 8) & 0xff;
            let blue = pixel & 0xff;
            alpha == 255 && red >= 220 && green >= 220 && blue >= 220
        }));
    }

    #[cfg(windows)]
    #[test]
    fn raster_capsule_expands_with_label_length() {
        fn alpha_width(pixels: &[u32], size: usize) -> usize {
            let occupied: Vec<usize> = (0..size)
                .filter(|x| (0..size).any(|y| pixels[y * size + x] >> 24 != 0))
                .collect();
            occupied
                .last()
                .zip(occupied.first())
                .map_or(0, |(last, first)| last - first + 1)
        }

        let two_digit_width = alpha_width(&render_badge_pixels("10", 16), 16);
        let overflow_width = alpha_width(&render_badge_pixels("99+", 16), 16);
        assert!(two_digit_width < overflow_width);
    }

    #[cfg(windows)]
    #[test]
    fn raster_single_digit_uses_larger_circle_geometry() {
        let pixels = render_badge_pixels("1", 16);
        let occupied: Vec<usize> = (0..16)
            .filter(|x| (0..16).any(|y| pixels[y * 16 + x] >> 24 != 0))
            .collect();
        let width = occupied
            .last()
            .zip(occupied.first())
            .map_or(0, |(last, first)| last - first + 1);
        assert!((15..=16).contains(&width));
    }

    #[cfg(windows)]
    #[test]
    fn raster_single_digit_uses_larger_glyph_geometry() {
        let pixels = render_badge_pixels("1", 16);
        let foreground: Vec<usize> = pixels
            .iter()
            .enumerate()
            .filter_map(|(index, pixel)| {
                let alpha = (pixel >> 24) & 0xff;
                let red = (pixel >> 16) & 0xff;
                let green = (pixel >> 8) & 0xff;
                let blue = pixel & 0xff;
                (alpha > 200 && red > 220 && green > 220 && blue > 220).then_some(index)
            })
            .collect();
        let rows: Vec<usize> = foreground.iter().map(|index| index / 16).collect();
        let height = rows
            .iter()
            .min()
            .zip(rows.iter().max())
            .map_or(0, |(first, last)| last - first + 1);
        assert!((8..=9).contains(&height));
    }
}
