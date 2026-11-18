//! Standalone Library demo; its fixture is never imported by the product.
//! cargo run -p flowy-studio --example library
use flowy_i18n::{I18n, Locale};
use flowy_studio::library::{LibraryItem, LibraryPanel};
use flowy_ui::{ThemeMode, ThemeOptions, UiContext};
use gpui::{
    Bounds, Context, Entity, KeyBinding, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};

#[allow(clippy::derive_partial_eq_without_eq)]
mod actions {
    gpui::actions!(library_demo, [Next, Previous, Capture, ToggleTheme]);
}
use actions::{Capture, Next, Previous, ToggleTheme};

struct LibraryDemo {
    panel: Entity<LibraryPanel>,
    ui: UiContext,
}

impl Render for LibraryDemo {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        self.panel
            .update(cx, |panel, cx| panel.sync_overlays(window, cx));
        div()
            .size_full()
            .on_action(|_: &Next, window, cx| window.focus_next(cx))
            .on_action(|_: &Previous, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|this, _: &ToggleTheme, _, cx| {
                this.ui.theme.update(cx, |state, cx| {
                    let current = state.resolved();
                    let mode = if current.dark {
                        ThemeMode::Light
                    } else {
                        ThemeMode::Dark
                    };
                    state.update(
                        ThemeOptions {
                            mode,
                            ..current.options
                        },
                        false,
                        cx,
                    );
                });
            }))
            .on_action(|_: &Capture, window, _| {
                let path = std::env::var_os("FLOWY_LIBRARY_CAPTURE")
                    .map_or_else(
                        || {
                            std::path::PathBuf::from(
                                ".scratch/library-native.png",
                            )
                        },
                        std::path::PathBuf::from,
                    );
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match window
                    .render_to_image()
                    .and_then(|image| image.save(&path).map_err(Into::into))
                {
                    Ok(()) => {
                        eprintln!("Library render saved to {}", path.display());
                    }
                    Err(error) => eprintln!("Library capture failed: {error}"),
                }
            })
            .on_action(
                |_: &gpui_ce_elements::editable_text::actions::Tab,
                 window,
                 cx| window.focus_next(cx),
            )
            .child(self.panel.clone())
    }
}

fn main() {
    gpui_platform::application().run(|cx| {
        cx.bind_keys([
            KeyBinding::new("tab", Next, None),
            KeyBinding::new("shift-tab", Previous, None),
            KeyBinding::new("f8", Capture, None),
            KeyBinding::new("f6", ToggleTheme, None),
        ]);
        cx.bind_keys(
            gpui_ce_elements::editable_text::actions::default_bindings()
                .as_keybindings(Some(
                gpui_ce_elements::editable_text::actions::DEFAULT_INPUT_CONTEXT,
            )),
        );
        flowy_ui::install_initial_tab_focus(cx).detach();
        let bounds = Bounds::centered(None, size(px(1584.), px(982.)), cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("Flowy Library Demo".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(800.), px(600.))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_rem_size(px(16.));
                    let mut i18n = I18n::embedded("Flowy");
                    i18n.set_locale(Locale::ZhCn);
                    let mode = if std::env::args().any(|arg| arg == "--dark") {
                        ThemeMode::Dark
                    } else {
                        ThemeMode::Light
                    };
                    let ui = UiContext::new(
                        ThemeOptions {
                            mode,
                            ..Default::default()
                        },
                        false,
                        cx,
                    );
                    let items: Vec<LibraryItem> = serde_json::from_str(
                        include_str!("fixtures/library.json"),
                    )
                    .expect("valid demo fixture");
                    cx.new(|cx| {
                        let panel = cx.new(|cx| {
                            LibraryPanel::new(items, i18n, ui.clone(), cx)
                        });
                        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                        LibraryDemo { panel, ui }
                    })
                },
            )
            .expect("open isolated library demo");
        if let Some(path) = std::env::var_os("FLOWY_LIBRARY_CAPTURE") {
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(800))
                    .await;
                let _ = handle.update(cx, |_, window, _| {
                    if let Err(error) =
                        window.render_to_image().and_then(|image| {
                            image
                                .save(std::path::Path::new(&path))
                                .map_err(Into::into)
                        })
                    {
                        eprintln!("Library startup capture failed: {error}");
                    }
                });
            })
            .detach();
        }
        cx.activate(true);
    });
}
