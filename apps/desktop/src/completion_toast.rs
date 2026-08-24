//! Always-on-top bottom-right completion popup (interactive toast window).
//!
//! Complements the OS Action Center toast: this window stays under Flowy's
//! control so Focus Assist / banner settings cannot suppress the interactive UI.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl};
use tracing::warn;

pub const COMPLETION_TOAST_LABEL: &str = "completion-toast";

const TOAST_WIDTH: u32 = 360;
const TOAST_HEIGHT: u32 = 108;
const MARGIN_LOGICAL: f64 = 16.0;
const AUTO_DISMISS_MS: u64 = 8_000;

/// Payload shown in the popup webview.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CompletionToastPayload {
    pub generation: u64,
    pub title: String,
    pub body: String,
    pub click_target: Option<String>,
}

#[derive(Default)]
struct ToastSession {
    payload: Option<CompletionToastPayload>,
}

#[derive(Clone, Default)]
pub struct CompletionToastState {
    inner: Arc<Mutex<ToastSession>>,
    generation: Arc<AtomicU64>,
}

impl CompletionToastState {
    pub fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn set_payload(&self, payload: CompletionToastPayload) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.payload = Some(payload);
    }

    pub fn take_if_generation(&self, generation: u64) -> Option<CompletionToastPayload> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        match guard.payload.as_ref() {
            Some(p) if p.generation == generation => guard.payload.take(),
            _ => None,
        }
    }

    pub fn current_generation(&self) -> Option<u64> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .payload
            .as_ref()
            .map(|p| p.generation)
    }

    #[cfg(test)]
    pub fn payload(&self) -> Option<CompletionToastPayload> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .payload
            .clone()
    }
}

/// Place a toast of `width`×`height` in the bottom-right of a work area.
pub fn bottom_right_position(
    work_x: i32,
    work_y: i32,
    work_width: u32,
    work_height: u32,
    scale_factor: f64,
    width: u32,
    height: u32,
) -> PhysicalPosition<i32> {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let margin = (MARGIN_LOGICAL * scale).round() as i32;
    let x = work_x + work_width.saturating_sub(width) as i32 - margin;
    let y = work_y + work_height.saturating_sub(height) as i32 - margin;
    PhysicalPosition::new(x.max(work_x), y.max(work_y))
}

/// Show (or replace) the interactive completion popup.
pub fn show_completion_toast(
    app: &AppHandle,
    title: &str,
    body: &str,
    click_target: Option<&str>,
) {
    let Some(state) = app.try_state::<CompletionToastState>() else {
        return;
    };
    let generation = state.next_generation();
    let payload = CompletionToastPayload {
        generation,
        title: title.to_owned(),
        body: body.to_owned(),
        click_target: click_target.map(str::to_owned),
    };
    state.set_payload(payload.clone());

    let app_for_show = app.clone();
    let state_for_show = state.inner().clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Err(error) = ensure_and_show(&app_for_show, &payload) {
            warn!(%error, "failed to show completion toast window");
            let _ = state_for_show.take_if_generation(generation);
        }
    }) {
        warn!(%error, "failed to dispatch completion toast show");
        return;
    }

    let app_for_dismiss = app.clone();
    let state_for_dismiss = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(AUTO_DISMISS_MS)).await;
        if state_for_dismiss.current_generation() != Some(generation) {
            return;
        }
        dismiss_toast(&app_for_dismiss, &state_for_dismiss, generation);
    });
}

fn ensure_and_show(app: &AppHandle, payload: &CompletionToastPayload) -> Result<(), String> {
    let window = match app.get_webview_window(COMPLETION_TOAST_LABEL) {
        Some(window) => window,
        None => {
            tauri::WebviewWindowBuilder::new(
                app,
                COMPLETION_TOAST_LABEL,
                WebviewUrl::App("index.html#/completion-toast".into()),
            )
            .title("Flowy")
            .inner_size(f64::from(TOAST_WIDTH), f64::from(TOAST_HEIGHT))
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?
        }
    };

    let position = resolve_toast_position(app);
    window
        .set_size(PhysicalSize::new(TOAST_WIDTH, TOAST_HEIGHT))
        .map_err(|e| e.to_string())?;
    window
        .set_position(position)
        .map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);
    window.show().map_err(|e| e.to_string())?;
    let _ = app.emit_to(COMPLETION_TOAST_LABEL, "completion-toast://show", payload.clone());
    // First open races the SPA mount; re-emit shortly so the listener is ready.
    let app_retry = app.clone();
    let payload_retry = payload.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        let _ = app_retry.emit_to(
            COMPLETION_TOAST_LABEL,
            "completion-toast://show",
            payload_retry,
        );
    });
    Ok(())
}

