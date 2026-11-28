//! Asynchronous inline rich preview inside the right-hand deliverables drawer.
//!
//! Reads and classifies files off the UI thread with strict size/path bounds:
//! Markdown, CSV/TSV tables, JSON/code source, plain text, raster images up to
//! 8 MiB, and lightweight PDF/PPTX document cards that hand full-screen multi-page
//! viewing off to `FlowyApp::PreviewPanel`.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use flowy_ui as theme;
use gpui::{
    AnyElement, Context, FontWeight, Image, ImageFormat, IntoElement, Task,
    Window, div, img, prelude::*, px,
};

use crate::deliverables::short_parent_path;
use crate::panel::{StudioEvent, StudioPanel};

pub const MAX_DRAWER_TEXT_BYTES: u64 = 1024 * 1024;
pub const MAX_DRAWER_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_DISPLAY_CHARS: usize = 32_768;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewMode {
    #[default]
    Rendered,
    Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Pdf,
    Pptx,
}

impl DocumentKind {
    #[must_use]
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Pptx => "PPTX",
        }
    }
}

pub enum DrawerPreviewBody {
    Loading,
    Markdown(String),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        truncated_rows: bool,
        raw: String,
    },
    Code {
        language: String,
        formatted: String,
        raw: String,
        line_count: usize,
    },
    PlainText(String),
    Image {
        image: Arc<Image>,
        ext: String,
        bytes: u64,
    },
    DocumentCard {
        kind: DocumentKind,
        bytes: u64,
    },
    Notice(&'static str),
}

impl std::fmt::Debug for DrawerPreviewBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loading => f.write_str("DrawerPreviewBody::Loading"),
            Self::Markdown(s) => f
                .debug_tuple("DrawerPreviewBody::Markdown")
                .field(&s.len())
                .finish(),
            Self::Table { headers, rows, .. } => f
                .debug_struct("DrawerPreviewBody::Table")
                .field("cols", &headers.len())
                .field("rows", &rows.len())
                .finish(),
            Self::Code {
                language,
                line_count,
                ..
            } => f
                .debug_struct("DrawerPreviewBody::Code")
                .field("language", language)
                .field("line_count", line_count)
                .finish(),
            Self::PlainText(s) => f
                .debug_tuple("DrawerPreviewBody::PlainText")
                .field(&s.len())
                .finish(),
            Self::Image { ext, bytes, .. } => f
                .debug_struct("DrawerPreviewBody::Image")
                .field("ext", ext)
                .field("bytes", bytes)
                .finish(),
            Self::DocumentCard { kind, bytes } => f
                .debug_struct("DrawerPreviewBody::DocumentCard")
                .field("kind", kind)
                .field("bytes", bytes)
                .finish(),
            Self::Notice(key) => f
                .debug_tuple("DrawerPreviewBody::Notice")
                .field(key)
                .finish(),
        }
    }
}

#[derive(Default)]
pub struct DrawerPreviewState {
    pub file: Option<String>,
    pub mode: PreviewMode,
    pub body: Option<DrawerPreviewBody>,
    pub generation: u64,
    pub task: Option<Task<()>>,
    pub copied_generation: Option<u64>,
    pub copy_task: Option<Task<()>>,
}

impl DrawerPreviewState {
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.file = None;
        self.mode = PreviewMode::Rendered;
        self.body = None;
        self.task = None;
        self.copied_generation = None;
        self.copy_task = None;
    }
}

#[must_use]
pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

#[must_use]
pub const fn supports_mode_toggle(body: &DrawerPreviewBody) -> bool {
    matches!(
        body,
        DrawerPreviewBody::Markdown(_)
            | DrawerPreviewBody::Table { .. }
            | DrawerPreviewBody::Code { .. }
    )
}

#[must_use]
pub fn extension_supports_fullscreen(path: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map_or_else(String::new, str::to_ascii_lowercase);
    matches!(
        ext.as_str(),
        "pdf" | "pptx" | "png" | "jpg" | "jpeg" | "webp"
    ) || crate::library::is_text_preview_ext(&ext)
}

fn is_code_ext(ext: &str) -> bool {
    matches!(
        ext,
        "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "html"
            | "htm"
            | "css"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "py"
            | "rs"
            | "sql"
            | "sh"
            | "ps1"
            | "ini"
            | "conf"
            | "cfg"
            | "env"
    )
}

