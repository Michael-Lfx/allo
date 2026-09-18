//! Desktop OS notification with an in-app fallback.
//!
//! Implements [`SystemTaskNotifier`] so requirement terminal transitions can
//! surface as native notifications. A Tauri command of the same shape lets the
//! renderer notify on conversation turn completion with a click deep link.

use std::sync::Arc;

use async_trait::async_trait;
use nomifun_app::{NotifyError, SystemNotification, SystemTaskNotifier};
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

pub struct DesktopTauriNotifier {
    app: AppHandle,
}

impl DesktopTauriNotifier {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    pub fn into_arc(self) -> Arc<dyn SystemTaskNotifier> {
        Arc::new(self)
    }
}

#[async_trait]
impl SystemTaskNotifier for DesktopTauriNotifier {
    async fn notify(&self, message: &SystemNotification) -> Result<(), NotifyError> {
        let attention_id = (message.source == "requirement" && !message.task_id.is_empty())
            .then(|| format!("requirement:{}", message.task_id));
        show_os_notification_with_attention(
            &self.app,
            &message.title,
            &message.body_text(),
            message.click_target.as_deref(),
            attention_id.as_deref(),
        )
    }
}

/// Show a native notification and optionally admit one pending-attention item.
/// The item is admitted before attempting the platform provider so a provider
/// failure still leaves the user with the badge and the Flowy fallback toast.
pub fn show_os_notification_with_attention(
    app: &AppHandle,
    title: &str,
    body: &str,
    click_target: Option<&str>,
    attention_id: Option<&str>,
) -> Result<(), NotifyError> {
    // Shutdown in progress: a toast now outlives nothing, and its badge/toast-window
    // work only races the exiting event loop. Suppress it on both entry paths
    // (renderer command and backend notifier).
    if app
        .try_state::<crate::QuitFlag>()
        .is_some_and(|flag| flag.0.load(std::sync::atomic::Ordering::SeqCst))
    {
        return Ok(());
    }

    if let Some(attention_id) = attention_id {
        let is_new = crate::taskbar_badge::mark_attention(app, attention_id)
            .map_err(NotifyError::Platform)?;
        if !is_new {
            return Ok(());
        }
    }

    #[cfg(windows)]
    let result = show_windows_toast(app, title, body, click_target);
    #[cfg(not(windows))]
    let result = {
        let _ = click_target;
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| NotifyError::Platform(error.to_string()))
    };

    match result {
        Ok(()) => Ok(()),
        Err(primary_error) => {
            // The Rust command owns the provider failure boundary. Returning
            // success after dispatching this fallback prevents tauriShell.ts
            // from showing a second notification through the plugin path.
            match crate::completion_toast::show_completion_toast(
                app,
                title,
                body,
                click_target,
                attention_id,
            ) {
                Ok(()) => {
                    tracing::warn!(
                        error = %primary_error,
                        "OS notification failed; using Flowy notification fallback"
                    );
                    Ok(())
                }
                Err(fallback_error) => Err(NotifyError::Platform(format!(
                    "{primary_error}; Flowy fallback failed: {fallback_error}"
                ))),
            }
        }
    }
}

// Async so the WinRT toast / taskbar-badge COM calls run on the async runtime
// instead of blocking the main-thread event loop (a stalled shell RPC there
// wedges every queued event — including the final exit request on quit).
#[tauri::command]
pub async fn show_os_notification_cmd(
    app: AppHandle,
    title: String,
    body: String,
    click_target: Option<String>,
    attention_id: Option<String>,
) -> Result<(), String> {
    show_os_notification_with_attention(
        &app,
        &title,
        &body,
        click_target.as_deref(),
        attention_id.as_deref(),
    )
    .map_err(|e| e.to_string())
}

pub(crate) fn emit_notification_deep_link(app: &AppHandle, click_target: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    let _ = app.emit("deep-link://received", vec![click_target.to_owned()]);
}

#[cfg(windows)]
fn show_windows_toast(
    app: &AppHandle,
    title: &str,
    body: &str,
    click_target: Option<&str>,
) -> Result<(), NotifyError> {
    use tauri_winrt_notification::Toast;

    // Always use the bundle identifier. `windows_aumid::register_for_toasts`
    // registers it under HKCU so Action Center brands the toast as Flowy
    // (even under `tauri dev`, which previously fell back to PowerShell).
    let app_id = app.config().identifier.clone();

    let mut toast = Toast::new(&app_id).title(title).text2(body);
    if let Some(target) = click_target {
        let app_handle = app.clone();
        let target = target.to_owned();
        toast = toast
            .add_button("打开", &target)
            .on_activated(move |args| {
                let open = args.filter(|s| !s.is_empty()).unwrap_or_else(|| target.clone());
                emit_notification_deep_link(&app_handle, &open);
                Ok(())
            });
    }
    toast
        .show()
        .map_err(|error| NotifyError::Platform(error.to_string()))
}