fn resolve_toast_position(app: &AppHandle) -> PhysicalPosition<i32> {
    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().into_iter().flatten().next());
    match monitor {
        Some(monitor) => {
            let work = monitor.work_area();
            bottom_right_position(
                work.position.x,
                work.position.y,
                work.size.width,
                work.size.height,
                monitor.scale_factor(),
                TOAST_WIDTH,
                TOAST_HEIGHT,
            )
        }
        None => PhysicalPosition::new(24, 24),
    }
}

fn dismiss_toast(app: &AppHandle, state: &CompletionToastState, generation: u64) {
    if state.take_if_generation(generation).is_none() {
        return;
    }
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(window) = app.get_webview_window(COMPLETION_TOAST_LABEL) {
            let _ = window.hide();
        }
    });
}

#[tauri::command]
pub fn activate_completion_toast(app: AppHandle, generation: u64) -> Result<(), String> {
    let Some(state) = app.try_state::<CompletionToastState>() else {
        return Ok(());
    };
    let Some(payload) = state.take_if_generation(generation) else {
        return Ok(());
    };
    if let Some(window) = app.get_webview_window(COMPLETION_TOAST_LABEL) {
        let _ = window.hide();
    }
    crate::taskbar_badge::clear_badge(&app);
    if let Some(target) = payload.click_target.as_deref() {
        crate::system_notify::emit_notification_deep_link(&app, target);
    } else {
        crate::show_main_window_public(&app);
    }
    Ok(())
}

#[tauri::command]
pub fn dismiss_completion_toast(app: AppHandle, generation: u64) -> Result<(), String> {
    let Some(state) = app.try_state::<CompletionToastState>() else {
        return Ok(());
    };
    dismiss_toast(&app, state.inner(), generation);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_right_uses_margin_inside_work_area() {
        let pos = bottom_right_position(0, 0, 1920, 1040, 1.0, 360, 108);
        assert_eq!(pos.x, 1920 - 360 - 16);
        assert_eq!(pos.y, 1040 - 108 - 16);
    }

    #[test]
    fn bottom_right_scales_margin_with_dpi() {
        let pos = bottom_right_position(100, 50, 1600, 900, 1.5, 360, 108);
        let margin = ((16.0_f64) * 1.5).round() as i32;
        assert_eq!(pos.x, 100 + 1600 - 360 - margin);
        assert_eq!(pos.y, 50 + 900 - 108 - margin);
    }

    #[test]
    fn bottom_right_clamps_to_work_origin_when_toast_larger() {
        let pos = bottom_right_position(10, 20, 100, 80, 1.0, 360, 108);
        assert_eq!(pos.x, 10);
        assert_eq!(pos.y, 20);
    }

    #[test]
    fn generation_replaces_previous_payload() {
        let state = CompletionToastState::default();
        let g1 = state.next_generation();
        state.set_payload(CompletionToastPayload {
            generation: g1,
            title: "a".into(),
            body: "1".into(),
            click_target: None,
        });
        let g2 = state.next_generation();
        state.set_payload(CompletionToastPayload {
            generation: g2,
            title: "b".into(),
            body: "2".into(),
            click_target: Some("flowy://navigate?route=/conversation/x".into()),
        });
        assert_eq!(state.current_generation(), Some(g2));
        assert!(state.take_if_generation(g1).is_none());
        let taken = state.take_if_generation(g2).unwrap();
        assert_eq!(taken.title, "b");
        assert!(state.payload().is_none());
    }
}
