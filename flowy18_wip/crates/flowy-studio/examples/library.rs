//! Standalone Library demo; its fixture is never imported by the product.
//! cargo run -p flowy-studio --example library
use flowy_i18n::{I18n, Locale};
use flowy_studio::library::{
    ArtifactAvailability, Category, LibraryEvent, LibraryItem, LibraryPanel,
};
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
            .child(self.panel.clone())
    }
}

fn main() {
    gpui_platform::application().run(|cx| {
        // GPUI Kit owns the editing keymap for the `"Input"` context its
        // fields render with. Install it first so the demo's own bindings win
        // at the same context depth.
        gpui_kit::base::init(cx);
        cx.bind_keys([
            // Tab moves focus instead of inserting an indent in a field.
            KeyBinding::new("tab", Next, Some("Input")),
            KeyBinding::new("shift-tab", Previous, Some("Input")),
            KeyBinding::new("tab", Next, None),
            KeyBinding::new("shift-tab", Previous, None),
            KeyBinding::new("f8", Capture, None),
            KeyBinding::new("f6", ToggleTheme, None),
        ]);
        flowy_ui::install_initial_tab_focus(cx).detach();
        let dimensions = if std::env::var_os("FLOWY_LIBRARY_SMALL").is_some() {
            size(px(800.), px(600.))
        } else {
            size(px(1584.), px(982.))
        };
        let bounds = Bounds::centered(None, dimensions, cx);
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
                    let args: Vec<_> = std::env::args().collect();
                    let mut i18n = I18n::embedded("Flowy");
                    i18n.set_locale(if args.iter().any(|arg| arg == "--en") {
                        Locale::En
                    } else {
                        Locale::ZhCn
                    });
                    let mode = if args.iter().any(|arg| arg == "--dark") {
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
                    let mut items: Vec<LibraryItem> = serde_json::from_str(
                        include_str!("fixtures/library.json"),
                    )
                    .expect("valid demo fixture");
                    let mut file_items = Vec::new();
                    let mut file_payloads = std::collections::HashMap::new();
                    for pair in
                        args.windows(2).filter(|pair| pair[0] == "--file")
                    {
                        let path = std::path::PathBuf::from(&pair[1])
                            .canonicalize()
                            .expect("preview file exists");
                        let id = format!("fixture-{}", file_items.len());
                        let bytes = std::fs::read(&path)
                            .expect("read explicit preview fixture");
                        file_payloads.insert(id.clone(), bytes);
                        file_items.push(LibraryItem {
                            id,
                            title: path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .into_owned(),
                            original_name: path
                                .file_name()
                                .unwrap()
                                .to_string_lossy()
                                .into_owned(),
                            description: path.display().to_string(),
                            category: Category::Document,
                            date: "2026-11-20".into(),
                            time: "12:00".into(),
                            pending: false,
                            preview: None,
                            size_bytes: path.metadata().ok().map(|m| m.len()),
                            extension: path
                                .extension()
                                .and_then(|value| value.to_str())
                                .unwrap_or_default()
                                .into(),
                            media_type: None,
                            delivered_at_ms: 0,
                            delivered_date: "2026-11-20".into(),
                            delivered_time: "12:00".into(),
                            source_session_id: String::new(),
                            source_title: "Explicit preview fixture".into(),
                            source_deleted: false,
                            source_unavailable: false,
                            favorite: false,
                            availability: ArtifactAvailability::Available,
                        });
                    }
                    if !file_items.is_empty() {
                        items = file_items;
                    }
                    let empty = args.iter().any(|arg| arg == "--empty");
                    let search_empty =
                        args.iter().any(|arg| arg == "--search-empty");
                    let error = args.iter().any(|arg| arg == "--error");
                    let loading = args.iter().any(|arg| arg == "--loading");
                    if empty || search_empty || error || loading {
                        items.clear();
                    }
                    let first = items.first().map(|item| item.id.clone());
                    let preview = args.iter().any(|arg| arg == "--preview");
                    cx.new(|cx| {
                        let panel = cx.new(|cx| {
                            let mut panel =
                                LibraryPanel::new(items, i18n, ui.clone(), cx);
                            panel.use_internal_previews();
                            if search_empty {
                                // An explicit filter with no source records exercises
                                // the no-match branch without adding a production
                                // query setter or fabricating an artifact.
                                panel.model.category = Category::Image;
                            }
                            if error {
                                panel.set_status(
                                    Some("library.error.catalog"),
                                    cx,
                                );
                            }
                            panel.set_loading(loading, cx);
                            panel
                        });
                        cx.subscribe(
                            &panel,
                            move |this: &mut LibraryDemo,
                                  _,
                                  event: &LibraryEvent,
                                  cx| {
                                if let LibraryEvent::ReadPreviewRequested {
                                    artifact_id,
                                    request_id,
                                } = event
                                {
                                    let result = file_payloads
                                        .get(artifact_id)
                                        .cloned()
                                        .ok_or("library.preview.unavailable");
                                    this.panel.update(cx, |panel, cx| {
                                        panel.set_preview_bytes(
                                            artifact_id,
                                            *request_id,
                                            result,
                                            cx,
                                        );
                                    });
                                }
                            },
                        )
                        .detach();
                        if preview
                            && let Some(id) = first {
                                panel.update(cx, |panel, cx| {
                                    panel.preview_item(&id, window, cx);
                                });
                            }
                        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                        LibraryDemo { panel, ui }
                    })
                },
            )
            .expect("open isolated library demo");
        if let Some(path) = std::env::var_os("FLOWY_LIBRARY_CAPTURE") {
            let capture_handle: gpui::AnyWindowHandle = handle.into();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(
                        std::env::var("FLOWY_LIBRARY_CAPTURE_DELAY_MS")
                            .ok()
                            .and_then(|value| value.parse().ok())
                            .unwrap_or(800),
                    ))
                    .await;
                // Occluded/headless native windows may not receive OS paint
                // callbacks. Drive layout and async image upload before capture.
                for _ in 0..3 {
                    let _ = capture_handle.update(cx, |_, window, cx| {
                        window.refresh();
                        window.draw(cx).clear(cx);
                    });
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(80))
                        .await;
                }
                let _ = capture_handle.update(cx, |_, window, cx| {
                    if let Err(error) =
                        window.render_to_image().and_then(|image| {
                            image
                                .save(std::path::Path::new(&path))
                                .map_err(Into::into)
                        })
                    {
                        eprintln!("Library startup capture failed: {error}");
                    }
                    if std::env::var_os("FLOWY_LIBRARY_CAPTURE_EXIT").is_some()
                    {
                        cx.quit();
                    }
                });
            })
            .detach();
        }
        cx.activate(true);
    });
}