fn bound_display_text(text: &str) -> (String, bool) {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(MAX_DISPLAY_CHARS).collect();
    let truncated = chars.next().is_some();
    (head, truncated)
}

fn read_bounded_bytes(
    file: std::fs::File,
    declared_len: u64,
    limit: u64,
) -> Result<Vec<u8>, &'static str> {
    if declared_len > limit {
        return Err("chat.preview_too_large");
    }
    let mut bytes = Vec::with_capacity(
        usize::try_from(declared_len.min(limit)).unwrap_or(0),
    );
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "chat.preview_missing")?;
    if bytes.len() as u64 > limit {
        return Err("chat.preview_too_large");
    }
    Ok(bytes)
}

pub fn load_drawer_preview(path: &str) -> DrawerPreviewBody {
    let Ok((safe_path, file, meta)) =
        crate::ports::open_safe_local_regular_file(Path::new(path))
    else {
        return DrawerPreviewBody::Notice("chat.preview_missing");
    };

    let extension = safe_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map_or_else(String::new, str::to_ascii_lowercase);

    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => {
            let bytes = match read_bounded_bytes(
                file,
                meta.len(),
                MAX_DRAWER_IMAGE_BYTES,
            ) {
                Ok(bytes) => bytes,
                Err(notice) => return DrawerPreviewBody::Notice(notice),
            };
            let byte_len = bytes.len() as u64;
            let format = match extension.as_str() {
                "png" => ImageFormat::Png,
                "webp" => ImageFormat::Webp,
                _ => ImageFormat::Jpeg,
            };
            DrawerPreviewBody::Image {
                image: Arc::new(Image::from_bytes(format, bytes)),
                ext: extension,
                bytes: byte_len,
            }
        }
        "pdf" => DrawerPreviewBody::DocumentCard {
            kind: DocumentKind::Pdf,
            bytes: meta.len(),
        },
        "pptx" => DrawerPreviewBody::DocumentCard {
            kind: DocumentKind::Pptx,
            bytes: meta.len(),
        },
        _ => {
            let bytes = match read_bounded_bytes(
                file,
                meta.len(),
                MAX_DRAWER_TEXT_BYTES,
            ) {
                Ok(bytes) => bytes,
                Err(notice) => return DrawerPreviewBody::Notice(notice),
            };
            if bytes.contains(&0) {
                return DrawerPreviewBody::Notice("chat.preview_binary");
            }
            let text = String::from_utf8_lossy(&bytes).into_owned();
            match extension.as_str() {
                "md" | "markdown" => DrawerPreviewBody::Markdown(text),
                "csv" | "tsv" => {
                    let delimiter = if extension == "tsv" { '\t' } else { ',' };
                    let (headers, rows, truncated_rows) =
                        crate::library::parse_delimited_table(&text, delimiter);
                    DrawerPreviewBody::Table {
                        headers,
                        rows,
                        truncated_rows,
                        raw: text,
                    }
                }
                ext if is_code_ext(ext) => {
                    let formatted = if ext == "json" {
                        crate::library::pretty_json_if_valid(&text)
                            .unwrap_or_else(|| text.clone())
                    } else {
                        text.clone()
                    };
                    let line_count = formatted
                        .lines()
                        .count()
                        .max(usize::from(!formatted.is_empty()));
                    DrawerPreviewBody::Code {
                        language: extension,
                        formatted,
                        raw: text,
                        line_count,
                    }
                }
                _ => DrawerPreviewBody::PlainText(text),
            }
        }
    }
}

fn copyable_text(body: &DrawerPreviewBody, mode: PreviewMode) -> Option<&str> {
    match body {
        DrawerPreviewBody::Markdown(text)
        | DrawerPreviewBody::PlainText(text) => Some(text.as_str()),
        DrawerPreviewBody::Table { raw, .. } => Some(raw.as_str()),
        DrawerPreviewBody::Code { formatted, raw, .. } => {
            Some(if mode == PreviewMode::Source {
                raw.as_str()
            } else {
                formatted.as_str()
            })
        }
        _ => None,
    }
}

