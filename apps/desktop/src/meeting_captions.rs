//! Always-on-top bottom meeting captions overlay (U4).
//!
//! Driven by tray status / meeting segment events — minimal, visible while recording.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl};
use tracing::warn;

pub const MEETING_CAPTIONS_LABEL: &str = "meeting-captions";

const CAPTION_WIDTH: u32 = 720;
const CAPTION_HEIGHT: u32 = 96;
const MARGIN_LOGICAL: f64 = 24.0;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct MeetingCaptionsPayload {
    pub visible: bool,
    pub text: String,
    pub speaker: String,
    pub is_partial: bool,
    pub phase: String,
}

#[derive(Default)]
struct CaptionsSession {
    payload: Option<MeetingCaptionsPayload>,
}

#[derive(Clone, Default)]
pub struct MeetingCaptionsState {
    inner: Arc<Mutex<CaptionsSession>>,
}

impl MeetingCaptionsState {
    pub fn set_payload(&self, payload: MeetingCaptionsPayload) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.payload = Some(payload);
    }

    pub fn clear(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.payload = None;
    }
}

fn bottom_center_position(
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
    let x = work_x + (work_width.saturating_sub(width) as i32) / 2;
    let y = work_y + work_height.saturating_sub(height) as i32 - margin;
    PhysicalPosition::new(x.max(work_x), y.max(work_y))
}

/// Show or refresh the floating captions window.
pub fn update_meeting_captions(app: &AppHandle, payload: MeetingCaptionsPayload) {
    let Some(state) = app.try_state::<MeetingCaptionsState>() else {
        return;
    };
    if !payload.visible || payload.text.trim().is_empty() {
        state.clear();
        hide_captions(app);
        return;
    }
    state.set_payload(payload.clone());
    let app_for_show = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if let Err(error) = ensure_and_show(&app_for_show, &payload) {
            warn!(%error, "failed to show meeting captions window");
        }
    }) {
        warn!(%error, "failed to dispatch meeting captions show");
    }
}

pub fn hide_meeting_captions(app: &AppHandle) {
    if let Some(state) = app.try_state::<MeetingCaptionsState>() {
        state.clear();
    }
    hide_captions(app);
}

fn hide_captions(app: &AppHandle) {
    let app_clone = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_clone.get_webview_window(MEETING_CAPTIONS_LABEL) {
            let _ = window.hide();
        }
    });
}

fn ensure_and_show(app: &AppHandle, payload: &MeetingCaptionsPayload) -> Result<(), String> {
    let window = match app.get_webview_window(MEETING_CAPTIONS_LABEL) {
        Some(window) => window,
        None => {
            tauri::WebviewWindowBuilder::new(
                app,
                MEETING_CAPTIONS_LABEL,
                WebviewUrl::App("index.html#/meeting-captions".into()),
            )
            .title("Meeting captions")
            .inner_size(f64::from(CAPTION_WIDTH), f64::from(CAPTION_HEIGHT))
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

    let position = resolve_caption_position(app);
    window
        .set_size(PhysicalSize::new(CAPTION_WIDTH, CAPTION_HEIGHT))
        .map_err(|e| e.to_string())?;
    window
        .set_position(position)
        .map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);
    window.show().map_err(|e| e.to_string())?;
    let _ = app.emit_to(MEETING_CAPTIONS_LABEL, "meeting-captions://update", payload.clone());
    let app_retry = app.clone();
    let payload_retry = payload.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let _ = app_retry.emit_to(
            MEETING_CAPTIONS_LABEL,
            "meeting-captions://update",
            payload_retry,
        );
    });
    Ok(())
}

fn resolve_caption_position(app: &AppHandle) -> PhysicalPosition<i32> {
    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().into_iter().flatten().next());
    match monitor {
        Some(monitor) => {
            let work = monitor.work_area();
            bottom_center_position(
                work.position.x,
                work.position.y,
                work.size.width,
                work.size.height,
                monitor.scale_factor(),
                CAPTION_WIDTH,
                CAPTION_HEIGHT,
            )
        }
        None => PhysicalPosition::new(120, 640),
    }
}
