//! Y1 desktop tray + global shortcuts for meeting recording.
//!
//! Closing the main window hides to tray and does **not** stop capture.
//! Tray Quit is the only path that tears down the process (and thus the
//! in-process meeting runtime).

use std::sync::Arc;
use std::time::Duration;

use nomifun_app::{DesktopServer, MeetingTrayPhase, MeetingTrayStatus};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::show_main_window_public as show_main_window;

pub const TRAY_ID: &str = "nomi-tray";

/// Event listened by the renderer to open the meeting page (`/meeting`).
pub const MEETING_OPEN_EVENT: &str = "meeting-open";

/// Default global shortcuts (documented for users / settings later).
///
/// | Action | Windows / Linux | macOS |
/// |--------|-----------------|-------|
/// | Start  | Ctrl+Shift+R    | Cmd+Shift+R |
/// | Pause / Resume toggle | Ctrl+Shift+P | Cmd+Shift+P |
/// | Stop   | Ctrl+Shift+S    | Cmd+Shift+S |
pub mod shortcuts {
    pub const START: &str = if cfg!(target_os = "macos") {
        "Cmd+Shift+R"
    } else {
        "Ctrl+Shift+R"
    };
    pub const PAUSE_TOGGLE: &str = if cfg!(target_os = "macos") {
        "Cmd+Shift+P"
    } else {
        "Ctrl+Shift+P"
    };
    pub const STOP: &str = if cfg!(target_os = "macos") {
        "Cmd+Shift+S"
    } else {
        "Ctrl+Shift+S"
    };
}

/// Handles for tray menu items that need label / enablement updates.
pub struct MeetingTrayMenu {
    pub show: MenuItem<tauri::Wry>,
    pub quit: MenuItem<tauri::Wry>,
    pub start: MenuItem<tauri::Wry>,
    pub pause: MenuItem<tauri::Wry>,
    pub resume: MenuItem<tauri::Wry>,
    pub stop: MenuItem<tauri::Wry>,
    pub open: MenuItem<tauri::Wry>,
}

/// Build the system tray (Show / meeting controls / Quit) and register global
/// shortcuts. Caller must already have managed [`DesktopServer`].
pub fn install_tray_and_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    let show = MenuItem::with_id(app, "tray-show", "Show Flowy", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "tray-meeting-start", "Start Meeting", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "tray-meeting-pause", "Pause Meeting", false, None::<&str>)?;
    let resume =
        MenuItem::with_id(app, "tray-meeting-resume", "Resume Meeting", false, None::<&str>)?;
    let stop = MenuItem::with_id(app, "tray-meeting-stop", "Stop Meeting", false, None::<&str>)?;
    let open =
        MenuItem::with_id(app, "tray-meeting-open", "Open Meeting Page", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &show,
            &sep1,
            &start,
            &pause,
            &resume,
            &stop,
            &open,
            &sep2,
            &quit,
        ],
    )?;

    if !app.manage(MeetingTrayMenu {
        show: show.clone(),
        quit: quit.clone(),
        start: start.clone(),
        pause: pause.clone(),
        resume: resume.clone(),
        stop: stop.clone(),
        open: open.clone(),
    }) {
        return Err(anyhow::anyhow!("meeting tray menu state was already registered"));
    }

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Flowy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-show" => show_main_window(app),
            "tray-meeting-start" => spawn_meeting_op(app, MeetingOp::Start),
            "tray-meeting-pause" => spawn_meeting_op(app, MeetingOp::Pause),
            "tray-meeting-resume" => spawn_meeting_op(app, MeetingOp::Resume),
            "tray-meeting-stop" => spawn_meeting_op(app, MeetingOp::Stop),
            "tray-meeting-open" => open_meeting_page(app),
            "tray-quit" => {
                app.state::<crate::QuitFlag>()
                    .0
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    }
    tray_builder.build(app)?;

    register_global_shortcuts(app)?;
    spawn_status_poller(app.clone());
    Ok(())
}

