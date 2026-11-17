//! Desktop system tray: Show / Quit. Closing the main window hides it to the
//! tray instead of quitting; tray Quit is the only path that tears down the
//! process.
//!
//! Meeting controls are intentionally **not** exposed here. The meeting surface
//! is currently hidden from the product UI (`fix(ui): 隐藏首页侧栏的会议入口`,
//! 2026-10-15), so the tray must not keep a hidden entrance (menu items or
//! global shortcuts) to it. The `/meeting` route itself is kept for deep links
//! and previews; the desktop shell does not poll meeting status for the tray.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::show_main_window_public as show_main_window;

pub const TRAY_ID: &str = "nomi-tray";

const TRAY_SHOW_ID: &str = "tray-show";
const TRAY_QUIT_ID: &str = "tray-quit";
/// The complete tray menu surface. Meeting controls must never be added here:
/// the meeting product surface is hidden and the tray is not an entrance.
const TRAY_MENU_ITEM_IDS: [&str; 2] = [TRAY_SHOW_ID, TRAY_QUIT_ID];

/// Handles for tray menu items that need label updates.
pub struct TrayMenu {
    pub show: MenuItem<tauri::Wry>,
    pub quit: MenuItem<tauri::Wry>,
}

/// Build the system tray (Show / Quit). Caller must already have managed
/// [`DesktopServer`].
pub fn install_tray(app: &AppHandle) -> anyhow::Result<()> {
    // The menu surface is the constant list above; keep construction and the
    // regression test reading the same source of truth.
    let show = MenuItem::with_id(app, TRAY_MENU_ITEM_IDS[0], "Show Flowy", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_MENU_ITEM_IDS[1], "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(app, &[&show, &separator, &quit])?;

    if !app.manage(TrayMenu {
        show: show.clone(),
        quit: quit.clone(),
    }) {
        return Err(anyhow::anyhow!("tray menu state was already registered"));
    }

    let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Flowy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-show" => show_main_window(app),
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
    Ok(())
}

/// Localize Show / Quit labels when the renderer has i18n ready.
#[tauri::command]
pub fn set_tray_labels(
    show: String,
    quit: String,
    items: tauri::State<'_, TrayMenu>,
) -> Result<(), String> {
    items.show.set_text(show).map_err(|e| e.to_string())?;
    items.quit.set_text(quit).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_menu_keeps_meeting_controls_hidden() {
        assert_eq!(TRAY_MENU_ITEM_IDS.len(), 2);
        assert!(
            !TRAY_MENU_ITEM_IDS
                .iter()
                .any(|id| id.contains("meeting")),
            "meeting controls must stay out of the tray menu"
        );
    }
}
