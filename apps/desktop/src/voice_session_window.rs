//! Native consumer of the host snapshot; lives independently of settings/main renderer.
use nomifun_app::voice_companion_manager::{
    VoiceCompanionProcessManager, VoiceSessionControlRequest, VoiceSessionPhase,
    VoiceSessionSnapshot,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WindowEvent};

const LABEL: &str = "voice-session";

#[tauri::command]
pub fn report_voice_context(window:tauri::WebviewWindow,conversation_id:Option<String>)->Result<(),String> {
    if window.label()!="main" {return Err("voice_context_window_invalid".into());}
    let manager=VoiceCompanionProcessManager::global().ok_or("voice_unavailable")?;
    let focused=window.is_focused().map_err(|_|"voice_context_unavailable")?
        && window.is_visible().map_err(|_|"voice_context_unavailable")?
        && !window.is_minimized().map_err(|_|"voice_context_unavailable")?;
    let candidate=if focused {
        let url=window.url().map_err(|_|"voice_context_unavailable")?;
        if conversation_id.is_none() && url.fragment().is_some_and(|hash|hash.starts_with("/conversation/")) {return Ok(());}
        if let Some(id)=&conversation_id {
            if url.fragment().and_then(|hash|hash.split('?').next())!=Some(format!("/conversation/{id}").as_str()) {return Err("voice_context_stale".into());}
        }
        conversation_id
    } else {None};
    manager.set_candidate_conversation(candidate);
    Ok(())
}

pub fn install(app: AppHandle, init_script: String) {
    let Some(manager) = VoiceCompanionProcessManager::global() else {
        return;
    };
    if let Some(main) = app.get_webview_window("main") {
        let owner = manager.clone();
        main.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                owner.set_candidate_conversation(None);
            }
        });
    }
    let mut events = manager.subscribe();
    let initial = manager.snapshot();
    tauri::async_runtime::spawn(async move {
        let mut next = Some(initial);
        loop {
            let snapshot = match next.take() {
                Some(snapshot) => snapshot,
                None => match events.recv().await {
                    Ok(snapshot) => snapshot,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => manager.snapshot(),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
            };
            let app_main = app.clone();
            let script = init_script.clone();
            let _ = app.run_on_main_thread(move || {
                if let Err(error) = update(&app_main, &script, snapshot) {
                    tracing::warn!(%error, "voice session window update failed");
                }
            });
        }
    });
}

fn update(
    app: &AppHandle,
    script: &str,
    snapshot: VoiceSessionSnapshot,
) -> Result<(), tauri::Error> {
    let wake = matches!(snapshot.phase,VoiceSessionPhase::Listening|VoiceSessionPhase::AwaitingConfirmation|VoiceSessionPhase::ListeningConfirm) && snapshot.session_id.is_some();
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None if wake => {
            let window = tauri::WebviewWindowBuilder::new(
                app,
                LABEL,
                WebviewUrl::App("index.html#/voice-session".into()),
            )
            .initialization_script(script)
            .title("Flowy")
            .inner_size(440.0, 520.0)
            .min_inner_size(360.0, 340.0)
            .resizable(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .build()?;
            let close_window = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let window = close_window.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(manager) = VoiceCompanionProcessManager::global() {
                            let current = manager.snapshot();
                            let scope = VoiceSessionControlRequest {
                                instance_id: current.instance_id,
                                session_id: current.session_id,
                                turn_id: current.turn_id,
                            };
                            if let Err(error) = manager.stop_session(scope).await {
                                tracing::warn!(%error, "voice session close rejected");
                                return;
                            }
                        }
                        let _ = window.hide();
                    });
                }
            });
            window.center()?;
            window
        }
        None => return Ok(()),
    };
    if wake {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};
            let handle = window.hwnd()?;
            let _ = ShowWindow(
                windows::Win32::Foundation::HWND(handle.0),
                SW_SHOWNOACTIVATE,
            );
        }
        #[cfg(not(windows))]
        window.show()?;
    }
    if snapshot.phase == VoiceSessionPhase::Idle
        && snapshot.user_text.is_none()
        && snapshot.assistant_text.is_none()
    {
        window.hide()?;
    }
    app.emit_to(LABEL, "voice-session://snapshot", snapshot)?;
    Ok(())
}