impl StudioPanel {
    pub fn drawer_preview_view(
        &self,
        path: &str,
        body: &DrawerPreviewBody,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let name = crate::ports::basename(path);
        let parent = short_parent_path(path);
        let mode = self.drawer_preview.mode;
        let generation = self.drawer_preview.generation;
        let copied = self.drawer_preview.copied_generation == Some(generation);

        let back_button = self.icon_tooltip_button(
            "preview-back",
            "preview-back-tip",
            self.t("chat.back"),
            theme::icons::arrow_left().size(px(14.)),
            theme::ControlVariant::Ghost,
            |panel, _, cx| {
                panel.set_preview_file(None, cx);
            },
            window,
            cx,
        );

        let mode_button = supports_mode_toggle(body).then(|| {
            let next_label = if mode == PreviewMode::Rendered {
                self.t("drawer.mode_source")
            } else {
                self.t("drawer.mode_rendered")
            };
            let icon = if mode == PreviewMode::Rendered {
                theme::icons::file_text().size(px(14.))
            } else {
                theme::icons::eye().size(px(14.))
            };
            self.icon_tooltip_button(
                "preview-mode-toggle",
                "preview-mode-toggle-tip",
                next_label,
                icon,
                if mode == PreviewMode::Source {
                    theme::ControlVariant::Secondary
                } else {
                    theme::ControlVariant::Ghost
                },
                |panel, _, cx| {
                    panel.toggle_drawer_preview_mode(cx);
                },
                window,
                cx,
            )
        });

        let copy_button = copyable_text(body, mode).map(|text| {
            let text = text.to_owned();
            let (label, icon) = if copied {
                (self.t("chat.copied"), theme::icons::check().size(px(14.)))
            } else {
                (self.t("drawer.copy"), theme::icons::copy().size(px(14.)))
            };
            self.icon_tooltip_button(
                "preview-copy",
                "preview-copy-tip",
                label,
                icon,
                theme::ControlVariant::Ghost,
                move |panel, _, cx| {
                    panel.copy_drawer_preview(generation, text.clone(), cx);
                },
                window,
                cx,
            )
        });

        let quote_path = path.to_owned();
        let quote_button = self.icon_tooltip_button(
            "preview-quote-file",
            "preview-quote-file-tip",
            self.t("drawer.quote_file"),
            theme::icons::quote().size(px(14.)),
            theme::ControlVariant::Ghost,
            move |panel, window, cx| {
                panel.quote_file_to_composer(&quote_path, Some(window), cx);
            },
            window,
            cx,
        );

        let reveal_path = path.to_owned();
        let reveal_button = self.icon_tooltip_button(
            "preview-reveal-folder",
            "preview-reveal-folder-tip",
            self.t("board.reveal_folder"),
            theme::icons::folder_open().size(px(14.)),
            theme::ControlVariant::Ghost,
            move |panel, _, cx| {
                panel.reveal_file(&reveal_path, cx);
            },
            window,
            cx,
        );

        let open_path = path.to_owned();
        let open_button = flowy_agent::can_open_with_default_app(&open_path)
            .then(|| {
                self.icon_tooltip_button(
                    "preview-open-external",
                    "preview-open-external-tip",
                    self.t("deliverables.open_external"),
                    theme::icons::external_link().size(px(14.)),
                    theme::ControlVariant::Ghost,
                    move |panel, _, cx| {
                        panel.open_file_externally(&open_path, cx);
                    },
                    window,
                    cx,
                )
            });

        let fullscreen_path = path.to_owned();
        let fullscreen_button =
            (!matches!(
                body,
                DrawerPreviewBody::Loading | DrawerPreviewBody::Notice(_)
            ) && extension_supports_fullscreen(&fullscreen_path))
            .then(|| {
                self.icon_tooltip_button(
                    "preview-fullscreen",
                    "preview-fullscreen-tip",
                    self.t("drawer.open_fullscreen"),
                    theme::icons::maximize().size(px(14.)),
                    theme::ControlVariant::Ghost,
                    move |_, _, cx| {
                        cx.emit(StudioEvent::PreviewLocalFile(
                            fullscreen_path.clone(),
                        ));
                    },
                    window,
                    cx,
                )
            });

        let rendered_body =
            self.render_drawer_preview_body(path, body, mode, cx);

        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(6.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .child(back_button)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(self.theme.ink())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .truncate()
                                    .child(name),
                            )
                            .when(!parent.is_empty(), |col| {
                                col.child(
                                    div()
                                        .text_xs()
                                        .text_color(self.theme.muted())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(parent),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .children(mode_button)
                            .children(copy_button)
                            .child(quote_button)
                            .child(reveal_button)
                            .children(open_button)
                            .children(fullscreen_button),
                    ),
            )
            .child(rendered_body)
            .into_any_element()
    }

    fn render_drawer_preview_body(
        &self,
        path: &str,
        body: &DrawerPreviewBody,
        mode: PreviewMode,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        match body {
            DrawerPreviewBody::Loading => div()
                .w_full()
                .p(self.theme.space(16.))
                .flex()
                .items_center()
                .gap(self.theme.space(8.))
                .text_xs()
                .text_color(self.theme.muted())
                .child(
                    theme::icons::loader_circle()
                        .size(px(14.))
                        .text_color(self.theme.muted()),
                )
                .child(self.t("library.preview.loading"))
                .into_any_element(),
            DrawerPreviewBody::Notice(key) => {
                let reveal_path = path.to_owned();
                div()
                    .w_full()
                    .p(self.theme.space(12.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .w_full()
                            .p(self.theme.space(12.))
                            .rounded_xl()
                            .border_1()
                            .border_color(self.theme.line())
                            .bg(self.theme.canvas())
                            .flex()
                            .flex_col()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .whitespace_normal()
                                    .child(self.t(key)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(self.theme.space(6.))
                                    .child(
                                        theme::Button::new(
                                            "drawer-notice-reveal",
                                            self.t("board.reveal_folder"),
                                        )
                                        .variant(
                                            theme::ControlVariant::Secondary,
                                        )
                                        .size(theme::ControlSize::Small)
                                        .render(
                                            &self.theme,
                                            cx.listener(
                                                move |panel, _, _, cx| {
                                                    panel.reveal_file(
                                                        &reveal_path,
                                                        cx,
                                                    );
                                                },
                                            ),
                                        ),
                                    ),
                            ),
                    )
                    .into_any_element()
            }
            DrawerPreviewBody::Markdown(text) => {
                if mode == PreviewMode::Source {
                    self.render_source_code_block(None, None, text)
                } else {
                    let (bounded, truncated) = bound_display_text(text);
                    div()
                        .w_full()
                        .p(self.theme.space(12.))
                        .flex()
                        .flex_col()
                        .gap(self.theme.space(8.))
                        .child(crate::markdown::render(
                            &bounded, self.theme, &self.i18n,
                        ))
                        .when(truncated, |col| {
                            col.child(self.preview_truncated_banner())
                        })
                        .into_any_element()
                }
            }
            DrawerPreviewBody::Table {
                headers,
                rows,
                truncated_rows,
                raw,
            } => {
                if mode == PreviewMode::Source {
                    self.render_source_code_block(None, None, raw)
                } else {
                    div()
                        .w_full()
                        .p(self.theme.space(12.))
                        .flex()
                        .flex_col()
                        .gap(self.theme.space(8.))
                        .child(crate::library::render_table_preview(
                            crate::library::TablePreview {
                                headers,
                                rows,
                                truncated_rows: *truncated_rows,
                            },
                            self.theme,
                            self.t("a11y.scroll_table_preview"),
                        ))
                        .when(*truncated_rows, |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .child(self.t("drawer.table_truncated")),
                            )
                        })
                        .into_any_element()
                }
            }
            DrawerPreviewBody::Code {
                language,
                formatted,
                raw,
                line_count,
            } => {
                let text = if mode == PreviewMode::Source {
                    raw
                } else {
                    formatted
                };
                self.render_source_code_block(
                    Some(language.as_str()),
                    Some(*line_count),
                    text,
                )
            }
            DrawerPreviewBody::PlainText(text) => {
                self.render_source_code_block(None, None, text)
            }
            DrawerPreviewBody::Image { image, ext, bytes } => {
                let full_path = path.to_owned();
                div()
                    .w_full()
                    .p(self.theme.space(12.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(8.))
                    .child(
                        div()
                            .w_full()
                            .rounded_lg()
                            .border_1()
                            .border_color(self.theme.line())
                            .overflow_hidden()
                            .bg(self.theme.canvas())
                            .child(img(image.clone()).w_full()),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(self.theme.space(8.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .child(format!(
                                        "{} · {}",
                                        ext.to_ascii_uppercase(),
                                        format_file_size(*bytes)
                                    )),
                            )
                            .child(
                                theme::Button::new(
                                    "drawer-image-fullscreen",
                                    self.t("drawer.open_fullscreen"),
                                )
                                .variant(theme::ControlVariant::Secondary)
                                .size(theme::ControlSize::Small)
                                .icon(theme::icons::maximize())
                                .render(
                                    &self.theme,
                                    cx.listener(move |_, _, _, cx| {
                                        cx.emit(StudioEvent::PreviewLocalFile(
                                            full_path.clone(),
                                        ));
                                    }),
                                ),
                            ),
                    )
                    .into_any_element()
            }
            DrawerPreviewBody::DocumentCard { kind, bytes } => {
                let full_path = path.to_owned();
                let ext_path = path.to_owned();
                let name = crate::ports::basename(path);
                let icon = crate::file_kind::from_path(path).map_or_else(
                    || {
                        theme::icons::file_text()
                            .size(px(28.))
                            .text_color(self.theme.muted())
                            .into_any_element()
                    },
                    |fk| fk.icon().size(px(32.)).into_any_element(),
                );
                div()
                    .w_full()
                    .p(self.theme.space(12.))
                    .child(
                        div()
                            .w_full()
                            .p(self.theme.space(16.))
                            .rounded_xl()
                            .border_1()
                            .border_color(self.theme.line())
                            .bg(self.theme.canvas())
                            .flex()
                            .flex_col()
                            .gap(self.theme.space(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(self.theme.space(10.))
                                    .child(div().flex_none().child(icon))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(
                                                        FontWeight::SEMIBOLD,
                                                    )
                                                    .text_color(self.theme.ink())
                                                    .overflow_hidden()
                                                    .whitespace_nowrap()
                                                    .truncate()
                                                    .child(name),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(
                                                        self.theme.muted(),
                                                    )
                                                    .child(format!(
                                                        "{} · {}",
                                                        kind.badge_label(),
                                                        format_file_size(
                                                            *bytes
                                                        )
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .whitespace_normal()
                                    .child(self.t(
                                        "drawer.document_preview_hint",
                                    )),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(self.theme.space(8.))
                                    .child(
                                        theme::Button::new(
                                            "drawer-document-fullscreen",
                                            self.t("drawer.open_fullscreen"),
                                        )
                                        .variant(
                                            theme::ControlVariant::Primary,
                                        )
                                        .size(theme::ControlSize::Small)
                                        .icon(theme::icons::maximize())
                                        .render(
                                            &self.theme,
                                            cx.listener(move |_, _, _, cx| {
                                                cx.emit(
                                                    StudioEvent::PreviewLocalFile(
                                                        full_path.clone(),
                                                    ),
                                                );
                                            }),
                                        ),
                                    )
                                    .when(
                                        flowy_agent::can_open_with_default_app(
                                            &ext_path,
                                        ),
                                        |row| {
                                            row.child(
                                                theme::Button::new(
                                                    "drawer-document-external",
                                                    self.t(
                                                        "deliverables.open_external",
                                                    ),
                                                )
                                                .variant(
                                                    theme::ControlVariant::Secondary,
                                                )
                                                .size(theme::ControlSize::Small)
                                                .icon(theme::icons::external_link())
                                                .render(
                                                    &self.theme,
                                                    cx.listener(move |panel, _, _, cx| {
                                                        panel.open_file_externally(
                                                            &ext_path,
                                                            cx,
                                                        );
                                                    }),
                                                ),
                                            )
                                        },
                                    ),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_source_code_block(
        &self,
        language: Option<&str>,
        line_count: Option<usize>,
        text: &str,
    ) -> AnyElement {
        let (bounded, truncated) = bound_display_text(text);
        let meta = match (language, line_count) {
            (Some(lang), Some(lines)) => {
                let line_label = self
                    .tf("drawer.code_lines", &[("count", &lines.to_string())]);
                Some(format!("{} · {line_label}", lang.to_ascii_uppercase()))
            }
            (Some(lang), None) => Some(lang.to_ascii_uppercase()),
            _ => None,
        };
        div()
            .w_full()
            .p(self.theme.space(12.))
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .w_full()
                    .rounded_lg()
                    .border_1()
                    .border_color(self.theme.line())
                    .bg(self.theme.soft().opacity(0.6))
                    .p(self.theme.space(10.))
                    .flex()
                    .flex_col()
                    .gap(self.theme.space(6.))
                    .when_some(meta, |card, meta_text| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(self.theme.muted())
                                .child(meta_text),
                        )
                    })
                    .child(
                        theme::scroll_area("drawer-source-scroll", &self.theme)
                            .horizontal()
                            .a11y_label(self.t("a11y.scroll_source_code"))
                            .debug_selector(|| {
                                "drawer-source-scroll".to_owned()
                            })
                            .w_full()
                            .viewport_style({
                                let ink = self.theme.ink();
                                let font_family =
                                    crate::markdown::mono_font().family;
                                move |s| {
                                    s.pb(px(14.))
                                        .text_xs()
                                        .text_color(ink)
                                        .font_family(font_family)
                                }
                            })
                            .child(bounded),
                    ),
            )
            .when(truncated, |col| col.child(self.preview_truncated_banner()))
            .into_any_element()
    }

    fn preview_truncated_banner(&self) -> AnyElement {
        div()
            .w_full()
            .px(self.theme.space(8.))
            .py(px(4.))
            .rounded_md()
            .bg(self.theme.soft())
            .text_xs()
            .text_color(self.theme.muted())
            .child(self.t("drawer.preview_truncated"))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DocumentKind, DrawerPreviewBody, MAX_DRAWER_TEXT_BYTES,
        load_drawer_preview,
    };

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "flowy-drawer-preview-{}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn missing_and_binary_and_oversized_files_report_expected_notices() {
        assert!(matches!(
            load_drawer_preview("flowy-definitely-absent-98765.txt"),
            DrawerPreviewBody::Notice("chat.preview_missing")
        ));

        let bin_path = scratch("binary.dat");
        std::fs::write(&bin_path, [0u8, 1, 2, 3]).unwrap();
        assert!(matches!(
            load_drawer_preview(bin_path.to_str().unwrap()),
            DrawerPreviewBody::Notice("chat.preview_binary")
        ));
        let _ = std::fs::remove_file(bin_path);

        let big_path = scratch("big.txt");
        let data = vec![b'a'; (MAX_DRAWER_TEXT_BYTES + 1) as usize];
        std::fs::write(&big_path, data).unwrap();
        assert!(matches!(
            load_drawer_preview(big_path.to_str().unwrap()),
            DrawerPreviewBody::Notice("chat.preview_too_large")
        ));
        let _ = std::fs::remove_file(big_path);
    }

    #[test]
    fn rich_formats_classify_markdown_csv_json_text_and_pdf() {
        let md_path = scratch("report.md");
        std::fs::write(&md_path, "# Title\n- item").unwrap();
        assert!(matches!(
            load_drawer_preview(md_path.to_str().unwrap()),
            DrawerPreviewBody::Markdown(ref s) if s.contains("# Title")
        ));
        let _ = std::fs::remove_file(md_path);

        let csv_path = scratch("metrics.csv");
        std::fs::write(&csv_path, "name,score\nalpha,99\n").unwrap();
        match load_drawer_preview(csv_path.to_str().unwrap()) {
            DrawerPreviewBody::Table { headers, rows, .. } => {
                assert_eq!(headers, vec!["name", "score"]);
                assert_eq!(rows, vec![vec!["alpha", "99"]]);
            }
            _ => panic!("expected csv table"),
        }
        let _ = std::fs::remove_file(csv_path);

        let json_path = scratch("config.json");
        std::fs::write(&json_path, r#"{"a":1,"b":"ok"}"#).unwrap();
        match load_drawer_preview(json_path.to_str().unwrap()) {
            DrawerPreviewBody::Code {
                language,
                formatted,
                ..
            } => {
                assert_eq!(language, "json");
                assert!(formatted.contains("  \"a\": 1"));
            }
            _ => panic!("expected code"),
        }
        let _ = std::fs::remove_file(json_path);

        let pdf_path = scratch("deck.pdf");
        std::fs::write(&pdf_path, b"%PDF-1.4\n1 0 obj\n").unwrap();
        assert!(matches!(
            load_drawer_preview(pdf_path.to_str().unwrap()),
            DrawerPreviewBody::DocumentCard {
                kind: DocumentKind::Pdf,
                ..
            }
        ));
        let _ = std::fs::remove_file(pdf_path);
    }
}
