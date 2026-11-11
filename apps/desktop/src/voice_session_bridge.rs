//! Desktop voice context and snapshot transport, independent of any voice window.
use nomifun_app::voice_companion_manager::{VoiceCompanionProcessManager, VoiceWindowContext};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

#[tauri::command]
pub fn report_voice_context(window: tauri::WebviewWindow, conversation_id: Option<String>, workspace: Option<String>, sequence: u64, view_only: Option<bool>) -> Result<(), String> {
    if window.label() != "main" { return Err("voice_context_window_invalid".into()); }
    let manager = VoiceCompanionProcessManager::global().ok_or("voice_unavailable")?;
    let not_minimized = !window.is_minimized().map_err(|_| "voice_context_unavailable")?;
    if !not_minimized { return Ok(()); }
    if view_only == Some(true) {
        manager.report_voice_view(conversation_id, sequence);
    } else {
        manager.report_window_context(VoiceWindowContext { conversation_id, workspace, sequence });
    }
    Ok(())
}

pub fn install(app: AppHandle) {
    let Some(manager) = VoiceCompanionProcessManager::global() else { return; };
    if let Some(main) = app.get_webview_window("main") {
        let owner = manager.clone();
        main.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) { owner.set_candidate_conversation(None); }
        });
    }
    let mut events = manager.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let snapshot = match events.recv().await {
                Ok(snapshot) => snapshot,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => manager.snapshot(),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            // Initial state is read from the authenticated host API; subscriptions never open windows.
            if let Err(error) = app.emit("voice-session://snapshot", snapshot) {
                tracing::warn!(%error, "voice snapshot transport failed");
            }
        }
    });
}