/// Localize Show / Quit (existing) plus meeting tray labels when the renderer
/// has i18n ready. Meeting keys are optional for backward compatibility.
#[tauri::command]
pub fn set_tray_labels(
    show: String,
    quit: String,
    start: Option<String>,
    pause: Option<String>,
    resume: Option<String>,
    stop: Option<String>,
    open: Option<String>,
    items: tauri::State<'_, MeetingTrayMenu>,
) -> Result<(), String> {
    items.show.set_text(show).map_err(|e| e.to_string())?;
    items.quit.set_text(quit).map_err(|e| e.to_string())?;
    if let Some(text) = start {
        items.start.set_text(text).map_err(|e| e.to_string())?;
    }
    if let Some(text) = pause {
        items.pause.set_text(text).map_err(|e| e.to_string())?;
    }
    if let Some(text) = resume {
        items.resume.set_text(text).map_err(|e| e.to_string())?;
    }
    if let Some(text) = stop {
        items.stop.set_text(text).map_err(|e| e.to_string())?;
    }
    if let Some(text) = open {
        items.open.set_text(text).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum MeetingOp {
    Start,
    Pause,
    Resume,
    TogglePause,
    Stop,
}

fn spawn_meeting_op(app: &AppHandle, op: MeetingOp) {
    let Some(server) = app.try_state::<Arc<DesktopServer>>() else {
        tracing::warn!("meeting tray: DesktopServer not ready");
        return;
    };
    let server = server.inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match op {
            MeetingOp::Start => server.meeting_tray_start().await,
            MeetingOp::Pause => server.meeting_tray_pause().await,
            MeetingOp::Resume => server.meeting_tray_resume().await,
            MeetingOp::TogglePause => server.meeting_tray_toggle_pause().await,
            MeetingOp::Stop => server.meeting_tray_stop().await,
        };
        match result {
            Ok(status) => apply_tray_status(&app, &status),
            Err(e) => tracing::warn!(error = %e, "meeting tray operation failed"),
        }
    });
}

fn open_meeting_page(app: &AppHandle) {
    show_main_window(app);
    let _ = app.emit(MEETING_OPEN_EVENT, "/meeting");
}

fn apply_tray_status(app: &AppHandle, status: &MeetingTrayStatus) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(status.tooltip()));
    }
    let Some(items) = app.try_state::<MeetingTrayMenu>() else {
        return;
    };
    let idle = status.phase == MeetingTrayPhase::Idle;
    let recording = status.phase == MeetingTrayPhase::Recording;
    let paused = status.phase == MeetingTrayPhase::Paused;
    let _ = items.start.set_enabled(idle);
    let _ = items.pause.set_enabled(recording);
    let _ = items.resume.set_enabled(paused);
    let _ = items.stop.set_enabled(recording || paused);

    let caption_text = status.latest_caption.clone().unwrap_or_default();
    let visible = !idle && !caption_text.trim().is_empty();
    crate::meeting_captions::update_meeting_captions(
        app,
        crate::meeting_captions::MeetingCaptionsPayload {
            visible,
            text: caption_text,
            speaker: String::new(),
            is_partial: status.latest_caption_partial,
            phase: match status.phase {
                MeetingTrayPhase::Idle => "idle".into(),
                MeetingTrayPhase::Recording => "recording".into(),
                MeetingTrayPhase::Paused => "paused".into(),
            },
        },
    );
    if idle {
        crate::meeting_captions::hide_meeting_captions(app);
    }
}

fn spawn_status_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(800));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            // Quit in progress: stop polling. Later ticks would only query the
            // backend after its database pool has closed and spam closed-pool
            // warnings while the process shuts down (the tray-quit wedge itself
            // was a main-thread issue; this loop was never the blocker).
            if app
                .try_state::<crate::QuitFlag>()
                .is_some_and(|flag| flag.0.load(std::sync::atomic::Ordering::SeqCst))
            {
                break;
            }
            let Some(server) = app.try_state::<Arc<DesktopServer>>() else {
                continue;
            };
            let status = server.inner().meeting_tray_status().await;
            apply_tray_status(&app, &status);
        }
    });
}

fn register_global_shortcuts(app: &AppHandle) -> anyhow::Result<()> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // Plugin + handler are registered on the Tauri Builder; here we only
    // claim the documented chords (best-effort if another app already owns them).
    for (label, chord) in [
        ("start", shortcuts::START),
        ("pause", shortcuts::PAUSE_TOGGLE),
        ("stop", shortcuts::STOP),
    ] {
        if let Err(e) = app.global_shortcut().register(chord) {
            tracing::warn!(
                shortcut = label,
                chord,
                error = %e,
                "meeting global shortcut registration failed"
            );
        }
    }
    Ok(())
}

/// Build the global-shortcut plugin with the Y1 meeting handlers.
/// Must be attached on the Tauri Builder before `run`.
pub fn global_shortcut_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

    #[cfg(target_os = "macos")]
    let mods = Modifiers::SUPER | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;

    let start = Shortcut::new(Some(mods), Code::KeyR);
    let pause = Shortcut::new(Some(mods), Code::KeyP);
    let stop = Shortcut::new(Some(mods), Code::KeyS);

    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if shortcut == &start {
                spawn_meeting_op(app, MeetingOp::Start);
            } else if shortcut == &pause {
                spawn_meeting_op(app, MeetingOp::TogglePause);
            } else if shortcut == &stop {
                spawn_meeting_op(app, MeetingOp::Stop);
            }
        })
        .build()
}
