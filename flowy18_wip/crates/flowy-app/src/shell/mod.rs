#![allow(clippy::derive_partial_eq_without_eq)]

mod autostart;
mod deep_link;
mod notify;
mod tray;

use gpui::{App, WindowHandle, actions};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

actions!(shell, [ShowMainWindow, QuitApp]);

static MAIN: OnceLock<Mutex<Option<WindowHandle<crate::app::FlowyApp>>>> =
    OnceLock::new();

thread_local! {
    static TRAY: RefCell<Option<gpui_tray::Tray>> = const { RefCell::new(None) };
}

pub use autostart::{is_enabled as autostart_enabled, set_enabled as set_autostart};
pub use deep_link::{DeepLink, parse as parse_deep_link, take_pending};
pub use notify::{NotifyKind, show_update_notification};

pub fn enqueue_urls(urls: Vec<String>) {
    deep_link::enqueue(urls);
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &ShowMainWindow, cx| {
        ensure_main_window(cx);
    });
    cx.on_action(|_: &QuitApp, cx| {
        cx.quit();
    });
    cx.on_system_notification_response(|response, cx| {
        if response.action_id.as_deref() == Some("open") {
            ensure_main_window(cx);
        }
    });
    cx.spawn(async move |cx| {
        let task = cx.update(|cx| {
            cx.register_url_scheme(crate::brand::url_scheme())
        });
        let _ = task.await;
    })
    .detach();
    if let Ok(handle) = tray::build(cx) {
        TRAY.with(|slot| {
            *slot.borrow_mut() = Some(handle);
        });
    }
}

pub fn bind_main_window(window: WindowHandle<crate::app::FlowyApp>) {
    *MAIN.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(window);
}

pub fn ensure_main_window(cx: &mut App) {
    let existing = *MAIN
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap();
    if let Some(window) = existing {
        let alive = window
            .update(cx, |_app, window, _cx| {
                window.activate();
            })
            .is_ok();
        if alive {
            cx.activate(true);
            return;
        }
    }
    let window = crate::app::open_main_window(cx);
    bind_main_window(window);
    cx.activate(true);
}

pub fn apply_pending_deep_links(
    app: &mut crate::app::FlowyApp,
    cx: &mut gpui::Context<'_, crate::app::FlowyApp>,
) {
    for url in take_pending() {
        match parse_deep_link(&url) {
            Some(DeepLink::Invite { code }) => {
                app.receive_invite_code(code, cx);
            }
            Some(DeepLink::Open) | None => {}
        }
    }
}
