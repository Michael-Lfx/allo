//! Deliverables drawer: the files one turn produced, with a light preview.
//!
//! Kept apart from `starter` and `feed` so the drawer can be replaced without
//! touching the chat projection or the home stage.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Arc;

use flowy_agent::FileEntry;
use flowy_ui as theme;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, FontWeight, Image,
    ImageFormat, IntoElement, SharedString, div, img, prelude::*, px,
};

use crate::panel::StudioPanel;

/// Classifies a deliverable path into its icon and localized file-kind key.
fn deliverable_kind(
    path: &str,
    theme: theme::ResolvedTheme,
) -> (AnyElement, &'static str) {
    let ext = Path::new(path)
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("pdf") => (
            theme::icons::pdf_file().size(px(20.)).into_any_element(),
            "deliverables.kind_document",
        ),
        Some("doc" | "docx" | "odt" | "rtf" | "md" | "markdown" | "txt") => (
            theme::icons::docx_file().size(px(20.)).into_any_element(),
            "deliverables.kind_document",
        ),
        Some("ppt" | "pptx" | "odp" | "key") => (
            theme::icons::ppt_file().size(px(20.)).into_any_element(),
            "deliverables.kind_document",
        ),
        Some("xls" | "xlsx" | "ods" | "csv" | "tsv" | "parquet" | "sql") => (
            theme::icons::xls_file().size(px(20.)).into_any_element(),
            "deliverables.kind_data",
        ),
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif",
        ) => (
            theme::icons::jpg_file().size(px(20.)).into_any_element(),
            "deliverables.kind_image",
        ),
        Some(
            "c" | "cc" | "cpp" | "cs" | "css" | "go" | "h" | "hpp" | "htm"
            | "html" | "java" | "js" | "json" | "jsx" | "kt" | "kts" | "php"
            | "ps1" | "py" | "rb" | "rs" | "scss" | "sh" | "swift" | "toml"
            | "ts" | "tsx" | "xml" | "yaml" | "yml",
        ) => (
            theme::icons::code_file().size(px(20.)).into_any_element(),
            "deliverables.kind_code",
        ),
        Some("7z" | "bz2" | "gz" | "rar" | "tar" | "tgz" | "xz" | "zip") => (
            theme::icons::zip_file().size(px(20.)).into_any_element(),
            "deliverables.kind_file",
        ),
        _ => (
            theme::icons::document_file()
                .size(px(18.))
                .text_color(theme.muted())
                .into_any_element(),
            "deliverables.kind_file",
        ),
    }
}

