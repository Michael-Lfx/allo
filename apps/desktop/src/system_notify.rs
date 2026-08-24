//! Desktop OS toast via `tauri-plugin-notification` (and WinRT on Windows).
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
        show_os_notification(
            &self.app,
            &message.title,
            &message.body_text(),
            message.click_target.as_deref(),
        )
    }
}

/// Show a native OS notification. When `click_target` is set (a `flowy://…`
/// deep link), clicking the toast brings the main window forward and emits the
/// deep-link event the renderer already handles.
pub fn show_os_notification(
    app: &AppHandle,
    title: &str,
    body: &str,
    click_target: Option<&str>,
) -> Result<(), NotifyError> {
    #[cfg(windows)]
    {
        show_windows_toast(app, title, body, click_target)
    }
    #[cfg(not(windows))]
    {
        let _ = click_target;
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| NotifyError::Platform(error.to_string()))
    }
}

#[tauri::command]
pub fn show_os_notification_cmd(
    app: AppHandle,
    title: String,
    body: String,
    click_target: Option<String>,
) -> Result<(), String> {
    show_os_notification(&app, &title, &body, click_target.as_deref()).map_err(|e| e.to_string())
}

fn emit_notification_deep_link(app: &AppHandle, click_target: &str) {
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
