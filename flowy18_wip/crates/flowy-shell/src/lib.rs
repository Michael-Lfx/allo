//! Desktop shell lifecycle: deep link, tray, notify, autostart.

#![allow(clippy::derive_partial_eq_without_eq)]

mod autostart;
mod deep_link;
mod notify;
// `gpui-tray` bridges several GPUI type graphs behind cargo features; the workspace
// selects `gpui-kit`, whose real package is `gpui-pre`, so the tray types match.
mod tray;

use gpui::{App, actions};
use std::cell::RefCell;
use std::sync::OnceLock;

actions!(shell, [ShowMainWindow, QuitApp]);

thread_local! {
    static TRAY: RefCell<Option<gpui_tray::Tray>> =
        const { RefCell::new(None) };
}

static BRAND: OnceLock<Brand> = OnceLock::new();
static OPEN_MAIN: OnceLock<fn(&mut App)> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub struct Brand {
    pub product: &'static str,
    pub bundle_id: &'static str,
    pub url_scheme: &'static str,
    pub logo_png: &'static [u8],
}

pub use autostart::{
    is_enabled as autostart_enabled, set_enabled as set_autostart,
};
pub use deep_link::{DeepLink, parse as parse_deep_link, take_pending};
pub use notify::{NotifyKind, show_update_notification};

pub fn brand() -> Brand {
    *BRAND
        .get()
        .expect("flowy_shell::configure_brand before brand()")
}

/// Bind product identity before `install`. Needed for autostart paths that run
/// in `main` ahead of the GPUI `run` callback.
pub fn configure_brand(brand: Brand) {
    tracing::debug!(
        target: "flowy.boot",
        stage = "shell::configure_brand",
        product = brand.product,
        scheme = brand.url_scheme,
        "brand"
    );
    let _ = BRAND.set(brand);
}

pub fn enqueue_urls(urls: Vec<String>) {
    tracing::debug!(
        target: "flowy.boot",
        stage = "shell::enqueue_urls",
        count = urls.len(),
        "deep link"
    );
    deep_link::enqueue(urls);
}

pub fn install(brand: Brand, open_main: fn(&mut App), cx: &mut App) {
    tracing::debug!(
        target: "flowy.boot",
        stage = "shell::install",
        product = brand.product,
        "enter"
    );
    configure_brand(brand);
    let _ = OPEN_MAIN.set(open_main);
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
        let scheme = brand.url_scheme;
        tracing::debug!(
            target: "flowy.boot",
            stage = "shell::register_url_scheme",
            scheme,
            "register"
        );
        let task = cx.update(|cx| cx.register_url_scheme(scheme));
        let _ = task.await;
    })
    .detach();
    match tray::build(cx) {
        Ok(handle) => {
            TRAY.with(|slot| {
                *slot.borrow_mut() = Some(handle);
            });
            tracing::debug!(
                target: "flowy.boot",
                stage = "shell::tray::build",
                "tray ready"
            );
        }
        Err(error) => {
            tracing::warn!(
                target: "flowy.boot",
                stage = "shell::tray::build",
                %error,
                "system tray unavailable"
            );
        }
    }
    tracing::debug!(
        target: "flowy.boot",
        stage = "shell::install",
        "done"
    );
}

/// Whether the tray icon was created on this thread.
///
/// The close button hides the window instead of quitting, so callers must
/// fall back to a real close when no tray exists to bring it back.
pub fn tray_available() -> bool {
    TRAY.with(|slot| slot.borrow().is_some())
}

pub fn ensure_main_window(cx: &mut App) {
    tracing::debug!(
        target: "flowy.boot",
        stage = "shell::ensure_main_window",
        "open_main"
    );
    OPEN_MAIN.get().expect("flowy_shell::install")(cx);
}
