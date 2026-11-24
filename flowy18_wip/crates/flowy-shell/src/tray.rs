//! System tray icon with a show/quit menu.
//!
//! `gpui-tray` bridges three GPUI type graphs behind cargo features; the
//! workspace selects `gpui-kit`, whose real package is `gpui-pre` — the same
//! crate the rest of the app uses, so the icon type is shared rather than
//! duplicated. See the workspace `Cargo.toml` for that pin.

use super::{QuitApp, ShowMainWindow};
use gpui::{App, Image, ImageFormat, MenuItem};
use gpui_tray::{Icon, Tray};

/// Builds the tray icon. Call once, at startup, after the app is running.
pub fn build(cx: &mut App) -> Result<Tray, gpui_tray::Error> {
    let image =
        Image::from_bytes(ImageFormat::Png, crate::brand().logo_png.to_vec());
    let icon = Icon::from_gpui(&image, cx).build()?;
    let product = crate::brand().product;
    Tray::builder()
        .icon(icon)
        .title(product)
        .tooltip(product)
        .menu(|_cx| {
            vec![
                MenuItem::action("Show", ShowMainWindow),
                MenuItem::separator(),
                MenuItem::action("Quit", QuitApp),
            ]
        })
        .on_activate(ShowMainWindow)
        .build(cx)
}
