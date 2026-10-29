//! Desktop voice context and snapshot transport, independent of any voice window.
use nomifun_app::voice_companion_manager::{VoiceCompanionProcessManager, VoiceWindowContext};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

#[tauri::command]
pub fn report_voice_context(window: tauri::WebviewWindow, conversation_id: Option<String>, workspace: Option<String>, sequence: u64, view_only: Option<bool>) -> Result<(), String> {
    if window.label() != "main" { return Err("voice_context_window_invalid".into()); }
    let manager = VoiceCompanionProcessManager::global().ok_or("voice_unavailable")?;
    let focused = window.is_focused().map_err(|_| "voice_context_unavailable")?
        && window.is_visible().map_err(|_| "voice_context_unavailable")?
        && !window.is_minimized().map_err(|_| "voice_context_unavailable")?;
    if !focused { return Ok(()); }
    let candidate = {
        let url = window.url().map_err(|_| "voice_context_unavailable")?;
        let route = url.fragment().and_then(|hash| hash.split('?').next()).unwrap_or("");
        if workspace.is_some() && route != "/guid" { return Err("voice_context_stale".into()); }
        if conversation_id.is_none() && url.fragment().is_some_and(|hash| hash.starts_with("/conversation/")) { return Ok(()); }
        if let Some(id) = &conversation_id {
            if url.fragment().and_then(|hash| hash.split('?').next()) != Some(format!("/conversation/{id}").as_str()) { return Err("voice_context_stale".into()); }
        }
        conversation_id
    };
    if view_only == Some(true) {
        manager.report_voice_view(candidate, sequence);
    } else {
        manager.report_window_context(VoiceWindowContext { conversation_id: candidate, workspace, sequence });
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