/// Truncated parent directory path, keeping the tail legible in a narrow drawer.
fn short_parent_path(path: &str) -> String {
    let parent = Path::new(path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default();
    const MAX: usize = 42;
    let count = parent.chars().count();
    if count <= MAX {
        return parent.to_owned();
    }
    let tail: String = parent.chars().skip(count - (MAX - 1)).collect();
    format!("\u{2026}{tail}")
}

/// Reading more than this on the UI path would stall the frame.
const MAX_PREVIEW_BYTES: u64 = 256 * 1024;

/// What the drawer shows for the selected file.
pub enum PreviewBody {
    Text(String),
    Image(Arc<Image>),
    /// Locale key explaining why the file cannot be shown.
    Notice(&'static str),
}

/// Reads one file for preview, classifying it by extension and size.
pub fn load_preview(path: &str) -> PreviewBody {
    let path = Path::new(path);
    let Ok(meta) = std::fs::metadata(path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    if !meta.is_file() {
        return PreviewBody::Notice("chat.preview_missing");
    }
    if meta.len() > MAX_PREVIEW_BYTES {
        return PreviewBody::Notice("chat.preview_too_large");
    }
    let Ok(bytes) = std::fs::read(path) else {
        return PreviewBody::Notice("chat.preview_missing");
    };
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => PreviewBody::Image(Arc::new(Image::from_bytes(
            ImageFormat::Png,
            bytes,
        ))),
        Some("jpg" | "jpeg") => PreviewBody::Image(Arc::new(
            Image::from_bytes(ImageFormat::Jpeg, bytes),
        )),
        Some("webp") => PreviewBody::Image(Arc::new(Image::from_bytes(
            ImageFormat::Webp,
            bytes,
        ))),
        _ => {
            if bytes.contains(&0) {
                return PreviewBody::Notice("chat.preview_binary");
            }
            PreviewBody::Text(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

impl StudioPanel {
    /// The whole right-hand drawer.
    pub(crate) fn deliverables_drawer(
        &self,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let files = self.deliverables.as_deref().unwrap_or(&[]);
        let title = self.t("chat.deliverables");
        let body = match (&self.preview_file, &self.preview_body) {
            (Some(path), Some(preview)) => {
                self.preview_view(path, preview, cx).into_any_element()
            }
            _ => self.deliverables_list(files, cx),
        };
        div()
            .id("deliverables-drawer")
            .w(px(360.))
            .h_full()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(self.theme.line())
            .bg(self.theme.card())
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(10.))
                    .child(div().flex_1().min_w_0().text_sm().child(title))
                    .child(
                        theme::Button::new(
                            "deliverables-close",
                            self.t("chat.close"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(|panel, _, _, cx| {
                                panel.close_deliverables(cx);
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .id("deliverables-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .with_animation(
                // The drawer travels in from the edge. Closing removes it at
                // once, which reads as a dismiss rather than a broken exit.
                "deliverables-drawer",
                Animation::new(crate::motion::SLOW)
                    .with_easing(crate::motion::out_quint()),
                |drawer, delta| {
                    drawer
                        .opacity(delta)
                        .relative()
                        .left(px(16. * (1.0 - delta)))
                },
            )
    }

    fn deliverables_list(
        &self,
        files: &[FileEntry],
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        if files.is_empty() {
            return div()
                .w_full()
                .px(self.theme.space(16.))
                .py(self.theme.space(32.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(self.theme.space(8.))
                .child(
                    theme::icons::document_file()
                        .size(px(24.))
                        .text_color(self.theme.muted()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(self.theme.muted())
                        .text_center()
                        .child(self.t("deliverables.empty")),
                )
                .into_any_element();
        }
        let mut column = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(6.))
            .p(self.theme.space(10.));
        for (index, file) in files.iter().enumerate() {
            let name = file
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&file.path)
                .to_owned();
            let parent = short_parent_path(&file.path);
            let (icon, kind_key) = deliverable_kind(&file.path, self.theme);
            let kind_label = self.t(kind_key);
            let path = file.path.clone();
            column = column.child(
                theme::interactive(
                    ("deliverable", index as u64),
                    SharedString::from(name.clone()),
                    &self.theme,
                )
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap(self.theme.space(10.))
                .px(self.theme.space(10.))
                .py(self.theme.space(8.))
                .rounded_xl()
                .border_1()
                .border_color(self.theme.line())
                .bg(self.theme.canvas())
                .cursor_pointer()
                .hover(|style| style.bg(self.theme.soft()))
                .child(
                    div()
                        .flex_none()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(self.theme.ink())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(name),
                        )
                        .when(!parent.is_empty(), |col| {
                            col.child(
                                div()
                                    .text_xs()
                                    .text_color(self.theme.muted())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(parent),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_none()
                        .px(self.theme.space(6.))
                        .py(px(2.))
                        .rounded_md()
                        .bg(self.theme.soft())
                        .text_xs()
                        .text_color(self.theme.muted())
                        .child(kind_label),
                )
                .on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.set_preview_file(Some(path.clone()), cx);
                    },
                )),
            );
        }
        column.into_any_element()
    }

    fn preview_view(
        &self,
        path: &str,
        preview: &PreviewBody,
        cx: &Context<'_, Self>,
    ) -> impl IntoElement {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned();
        let external_path = path.to_owned();
        let body = match preview {
            PreviewBody::Text(text) => div()
                .p(self.theme.space(12.))
                .text_xs()
                .child(text.clone())
                .into_any_element(),
            PreviewBody::Image(image) => div()
                .p(self.theme.space(12.))
                .child(img(image.clone()).w_full())
                .into_any_element(),
            PreviewBody::Notice(key) => div()
                .p(self.theme.space(12.))
                .text_xs()
                .text_color(self.theme.muted())
                .child(self.t(key))
                .into_any_element(),
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(self.theme.space(8.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(self.theme.space(8.))
                    .px(self.theme.space(12.))
                    .py(self.theme.space(8.))
                    .border_b_1()
                    .border_color(self.theme.line())
                    .child(
                        theme::Button::new("preview-back", self.t("chat.back"))
                            .variant(theme::ControlVariant::Ghost)
                            .size(theme::ControlSize::Small)
                            .render(
                                &self.theme,
                                cx.listener(|panel, _, _, cx| {
                                    panel.set_preview_file(None, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(name),
                    )
                    .child(
                        theme::Button::new(
                            "preview-open-external",
                            self.t("deliverables.open_external"),
                        )
                        .variant(theme::ControlVariant::Ghost)
                        .size(theme::ControlSize::Small)
                        .render(
                            &self.theme,
                            cx.listener(move |panel, _, _, cx| {
                                if open::that(&external_path).is_err() {
                                    panel.emit_notice(
                                        Some("library.error.open"),
                                        cx,
                                    );
                                }
                            }),
                        ),
                    ),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PREVIEW_BYTES, PreviewBody, load_preview};

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("flowy-deliverables-{}-{name}", std::process::id()))
    }

    #[test]
    fn a_missing_file_reports_a_notice() {
        let preview = load_preview("flowy-definitely-absent-98765.txt");
        assert!(matches!(
            preview,
            PreviewBody::Notice("chat.preview_missing")
        ));
    }

    #[test]
    fn binary_content_reports_a_notice() {
        let path = scratch("binary.dat");
        std::fs::write(&path, [0u8, 1, 2, 3]).expect("write");
        assert!(matches!(
            load_preview(path.to_str().expect("utf8")),
            PreviewBody::Notice("chat.preview_binary")
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn text_content_is_returned_as_text() {
        let path = scratch("note.txt");
        std::fs::write(&path, "hello").expect("write");
        match load_preview(path.to_str().expect("utf8")) {
            PreviewBody::Text(text) => assert_eq!(text, "hello"),
            _ => panic!("expected a text preview"),
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_content_reports_a_notice() {
        let path = scratch("big.txt");
        let data = vec![b'a'; (MAX_PREVIEW_BYTES + 1) as usize];
        std::fs::write(&path, &data).expect("write");
        assert!(matches!(
            load_preview(path.to_str().expect("utf8")),
            PreviewBody::Notice("chat.preview_too_large")
        ));
        let _ = std::fs::remove_file(path);
    }
}
