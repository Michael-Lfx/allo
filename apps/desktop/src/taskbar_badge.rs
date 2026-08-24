//! Windows taskbar overlay badge for completed-task count.
//!
//! Each OS notification that surfaces a finished conversation/requirement
//! increments the badge. Focusing the main window (or acting on a toast)
//! clears it. Non-Windows hosts keep an in-memory count but paint nothing.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

/// Process-wide completed-task badge. Managed via `app.manage`.
#[derive(Default)]
pub struct TaskCompletionBadge {
    count: Mutex<u32>,
    /// Owned HICON pointer bits (`HICON.0 as isize`). `0` means none.
    /// Stored as `isize` so the managed state stays `Send + Sync`.
    #[cfg(windows)]
    overlay_icon: Mutex<isize>,
}

/// Label painted on the overlay icon. `None` means clear the overlay.
pub fn badge_text(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=99 => Some(count.to_string()),
        _ => Some("99".to_owned()),
    }
}

/// Saturating increment used when a completion notification is delivered.
pub fn next_count(current: u32) -> u32 {
    current.saturating_add(1)
}

impl TaskCompletionBadge {
    pub fn count(&self) -> u32 {
        *self.count.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_count(&self, count: u32) -> u32 {
        let mut guard = self.count.lock().unwrap_or_else(|p| p.into_inner());
        *guard = count;
        count
    }

    pub fn increment(&self) -> u32 {
        let mut guard = self.count.lock().unwrap_or_else(|p| p.into_inner());
        *guard = next_count(*guard);
        *guard
    }

    pub fn clear(&self) -> u32 {
        self.set_count(0)
    }
}

/// Bump the badge after a completion notification is shown.
pub fn on_completion_notified(app: &AppHandle) {
    let Some(state) = app.try_state::<TaskCompletionBadge>() else {
        return;
    };
    let count = state.inner().increment();
    apply_overlay(app, state.inner(), count);
}

/// Clear the badge (main window focused, toast clicked, tray show, …).
pub fn clear_badge(app: &AppHandle) {
    let Some(state) = app.try_state::<TaskCompletionBadge>() else {
        return;
    };
    let count = state.inner().clear();
    apply_overlay(app, state.inner(), count);
}

fn apply_overlay(app: &AppHandle, state: &TaskCompletionBadge, count: u32) {
    #[cfg(windows)]
    {
        if let Err(error) = apply_windows_overlay(app, state, count) {
            tracing::warn!(%error, count, "failed to update taskbar completion badge");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, state, count);
    }
}

#[cfg(windows)]
fn apply_windows_overlay(
    app: &AppHandle,
    state: &TaskCompletionBadge,
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

        let mut overlay_guard = state
            .overlay_icon
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if *overlay_guard != 0 {
            let old = HICON(*overlay_guard as *mut std::ffi::c_void);
            let _ = unsafe { DestroyIcon(old) };
            *overlay_guard = 0;
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
                *overlay_guard = icon.0 as isize;
                let description: Vec<u16> = label
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    taskbar
                        .SetOverlayIcon(
                            HWND(hwnd.0),
                            icon,
                            PCWSTR::from_raw(description.as_ptr()),
                        )
                        .map_err(|e| format!("SetOverlayIcon: {e}"))?;
                }
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
fn create_badge_icon(label: &str) -> Result<windows::Win32::UI::WindowsAndMessaging::HICON, String> {
    use windows::Win32::Foundation::TRUE;
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};

    const SIZE: i32 = 16;

    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return Err("CreateCompatibleDC failed".to_owned());
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE, // top-down
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

        let pixels =
            std::slice::from_raw_parts_mut(bits as *mut u32, (SIZE * SIZE) as usize);
        pixels.fill(0);
        let cx = (SIZE - 1) as f32 / 2.0;
        let cy = cx;
        let radius = cx;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx * dx + dy * dy <= radius * radius {
                    // BGRA word: B=0x22 G=0x22 R=0xE0 A=0xFF
                    pixels[(y * SIZE + x) as usize] = 0xFF_E0_22_22;
                }
            }
        }
        paint_label(pixels, SIZE as usize, label);

        // 1-bpp mask: 0 = show color pixel. All zeros → full color circle shows.
        let mask_bytes = ((SIZE + 15) / 16) * 2 * SIZE;
        let mask_buf = vec![0u8; mask_bytes as usize];
        let hmask = CreateBitmap(SIZE, SIZE, 1, 1, Some(mask_buf.as_ptr() as *const _));
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

/// Paint a tiny white digit/label into the center of a 16×16 BGRA buffer.
/// Digits are 3×5 glyphs so "99" still fits.
#[cfg(windows)]
fn paint_label(pixels: &mut [u32], size: usize, label: &str) {
    const GLYPH_W: usize = 3;
    const GLYPH_H: usize = 5;
    const WHITE: u32 = 0xFF_FF_FF_FF;

    fn glyph(ch: char) -> Option<[u8; GLYPH_H]> {
        Some(match ch {
            '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
            '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
            '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
            '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
            '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
            '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
            '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
            '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
            '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
            '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
            _ => return None,
        })
    }

    let chars: Vec<char> = label.chars().filter(|c| c.is_ascii_digit()).collect();
    if chars.is_empty() {
        return;
    }
    let total_w = chars.len() * GLYPH_W + chars.len().saturating_sub(1);
    let origin_x = size.saturating_sub(total_w) / 2;
    let origin_y = size.saturating_sub(GLYPH_H) / 2;

    let mut x_off = origin_x;
    for ch in chars {
        let Some(rows) = glyph(ch) else {
            continue;
        };
        for (row_i, row) in rows.iter().enumerate() {
            for col in 0..GLYPH_W {
                if (row >> (2 - col)) & 1 == 1 {
                    let x = x_off + col;
                    let y = origin_y + row_i;
                    if x < size && y < size {
                        pixels[y * size + x] = WHITE;
                    }
                }
            }
        }
        x_off += GLYPH_W + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_text_clears_at_zero() {
        assert_eq!(badge_text(0), None);
    }

    #[test]
    fn badge_text_shows_exact_count_up_to_99() {
        assert_eq!(badge_text(1).as_deref(), Some("1"));
        assert_eq!(badge_text(3).as_deref(), Some("3"));
        assert_eq!(badge_text(99).as_deref(), Some("99"));
    }

    #[test]
    fn badge_text_caps_above_99() {
        assert_eq!(badge_text(100).as_deref(), Some("99"));
        assert_eq!(badge_text(u32::MAX).as_deref(), Some("99"));
    }

    #[test]
    fn next_count_saturates() {
        assert_eq!(next_count(0), 1);
        assert_eq!(next_count(2), 3);
        assert_eq!(next_count(u32::MAX), u32::MAX);
    }

    #[test]
    fn state_increment_and_clear() {
        let badge = TaskCompletionBadge::default();
        assert_eq!(badge.count(), 0);
        assert_eq!(badge.increment(), 1);
        assert_eq!(badge.increment(), 2);
        assert_eq!(badge.increment(), 3);
        assert_eq!(badge.clear(), 0);
        assert_eq!(badge.count(), 0);
    }
}
